//! Appairage « style Bluetooth » : un ticket (QR ou texte) + un secret à usage unique.
//!
//! ```text
//! PC (émetteur)                                  Téléphone (récepteur)
//!  start_pairing() ─▶ ticket {adresse, secret}  ─(QR / texte)─▶ scan
//!                                                connexion iroh `cydera/pair/1`
//!                     ◀── Req{nom, HMAC(secret, ids)}  ──────────
//!  vérifie secret (valide 5 min, 3 essais, brûlé dès qu'il est correct)
//!  demande confirmation à l'utilisateur (code à 6 chiffres sur les deux écrans)
//!                     ── Ok{nom, identifiants?}  ───────────────▶
//!  chacun inscrit l'autre dans sa liste de confiance
//! ```
//! L'identité de chaque côté est authentifiée par TLS (clé iroh) ; le secret prouve que le
//! téléphone a vu le QR. Les identifiants Celcat ne transitent que si l'utilisateur le coche,
//! dans le canal chiffré, et seulement vers un appareil qu'il vient de confirmer.
use crate::SyncError;
use crate::engine::{Shared, SyncEngine};
use crate::wire::{recv_json, send_json};
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit, Mac};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

pub const PAIR_ALPN: &[u8] = b"cydera/pair/1";
const TICKET_PREFIX: &str = "cydera1:";
const MAX_ATTEMPTS: u8 = 3;
/// Temps laissé à l'utilisateur pour confirmer sur le PC.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(90);

/// Identifiants à transmettre au nouvel appareil (optionnel).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub federation_id: String,
    pub password: String,
    pub mail_user: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("username", &self.username).field("password", &"***").finish()
    }
}

/// Ce que le PC affiche (QR + texte) pour appairer un appareil.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PairTicket {
    pub v: u8,
    pub addr: EndpointAddr,
    /// Secret à usage unique (32 octets, en hexadécimal).
    pub secret: String,
    /// Nom du PC, affiché sur le téléphone.
    pub name: String,
}

impl PairTicket {
    /// Texte copiable / contenu du QR code.
    pub fn to_text(&self) -> String {
        let json = serde_json::to_vec(self).expect("ticket sérialisable");
        format!("{TICKET_PREFIX}{}", BASE32_NOPAD.encode(&json).to_lowercase())
    }

    pub fn from_text(s: &str) -> Result<Self, SyncError> {
        let body = s.trim().strip_prefix(TICKET_PREFIX).ok_or_else(|| SyncError::Pairing("ticket invalide".into()))?;
        let json = BASE32_NOPAD.decode(body.to_uppercase().as_bytes()).map_err(|_| SyncError::Pairing("ticket illisible".into()))?;
        let t: PairTicket = serde_json::from_slice(&json).map_err(|_| SyncError::Pairing("ticket illisible".into()))?;
        if t.v != 1 {
            return Err(SyncError::Pairing("version de ticket non gérée".into()));
        }
        Ok(t)
    }

    fn secret_bytes(&self) -> Result<Vec<u8>, SyncError> {
        data_encoding::HEXLOWER.decode(self.secret.as_bytes()).map_err(|_| SyncError::Pairing("secret invalide".into()))
    }

    /// Code de comparaison (6 chiffres) que le téléphone peut afficher dès le scan.
    pub fn confirmation_code(&self, my_id: &EndpointId) -> Result<String, SyncError> {
        Ok(confirmation_code(&self.secret_bytes()?, my_id, &self.addr.id))
    }
}

/// HMAC lié au secret et aux deux identités (une preuve ne vaut pas pour une autre clé).
fn proof_mac(secret: &[u8], phone: &EndpointId, pc: &EndpointId) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("HMAC accepte toute taille de clé");
    m.update(b"cydera-pair-v1");
    m.update(phone.to_string().as_bytes());
    m.update(b"|");
    m.update(pc.to_string().as_bytes());
    m
}

fn proof(secret: &[u8], phone: &EndpointId, pc: &EndpointId) -> Vec<u8> {
    proof_mac(secret, phone, pc).finalize().into_bytes().to_vec()
}

pub fn confirmation_code(secret: &[u8], phone: &EndpointId, pc: &EndpointId) -> String {
    let mut h = Sha256::new();
    h.update(b"cydera-code-v1");
    h.update(secret);
    h.update(phone.to_string().as_bytes());
    h.update(pc.to_string().as_bytes());
    let d = h.finalize();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

// ---------- état côté PC ----------

struct Pending {
    secret: Vec<u8>,
    name: String,
    expires: Instant,
    attempts: u8,
}

#[derive(Default)]
pub(crate) struct PairingState {
    pending: Mutex<Option<Pending>>,
    requests: Mutex<Option<mpsc::UnboundedSender<PairRequest>>>,
}

/// Demande d'appairage reçue : à présenter à l'utilisateur sur le PC.
#[derive(Debug)]
pub struct PairRequest {
    pub peer: EndpointId,
    pub name: String,
    /// Code à comparer avec celui affiché sur le téléphone.
    pub code: String,
    reply: oneshot::Sender<Decision>,
}

impl PairRequest {
    pub fn answer(self, d: Decision) {
        let _ = self.reply.send(d);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Decision {
    pub accept: bool,
    /// Identifiants à envoyer (uniquement si `accept`).
    pub credentials: Option<Credentials>,
}

// ---------- protocole ----------

#[derive(Serialize, Deserialize)]
struct PairReq {
    name: String,
    proof: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
enum PairResp {
    Ok { name: String, credentials: Option<Credentials> },
    Denied,
    Rejected(String),
}

#[derive(Debug)]
pub struct PairOutcome {
    pub peer_name: String,
    pub credentials: Option<Credentials>,
}

impl SyncEngine {
    /// Côté PC : ouvre une fenêtre d'appairage et renvoie le ticket à afficher.
    /// Remplace tout appairage en cours. Le secret expire après `ttl` et après 3 essais ratés.
    pub fn start_pairing(&self, device_name: &str, ttl: Duration) -> PairTicket {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).expect("générateur aléatoire du système");
        *lock(&self.shared().pairing.pending) =
            Some(Pending { secret: secret.to_vec(), name: device_name.to_string(), expires: Instant::now() + ttl, attempts: 0 });
        PairTicket { v: 1, addr: compact(self.addr()), secret: data_encoding::HEXLOWER.encode(&secret), name: device_name.to_string() }
    }

    pub fn cancel_pairing(&self) {
        *lock(&self.shared().pairing.pending) = None;
    }

    pub fn pairing_active(&self) -> bool {
        lock(&self.shared().pairing.pending).as_ref().is_some_and(|p| p.expires > Instant::now())
    }

    /// Côté PC : flux des demandes à confirmer (à consommer par l'interface, une seule fois).
    pub fn take_pair_requests(&self) -> mpsc::UnboundedReceiver<PairRequest> {
        let (tx, rx) = mpsc::unbounded_channel();
        *lock(&self.shared().pairing.requests) = Some(tx);
        rx
    }

    /// Côté téléphone : appaire avec le PC décrit par le ticket (scanné ou collé).
    pub async fn pair_with(&self, ticket: &PairTicket, my_name: &str) -> Result<PairOutcome, SyncError> {
        let secret = ticket.secret_bytes()?;
        let fut = self.pair_inner(ticket, &secret, my_name);
        tokio::time::timeout(CONFIRM_TIMEOUT + Duration::from_secs(30), fut).await.map_err(|_| SyncError::Timeout)?
    }

    async fn pair_inner(&self, ticket: &PairTicket, secret: &[u8], my_name: &str) -> Result<PairOutcome, SyncError> {
        let net = |e: &dyn std::fmt::Display| SyncError::Net(e.to_string());
        let conn = self.endpoint().connect(ticket.addr.clone(), PAIR_ALPN).await.map_err(|e| net(&e))?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| net(&e))?;
        let req = PairReq { name: my_name.to_string(), proof: proof(secret, &self.id(), &ticket.addr.id) };
        send_json(&mut send, &req).await?;
        send.finish().map_err(|e| net(&e))?;
        let resp: PairResp = recv_json(&mut recv).await?;
        conn.close(0u32.into(), b"done");
        match resp {
            PairResp::Ok { name, credentials } => {
                let sh = self.shared();
                let mut peers = lock(&sh.peers);
                peers.add(&ticket.addr.id, &name);
                peers.save()?;
                Ok(PairOutcome { peer_name: name, credentials })
            }
            PairResp::Denied => Err(SyncError::Pairing("refusé sur l'autre appareil".into())),
            PairResp::Rejected(why) => Err(SyncError::Pairing(why)),
        }
    }
}

/// Réduit l'adresse pour un QR lisible : relais + au plus 3 adresses IPv4 locales utilisables
/// (ni loopback ni liaison locale). Le reste est retrouvé par l'annuaire / mDNS.
fn compact(mut a: EndpointAddr) -> EndpointAddr {
    use iroh::TransportAddr;
    let mut ips = 0;
    a.addrs.retain(|t| match t {
        TransportAddr::Ip(sa) => match sa.ip() {
            std::net::IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() && ips < 3 => {
                ips += 1;
                true
            }
            _ => false,
        },
        _ => true,
    });
    a
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
pub(crate) struct PairHandler(pub(crate) Arc<Shared>);

impl std::fmt::Debug for PairHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairHandler")
    }
}

impl ProtocolHandler for PairHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        handle_pair(&self.0, &conn).await.map_err(AcceptError::from_err)
    }
}

async fn handle_pair(sh: &Shared, conn: &Connection) -> Result<(), SyncError> {
    let net = |e: &dyn std::fmt::Display| SyncError::Net(e.to_string());
    let phone = conn.remote_id();
    let (mut send, mut recv) =
        tokio::time::timeout(Duration::from_secs(15), conn.accept_bi()).await.map_err(|_| SyncError::Timeout)?.map_err(|e| net(&e))?;
    let req: PairReq = recv_json(&mut recv).await?;

    // 1) Secret : présent, non expiré, essais limités, preuve correcte.
    let verdict: Result<(Vec<u8>, String), String> = {
        let mut guard = lock(&sh.pairing.pending);
        match guard.as_mut() {
            None => Err("aucun appairage en cours".into()),
            Some(p) if p.expires <= Instant::now() => {
                *guard = None;
                Err("ticket expiré".into())
            }
            Some(p) => {
                p.attempts += 1;
                let ok = proof_mac(&p.secret, &phone, &sh.local_id).verify_slice(&req.proof).is_ok();
                if ok {
                    // Brûlé : un secret correct ne sert qu'une fois, quelle que soit la suite.
                    let p = guard.take().expect("présent");
                    Ok((p.secret, p.name))
                } else {
                    if p.attempts >= MAX_ATTEMPTS {
                        *guard = None;
                    }
                    Err("secret invalide".into())
                }
            }
        }
    };
    let (secret, my_name) = match verdict {
        Ok(v) => v,
        Err(why) => {
            send_json(&mut send, &PairResp::Rejected(why.clone())).await?;
            send.finish().map_err(|e| net(&e))?;
            let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
            return Err(SyncError::Pairing(why));
        }
    };

    // 2) Confirmation humaine sur le PC.
    let tx = lock(&sh.pairing.requests).clone();
    let decision = match tx {
        Some(tx) => {
            let (reply, rx) = oneshot::channel();
            let code = confirmation_code(&secret, &phone, &sh.local_id);
            if tx.send(PairRequest { peer: phone, name: req.name.clone(), code, reply }).is_err() {
                Decision::default()
            } else {
                tokio::time::timeout(CONFIRM_TIMEOUT, rx).await.ok().and_then(|r| r.ok()).unwrap_or_default()
            }
        }
        None => Decision::default(),
    };

    // 3) Réponse.
    if decision.accept {
        {
            let mut peers = lock(&sh.peers);
            peers.add(&phone, &req.name);
            peers.save()?;
        }
        send_json(&mut send, &PairResp::Ok { name: my_name, credentials: decision.credentials }).await?;
    } else {
        send_json(&mut send, &PairResp::Denied).await?;
    }
    send.finish().map_err(|e| net(&e))?;
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    Ok(())
}
