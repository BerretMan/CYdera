//! Chrono + journal sur disque : migration de l'ancien format, suppression / annulation,
//! persistance, et réception d'événements d'un autre appareil.
use cydera_core::journal::{Event, Journal, Op, VersionVector};
use cydera_core::platform::{self, Dirs};
use cydera_core::study::{Chrono, Session, SessionKind};

fn legacy_session(id: &str, subject: &str) -> Session {
    let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    Session {
        id: id.into(),
        start: d.and_hms_opt(9, 0, 0).unwrap(),
        end: d.and_hms_opt(10, 0, 0).unwrap(),
        secs: 3600,
        subject: subject.into(),
        part: "Cours".into(),
        kind: SessionKind::Work,
    }
}

#[test]
fn migration_suppression_annulation_et_synchro() {
    let root = std::env::temp_dir().join(format!("cydera-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dirs = Dirs::under(root.clone());
    let app_dir = dirs.data.join("cydera");
    std::fs::create_dir_all(&app_dir).unwrap();
    platform::init(dirs.clone());

    // Ancien format : seulement revisions.json
    let legacy = serde_json::json!({
        "sessions": [legacy_session("old1", "Maths"), legacy_session("old2", "Physique")],
        "custom": ["Anglais"],
        "last": null
    });
    std::fs::write(app_dir.join("revisions.json"), legacy.to_string()).unwrap();

    let mut ch = Chrono::load();
    assert_eq!(ch.sessions().len(), 2, "sessions migrées");
    assert!(ch.known_subjects().contains(&"Anglais".to_string()));
    assert!(app_dir.join("journal.json").exists(), "journal créé à la migration");
    let before = ch.sync_version();
    assert_eq!(before.values().sum::<u64>(), 3, "2 sessions + 1 matière");

    // Suppression puis annulation : la session revient (sous un nouvel id), l'ancienne reste supprimée.
    ch.delete("old1");
    assert_eq!(ch.sessions().len(), 1);
    ch.undo_delete();
    assert_eq!(ch.sessions().len(), 2);
    assert!(ch.sessions().iter().all(|s| s.id != "old1"));

    // Rechargement : même état.
    let after: Vec<String> = ch.sessions().iter().map(|s| s.id.clone()).collect();
    let ch2 = Chrono::load();
    let reloaded: Vec<String> = ch2.sessions().iter().map(|s| s.id.clone()).collect();
    assert_eq!(after, reloaded);

    // Un autre appareil (le téléphone) a ajouté une session et supprimé old2.
    let mut ch = ch2;
    let mut tel = Journal::new("telephone".into());
    tel.emit(Op::SessionAdd(legacy_session("tel1", "Chimie")));
    tel.emit(Op::SessionDel { id: "old2".into() });
    let events: Vec<Event> = tel.since(&VersionVector::new());
    assert_eq!(ch.sync_apply(events.clone()), 2);
    assert_eq!(ch.sync_apply(events), 0, "idempotent");
    let subjects: Vec<&str> = ch.sessions().iter().map(|s| s.subject.as_str()).collect();
    assert!(subjects.contains(&"Chimie") && !subjects.contains(&"Physique"));

    // Et le téléphone récupère ce qui lui manque.
    let missing = ch.sync_events_since(tel.version());
    assert!(!missing.is_empty());
    tel.apply(missing);
    assert_eq!(tel.state().sessions.len(), ch.sessions().len());

    let _ = std::fs::remove_dir_all(&root);
}
