//! Moteur de synchro : un `Endpoint` iroh qui accepte (uniquement) les appareils appairés
//! et sait synchroniser le journal avec eux.
use crate::SyncError;
use crate::peers::PeerList;
use crate::wire::{self, ALPN, Msg};
use cydera_core::journal::Journal;
use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discovery {
    /// Usage normal : relais iroh (traversée de NAT), annuaire DNS et découverte mDNS locale.
    Internet,
    /// Réseau local seulement, sans relais ni annuaire (tests).
    LocalOnly,
}

pub struct EngineConfig {
    pub secret_key: SecretKey,
    pub journal: Arc<Mutex<Journal>>,
    pub peers: Arc<Mutex<PeerList>>,
    /// Appelé (depuis un thread réseau) quand des événements distants ont été intégrés.
    pub on_change: Arc<dyn Fn() + Send + Sync>,
    pub discovery: Discovery,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub received: usize,
    pub sent: usize,
}

pub(crate) struct Shared {
    pub(crate) journal: Arc<Mutex<Journal>>,
    pub(crate) peers: Arc<Mutex<PeerList>>,
    pub(crate) local_id: EndpointId,
    on_change: Arc<dyn Fn() + Send + Sync>,
    pub(crate) pairing: crate::pairing::PairingState,
}

impl Shared {
    pub(crate) fn trusted(&self, id: &EndpointId) -> bool {
        self.peers.lock().map(|p| p.is_trusted(id)).unwrap_or(false)
    }

    fn apply(&self, events: Vec<cydera_core::journal::Event>) -> usize {
        let n = match self.journal.lock() {
            Ok(mut j) => j.apply(wire::sanitize(events)),
            Err(_) => 0,
        };
        if n > 0 {
            (self.on_change)();
        }
        n
    }

    fn mark_synced(&self, id: &EndpointId) {
        if let Ok(mut p) = self.peers.lock() {
            p.mark_synced(id);
            let _ = p.save();
        }
    }
}

#[derive(Clone)]
struct Handler(Arc<Shared>);

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handler")
    }
}

impl ProtocolHandler for Handler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id();
        // Contrôle d'accès AVANT toute lecture : un appareil non appairé n'obtient rien.
        if !self.0.trusted(&remote) {
            conn.close(403u32.into(), b"non apparie");
            return Err(AcceptError::from_err(SyncError::Untrusted));
        }
        serve(&self.0, &conn, remote).await.map_err(AcceptError::from_err)
    }
}

async fn serve(sh: &Shared, conn: &Connection, remote: EndpointId) -> Result<(), SyncError> {
    let net = |e: &dyn std::fmt::Display| SyncError::Net(e.to_string());
    let (mut send, mut recv) =
        tokio::time::timeout(SYNC_TIMEOUT, conn.accept_bi()).await.map_err(|_| SyncError::Timeout)?.map_err(|e| net(&e))?;
    let Msg::Hello { vv } = wire::recv_msg(&mut recv).await? else {
        return Err(SyncError::Proto("Hello attendu".into()));
    };
    let (my_vv, events) = {
        let j = sh.journal.lock().map_err(|_| SyncError::Proto("journal verrouillé".into()))?;
        (j.version().clone(), j.since(&vv))
    };
    wire::send_msg(&mut send, &Msg::Offer { vv: my_vv, events }).await?;
    let Msg::Push { events } = wire::recv_msg(&mut recv).await? else {
        return Err(SyncError::Proto("Push attendu".into()));
    };
    let applied = sh.apply(events);
    wire::send_msg(&mut send, &Msg::Ack { applied }).await?;
    send.finish().map_err(|e| net(&e))?;
    sh.mark_synced(&remote);
    // Laisse l'initiateur lire l'Ack puis fermer la connexion.
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    Ok(())
}

pub struct SyncEngine {
    shared: Arc<Shared>,
    router: Router,
}

impl SyncEngine {
    pub async fn start(cfg: EngineConfig) -> Result<Self, SyncError> {
        let builder = match cfg.discovery {
            Discovery::Internet => Endpoint::builder(presets::N0)
                // Service mDNS propre à Cydera : seuls les appareils Cydera du réseau local se voient.
                .address_lookup(iroh_mdns_address_lookup::MdnsAddressLookup::builder().service_name("cydera")),
            Discovery::LocalOnly => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled),
        };
        let endpoint = builder.secret_key(cfg.secret_key).bind().await.map_err(|e| SyncError::Net(e.to_string()))?;
        let shared = Arc::new(Shared {
            local_id: endpoint.id(),
            journal: cfg.journal,
            peers: cfg.peers,
            on_change: cfg.on_change,
            pairing: crate::pairing::PairingState::default(),
        });
        let router = Router::builder(endpoint)
            .accept(ALPN, Handler(shared.clone()))
            .accept(crate::pairing::PAIR_ALPN, crate::pairing::PairHandler(shared.clone()))
            .spawn();
        Ok(Self { shared, router })
    }

    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint().id()
    }

    /// Adresse actuelle (identité + chemins connus) : c'est ce qui va dans le ticket d'appairage.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint().addr()
    }

    /// Synchronise avec un appareil appairé. Refuse d'appeler un appareil hors liste de confiance.
    pub async fn sync_with(&self, addr: impl Into<EndpointAddr>) -> Result<SyncReport, SyncError> {
        let addr = addr.into();
        let remote = addr.id;
        if !self.shared.trusted(&remote) {
            return Err(SyncError::Untrusted);
        }
        let rep = tokio::time::timeout(SYNC_TIMEOUT, self.initiate(addr)).await.map_err(|_| SyncError::Timeout)??;
        self.shared.mark_synced(&remote);
        Ok(rep)
    }

    async fn initiate(&self, addr: EndpointAddr) -> Result<SyncReport, SyncError> {
        let net = |e: &dyn std::fmt::Display| SyncError::Net(e.to_string());
        let conn = self.endpoint().connect(addr, ALPN).await.map_err(|e| net(&e))?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| net(&e))?;
        let vv = self.shared.journal.lock().map_err(|_| SyncError::Proto("journal verrouillé".into()))?.version().clone();
        wire::send_msg(&mut send, &Msg::Hello { vv }).await?;
        let Msg::Offer { vv: theirs, events } = wire::recv_msg(&mut recv).await? else {
            return Err(SyncError::Proto("Offer attendu".into()));
        };
        let received = self.shared.apply(events);
        let push = self.shared.journal.lock().map_err(|_| SyncError::Proto("journal verrouillé".into()))?.since(&theirs);
        let sent = push.len();
        wire::send_msg(&mut send, &Msg::Push { events: push }).await?;
        send.finish().map_err(|e| net(&e))?;
        let Msg::Ack { .. } = wire::recv_msg(&mut recv).await? else {
            return Err(SyncError::Proto("Ack attendu".into()));
        };
        conn.close(0u32.into(), b"done");
        Ok(SyncReport { received, sent })
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub async fn shutdown(self) {
        let _ = self.router.shutdown().await;
    }
}

/// Clé secrète de l'appareil : créée au premier lancement, stockée en 0600. C'est l'identité réseau.
pub fn load_or_create_key(path: &Path) -> Result<SecretKey, SyncError> {
    if let Ok(b) = std::fs::read(path)
        && let Ok(arr) = <[u8; 32]>::try_from(b.as_slice())
    {
        return Ok(SecretKey::from_bytes(&arr));
    }
    let mut arr = [0u8; 32];
    getrandom::fill(&mut arr).map_err(|e| SyncError::Net(e.to_string()))?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, arr)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(SecretKey::from_bytes(&arr))
}
