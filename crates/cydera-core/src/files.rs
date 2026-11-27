//! Écriture des fichiers de données de l'appli (`<données>/cydera/`), partagée par le chrono,
//! le journal de synchro et les notes.

use std::path::PathBuf;

/// Dossier des données synchronisées et locales (`~/.local/share/cydera` sur PC).
pub fn dir() -> Option<PathBuf> {
    crate::platform::data_dir().map(|d| d.join("cydera"))
}

/// Écrit `content` dans `<dossier>/name` via un fichier temporaire puis un renommage :
/// un arrêt brutal ne laisse jamais un fichier à moitié écrit.
pub fn write_atomic(name: &str, content: &str) {
    let Some(d) = dir() else { return };
    let _ = std::fs::create_dir_all(&d);
    let (p, tmp) = (d.join(name), d.join(format!("{name}.tmp")));
    if std::fs::write(&tmp, content).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

pub fn read(name: &str) -> Option<String> {
    dir().and_then(|d| std::fs::read_to_string(d.join(name)).ok())
}
