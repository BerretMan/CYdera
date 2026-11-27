//! « PC factice » en ligne de commande pour tester l'appairage et la synchro avec un vrai
//! téléphone / émulateur, sans interface :
//!   cargo run -p cydera-sync --example pc_peer
//! Affiche un ticket (à coller dans l'appli), accepte la première demande en envoyant des
//! identifiants factices, puis reste à l'écoute et synchronise.
use cydera_core::journal::{Journal, Op};
use cydera_core::study::{Session, SessionKind};
use cydera_sync::{Credentials, Decision, Discovery, EngineConfig, PeerList, SyncEngine};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let key = {
        let mut b = [0u8; 32];
        getrandom_fill(&mut b);
        iroh::SecretKey::from_bytes(&b)
    };
    let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
    let mut j = Journal::new("pc-factice".into());
    j.emit(Op::CustomAdd("Depuis le PC factice".into()));
    j.emit(Op::SessionAdd(Session {
        id: "pc-1".into(),
        start: d.and_hms_opt(9, 0, 0).unwrap(),
        end: d.and_hms_opt(10, 0, 0).unwrap(),
        secs: 3600,
        subject: "Depuis le PC factice".into(),
        part: "Cours".into(),
        kind: SessionKind::Work,
    }));
    let journal = Arc::new(Mutex::new(j));
    let engine = SyncEngine::start(EngineConfig {
        secret_key: key,
        journal: journal.clone(),
        peers: Arc::new(Mutex::new(PeerList::in_memory())),
        on_change: Arc::new(|| println!("[pc] des événements ont été reçus du téléphone")),
        discovery: Discovery::Internet,
    })
    .await
    .expect("moteur");
    tokio::time::timeout(Duration::from_secs(20), engine.endpoint().online()).await.ok();
    let ticket = engine.start_pairing("PC factice", Duration::from_secs(600));
    println!("TICKET={}", ticket.to_text());
    println!("[pc] en attente d'une demande d'appairage (10 min)…");
    let mut rx = engine.take_pair_requests();
    if let Some(req) = rx.recv().await {
        println!("[pc] demande de « {} » — code {}", req.name, req.code);
        let creds = Credentials {
            username: "e-factice".into(),
            federation_id: "000".into(),
            password: "mdp-factice".into(),
            mail_user: "e-factice".into(),
        };
        req.answer(Decision { accept: true, credentials: Some(creds) });
        println!("[pc] accepté, identifiants factices envoyés");
    }
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let st = journal.lock().unwrap().state();
        println!("[pc] état : {} sessions, matières {:?}", st.sessions.len(), st.custom);
    }
}

fn getrandom_fill(b: &mut [u8; 32]) {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(b)).expect("/dev/urandom");
}
