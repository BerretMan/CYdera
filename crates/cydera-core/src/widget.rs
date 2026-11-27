//! Échanges avec les widgets d'écran d'accueil (Android), par fichiers dans le dossier de données :
//!
//! - `widget.json` (écrit ici) : état du chrono pour le widget « Chrono » ;
//! - `widget-edt.json` (écrit ici) : cours des prochains jours pour le widget « Emploi du temps » ;
//! - `widget-cmd/<ms>-<n>.start|pause` (écrits par le widget, Java) : appuis sur ▶/⏸ quand l'app
//!   est fermée ou en arrière-plan. Le chrono les rejoue à l'heure exacte de l'appui
//!   (`Chrono::apply_widget`), puis les supprime : le widget n'écrit jamais dans le journal.
//!
//! Les heures échangées sont des millisecondes Unix (UTC) ; l'app travaille en heure locale.

use crate::files::{dir, write_atomic};
use crate::model::{Cours, Kind};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Start,
    Pause,
}

/// État du chrono vu par le widget.
#[derive(Clone, Serialize, PartialEq, Debug, Default)]
pub struct ChronoState {
    /// Matière sélectionnée (vide : le widget ouvre l'app au lieu de lancer le chrono).
    pub subject: String,
    pub part: String,
    pub running: bool,
    /// Début de la session en cours (ms Unix), 0 à l'arrêt.
    pub start_ms: i64,
    /// Temps déjà enregistré aujourd'hui sur la matière, hors session en cours (secondes).
    pub today_secs: i64,
    /// Jour auquel `today_secs` se rapporte (« 2026-11-27 ») : périmé le lendemain.
    pub day: String,
}

#[derive(Serialize)]
struct EdtCourse {
    start_ms: i64,
    end_ms: i64,
    title: String,
    category: String,
    room: String,
    kind: &'static str,
}

pub fn to_ms(t: NaiveDateTime) -> i64 {
    Local.from_local_datetime(&t).earliest().map_or_else(|| t.and_utc().timestamp_millis(), |d| d.timestamp_millis())
}

pub fn from_ms(ms: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&Local).naive_local())
}

/// Lit et supprime les commandes en attente, dans l'ordre chronologique.
pub fn take_commands() -> Vec<(NaiveDateTime, Cmd)> {
    let Some(d) = dir().map(|d| d.join("widget-cmd")) else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&d) else { return Vec::new() };
    let mut out: Vec<(i64, String, Cmd)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let parsed = name.split_once('.').and_then(|(stem, ext)| {
            let cmd = match ext {
                "start" => Cmd::Start,
                "pause" => Cmd::Pause,
                _ => return None, // fichier temporaire en cours d'écriture
            };
            stem.split('-').next()?.parse::<i64>().ok().map(|ms| (ms, cmd))
        });
        if let Some((ms, cmd)) = parsed {
            let _ = std::fs::remove_file(e.path());
            out.push((ms, name, cmd));
        }
    }
    out.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    out.into_iter().filter_map(|(ms, _, c)| from_ms(ms).map(|t| (t, c))).collect()
}

pub fn write_chrono(st: &ChronoState) {
    if let Ok(s) = serde_json::to_string(st) {
        write_atomic("widget.json", &s);
    }
}

fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Cm => "cm",
        Kind::Td => "td",
        Kind::Tp => "tp",
        Kind::Exam => "exam",
        Kind::Indispo => "indispo",
        Kind::Autre => "autre",
    }
}

/// Cours d'aujourd'hui et des 8 jours suivants (sans les indisponibilités).
pub fn write_edt(courses: &[Cours], now: NaiveDateTime) {
    let from = now.date().and_hms_opt(0, 0, 0).unwrap();
    let to = from + chrono::Duration::days(9);
    let v: Vec<EdtCourse> = courses
        .iter()
        .filter(|c| c.end > from && c.start < to && c.kind != Kind::Indispo)
        .map(|c| EdtCourse {
            start_ms: to_ms(c.start),
            end_ms: to_ms(c.end),
            title: c.matiere.clone(),
            category: c.category.clone(),
            room: c.salle_courte.clone(),
            kind: kind_str(c.kind),
        })
        .collect();
    if let Ok(s) = serde_json::to_string(&serde_json::json!({ "written_ms": to_ms(now), "courses": v })) {
        write_atomic("widget-edt.json", &s);
    }
}
