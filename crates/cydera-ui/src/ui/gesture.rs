//! Gestes tactiles de la zone de contenu (mobile surtout, mais aussi pavé tactile / souris) :
//! - glisser à gauche / à droite : jour (ou semaine) suivant / précédent, le contenu suit le doigt ;
//! - tirer vers le bas en haut de la liste : synchroniser (« pull to refresh »).
//!
//! On lit l'entrée brute (origine de l'appui, position, vitesse) plutôt que les réponses des widgets :
//! le geste marche quel que soit l'élément sous le doigt (carte, vide, bandeau des jours), et egui
//! n'émet pas de clic quand le pointeur a bougé, donc un glissement n'ouvre jamais une carte.

use super::motion;
use super::theme::{self, mono_bold};
use eframe::egui::{self, Align2, Context, Id, LayerId, Order, Rect, Vec2, pos2};

/// Clé où les vues rangent le décalage vertical de leur `ScrollArea` (le « tirer » n'est permis qu'en haut).
pub const SCROLL_Y: &str = "content_scroll_y";

const LOCK: f32 = 14.0; // distance avant de choisir un axe
const SWIPE_DIST: f32 = 80.0; // distance suffisante pour changer de page
const FLICK_SPEED: f32 = 500.0; // ou un geste vif (pt/s) d'au moins…
const FLICK_DIST: f32 = 30.0; // …cette distance
const PULL_MAX: f32 = 110.0;
const PULL_TRIGGER: f32 = 70.0;

#[derive(Clone, Copy, Default, PartialEq)]
enum Axis {
    #[default]
    Undecided,
    Horizontal,
    Pull,
    Ignored,
}

#[derive(Clone, Copy, Default)]
struct State {
    tracking: bool,
    origin: egui::Pos2,
    axis: Axis,
    /// Décalage affiché (suit le doigt, puis revient à 0 en douceur).
    offset: Vec2,
}

pub enum Gesture {
    /// -1 = précédent, +1 = suivant.
    Swipe(i64),
    Refresh,
}

fn sid() -> Id {
    Id::new("cydera_gesture")
}

/// À appeler une fois par image, AVANT de dessiner le contenu. `area` = zone de contenu.
pub fn track(ctx: &Context, area: Rect, can_pull: bool) -> Option<Gesture> {
    let mut s: State = ctx.data(|d| d.get_temp(sid())).unwrap_or_default();
    let mut out = None;
    let scroll_top = ctx.data(|d| d.get_temp::<f32>(Id::new(SCROLL_Y))).unwrap_or(0.0) <= 1.0;
    let (pressed, down, released, origin, pos, vel) = ctx.input(|i| {
        let p = &i.pointer;
        (p.any_pressed(), p.primary_down(), p.any_released(), p.press_origin(), p.interact_pos(), p.velocity())
    });

    if pressed {
        // On ne suit que les appuis qui commencent sur le contenu lui-même (pas sur une fenêtre ou un menu).
        let on_content = origin.is_some_and(|o| area.contains(o) && ctx.layer_id_at(o).is_none_or(|l| l.order == Order::Background));
        s.tracking = on_content;
        s.origin = origin.unwrap_or_default();
        s.axis = Axis::Undecided;
    }

    if s.tracking && down {
        if let Some(p) = pos {
            let d = p - s.origin;
            if s.axis == Axis::Undecided && d.length() > LOCK {
                s.axis = if d.x.abs() > 1.3 * d.y.abs() {
                    Axis::Horizontal
                } else if can_pull && scroll_top && d.y > 0.0 && d.y > 1.3 * d.x.abs() {
                    Axis::Pull
                } else {
                    Axis::Ignored
                };
            }
            s.offset = match s.axis {
                // le contenu suit le doigt, avec un peu de résistance
                Axis::Horizontal => Vec2::new(d.x * 0.85, 0.0),
                // élastique : de plus en plus dur à tirer
                Axis::Pull => Vec2::new(0.0, PULL_MAX * (1.0 - (-d.y.max(0.0) / 160.0).exp())),
                _ => Vec2::ZERO,
            };
        }
    } else if s.tracking && (released || !down) {
        if let Some(p) = pos.or(Some(s.origin + s.offset)) {
            let d = p - s.origin;
            match s.axis {
                Axis::Horizontal => {
                    let flick = vel.x.abs() > FLICK_SPEED && d.x.abs() > FLICK_DIST && vel.x.signum() == d.x.signum();
                    if d.x.abs() > SWIPE_DIST || flick {
                        let dir = if d.x < 0.0 { 1 } else { -1 };
                        out = Some(Gesture::Swipe(dir));
                        // la nouvelle page arrive du côté d'où vient le doigt
                        s.offset = Vec2::new(dir as f32 * 60.0, 0.0);
                    }
                }
                Axis::Pull if s.offset.y >= PULL_TRIGGER => out = Some(Gesture::Refresh),
                _ => {}
            }
        }
        s.tracking = false;
        s.axis = Axis::Undecided;
    } else {
        // retour en douceur (désactivé si l'utilisateur réduit les animations)
        if motion::reduced() {
            s.offset = Vec2::ZERO;
        } else {
            let dt = ctx.input(|i| i.stable_dt).min(1.0 / 20.0);
            s.offset *= (-dt * 16.0).exp();
            if s.offset.length() < 0.5 {
                s.offset = Vec2::ZERO;
            }
        }
    }
    if s.offset != Vec2::ZERO {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(sid(), s));
    out
}

/// Décalage courant du contenu (à appliquer avec `Ui::with_visual_transform`).
pub fn offset(ctx: &Context) -> Vec2 {
    ctx.data(|d| d.get_temp::<State>(sid())).map_or(Vec2::ZERO, |s| s.offset)
}

/// Indicateur « tirer pour synchroniser », dessiné au-dessus du contenu décalé.
pub fn paint_pull(ctx: &Context, area: Rect) {
    let s: State = ctx.data(|d| d.get_temp(sid())).unwrap_or_default();
    let h = s.offset.y;
    if h < 4.0 {
        return;
    }
    let ready = h >= PULL_TRIGGER;
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("cydera_pull")));
    let y = area.top() + h / 2.0;
    let (txt, col) = if ready { ("↑ RELÂCHER POUR SYNCHRONISER", theme::ACCENT) } else { ("↓ TIRER POUR SYNCHRONISER", theme::MUTED) };
    let a = (h / PULL_TRIGGER).clamp(0.0, 1.0);
    painter.text(pos2(area.center().x, y), Align2::CENTER_CENTER, txt, mono_bold(11.5), col.gamma_multiply(a));
}
