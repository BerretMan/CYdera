//! Mouvement : ressorts pilotés par la vélocité, interruptibles, désactivables (Reduce Motion).
//!
//! Principes (HIG `motion.md › Let people cancel motion`, `accessibility.md › Reduce Motion`) :
//! - un changement de cible en cours de route CONSERVE la vitesse : pas de saut, pas de file d'attente ;
//! - le retour d'appui est immédiat (même frame que le `pointer-down`), le relâchement ne bloque rien ;
//! - si l'utilisateur demande moins de mouvement, tout passe directement à la valeur cible.

use eframe::egui::{Context, Id};
use std::sync::OnceLock;

/// Raideur / amortissement : ζ ≈ 0,9 (un soupçon de rebond, jamais de flottement), ~140 ms.
const STIFFNESS: f32 = 900.0;
const DAMPING: f32 = 54.0;
const SUBSTEP: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Default)]
struct Spring {
    x: f32,
    v: f32,
    init: bool,
}

/// `true` si l'utilisateur a demandé moins d'animations (`CYDERA_REDUCE_MOTION=1`
/// ou GNOME : `enable-animations=false`). Évalué une seule fois.
pub fn reduced() -> bool {
    static R: OnceLock<bool> = OnceLock::new();
    *R.get_or_init(|| {
        if std::env::var("CYDERA_REDUCE_MOTION").is_ok_and(|v| v == "1" || v == "true") {
            return true;
        }
        system_reduced()
    })
}

#[cfg(not(target_os = "android"))]
fn system_reduced() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "false")
        .unwrap_or(false)
}

#[cfg(target_os = "android")]
fn system_reduced() -> bool {
    false
}

/// Avance le ressort `id` vers `target` et renvoie sa position. Demande un nouveau rendu
/// tant que le ressort n'est pas au repos (et seulement dans ce cas).
pub fn spring(ctx: &Context, id: Id, target: f32) -> f32 {
    if reduced() {
        return target;
    }
    let key = id.with("spring");
    let mut s: Spring = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    if !s.init {
        s = Spring { x: target, v: 0.0, init: true };
    }
    let mut dt = ctx.input(|i| i.stable_dt).min(1.0 / 20.0);
    while dt > 0.0 {
        let h = dt.min(SUBSTEP);
        let a = -STIFFNESS * (s.x - target) - DAMPING * s.v;
        s.v += a * h;
        s.x += s.v * h;
        dt -= h;
    }
    if (s.x - target).abs() < 0.0015 && s.v.abs() < 0.02 {
        s.x = target;
        s.v = 0.0;
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(key, s));
    s.x
}

/// Même chose, mais en forçant la position (appui : réponse visible dans la frame même).
pub fn snap_at_least(ctx: &Context, id: Id, min: f32) {
    if reduced() {
        return;
    }
    let key = id.with("spring");
    let mut s: Spring = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    if !s.init || s.x < min {
        s = Spring { x: min, v: 0.0, init: true };
        ctx.data_mut(|d| d.insert_temp(key, s));
    }
}

/// Retour tactile d'un élément cliquable : `(survol, appui)` entre 0 et 1.
/// L'appui répond dès le `pointer-down` ; l'action, elle, reste au relâchement
/// (on peut annuler en sortant de la cible).
pub fn press(ctx: &Context, id: Id, hovered: bool, down: bool) -> (f32, f32) {
    let hid = id.with("hov");
    let pid = id.with("prs");
    if down {
        snap_at_least(ctx, pid, 0.7);
    }
    let h = spring(ctx, hid, if hovered || down { 1.0 } else { 0.0 });
    let p = spring(ctx, pid, if down { 1.0 } else { 0.0 });
    (h.clamp(0.0, 1.0), p.clamp(0.0, 1.0))
}

pub fn lerp_color(a: eframe::egui::Color32, b: eframe::egui::Color32, t: f32) -> eframe::egui::Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}
