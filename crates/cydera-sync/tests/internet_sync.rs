//! Test réseau réel (ignoré par défaut) : deux moteurs en mode `Internet`, l'un joint l'autre
//! uniquement via le relais iroh (aucune adresse IP directe fournie), comme deux appareils
//! derrière des box différentes.
//!   cargo test -p cydera-sync --test internet_sync -- --ignored --nocapture
use cydera_core::journal::{Journal, Op};
use cydera_sync::{Discovery, EngineConfig, PeerList, SyncEngine};
use iroh::{EndpointAddr, SecretKey, TransportAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

async fn start(seed: u8, device: &str) -> (SyncEngine, Arc<Mutex<Journal>>, Arc<Mutex<PeerList>>) {
    let journal = Arc::new(Mutex::new(Journal::new(device.into())));
    let peers = Arc::new(Mutex::new(PeerList::in_memory()));
    let engine = SyncEngine::start(EngineConfig {
        secret_key: SecretKey::from_bytes(&[seed; 32]),
        journal: journal.clone(),
        peers: peers.clone(),
        on_change: Arc::new(|| {}),
        discovery: Discovery::Internet,
    })
    .await
    .expect("moteur");
    (engine, journal, peers)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "nécessite internet (relais iroh)"]
async fn synchro_via_relais_sans_adresse_directe() {
    // Clés dérivées de graines : identités de test, sans valeur.
    let (pc, pc_j, pc_p) = start(41, "pc").await;
    let (tel, tel_j, tel_p) = start(42, "tel").await;
    pc_p.lock().unwrap().add(&tel.id(), "Téléphone");
    tel_p.lock().unwrap().add(&pc.id(), "PC");

    tokio::time::timeout(Duration::from_secs(20), pc.endpoint().online()).await.expect("PC en ligne (relais joint)");
    let full = pc.addr();
    let relay_only: Vec<TransportAddr> = full.addrs.iter().filter(|a| matches!(a, TransportAddr::Relay(_))).cloned().collect();
    println!("adresse du PC : {full:?}\nrelais seul : {relay_only:?}");
    assert!(!relay_only.is_empty(), "le PC doit avoir un relais d'accueil");

    pc_j.lock().unwrap().emit(Op::CustomAdd("Depuis le PC".into()));
    tel_j.lock().unwrap().emit(Op::CustomAdd("Depuis le téléphone".into()));

    let rep = tel.sync_with(EndpointAddr::from_parts(pc.id(), relay_only)).await.expect("synchro via relais");
    println!("rapport : {rep:?}");
    assert_eq!((rep.received, rep.sent), (1, 1));
    assert_eq!(pc_j.lock().unwrap().state(), tel_j.lock().unwrap().state());
}
