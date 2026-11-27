//! Écran « Notes » : moyennes par UE, semestre et année, saisie des notes, choix / import /
//! modification du profil (structure UE → matières de la fiche ECTS).
//!
//! Fenêtre sur PC, page plein écran sur téléphone (`super::page`). Les données vivent dans
//! `cydera_core::grades::Book` (journal synchronisé) ; ce module ne garde que l'état d'affichage.

use super::{
    mobile, page,
    theme::{self, bold, mono, mono_bold, regular},
};
use crate::files;
use cydera_core::grades::calc::{self, Alert, Report};
use cydera_core::grades::model::{Ec, Grade, Profile, Semester, Ue};
use cydera_core::grades::{Book, new_id, templates};
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke, pos2, vec2};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Formulaire d'une note : valeur sur 20 et coef (champs texte, virgule décimale acceptée).
struct NoteForm {
    value: String,
    weight: String,
    /// Note en cours de modification (sinon : ajout) ; son libellé éventuel est conservé.
    editing: Option<String>,
    label: String,
    error: Option<String>,
}

impl Default for NoteForm {
    fn default() -> Self {
        Self { value: String::new(), weight: "1".into(), editing: None, label: String::new(), error: None }
    }
}

enum Import {
    /// Lecture du PDF en cours (fil séparé).
    Busy(Receiver<Result<Vec<Profile>, String>>),
    /// Profils détectés : (à importer ?, profil modifiable).
    Preview(Vec<(bool, Profile)>),
}

#[derive(Default)]
pub struct GradesUi {
    sem: usize,
    open_ec: Option<String>,
    form: NoteForm,
    /// Brouillon du profil en cours de modification.
    edit: Option<Profile>,
    confirm_delete: bool,
    choosing: bool,
    import: Option<Import>,
    msg: Option<(String, bool)>,
}

impl GradesUi {
    /// Un sous-écran (choix, import, modification) est ouvert par-dessus les moyennes.
    pub fn in_subview(&self) -> bool {
        self.import.is_some() || self.edit.is_some() || self.choosing
    }

    /// Retour : ferme le sous-écran ouvert. Faux s'il n'y en avait pas.
    pub fn back(&mut self) -> bool {
        if self.import.is_some() {
            self.import = None;
        } else if self.edit.is_some() {
            self.edit = None;
            self.confirm_delete = false;
        } else if self.choosing {
            self.choosing = false;
        } else {
            return false;
        }
        true
    }

    /// PDF déposé sur la fenêtre (PC).
    pub fn import_bytes(&mut self, ctx: &egui::Context, bytes: Vec<u8>) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(files::parse_ects(&bytes));
            ctx.request_repaint();
        });
        self.msg = None;
        self.import = Some(Import::Busy(rx));
    }

    /// Dialogue « Ouvrir » du système puis lecture, hors du fil de l'interface.
    fn pick_and_import(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = match files::pick_pdf() {
                None => Err(String::new()), // annulé
                Some(p) => std::fs::read(&p)
                    .map_err(|e| format!("Lecture de {} impossible : {e}", p.display()))
                    .and_then(|b| files::parse_ects(&b)),
            };
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.msg = None;
        self.import = Some(Import::Busy(rx));
    }
}

/// « 12,34 »
fn fmt(v: f32) -> String {
    format!("{v:.2}").replace('.', ",")
}

fn fmt_num(v: f32) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}").replace('.', ",") }
}

fn parse(s: &str) -> Option<f32> {
    s.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Couleur d'une moyenne : vert ≥ 10, jaune ≥ 8, rouge en dessous.
fn avg_color(v: Option<f32>) -> Color32 {
    match v {
        None => theme::DIM,
        Some(x) if x >= calc::PASS => theme::OK,
        Some(x) if x >= 8.0 => theme::WARN,
        _ => theme::NOW,
    }
}

fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, touch: bool) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(vec2(0.0, if touch { 44.0 } else { 0.0 })))
}

fn wide_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, touch: bool) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(vec2(ui.available_width(), if touch { 48.0 } else { 32.0 })))
}

/// Téléphone : quand le clavier s'ouvre, la zone visible rétrécit ; on ramène le champ actif
/// dans la partie visible (sinon il reste caché sous le clavier).
fn keep_visible(ui: &egui::Ui, r: &egui::Response) {
    // défilement immédiat (pas animé) : une animation relancée à chaque changement de taille
    // pendant la montée du clavier faisait sautiller la page
    if r.has_focus() && !ui.clip_rect().contains_rect(r.rect) {
        r.scroll_to_me_animation(Some(egui::Align::Center), egui::style::ScrollAnimation::none());
    }
}

/// Champ texte qui reste visible au-dessus du clavier.
fn field(ui: &mut egui::Ui, te: egui::TextEdit<'_>) -> egui::Response {
    let r = ui.add(te);
    keep_visible(ui, &r);
    r
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(bold(11.5)).color(theme::DIM));
}

pub fn show(ctx: &egui::Context, book: &mut Book, st: &mut GradesUi, open: &mut bool) {
    if let Some(Import::Busy(rx)) = &st.import {
        match rx.try_recv() {
            Ok(Ok(ps)) => st.import = Some(Import::Preview(ps.into_iter().map(|p| (true, p)).collect())),
            Ok(Err(e)) => {
                st.import = None;
                if !e.is_empty() {
                    st.msg = Some((e, false));
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => st.import = None,
        }
    }
    let touch = page::is_page(ctx);
    let title = if st.import.is_some() {
        "Importer une fiche ECTS"
    } else if st.edit.is_some() {
        "Modifier le profil"
    } else if st.choosing {
        "Nouveau profil"
    } else {
        "Notes"
    };
    let win = egui::Window::new(RichText::new(title).font(bold(18.0)))
        .collapsible(false)
        .default_size([640.0_f32.min(ctx.content_rect().width() - 24.0), 700.0_f32.min(ctx.content_rect().height() - 48.0)]);
    // En page mobile, le retour de l'en-tête ferme d'abord le sous-écran.
    let sub = touch && st.in_subview();
    let mut keep = true;
    let max_h = (ctx.content_rect().height() - 140.0).max(300.0);
    let shown = page::show(ctx, egui::Id::new("grades"), title, win, Some(if sub { &mut keep } else { open }), true, |ui| {
        if touch {
            body(ui, ctx, book, st, touch);
        } else {
            egui::ScrollArea::vertical().id_salt("grades_scroll").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
                ui.set_width(ui.available_width());
                body(ui, ctx, book, st, touch);
            });
        }
    });
    if sub && shown.back {
        st.back();
    }
}

fn body(ui: &mut egui::Ui, ctx: &egui::Context, book: &mut Book, st: &mut GradesUi, touch: bool) {
    if let Some((m, ok)) = &st.msg {
        ui.add(egui::Label::new(RichText::new(m).color(if *ok { theme::OK } else { theme::NOW })).wrap());
        ui.add_space(6.0);
    }
    if st.import.is_some() {
        import_view(ui, book, st, touch);
    } else if st.edit.is_some() {
        editor(ui, book, st, touch);
    } else if st.choosing || book.active().is_none() {
        chooser(ui, ctx, book, st, touch);
    } else {
        overview(ui, book, st, touch);
    }
}

// ---------------------------------------------------------------- choix du profil

fn chooser(ui: &mut egui::Ui, ctx: &egui::Context, book: &mut Book, st: &mut GradesUi, touch: bool) {
    let first = book.profiles().is_empty();
    if first {
        ui.add(
            egui::Label::new(
                RichText::new("Choisis ton parcours : la liste des UE, matières et coefficients vient de la fiche ECTS de l'école.")
                    .color(theme::MUTED),
            )
            .wrap(),
        );
        ui.add_space(8.0);
    }
    section(ui, "PARCOURS PRÊTS À L'EMPLOI");
    ui.add_space(4.0);
    for (label, p) in templates::templates() {
        let n_ec: usize = p.semesters.iter().flat_map(|s| &s.ues).map(|u| u.ecs.len()).sum();
        let detail = format!("{} semestres · {} ECTS · {} matières", p.semesters.len(), fmt_num(p.ects_total()), n_ec);
        let resp = theme::card(ui, Margin::symmetric(14, 12), |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&label).font(bold(16.0)));
            ui.label(RichText::new(detail).font(regular(12.5)).color(theme::MUTED));
        })
        .response;
        let click =
            ui.interact(resp.rect, ui.id().with(("template", &label)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if click.clicked() {
            let name = p.name.clone();
            book.add_profile(p);
            st.choosing = false;
            st.sem = 0;
            st.msg = Some((format!("Profil « {name} » créé."), true));
        }
        ui.add_space(6.0);
    }
    ui.add_space(8.0);
    section(ui, "AUTRE FICHE");
    ui.add_space(4.0);
    if files::can_import() && files::can_pick() {
        if wide_button(ui, "Importer une fiche ECTS (PDF)…", touch).clicked() {
            st.pick_and_import(ctx);
        }
        ui.label(
            RichText::new("ou glisse le PDF sur la fenêtre. Le profil importé arrive ensuite sur tes autres appareils.")
                .font(regular(12.0))
                .color(theme::DIM),
        );
    } else {
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Pour importer une autre fiche ECTS, fais-le depuis Cydera sur le PC : le profil arrivera ici par la synchro.",
                )
                .font(regular(12.5))
                .color(theme::DIM),
            )
            .wrap(),
        );
    }
    ui.add_space(6.0);
    if wide_button(ui, "Profil vide (à remplir à la main)", touch).clicked() {
        let p = Profile {
            id: String::new(),
            name: "Mon profil".into(),
            year: String::new(),
            semesters: vec![
                Semester { name: "Semestre 1".into(), ues: Vec::new() },
                Semester { name: "Semestre 2".into(), ues: Vec::new() },
            ],
        };
        book.add_profile(p);
        st.choosing = false;
        st.edit = book.active().cloned();
    }
    if !first {
        ui.add_space(6.0);
        if wide_button(ui, "Annuler", touch).clicked() {
            st.choosing = false;
        }
    }
}

fn import_view(ui: &mut egui::Ui, book: &mut Book, st: &mut GradesUi, touch: bool) {
    let Some(Import::Preview(list)) = &mut st.import else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Lecture de la fiche…").color(theme::MUTED));
        });
        return;
    };
    ui.add(
        egui::Label::new(
            RichText::new(if list.len() > 1 {
                "La fiche propose plusieurs parcours (UE à choix) : un profil par parcours. Renomme-les si besoin."
            } else {
                "Vérifie le profil détecté avant de l'importer."
            })
            .color(theme::MUTED),
        )
        .wrap(),
    );
    ui.add_space(8.0);
    for (i, (on, p)) in list.iter_mut().enumerate() {
        theme::card(ui, Margin::symmetric(12, 10), |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.checkbox(on, "");
                field(ui, egui::TextEdit::singleline(&mut p.name).id_salt(("import_name", i)).desired_width(f32::INFINITY));
            });
            for s in &p.semesters {
                let ects: f32 = s.ues.iter().map(|u| u.ects).sum();
                ui.label(RichText::new(format!("{} · {} ECTS", s.name, fmt_num(ects))).font(bold(12.5)).color(theme::DIM));
                for u in &s.ues {
                    let line = format!(
                        "{} {} · coef {} · {} matière{}",
                        u.code,
                        u.name,
                        fmt_num(u.coef),
                        u.ecs.len(),
                        if u.ecs.len() > 1 { "s" } else { "" }
                    );
                    ui.add(egui::Label::new(RichText::new(line).font(regular(12.5)).color(theme::MUTED)).truncate());
                }
            }
        });
        ui.add_space(6.0);
    }
    let n = list.iter().filter(|(on, _)| *on).count();
    let mut done = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                n > 0,
                egui::Button::new(RichText::new(format!("Importer ({n})")).font(bold(14.0)))
                    .min_size(vec2(140.0, if touch { 48.0 } else { 34.0 })),
            )
            .clicked()
        {
            done = true;
        }
        if button(ui, "Annuler", touch).clicked() {
            st.import = None;
        }
    });
    if done && let Some(Import::Preview(list)) = st.import.take() {
        let mut first = None;
        for (_, p) in list.into_iter().filter(|(on, _)| *on) {
            let id = book.add_profile(p);
            first.get_or_insert(id);
        }
        if let Some(id) = first {
            book.set_active(&id);
        }
        st.choosing = false;
        st.sem = 0;
        st.msg = Some((format!("{n} profil{} importé{}.", if n > 1 { "s" } else { "" }, if n > 1 { "s" } else { "" }), true));
    }
}

// ---------------------------------------------------------------- moyennes

fn overview(ui: &mut egui::Ui, book: &mut Book, st: &mut GradesUi, touch: bool) {
    let Some(p) = book.active().cloned() else { return };
    let grades = book.grades(&p.id);
    let r = calc::report(&p, &grades);

    // profil + actions
    ui.horizontal(|ui| {
        let w = (ui.available_width() - if touch { 200.0 } else { 190.0 }).max(120.0);
        let mut pick = None;
        egui::ComboBox::from_id_salt("grades_profile").selected_text(RichText::new(&p.name).font(bold(14.0))).width(w).show_ui(ui, |ui| {
            for q in book.profiles() {
                if ui.selectable_label(q.id == p.id, &q.name).clicked() {
                    pick = Some(q.id.clone());
                }
            }
        });
        if let Some(id) = pick {
            book.set_active(&id);
            st.sem = 0;
            st.open_ec = None;
        }
        if button(ui, "Nouveau", touch).clicked() {
            st.choosing = true;
            st.msg = None;
        }
        if button(ui, "Modifier", touch).clicked() {
            st.edit = Some(p.clone());
            st.msg = None;
        }
    });
    ui.add_space(8.0);

    summary(ui, &p, &r);
    ui.add_space(10.0);

    if p.semesters.is_empty() {
        ui.label(RichText::new("Ce profil est vide : « Modifier » pour ajouter UE et matières.").color(theme::MUTED));
        return;
    }
    st.sem = st.sem.min(p.semesters.len() - 1);
    if p.semesters.len() > 1 {
        let labels: Vec<String> = p.semesters.iter().map(|s| s.name.clone()).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        if let Some(i) = mobile::segmented(ui, &refs, st.sem) {
            st.sem = i;
            st.open_ec = None;
        }
        ui.add_space(10.0);
    }
    let sem = &p.semesters[st.sem];
    if sem.ues.is_empty() {
        ui.label(RichText::new("Aucune UE dans ce semestre.").color(theme::MUTED));
    }
    for u in &sem.ues {
        ue_card(ui, book, st, &p, u, &grades, &r, touch);
        ui.add_space(8.0);
    }
    ui.label(
        RichText::new("Moyenne d'UE = matières pondérées par leur coef ; semestre et année = UE pondérées par leur coef. Une matière sous sa note seuil empêche de valider l'UE.")
            .font(regular(11.5))
            .color(theme::DIM),
    );
}

fn summary(ui: &mut egui::Ui, p: &Profile, r: &Report) {
    let mut tiles: Vec<(String, calc::Avg)> = vec![("ANNÉE".into(), r.year)];
    for (s, a) in p.semesters.iter().zip(&r.semesters) {
        tiles.push((s.name.replace("Semestre ", "S").to_uppercase(), *a));
    }
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let n = tiles.len() as f32;
        let w = ((ui.available_width() - gap * (n - 1.0)) / n - 26.0).clamp(56.0, 170.0);
        for (i, (t, a)) in tiles.iter().enumerate() {
            let fill = if i == 0 { theme::CARD_HOVER } else { theme::CARD };
            theme::card_ex(ui, fill, Stroke::new(1.0, theme::LINE), theme::RADIUS, Margin::symmetric(12, 8), |ui| {
                ui.set_width(w);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new(t).font(mono_bold(11.0)).color(theme::DIM));
                    let v = a.value.map(fmt).unwrap_or_else(|| "—".into());
                    ui.add(
                        egui::Label::new(RichText::new(v).font(mono_bold(if i == 0 { 24.0 } else { 20.0 })).color(avg_color(a.value)))
                            .truncate(),
                    );
                    let note = match (a.value, a.complete) {
                        (None, _) => "aucune note",
                        (Some(_), false) => "provisoire",
                        (Some(_), true) => "définitive",
                    };
                    ui.label(RichText::new(note).font(regular(11.0)).color(theme::DIM));
                });
            });
        }
    });
    let below = r.alerts.iter().filter(|a| matches!(a, Alert::Seuil { .. })).count();
    if below > 0 {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("{below} matière{} sous la note seuil", if below > 1 { "s" } else { "" }))
                .font(bold(13.0))
                .color(theme::NOW),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn ue_card(ui: &mut egui::Ui, book: &mut Book, st: &mut GradesUi, p: &Profile, u: &Ue, grades: &[Grade], r: &Report, touch: bool) {
    let avg = r.ue.get(&u.id).copied().unwrap_or_default();
    let below = u.ecs.iter().any(|e| r.ec_below(&e.id));
    // « validée » seulement quand toutes les matières sont notées ; une note sous le seuil se voit tout de suite
    let status = match (avg.value, avg.complete) {
        (None, _) => None,
        _ if below => Some(("sous le seuil", theme::NOW)),
        (Some(_), false) => Some(("provisoire", theme::DIM)),
        (Some(a), true) if a >= calc::PASS => Some(("validée", theme::OK)),
        (Some(_), true) => Some(("non validée", theme::NOW)),
    };
    let stroke = match status {
        Some((_, c)) if c == theme::NOW => Stroke::new(1.0, theme::NOW.gamma_multiply(0.6)),
        _ => Stroke::new(1.0, theme::LINE),
    };
    theme::card_ex(ui, theme::CARD, stroke, theme::RADIUS, Margin::symmetric(12, 10), |ui| {
        ui.set_width(ui.available_width());
        let full = ui.available_width();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width((full - 96.0).max(80.0));
                ui.label(
                    RichText::new(format!("{} · COEF {} · {} ECTS", u.code, fmt_num(u.coef), fmt_num(u.ects)))
                        .font(mono(10.5))
                        .color(theme::DIM),
                );
                ui.add(egui::Label::new(RichText::new(&u.name).font(bold(15.0))).wrap());
            });
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.label(RichText::new(avg.value.map(fmt).unwrap_or_else(|| "—".into())).font(mono_bold(20.0)).color(avg_color(avg.value)));
                if let Some((t, c)) = status {
                    ui.label(RichText::new(t).font(regular(11.0)).color(c));
                }
            });
        });
        ui.add_space(4.0);
        for e in &u.ecs {
            let open = st.open_ec.as_deref() == Some(e.id.as_str());
            if ec_row(ui, e, r.ec.get(&e.id).copied().flatten(), r.ec_below(&e.id), open, touch) {
                st.open_ec = if open { None } else { Some(e.id.clone()) };
                st.form = NoteForm::default();
            }
            if open && e.coef > 0.0 {
                notes_panel(ui, book, st, p, e, grades, touch);
            }
        }
    });
}

/// Ligne d'une matière (touchée : déplie ses notes). Renvoie vrai au clic.
fn ec_row(ui: &mut egui::Ui, e: &Ec, avg: Option<f32>, below: bool, open: bool, touch: bool) -> bool {
    let h = if touch { 46.0 } else { 32.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = ui.painter();
    if open || resp.hovered() {
        p.rect_filled(r, CornerRadius::same(3), if open { theme::CARD_HOVER } else { theme::CARD_HOVER.gamma_multiply(0.6) });
    }
    p.line_segment([pos2(r.left(), r.top()), pos2(r.right(), r.top())], Stroke::new(1.0, theme::LINE_FAINT));
    let cy = r.center().y;
    // à droite : moyenne (ou « non évalué »), coef
    let right_txt = if e.coef <= 0.0 { "non évalué".to_string() } else { avg.map(fmt).unwrap_or_else(|| "—".into()) };
    let right_font = if e.coef <= 0.0 { regular(12.0) } else { mono_bold(15.0) };
    let rg = p.layout_no_wrap(right_txt, right_font, if e.coef <= 0.0 { theme::DIM } else { avg_color(avg) });
    let rx = r.right() - 8.0 - rg.size().x;
    p.galley(pos2(rx, cy - rg.size().y / 2.0), rg, theme::TEXT);
    let cg = p.layout_no_wrap(format!("×{}", fmt_num(e.coef)), mono(11.0), theme::DIM);
    let cx = rx - 10.0 - cg.size().x;
    p.galley(pos2(cx, cy - cg.size().y / 2.0), cg, theme::DIM);
    let mut name_end = cx - 8.0;
    if below {
        let txt = format!("< {}", fmt_num(e.seuil.unwrap_or(6.0)));
        let g = p.layout_no_wrap(txt, mono_bold(10.5), Color32::WHITE);
        let pill = egui::Rect::from_min_size(pos2(name_end - g.size().x - 10.0, cy - 9.0), vec2(g.size().x + 10.0, 18.0));
        p.rect_filled(pill, CornerRadius::same(9), theme::NOW);
        p.galley(pill.center() - g.size() / 2.0, g, Color32::WHITE);
        name_end = pill.left() - 6.0;
    }
    // à gauche : code + intitulé, coupé proprement
    let clip = egui::Rect::from_min_max(pos2(r.left() + 6.0, r.top()), pos2(name_end.max(r.left() + 40.0), r.bottom()));
    let name = format!("{}  {}", e.code, e.name);
    let mut job = egui::text::LayoutJob::simple_singleline(
        name,
        regular(if touch { 14.5 } else { 13.5 }),
        if e.coef <= 0.0 { theme::DIM } else { theme::TEXT },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(clip.width());
    let g = p.layout_job(job);
    p.galley(pos2(clip.left(), cy - g.size().y / 2.0), g, theme::TEXT);
    resp.clicked()
}

/// Pastille d'une note (« 14,5 », « 12 ×2 ») ; touchée = la modifier.
fn note_chip(ui: &mut egui::Ui, g: &Grade, selected: bool, touch: bool) -> egui::Response {
    let val = if (g.out_of - 20.0).abs() > f32::EPSILON { format!("{}/{}", fmt_num(g.value), fmt_num(g.out_of)) } else { fmt_num(g.value) };
    let col = avg_color(Some(g.on20()));
    let p = ui.painter();
    let gv = p.layout_no_wrap(val, mono_bold(if touch { 16.0 } else { 14.0 }), col);
    let gw = ((g.weight - 1.0).abs() > f32::EPSILON).then(|| p.layout_no_wrap(format!("×{}", fmt_num(g.weight)), mono(11.0), theme::DIM));
    let w = gv.size().x + gw.as_ref().map_or(0.0, |x| x.size().x + 5.0) + 24.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w.max(48.0), if touch { 40.0 } else { 30.0 }), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = ui.painter();
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.18)
    } else if resp.hovered() {
        theme::CARD_HOVER
    } else {
        theme::CARD
    };
    let stroke = Stroke::new(1.0, if selected { theme::ACCENT } else { theme::LINE });
    p.rect(r, CornerRadius::same(20), fill, stroke, egui::StrokeKind::Inside);
    let total = gv.size().x + gw.as_ref().map_or(0.0, |x| x.size().x + 5.0);
    let mut x = r.center().x - total / 2.0;
    p.galley(pos2(x, r.center().y - gv.size().y / 2.0), gv.clone(), col);
    x += gv.size().x + 5.0;
    if let Some(gw) = gw {
        p.galley(pos2(x, r.center().y - gw.size().y / 2.0 + 1.0), gw, theme::DIM);
    }
    if g.label.is_empty() { resp } else { resp.on_hover_text(&g.label) }
}

fn notes_panel(ui: &mut egui::Ui, book: &mut Book, st: &mut GradesUi, p: &Profile, e: &Ec, grades: &[Grade], touch: bool) {
    egui::Frame::new().fill(theme::PANEL).corner_radius(CornerRadius::same(3)).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // notes déjà saisies : une pastille chacune
        let mine: Vec<&Grade> = grades.iter().filter(|g| g.ec == e.id).collect();
        if mine.is_empty() {
            ui.label(RichText::new("Aucune note pour l'instant.").font(regular(12.5)).color(theme::MUTED));
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for g in &mine {
                    let sel = st.form.editing.as_deref() == Some(g.id.as_str());
                    if note_chip(ui, g, sel, touch).clicked() {
                        st.form = if sel {
                            NoteForm::default()
                        } else {
                            NoteForm {
                                value: fmt_num(g.on20()),
                                weight: fmt_num(g.weight),
                                editing: Some(g.id.clone()),
                                label: g.label.clone(),
                                error: None,
                            }
                        };
                    }
                }
            });
            if st.form.editing.is_none() {
                ui.label(
                    RichText::new(if touch { "Touche une note pour la modifier." } else { "Clique sur une note pour la modifier." })
                        .font(regular(11.5))
                        .color(theme::DIM),
                );
            }
        }
        ui.add_space(6.0);

        // saisie : note /20, coef, bouton — sur une seule ligne
        let f = &mut st.form;
        let h = if touch { 44.0 } else { 30.0 }; // hauteur du bouton, alignée sur les champs
        let mut submit = false;
        let mut by_enter = false;
        let mut value_id = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let r = field(
                ui,
                egui::TextEdit::singleline(&mut f.value)
                    .id_salt(("note_v", &e.id))
                    .hint_text("Note")
                    .font(mono_bold(if touch { 18.0 } else { 15.0 }))
                    .horizontal_align(egui::Align::Center)
                    .margin(Margin::symmetric(6, if touch { 10 } else { 5 }))
                    .desired_width(if touch { 84.0 } else { 70.0 }),
            );
            by_enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            value_id = Some(r.id);
            ui.label(RichText::new("/ 20").font(mono(13.0)).color(theme::DIM));
            ui.add_space(6.0);
            ui.label(RichText::new("coef").color(theme::MUTED));
            let r = field(
                ui,
                egui::TextEdit::singleline(&mut f.weight)
                    .id_salt(("note_w", &e.id))
                    .font(mono(if touch { 16.0 } else { 14.0 }))
                    .horizontal_align(egui::Align::Center)
                    .margin(Margin::symmetric(6, if touch { 10 } else { 5 }))
                    .desired_width(if touch { 44.0 } else { 36.0 }),
            );
            by_enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let txt = if f.editing.is_some() { "Enregistrer" } else { "Ajouter" };
                let b = egui::Button::new(RichText::new(txt).font(bold(14.0)).color(Color32::BLACK))
                    .fill(theme::ACCENT)
                    .min_size(vec2(if touch { 110.0 } else { 96.0 }, h));
                submit |= ui.add(b).clicked();
            });
        });
        submit |= by_enter;
        if let Some(id) = f.editing.clone() {
            ui.horizontal(|ui| {
                if button(ui, RichText::new("Supprimer cette note").color(theme::NOW), touch).clicked() {
                    book.delete_grade(&id);
                    *f = NoteForm::default();
                }
                if button(ui, "Annuler", touch).clicked() {
                    *f = NoteForm::default();
                }
            });
        }
        if let Some(err) = &f.error {
            ui.label(RichText::new(err).font(regular(12.5)).color(theme::NOW));
        }
        if submit && !f.value.trim().is_empty() {
            match (parse(&f.value), parse(&f.weight)) {
                (Some(v), Some(w)) if (0.0..=20.0).contains(&v) && w > 0.0 => {
                    let grade = Grade {
                        id: f.editing.clone().unwrap_or_else(new_id),
                        profile: p.id.clone(),
                        ec: e.id.clone(),
                        label: f.label.clone(),
                        value: v,
                        out_of: 20.0,
                        weight: w,
                    };
                    book.save_grade(grade);
                    *f = NoteForm::default();
                    // Entrée : le curseur reste dans « Note » pour enchaîner les saisies
                    if let (true, Some(id)) = (by_enter, value_id) {
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                }
                (Some(_), Some(w)) if w > 0.0 => f.error = Some("La note doit être entre 0 et 20.".into()),
                (None, _) => f.error = Some("Note invalide (ex. 14,5).".into()),
                _ => f.error = Some("Le coef doit être positif.".into()),
            }
        }
    });
    ui.add_space(4.0);
}

// ---------------------------------------------------------------- modification du profil

enum EdAct {
    AddSem,
    DelSem(usize),
    AddUe(usize),
    DelUe(usize, usize),
    AddEc(usize, usize),
    DelEc(usize, usize, usize),
}

fn num_field(ui: &mut egui::Ui, v: &mut f32, max: f32) {
    // virgule décimale à l'affichage comme à la saisie
    ui.add(
        egui::DragValue::new(v)
            .range(0.0..=max)
            .speed(0.1)
            .max_decimals(2)
            .custom_formatter(|x, _| fmt_num(x as f32))
            .custom_parser(|t| parse(t).map(f64::from)),
    );
}

fn editor(ui: &mut egui::Ui, book: &mut Book, st: &mut GradesUi, touch: bool) {
    let Some(d) = st.edit.as_mut() else { return };
    let mut acts = Vec::new();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Nom").color(theme::MUTED));
        field(ui, egui::TextEdit::singleline(&mut d.name).id_salt("ed_name").desired_width(f32::INFINITY));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Année").color(theme::MUTED));
        field(ui, egui::TextEdit::singleline(&mut d.year).id_salt("ed_year").hint_text("2026-2027").desired_width(f32::INFINITY));
    });
    ui.add(
        egui::Label::new(
            RichText::new("Les notes déjà saisies restent attachées à leur matière. Supprimer une matière masque ses notes.")
                .font(regular(11.5))
                .color(theme::DIM),
        )
        .wrap(),
    );
    ui.add_space(8.0);
    for (si, s) in d.semesters.iter_mut().enumerate() {
        // bouton à droite d'abord : le champ prend la place qui reste (sinon la ligne déborde)
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, "Suppr. semestre", touch).clicked() {
                    acts.push(EdAct::DelSem(si));
                }
                field(ui, egui::TextEdit::singleline(&mut s.name).id_salt(("ed_sem", si)).font(bold(15.0)).desired_width(f32::INFINITY));
            });
        });
        for (ui_i, u) in s.ues.iter_mut().enumerate() {
            theme::card(ui, Margin::symmetric(10, 8), |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    field(ui, egui::TextEdit::singleline(&mut u.code).id_salt(("ed_uc", si, ui_i)).desired_width(52.0));
                    field(
                        ui,
                        egui::TextEdit::singleline(&mut u.name)
                            .id_salt(("ed_un", si, ui_i))
                            .hint_text("Intitulé de l'UE")
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ECTS").color(theme::MUTED));
                    num_field(ui, &mut u.ects, 60.0);
                    ui.label(RichText::new("coef").color(theme::MUTED));
                    num_field(ui, &mut u.coef, 100.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, "Suppr. UE", touch).clicked() {
                            acts.push(EdAct::DelUe(si, ui_i));
                        }
                    });
                });
                for (ei, e) in u.ecs.iter_mut().enumerate() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        field(ui, egui::TextEdit::singleline(&mut e.code).id_salt(("ed_ec", si, ui_i, ei)).desired_width(52.0));
                        field(
                            ui,
                            egui::TextEdit::singleline(&mut e.name)
                                .id_salt(("ed_en", si, ui_i, ei))
                                .hint_text("Matière")
                                .desired_width(f32::INFINITY),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("coef").color(theme::MUTED));
                        num_field(ui, &mut e.coef, 100.0);
                        let mut has = e.seuil.is_some();
                        if ui.checkbox(&mut has, "seuil").changed() {
                            e.seuil = has.then_some(6.0);
                        }
                        if let Some(s) = e.seuil.as_mut() {
                            num_field(ui, s, 20.0);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if button(ui, "Suppr.", touch).clicked() {
                                acts.push(EdAct::DelEc(si, ui_i, ei));
                            }
                        });
                    });
                }
                ui.add_space(4.0);
                if button(ui, "+ Matière", touch).clicked() {
                    acts.push(EdAct::AddEc(si, ui_i));
                }
            });
            ui.add_space(6.0);
        }
        if button(ui, "+ UE", touch).clicked() {
            acts.push(EdAct::AddUe(si));
        }
        ui.add_space(10.0);
    }
    if button(ui, "+ Semestre", touch).clicked() {
        acts.push(EdAct::AddSem);
    }
    for a in acts {
        match a {
            EdAct::AddSem => d.semesters.push(Semester { name: format!("Semestre {}", d.semesters.len() + 1), ues: Vec::new() }),
            EdAct::DelSem(s) => {
                d.semesters.remove(s);
            }
            EdAct::AddUe(s) => {
                let n = d.semesters[s].ues.len() + 1;
                d.semesters[s].ues.push(Ue {
                    id: String::new(),
                    code: format!("UE{n}"),
                    name: String::new(),
                    ects: 1.0,
                    coef: 1.0,
                    ecs: Vec::new(),
                });
            }
            EdAct::DelUe(s, u) => {
                d.semesters[s].ues.remove(u);
            }
            EdAct::AddEc(s, u) => {
                let ecs = &mut d.semesters[s].ues[u].ecs;
                ecs.push(Ec { id: String::new(), code: format!("EC{}", ecs.len() + 1), name: String::new(), coef: 1.0, seuil: Some(6.0) });
            }
            EdAct::DelEc(s, u, e) => {
                d.semesters[s].ues[u].ecs.remove(e);
            }
        }
    }

    ui.add_space(12.0);
    let mut save = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(RichText::new("Enregistrer").font(bold(14.0))).min_size(vec2(140.0, if touch { 48.0 } else { 34.0 })))
            .clicked()
        {
            save = true;
        }
        if button(ui, "Annuler", touch).clicked() {
            cancel = true;
        }
    });
    ui.add_space(16.0);
    if !st.confirm_delete {
        if button(ui, RichText::new("Supprimer ce profil…").color(theme::NOW), touch).clicked() {
            st.confirm_delete = true;
        }
    } else {
        ui.label(RichText::new("Supprimer le profil et toutes ses notes, sur tous tes appareils ?").color(theme::NOW));
        ui.horizontal(|ui| {
            if button(ui, RichText::new("Supprimer").color(theme::NOW).strong(), touch).clicked() {
                if let Some(d) = st.edit.take() {
                    book.delete_profile(&d.id);
                    st.msg = Some((format!("Profil « {} » supprimé.", d.name), true));
                }
                st.confirm_delete = false;
            }
            if button(ui, "Garder", touch).clicked() {
                st.confirm_delete = false;
            }
        });
    }
    if save {
        if let Some(mut d) = st.edit.take() {
            if d.name.trim().is_empty() {
                d.name = "Mon profil".into();
            }
            book.save_profile(d);
            st.msg = Some(("Profil enregistré.".into(), true));
        }
        st.confirm_delete = false;
    }
    if cancel {
        st.edit = None;
        st.confirm_delete = false;
    }
}
