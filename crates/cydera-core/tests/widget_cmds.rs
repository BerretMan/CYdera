//! Appuis ▶/⏸ du widget Android rejoués par le chrono : app fermée (démarrage à froid),
//! chrono lancé depuis le widget puis arrêté depuis le widget, fichiers temporaires ignorés.
use chrono::Duration;
use cydera_core::model::now;
use cydera_core::platform::{self, Dirs};
use cydera_core::study::{Chrono, SessionKind};
use cydera_core::widget::{self, to_ms};

fn cmd(dir: &std::path::Path, at: chrono::NaiveDateTime, n: u32, action: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{:013}-{n}.{action}", to_ms(at))), "").unwrap();
}

#[test]
fn widget_app_fermee() {
    let root = std::env::temp_dir().join(format!("cydera-widget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dirs = Dirs::under(root.clone());
    let app_dir = dirs.data.join("cydera");
    let cmd_dir = app_dir.join("widget-cmd");
    std::fs::create_dir_all(&app_dir).unwrap();
    platform::init(dirs);
    std::fs::write(app_dir.join("revisions.json"), r#"{"sessions":[],"custom":["Maths"],"last":["Maths","TD"]}"#).unwrap();
    let t = now();

    // ▶ puis ⏸ pendant que l'app était fermée : une session de 20 min à la bonne heure.
    cmd(&cmd_dir, t - Duration::minutes(30), 0, "start");
    cmd(&cmd_dir, t - Duration::minutes(10), 1, "pause");
    std::fs::write(cmd_dir.join("123-9.tmp"), "").unwrap();
    let ch = Chrono::load();
    assert!(!ch.running());
    let s: Vec<_> = ch.sessions().iter().filter(|s| s.kind == SessionKind::Work).collect();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].secs, 20 * 60);
    assert_eq!((s[0].subject.as_str(), s[0].part.as_str()), ("Maths", "TD"));
    assert!(widget::take_commands().is_empty(), "commandes consommées");
    assert!(cmd_dir.join("123-9.tmp").exists(), "fichier temporaire laissé en place");
    drop(ch);

    // ▶ app fermée : au lancement, le chrono tourne depuis l'heure de l'appui.
    cmd(&cmd_dir, t - Duration::minutes(5), 2, "start");
    let ch = Chrono::load();
    assert!(ch.running());
    assert!((ch.run_secs() - 300).abs() <= 2, "run_secs = {}", ch.run_secs());
    let st = ch.widget_state();
    assert!(st.running && st.subject == "Maths" && st.today_secs == 20 * 60);
    drop(ch); // app tuée sans pause : l'état « en cours » reste sur disque

    // ⏸ sur le widget pendant que l'app était tuée : reprise puis arrêt à l'heure de l'appui,
    // et non troncature au dernier checkpoint.
    cmd(&cmd_dir, t - Duration::minutes(1), 3, "pause");
    let ch = Chrono::load();
    assert!(!ch.running());
    assert!(ch.notice.is_none(), "pas de message de récupération après plantage");
    let last = ch.sessions().iter().filter(|s| s.kind == SessionKind::Work).max_by_key(|s| s.start).unwrap();
    assert_eq!(last.secs, 4 * 60);
    let _ = std::fs::remove_dir_all(&root);
}
