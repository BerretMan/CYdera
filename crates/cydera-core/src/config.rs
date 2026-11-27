use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration optionnelle : `~/.config/cydera/config.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub username: String,
    pub federation_id: String,
    pub base_url: String,
    /// Durée de validité du cache, en minutes.
    pub refresh_minutes: i64,
    /// Serveur IMAP des mails étudiants.
    pub mail_host: String,
    /// Identifiant de messagerie (vide = pas de mails).
    pub mail_user: String,
    /// Nom de cet appareil montré aux appareils appairés (vide = nom de la machine).
    pub device_name: String,
    /// Vérifier automatiquement les nouvelles versions sur GitHub (au lancement puis toutes les 6 h).
    pub maj_auto: bool,
    /// Version dont l'utilisateur a refusé la mise à jour (« Ignorer cette version »).
    pub maj_ignoree: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Vides : l'écran de connexion les demande au premier lancement.
            username: String::new(),
            federation_id: String::new(),
            base_url: "https://celcat-calendar.cyu.fr".into(),
            refresh_minutes: 15,
            mail_host: "mail.etu.cyu.fr".into(),
            mail_user: String::new(),
            device_name: String::new(),
            maj_auto: true,
            maj_ignoree: String::new(),
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        crate::platform::config_dir().map(|d| d.join("cydera").join("config.json"))
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
}

fn secret_path() -> Option<PathBuf> {
    crate::platform::data_dir().map(|d| d.join("celcat_password"))
}

/// Enregistre le mot de passe saisi dans l'appli (dossier privé de l'appli, droits 0600).
/// Sur Android ce dossier est inaccessible aux autres applications.
pub fn save_password(pwd: &str) -> std::io::Result<()> {
    let p = secret_path().ok_or_else(|| std::io::Error::other("dossier de données introuvable"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, pwd.trim())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Mot de passe : variable `CELCAT_PASSWORD`, sinon fichier enregistré par l'appli,
/// sinon `~/CELCAT_PASSWORD` (utile quand l'appli est lancée depuis un .desktop qui ne lit pas le .bashrc).
pub fn load_password() -> Option<String> {
    if let Ok(p) = std::env::var("CELCAT_PASSWORD") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }
    let saved = secret_path().and_then(|f| std::fs::read_to_string(f).ok()).map(|p| p.trim().to_string());
    if let Some(p) = saved.filter(|p| !p.is_empty()) {
        return Some(p);
    }
    let file = crate::platform::home_dir()?.join("CELCAT_PASSWORD");
    let p = std::fs::read_to_string(file).ok()?.trim().to_string();
    (!p.is_empty()).then_some(p)
}

/// Mot de passe de la messagerie : `CYU_MAIL_PASSWORD` ou fichier `~/CYU_MAIL_PASSWORD`.
pub fn load_mail_password() -> Option<String> {
    if let Ok(p) = std::env::var("CYU_MAIL_PASSWORD") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }
    if let Some(p) = crate::platform::home_dir()
        .and_then(|h| std::fs::read_to_string(h.join("CYU_MAIL_PASSWORD")).ok())
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    {
        return Some(p);
    }
    // Même compte CY que Celcat : repli sur le mot de passe Celcat.
    load_password()
}

/// Anciennes données (« cydesktop », puis « cyagartha ») : déplacées une seule fois vers « cydera ».
pub fn migrate_legacy() {
    for base in [crate::platform::config_dir(), crate::platform::cache_dir(), crate::platform::data_dir()].into_iter().flatten() {
        let new = base.join("cydera");
        for old in ["cyagartha", "cydesktop"] {
            let old = base.join(old);
            if old.exists() && !new.exists() {
                let _ = std::fs::rename(old, &new);
            }
        }
    }
}
