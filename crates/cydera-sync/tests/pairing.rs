//! Appairage : ticket, secret à usage unique, confirmation humaine, liste de confiance.
use cydera_core::journal::{Journal, Op};
use cydera_sync::{Credentials, Decision, Discovery, EngineConfig, PairTicket, PeerList, SyncEngine, SyncError};
use iroh::{EndpointAddr, SecretKey};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Node {
    engine: SyncEngine,
    journal: Arc<Mutex<Journal>>,
    peers: Arc<Mutex<PeerList>>,
}

async fn node(seed: u8, device: &str) -> Node {
    let journal = Arc::new(Mutex::new(Journal::new(device.into())));
    let peers = Arc::new(Mutex::new(PeerList::in_memory()));
    let engine = SyncEngine::start(EngineConfig {
        secret_key: SecretKey::from_bytes(&[seed; 32]),
        journal: journal.clone(),
        peers: peers.clone(),
        on_change: Arc::new(|| {}),
        discovery: Discovery::LocalOnly,
    })
    .await
    .expect("moteur");
    Node { engine, journal, peers }
}

fn loopback(n: &Node) -> EndpointAddr {
    let port = n.engine.endpoint().bound_sockets().iter().find(|s| s.is_ipv4()).unwrap().port();
    EndpointAddr::new(n.engine.id()).with_ip_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

/// Ticket du PC, rendu joignable en loopback pour le test.
fn ticket(pc: &Node, ttl: Duration) -> PairTicket {
    let mut t = pc.engine.start_pairing("PC de Lucas", ttl);
    t.addr = loopback(pc);
    t
}

fn creds() -> Credentials {
    Credentials { username: "e-test".into(), federation_id: "123".into(), password: "s3cret".into(), mail_user: "e-test".into() }
}

/// Simule l'utilisateur du PC : répond à chaque demande selon `accept`.
fn auto_user(pc: &Node, accept: bool, with_creds: bool) -> tokio::task::JoinHandle<Vec<String>> {
    let mut rx = pc.engine.take_pair_requests();
    tokio::spawn(async move {
        let mut seen = vec![];
        while let Some(req) = rx.recv().await {
            seen.push(format!("{}|{}", req.name, req.code));
            req.answer(Decision { accept, credentials: (accept && with_creds).then(creds) });
        }
        seen
    })
}

#[test]
fn ticket_aller_retour_texte() {
    let t =
        PairTicket { v: 1, addr: EndpointAddr::new(SecretKey::from_bytes(&[9; 32]).public()), secret: "ab".repeat(32), name: "PC".into() };
    let text = t.to_text();
    assert!(text.starts_with("cydera1:") && text.is_ascii());
    assert_eq!(PairTicket::from_text(&text).unwrap(), t);
    assert_eq!(PairTicket::from_text(&format!("  {text}\n")).unwrap(), t, "espaces tolérés");
    assert!(PairTicket::from_text("n'importe quoi").is_err());
    assert!(PairTicket::from_text("cydera1:!!!").is_err());
    let dbg = format!("{:?}", creds());
    assert!(!dbg.contains("s3cret"), "le mot de passe ne doit pas apparaître dans les logs");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn appairage_nominal_puis_synchro() {
    let pc = node(11, "pc").await;
    let tel = node(12, "tel").await;
    let user = auto_user(&pc, true, true);
    let t = ticket(&pc, Duration::from_secs(300));
    // Le texte du ticket (QR) traverse tel quel.
    let t = PairTicket { addr: t.addr.clone(), ..PairTicket::from_text(&t.to_text()).unwrap() };

    let phone_code = t.confirmation_code(&tel.engine.id()).unwrap();
    let out = tel.engine.pair_with(&t, "Pixel de Lucas").await.expect("appairage");
    assert_eq!(out.peer_name, "PC de Lucas");
    assert_eq!(out.credentials, Some(creds()), "identifiants transférés après confirmation");

    // Chacun connaît l'autre.
    assert!(tel.peers.lock().unwrap().is_trusted(&pc.engine.id()));
    assert!(pc.peers.lock().unwrap().is_trusted(&tel.engine.id()));
    assert_eq!(pc.peers.lock().unwrap().peers[0].name, "Pixel de Lucas");
    assert!(!pc.engine.pairing_active(), "secret brûlé");

    // Le code affiché sur le PC est celui que le téléphone a calculé.
    pc.engine.cancel_pairing();
    drop(pc.engine.take_pair_requests());
    let seen = user.await.unwrap();
    assert_eq!(seen, vec![format!("Pixel de Lucas|{phone_code}")]);

    // Et la synchro fonctionne aussitôt.
    pc.journal.lock().unwrap().emit(Op::CustomAdd("Anglais".into()));
    let rep = tel.engine.sync_with(loopback(&pc)).await.expect("synchro après appairage");
    assert_eq!((rep.received, rep.sent), (1, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secret_rejoue_apres_succes_est_refuse() {
    let pc = node(21, "pc").await;
    let tel = node(22, "tel").await;
    let autre = node(23, "autre").await;
    let _user = auto_user(&pc, true, false);
    let t = ticket(&pc, Duration::from_secs(300));
    tel.engine.pair_with(&t, "Téléphone").await.unwrap();
    let err = autre.engine.pair_with(&t, "Intrus").await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(ref m) if m.contains("aucun appairage")), "{err}");
    assert!(!pc.peers.lock().unwrap().is_trusted(&autre.engine.id()));
    assert!(autre.engine.sync_with(loopback(&pc)).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mauvais_secret_puis_trois_essais_brulent_le_ticket() {
    let pc = node(31, "pc").await;
    let tel = node(32, "tel").await;
    let _user = auto_user(&pc, true, false);
    let good = ticket(&pc, Duration::from_secs(300));
    let mut bad = good.clone();
    bad.secret = "00".repeat(32);
    for _ in 0..3 {
        let err = tel.engine.pair_with(&bad, "Téléphone").await.unwrap_err();
        assert!(matches!(err, SyncError::Pairing(ref m) if m.contains("secret invalide")), "{err}");
    }
    assert!(!pc.engine.pairing_active(), "3 échecs : appairage fermé");
    // Même le bon secret ne passe plus : il faut régénérer un ticket.
    assert!(tel.engine.pair_with(&good, "Téléphone").await.is_err());
    assert!(pc.peers.lock().unwrap().peers.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ticket_expire() {
    let pc = node(41, "pc").await;
    let tel = node(42, "tel").await;
    let _user = auto_user(&pc, true, false);
    let t = ticket(&pc, Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(120)).await;
    let err = tel.engine.pair_with(&t, "Téléphone").await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(ref m) if m.contains("expiré") || m.contains("aucun")), "{err}");
    assert!(pc.peers.lock().unwrap().peers.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refus_de_l_utilisateur() {
    let pc = node(51, "pc").await;
    let tel = node(52, "tel").await;
    let _user = auto_user(&pc, false, true);
    let t = ticket(&pc, Duration::from_secs(300));
    let err = tel.engine.pair_with(&t, "Téléphone").await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(ref m) if m.contains("refusé")), "{err}");
    assert!(pc.peers.lock().unwrap().peers.is_empty());
    assert!(tel.peers.lock().unwrap().peers.is_empty());
    assert!(!pc.engine.pairing_active(), "le secret est brûlé même après un refus");
    assert!(tel.engine.sync_with(loopback(&pc)).await.is_err(), "pas de synchro sans appairage");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sans_interface_pour_confirmer_on_refuse() {
    // Aucun récepteur de demandes (app fermée / pas d'UI) : jamais d'appairage implicite.
    let pc = node(61, "pc").await;
    let tel = node(62, "tel").await;
    let t = ticket(&pc, Duration::from_secs(300));
    assert!(tel.engine.pair_with(&t, "Téléphone").await.is_err());
    assert!(pc.peers.lock().unwrap().peers.is_empty());
}
