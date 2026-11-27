//! Répertoires de l'application. Sur desktop ils viennent de `dirs` ; sur Android
//! l'hôte appelle `init` avec le dossier privé de l'appli (`internal_data_path`).
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Dirs {
    pub config: PathBuf,
    pub cache: PathBuf,
    pub data: PathBuf,
    pub home: Option<PathBuf>,
}

impl Dirs {
    /// Tout sous une seule racine (Android : `files/`).
    pub fn under(root: PathBuf) -> Self {
        Self { config: root.join("config"), cache: root.join("cache"), data: root.join("data"), home: None }
    }
}

static DIRS: OnceLock<Dirs> = OnceLock::new();

/// À appeler une fois au démarrage (obligatoire sur Android, facultatif sur desktop).
pub fn init(d: Dirs) {
    let _ = DIRS.set(d);
}

pub fn config_dir() -> Option<PathBuf> {
    DIRS.get().map(|d| d.config.clone()).or_else(sys_config)
}
pub fn cache_dir() -> Option<PathBuf> {
    DIRS.get().map(|d| d.cache.clone()).or_else(sys_cache)
}
pub fn data_dir() -> Option<PathBuf> {
    DIRS.get().map(|d| d.data.clone()).or_else(sys_data)
}
pub fn home_dir() -> Option<PathBuf> {
    match DIRS.get() {
        Some(d) => d.home.clone(),
        None => sys_home(),
    }
}

#[cfg(not(target_os = "android"))]
fn sys_config() -> Option<PathBuf> {
    dirs::config_dir()
}
#[cfg(not(target_os = "android"))]
fn sys_cache() -> Option<PathBuf> {
    dirs::cache_dir()
}
#[cfg(not(target_os = "android"))]
fn sys_data() -> Option<PathBuf> {
    dirs::data_dir()
}
#[cfg(not(target_os = "android"))]
fn sys_home() -> Option<PathBuf> {
    dirs::home_dir()
}
#[cfg(target_os = "android")]
fn sys_config() -> Option<PathBuf> {
    None
}
#[cfg(target_os = "android")]
fn sys_cache() -> Option<PathBuf> {
    None
}
#[cfg(target_os = "android")]
fn sys_data() -> Option<PathBuf> {
    None
}
#[cfg(target_os = "android")]
fn sys_home() -> Option<PathBuf> {
    None
}
