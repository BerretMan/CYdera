pub mod agenda;
pub mod card;
pub mod day;
pub mod devices;
pub mod gesture;
pub mod grades;
pub mod mobile;
pub mod motion;
pub mod page;
pub mod revisions;
pub mod side;
pub mod stats;
pub mod telemetry;
pub mod theme;
pub mod update;
pub mod week;

use crate::model::Cours;
use chrono::{Datelike, NaiveDate};

/// Ce que l'utilisateur a demandé en interagissant avec une vue.
pub enum Action {
    Detail(Cours),
    OpenDay(NaiveDate),
    Select(NaiveDate),
    /// Ouvre la fenêtre des restos U (bouton de la pause déjeuner).
    Restos,
}

pub const JOURS_COURTS: [&str; 7] = ["LUN", "MAR", "MER", "JEU", "VEN", "SAM", "DIM"];
pub const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
pub const MOIS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

pub fn jour_court(d: NaiveDate) -> &'static str {
    JOURS_COURTS[d.weekday().num_days_from_monday() as usize]
}

pub fn jour(d: NaiveDate) -> &'static str {
    JOURS[d.weekday().num_days_from_monday() as usize]
}

pub fn mois(d: NaiveDate) -> &'static str {
    MOIS[d.month0() as usize]
}

/// « mardi 17 novembre »
pub fn date_longue(d: NaiveDate) -> String {
    format!("{} {} {}", jour(d), d.day(), mois(d))
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}
