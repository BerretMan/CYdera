//! Presse-papiers système fourni par l'hôte (Android : `ClipboardManager` via JNI).
//!
//! Sur Android en `NativeActivity`, il n'y a pas d'`InputConnection` : le « Coller » du clavier
//! ne parvient jamais à l'appli et egui-winit n'a pas de presse-papiers. L'appli affiche donc sa
//! propre pastille « COLLER » et renvoie elle-même les copies d'egui vers le système.
use std::sync::OnceLock;

type Get = Box<dyn Fn() -> Option<String> + Send + Sync>;
type Set = Box<dyn Fn(&str) + Send + Sync>;
static PROVIDER: OnceLock<(Get, Set)> = OnceLock::new();

pub fn set_provider(get: impl Fn() -> Option<String> + Send + Sync + 'static, set: impl Fn(&str) + Send + Sync + 'static) {
    let _ = PROVIDER.set((Box::new(get), Box::new(set)));
}

/// Vrai si l'hôte fournit son propre presse-papiers (Android).
pub fn native() -> bool {
    PROVIDER.get().is_some()
}

pub fn get() -> Option<String> {
    PROVIDER.get().and_then(|(g, _)| g()).filter(|s| !s.is_empty())
}

pub fn set(text: &str) {
    if let Some((_, s)) = PROVIDER.get() {
        s(text);
    }
}
