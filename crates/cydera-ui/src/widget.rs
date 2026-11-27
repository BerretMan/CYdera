//! Widgets d'écran d'accueil fournis par l'hôte (Android : `AppWidgetProvider` Java).
//!
//! Sans hôte (bureau), rien n'est écrit. Avec hôte, `App::poll` rejoue chaque seconde les appuis
//! ▶/⏸ du widget (`core::widget::take_commands`), réécrit `widget.json` quand l'état du chrono
//! change et `widget-edt.json` quand l'emploi du temps change, puis demande un rafraîchissement.
use std::sync::OnceLock;

type Notify = Box<dyn Fn() + Send + Sync>;
static NOTIFY: OnceLock<Notify> = OnceLock::new();

/// L'hôte installe la fonction qui redessine ses widgets (Android : broadcast vers `WidgetActions`).
pub fn set_notifier(f: impl Fn() + Send + Sync + 'static) {
    let _ = NOTIFY.set(Box::new(f));
}

pub fn enabled() -> bool {
    NOTIFY.get().is_some()
}

pub fn notify() {
    if let Some(f) = NOTIFY.get() {
        f();
    }
}
