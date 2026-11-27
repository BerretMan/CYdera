//! Écrans secondaires (examens, révisions, resto, mails, appareils…) : fenêtre flottante sur
//! ordinateur, page plein écran fixe sur téléphone (HIG : pas de fenêtres déplaçables sur mobile,
//! retour par un grand bouton en haut à gauche, cible ≥ 44 pt).
//!
//! L'application déclare chaque image la zone des pages (`set_area`) et la page à afficher par-dessus
//! les autres (`top`) : une seule page est dessinée à la fois, comme une pile de navigation.

use super::motion;
use super::theme::{self, bold};
use eframe::egui::{self, CornerRadius, Id, Rect, Sense, Stroke, pos2, vec2};

const HEADER_H: f32 = 56.0;
const BACK: f32 = 48.0;

#[derive(Clone, Copy)]
struct PageArea {
    rect: Rect,
    top: Id,
    /// Bas du fond opaque : la barre d'onglets si elle est visible, sinon le bas de l'écran
    /// (pendant que le clavier monte, la zone sous la page ne doit pas laisser voir l'agenda).
    bg_bottom: f32,
}

fn key() -> Id {
    Id::new("cydera_page_area")
}

/// Mode page (téléphone) pour cette image : `rect` = zone utile (sous la barre système, au-dessus de
/// la barre d'onglets ou du clavier), `top` = identifiant de la seule page à dessiner,
/// `bg_bottom` = jusqu'où peindre le fond. `None` = fenêtres (bureau).
pub fn set_area(ctx: &egui::Context, area: Option<(Rect, Id, f32)>) {
    let v = area.map(|(rect, top, bg_bottom)| PageArea { rect, top, bg_bottom: bg_bottom.max(rect.bottom()) });
    ctx.data_mut(|d| d.insert_temp(key(), v));
}

fn area(ctx: &egui::Context) -> Option<PageArea> {
    ctx.data(|d| d.get_temp::<Option<PageArea>>(key())).flatten()
}

/// Vrai si les écrans secondaires s'affichent en pages plein écran.
pub fn is_page(ctx: &egui::Context) -> bool {
    area(ctx).is_some()
}

/// Résultat d'un affichage.
pub struct Shown<R> {
    /// `None` si rien n'a été dessiné (fenêtre repliée, ou page cachée sous une autre).
    pub inner: Option<R>,
    /// Le bouton retour de la page a été touché.
    pub back: bool,
}

/// Affiche `add` dans `window` (bureau) ou dans une page plein écran (téléphone).
/// `open` : `Some` = l'écran se ferme (croix de la fenêtre / retour de la page) ; `None` = pas de fermeture.
/// `scroll` : en mode page, envelopper le contenu dans une zone défilante (sinon il gère la sienne).
pub fn show<R>(
    ctx: &egui::Context,
    id: Id,
    title: &str,
    window: egui::Window<'_>,
    open: Option<&mut bool>,
    scroll: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Shown<R> {
    let Some(pa) = area(ctx) else {
        let w = match open {
            Some(o) => window.open(o),
            None => window,
        };
        return Shown { inner: w.id(id).show(ctx, add).and_then(|r| r.inner), back: false };
    };
    if pa.top != id {
        return Shown { inner: None, back: false };
    }
    let closable = open.is_some();
    let rect = pa.rect;
    let mut back = false;
    let r =
        egui::Area::new(id.with("page")).order(egui::Order::Middle).fixed_pos(rect.min).movable(false).constrain(false).show(ctx, |ui| {
            // Fond opaque jusqu'en haut de l'écran (il recouvre l'en-tête de l'appli) + capture des appuis.
            let vp = ctx.viewport_rect();
            let bg = Rect::from_min_max(pos2(vp.left(), vp.top()), pos2(vp.right(), pa.bg_bottom));
            ui.set_clip_rect(bg);
            let _ = ui.interact(bg, id.with("page_bg"), Sense::click_and_drag());
            ui.painter().rect_filled(bg, CornerRadius::ZERO, theme::CARBON);

            // En-tête : retour à gauche, titre centré, filet en dessous.
            let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
            let p = ui.painter().clone();
            p.line_segment([pos2(head.left(), head.bottom()), pos2(head.right(), head.bottom())], Stroke::new(1.0, theme::LINE));
            let title_g = p.layout_no_wrap(title.to_string(), bold(18.0), theme::TEXT);
            let max_title = head.width() - 2.0 * (BACK + 12.0);
            let tpos = pos2(head.center().x - title_g.size().x.min(max_title) / 2.0, head.center().y - title_g.size().y / 2.0);
            p.with_clip_rect(Rect::from_center_size(head.center(), vec2(max_title, HEADER_H))).galley(tpos, title_g, theme::TEXT);
            if closable {
                let br = Rect::from_min_size(pos2(head.left() + 4.0, head.center().y - BACK / 2.0), vec2(BACK, BACK));
                let resp = ui.interact(br, id.with("page_back"), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Retour"));
                let (hv, pr) = motion::press(ctx, id.with("page_back_fx"), resp.hovered(), resp.is_pointer_button_down_on());
                let a = hv.max(pr);
                if a > 0.01 {
                    p.circle_filled(br.center(), BACK / 2.0, motion::lerp_color(theme::CARD_HOVER, theme::PRESS, pr).gamma_multiply(a));
                }
                // chevron « ‹ » dessiné (le glyphe manque dans la police proportionnelle)
                let c = br.center();
                let s = Stroke::new(2.4, theme::TEXT);
                p.line_segment([pos2(c.x + 4.0, c.y - 9.0), pos2(c.x - 5.0, c.y)], s);
                p.line_segment([pos2(c.x - 5.0, c.y), pos2(c.x + 4.0, c.y + 9.0)], s);
                back = resp.clicked();
            }

            // Contenu
            let body = Rect::from_min_max(pos2(rect.left() + 14.0, head.bottom() + 10.0), pos2(rect.right() - 14.0, rect.bottom() - 6.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
            child.set_clip_rect(body.expand2(vec2(14.0, 0.0)).intersect(bg));
            if scroll {
                egui::ScrollArea::vertical()
                    .id_salt(id.with("page_scroll"))
                    .auto_shrink([false, false])
                    .max_height(body.height())
                    .show(&mut child, |ui| {
                        ui.set_width(ui.available_width());
                        add(ui)
                    })
                    .inner
            } else {
                add(&mut child)
            }
        });
    if back && let Some(o) = open {
        *o = false;
    }
    Shown { inner: Some(r.inner), back }
}
