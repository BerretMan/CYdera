//! Éléments propres au téléphone : barre d'onglets en bas (HIG tab-bars.md : la navigation
//! principale se fait par onglets, à portée de pouce), lignes de la feuille « Plus », pastille.

use super::motion;
use super::theme::{self, bold, mono_bold, regular};
use eframe::egui::{self, Align2, Color32, CornerRadius, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Exams,
    Rev,
    /// Notes et moyennes (icône calculatrice).
    Grades,
    Mails,
    More,
}

pub const TAB_H: f32 = 60.0;

/// Barre d'onglets. L'emploi du temps est l'écran de fond (aucun onglet actif) ; chaque onglet
/// ouvre sa fenêtre par-dessus, et toucher l'onglet actif la referme. Renvoie l'onglet touché.
pub fn tab_bar(ui: &mut egui::Ui, active: Option<Tab>, unread: usize, rev_running: bool) -> Option<Tab> {
    let tabs = [(Tab::Exams, "Examens"), (Tab::Rev, "Révisions"), (Tab::Grades, "Notes"), (Tab::Mails, "Mails"), (Tab::More, "Plus")];
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_H), Sense::hover());
    let w = rect.width() / tabs.len() as f32;
    let mut out = None;
    for (i, (tab, label)) in tabs.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, TAB_H));
        let id = ui.id().with(("tab", i));
        let resp = ui.interact(r, id, Sense::click());
        let sel = Some(*tab) == active;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, sel, *label));
        let (_, pr) = motion::press(ui.ctx(), id, resp.hovered(), resp.is_pointer_button_down_on());
        let p = ui.painter();
        let ic = pos2(r.center().x, r.top() + 20.0);
        // pastille derrière l'icône active (elle « respire » à l'appui)
        let on = motion::spring(ui.ctx(), id.with("on"), if sel { 1.0 } else { 0.0 });
        if on > 0.01 || pr > 0.01 {
            let pw = 30.0 + 26.0 * on.max(pr * 0.6);
            let pill = Rect::from_center_size(ic, vec2(pw, 30.0));
            p.rect_filled(pill, CornerRadius::same(15), theme::ACCENT.gamma_multiply(0.16 * on.max(pr)));
        }
        let col = motion::lerp_color(theme::DIM, theme::ACCENT, on);
        icon(p, ic, *tab, col);
        let lcol = motion::lerp_color(theme::MUTED, theme::TEXT, on);
        p.text(pos2(r.center().x, r.top() + 41.0), Align2::CENTER_TOP, *label, if sel { bold(11.0) } else { regular(11.0) }, lcol);
        if *tab == Tab::Mails && unread > 0 {
            let b = pos2(ic.x + 12.0, ic.y - 9.0);
            let txt = if unread > 9 { "9+".to_string() } else { unread.to_string() };
            let g = p.layout_no_wrap(txt, mono_bold(9.5), Color32::WHITE);
            let br = Rect::from_center_size(b, vec2((g.size().x + 8.0).max(16.0), 16.0));
            p.rect_filled(br, CornerRadius::same(8), theme::NOW);
            p.galley(br.center() - g.size() / 2.0, g, Color32::WHITE);
        }
        if *tab == Tab::Rev && rev_running {
            p.circle_filled(pos2(ic.x + 12.0, ic.y - 8.0), 4.0, theme::OK);
        }
        if resp.clicked() {
            out = Some(*tab);
        }
    }
    out
}

/// Pictogrammes vectoriels (aucune dépendance aux polices d'emoji).
fn icon(p: &Painter, c: Pos2, tab: Tab, col: Color32) {
    let s = Stroke::new(1.7, col);
    match tab {
        Tab::Exams => {
            // feuille d'examen : page à coin corné, lignes, coche
            let r = Rect::from_center_size(c, vec2(16.0, 20.0));
            let k = 5.0;
            let pts = vec![
                pos2(r.left(), r.top()),
                pos2(r.right() - k, r.top()),
                pos2(r.right(), r.top() + k),
                pos2(r.right(), r.bottom()),
                pos2(r.left(), r.bottom()),
                pos2(r.left(), r.top()),
            ];
            p.add(egui::Shape::line(pts, s));
            p.line_segment([pos2(r.right() - k, r.top()), pos2(r.right() - k, r.top() + k)], s);
            p.line_segment([pos2(r.right() - k, r.top() + k), pos2(r.right(), r.top() + k)], s);
            p.line_segment([pos2(r.left() + 3.5, r.top() + 6.0), pos2(r.right() - 7.0, r.top() + 6.0)], s);
            p.line_segment([pos2(r.left() + 3.5, r.top() + 10.0), pos2(r.right() - 3.5, r.top() + 10.0)], s);
            p.add(egui::Shape::line(
                vec![
                    pos2(r.left() + 4.0, r.bottom() - 5.0),
                    pos2(r.left() + 6.5, r.bottom() - 2.5),
                    pos2(r.left() + 11.0, r.bottom() - 7.5),
                ],
                s,
            ));
        }
        Tab::Rev => {
            // chronomètre : cadran, bouton, aiguille
            let cc = pos2(c.x, c.y + 1.5);
            p.circle_stroke(cc, 8.5, s);
            p.line_segment([pos2(c.x, cc.y - 8.5), pos2(c.x, cc.y - 11.0)], s);
            p.line_segment([pos2(c.x - 3.0, cc.y - 11.5), pos2(c.x + 3.0, cc.y - 11.5)], s);
            p.line_segment([pos2(cc.x + 7.0, cc.y - 7.0), pos2(cc.x + 8.8, cc.y - 8.8)], s);
            p.line_segment([cc, pos2(cc.x + 3.5, cc.y - 3.5)], s);
            p.circle_filled(cc, 1.4, col);
        }
        Tab::Grades => {
            // calculatrice : boîtier, écran, 3×2 touches
            let body = Rect::from_center_size(c, vec2(16.0, 21.0));
            p.rect_stroke(body, CornerRadius::same(3), s, StrokeKind::Middle);
            let screen = Rect::from_min_max(pos2(body.left() + 3.0, body.top() + 3.0), pos2(body.right() - 3.0, body.top() + 7.5));
            p.rect_filled(screen, CornerRadius::same(1), col);
            for row in 0..3 {
                for k in 0..2 {
                    let kc = pos2(body.left() + 5.0 + 6.0 * k as f32, body.top() + 11.0 + 3.6 * row as f32);
                    p.rect_filled(Rect::from_center_size(kc, vec2(3.2, 2.2)), CornerRadius::same(1), col);
                }
            }
        }
        Tab::Mails => {
            let r = Rect::from_center_size(c, vec2(21.0, 15.0));
            p.rect_stroke(r, CornerRadius::same(3), s, StrokeKind::Middle);
            p.line_segment([pos2(r.left() + 1.0, r.top() + 1.5), pos2(c.x, c.y + 1.5)], s);
            p.line_segment([pos2(r.right() - 1.0, r.top() + 1.5), pos2(c.x, c.y + 1.5)], s);
        }
        Tab::More => {
            for dx in [-7.0, 0.0, 7.0] {
                p.circle_filled(pos2(c.x + dx, c.y), 2.3, col);
            }
        }
    }
}

/// Ligne de la feuille « Plus » : libellé à gauche, détail à droite, chevron.
pub fn sheet_row(ui: &mut egui::Ui, title: &str, detail: &str, detail_col: Color32, enabled: bool) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    let id = ui.id().with(("sheet_row", title));
    let resp = ui.interact(r, id, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title));
    let (hv, pr) = motion::press(ui.ctx(), id, resp.hovered(), resp.is_pointer_button_down_on());
    let p = ui.painter();
    if hv.max(pr) > 0.01 {
        p.rect_filled(r, CornerRadius::same(12), motion::lerp_color(theme::CARD, theme::PRESS, pr).gamma_multiply(hv.max(pr)));
    }
    let tcol = if enabled { theme::TEXT } else { theme::DIM };
    p.text(pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, title, regular(16.0), tcol);
    // chevron dessiné
    let cx = r.right() - 16.0;
    let cs = Stroke::new(1.8, theme::DIM);
    p.line_segment([pos2(cx - 4.0, r.center().y - 6.0), pos2(cx + 1.0, r.center().y)], cs);
    p.line_segment([pos2(cx + 1.0, r.center().y), pos2(cx - 4.0, r.center().y + 6.0)], cs);
    if !detail.is_empty() {
        let g = p.layout_no_wrap(detail.to_string(), regular(13.0), detail_col);
        p.galley(pos2(r.right() - 34.0 - g.size().x, r.center().y - g.size().y / 2.0), g, detail_col);
    }
    enabled && resp.clicked()
}

/// Petit bouton en pilule (ex. « Aujourd'hui » dans l'en-tête).
pub fn pill_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), mono_bold(12.0), theme::ACCENT);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 26.0, 34.0), Sense::click());
    let (hv, pr) = motion::press(ui.ctx(), resp.id, resp.hovered(), resp.is_pointer_button_down_on());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(17), motion::lerp_color(theme::ACCENT.gamma_multiply(0.12 + 0.08 * hv), theme::PRESS, pr));
    p.rect_stroke(r, CornerRadius::same(17), Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)), StrokeKind::Inside);
    p.galley(r.center() - g.size() / 2.0, g, theme::ACCENT);
    resp
}

/// Sélecteur segmenté arrondi pleine largeur (ex. « Jour | Semaine » en haut de l'emploi du temps).
/// Renvoie l'index touché s'il diffère de `current`.
pub fn segmented(ui: &mut egui::Ui, labels: &[&str], current: usize) -> Option<usize> {
    let h = 38.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(19), theme::CARD);
    p.rect_stroke(rect, CornerRadius::same(19), Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    let n = labels.len().max(1) as f32;
    let cw = (rect.width() - 6.0) / n;
    let id = ui.id().with("segmented");
    let pos = motion::spring(ui.ctx(), id, current as f32);
    let ind = Rect::from_min_size(pos2(rect.left() + 3.0 + cw * pos, rect.top() + 3.0), vec2(cw, h - 6.0));
    p.rect_filled(ind, CornerRadius::same(16), theme::ACCENT);
    let mut out = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + 3.0 + cw * i as f32, rect.top() + 3.0), vec2(cw, h - 6.0));
        let resp = ui.interact(r, id.with(i), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, i == current, *label));
        let near = (1.0 - (pos - i as f32).abs()).clamp(0.0, 1.0);
        let col = motion::lerp_color(theme::MUTED, Color32::BLACK, near);
        p.text(r.center(), Align2::CENTER_CENTER, *label, if i == current { bold(14.0) } else { regular(14.0) }, col);
        if resp.clicked() && i != current {
            out = Some(i);
        }
    }
    out
}
