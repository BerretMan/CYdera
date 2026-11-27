//! Trois moteurs iroh dans le même processus, réseau local (loopback), sans relais :
//! deux appareils appairés convergent ; un troisième, non appairé, est refusé.
use cydera_core::grades::{
    self,
    model::{Grade, Profile},
    templates,
};
use cydera_core::journal::{Journal, Op};
use cydera_core::study::{Session, SessionKind};
use cydera_sync::{Discovery, EngineConfig, PeerList, SyncEngine, SyncError};
use iroh::{EndpointAddr, SecretKey};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn sess(id: &str, h: u32) -> Session {
    let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    Session {
        id: id.into(),
        start: d.and_hms_opt(h, 0, 0).unwrap(),
        end: d.and_hms_opt(h + 1, 0, 0).unwrap(),
        secs: 3600,
        subject: "Maths".into(),
        part: "Cours".into(),
        kind: SessionKind::Work,
    }
}

struct Node {
    engine: SyncEngine,
    journal: Arc<Mutex<Journal>>,
    peers: Arc<Mutex<PeerList>>,
    changes: Arc<AtomicUsize>,
}

fn key(seed: u8) -> SecretKey {
    SecretKey::from_bytes(&[seed; 32])
}

async fn node(seed: u8, device: &str) -> Node {
    let journal = Arc::new(Mutex::new(Journal::new(device.into())));
    let peers = Arc::new(Mutex::new(PeerList::in_memory()));
    let changes = Arc::new(AtomicUsize::new(0));
    let c = changes.clone();
    let engine = SyncEngine::start(EngineConfig {
        secret_key: key(seed),
        journal: journal.clone(),
        peers: peers.clone(),
        on_change: Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }),
        discovery: Discovery::LocalOnly,
    })
    .await
    .expect("démarrage du moteur");
    Node { engine, journal, peers, changes }
}

/// Adresse joignable en loopback (le moteur écoute sur toutes les interfaces).
fn loopback(n: &Node) -> EndpointAddr {
    let port = n.engine.endpoint().bound_sockets().iter().find(|s| s.is_ipv4()).expect("socket IPv4").port();
    EndpointAddr::new(n.engine.id()).with_ip_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

fn pair(a: &Node, b: &Node, name_of_b: &str, name_of_a: &str) {
    a.peers.lock().unwrap().add(&b.engine.id(), name_of_b);
    b.peers.lock().unwrap().add(&a.engine.id(), name_of_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deux_appareils_appaires_convergent_et_un_inconnu_est_refuse() {
    let pc = node(1, "pc").await;
    let tel = node(2, "tel").await;
    let intrus = node(3, "intrus").await;
    pair(&pc, &tel, "Téléphone", "PC");

    // Modifications hors ligne des deux côtés.
    {
        let mut j = pc.journal.lock().unwrap();
        j.emit(Op::SessionAdd(sess("a", 8)));
        j.emit(Op::SessionAdd(sess("b", 10)));
        j.emit(Op::CustomAdd("Anglais".into()));
    }
    {
        let mut j = tel.journal.lock().unwrap();
        j.emit(Op::SessionAdd(sess("c", 9)));
    }

    // Le téléphone appelle le PC.
    let rep = tel.engine.sync_with(loopback(&pc)).await.expect("synchro");
    assert_eq!((rep.received, rep.sent), (3, 1));
    assert_eq!(pc.journal.lock().unwrap().state(), tel.journal.lock().unwrap().state());
    assert_eq!(pc.journal.lock().unwrap().state().sessions.len(), 3);
    assert!(pc.changes.load(Ordering::SeqCst) >= 1, "le PC est notifié");
    assert!(tel.changes.load(Ordering::SeqCst) >= 1, "le téléphone est notifié");

    // Suppression côté PC, puis nouvelle synchro dans l'autre sens.
    pc.journal.lock().unwrap().emit(Op::SessionDel { id: "c".into() });
    let rep = pc.engine.sync_with(loopback(&tel)).await.expect("synchro retour");
    assert_eq!((rep.received, rep.sent), (0, 1));
    assert_eq!(tel.journal.lock().unwrap().state().sessions.len(), 2);

    // Notes : profil importé sur le PC, note saisie sur le téléphone, les deux convergent.
    let profile: Profile = templates::templates().remove(0).1;
    let profile = Profile { id: "p1".into(), ..profile };
    let ec = profile.semesters[0].ues[0].ecs[0].id.clone();
    pc.journal.lock().unwrap().emit(Op::ProfileSave { profile, ts: 1 });
    pc.engine.sync_with(loopback(&tel)).await.expect("synchro du profil");
    let grade = Grade { id: "g1".into(), profile: "p1".into(), ec, label: "Partiel".into(), value: 14.5, out_of: 20.0, weight: 1.0 };
    tel.journal.lock().unwrap().emit(Op::GradeSave { grade, ts: 2 });
    let rep = tel.engine.sync_with(loopback(&pc)).await.expect("synchro de la note");
    assert_eq!((rep.received, rep.sent), (0, 1));
    let (on_pc, on_tel) = (grades::project(&pc.journal.lock().unwrap()), grades::project(&tel.journal.lock().unwrap()));
    assert_eq!(on_pc, on_tel);
    assert_eq!(on_pc.grades[0].value, 14.5);
    assert_eq!(on_pc.profiles[0].name, "ING2 GI-a");

    // Rien à échanger : idempotent.
    let rep = tel.engine.sync_with(loopback(&pc)).await.unwrap();
    assert_eq!((rep.received, rep.sent), (0, 0));

    // L'intrus (clé inconnue) : 1) ne peut pas appeler un appareil non appairé, 2) est refusé en face.
    assert!(matches!(intrus.engine.sync_with(loopback(&pc)).await, Err(SyncError::Untrusted)));
    // Même en se déclarant appairé de son côté, le PC le rejette.
    intrus.peers.lock().unwrap().add(&pc.engine.id(), "PC");
    let before = pc.journal.lock().unwrap().state();
    intrus.journal.lock().unwrap().emit(Op::CustomAdd("Pirate".into()));
    assert!(intrus.engine.sync_with(loopback(&pc)).await.is_err(), "le PC doit refuser l'intrus");
    assert_eq!(pc.journal.lock().unwrap().state(), before, "aucune donnée de l'intrus n'est acceptée");
    assert!(!pc.journal.lock().unwrap().state().custom.contains(&"Pirate".to_string()));

    tokio::join!(pc.engine.shutdown(), tel.engine.shutdown(), intrus.engine.shutdown());
}
