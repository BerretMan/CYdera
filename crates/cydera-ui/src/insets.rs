//! Marges système (barre d'état, barre de navigation, encoche, clavier).
//! L'hôte (Android) fournit une fonction qui les renvoie en pixels physiques
//! `[haut, droite, bas, gauche]` ; `App::raw_input_hook` les transmet à egui.
use std::sync::OnceLock;

type Provider = Box<dyn Fn() -> [f32; 4] + Send + Sync>;
static PROVIDER: OnceLock<Provider> = OnceLock::new();

pub fn set_provider(f: impl Fn() -> [f32; 4] + Send + Sync + 'static) {
    let _ = PROVIDER.set(Box::new(f));
}

pub fn current_px() -> Option<[f32; 4]> {
    PROVIDER.get().map(|f| f())
}
