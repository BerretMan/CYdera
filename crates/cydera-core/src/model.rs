#[cfg(test)]
use chrono::Datelike;
use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Kind {
    Cm,
    Td,
    Tp,
    Exam,
    Indispo,
    Autre,
}

impl Kind {
    pub fn from_category(cat: &str) -> Self {
        let c = cat.to_lowercase();
        if c.contains("indispo") {
            Kind::Indispo
        } else if c.contains("exam") || c.contains("contr") || c.contains("partiel") || c.contains("soutenance") {
            Kind::Exam
        } else if c.contains("tp") {
            Kind::Tp
        } else if c.contains("td") {
            Kind::Td
        } else if c.contains("cm") || c.contains("cours") {
            Kind::Cm
        } else {
            Kind::Autre
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cours {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub matiere: String,
    /// Catégorie brute renvoyée par Celcat (« CM », « TD », « Examen »…).
    pub category: String,
    pub kind: Kind,
    pub prof: String,
    /// Libellé complet de la salle.
    pub salle: String,
    /// Libellé court (« E211 »).
    pub salle_courte: String,
}

impl Cours {
    pub fn date(&self) -> NaiveDate {
        self.start.date()
    }

    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }

    pub fn label(&self) -> String {
        if self.kind == Kind::Indispo || self.category.is_empty() {
            self.matiere.clone()
        } else {
            format!("{} - {}", self.category, self.matiere)
        }
    }

    pub fn is_past(&self, now: NaiveDateTime) -> bool {
        self.end <= now
    }

    pub fn is_now(&self, now: NaiveDateTime) -> bool {
        self.start <= now && now < self.end
    }
}

pub fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Raccourcit un nom de salle Celcat : « PAU E211 SALLE POLYVALENTE (TD ET INFO) » → « E211 ».
pub fn short_room(full: &str) -> String {
    let parts: Vec<String> = full
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.split_whitespace()
                .find(|t| t.chars().any(|c| c.is_ascii_digit()))
                .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
                .unwrap_or_else(|| p.to_string())
        })
        .collect();
    if parts.is_empty() { full.to_string() } else { parts.join(", ") }
}

pub fn fmt_duration(min: i64) -> String {
    let (h, m) = (min / 60, min % 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// Bilan d'un ensemble de cours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Nombre de séances d'1 h 30 (un bloc de 3 h 15 avec pause = 2 séances).
    pub seances: i64,
    /// Temps de cours effectif, pauses internes déduites.
    pub minutes: i64,
}

const SEANCE: i64 = 90;
const PAUSE: i64 = 15;

/// Calcule le bilan en :
/// - fusionnant les cours qui se chevauchent (groupes en parallèle, ex. LV1 / Écoute :
///   on n'en suit qu'un, il ne faut pas compter deux fois le créneau) ;
/// - découpant chaque plage en séances d'1 h 30 séparées par 15 min de pause
///   (08:00–11:15 = 2 séances = 3 h).
pub fn stats<'a>(cours: impl IntoIterator<Item = &'a Cours>) -> Stats {
    let mut v: Vec<(NaiveDateTime, NaiveDateTime)> = cours.into_iter().map(|c| (c.start, c.end)).collect();
    v.sort();
    let mut merged: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();
    for (s, e) in v {
        match merged.last_mut() {
            Some(last) if s < last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    let mut st = Stats::default();
    for (s, e) in merged {
        let len = (e - s).num_minutes().max(0);
        let n = (((len + PAUSE) as f64) / (SEANCE + PAUSE) as f64).round().max(1.0) as i64;
        st.seances += n;
        st.minutes += (len - PAUSE * (n - 1)).max(0);
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(d: u32, s: &str, e: &str) -> Cours {
        let dt = |t: &str| NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_time(chrono::NaiveTime::parse_from_str(t, "%H:%M").unwrap());
        Cours {
            start: dt(s),
            end: dt(e),
            matiere: String::new(),
            category: "TD".into(),
            kind: Kind::Td,
            prof: String::new(),
            salle: String::new(),
            salle_courte: String::new(),
        }
    }

    #[test]
    fn stats_semaine_47() {
        let week = [
            // Lundi : bloc 3h15, puis LV1 / Écoute en parallèle
            c(16, "08:00", "11:15"),
            c(16, "11:30", "13:00"),
            c(16, "14:15", "15:45"),
            c(16, "14:15", "15:45"),
            c(16, "16:00", "17:30"),
            c(16, "16:00", "17:30"),
            c(17, "08:00", "09:30"),
            c(17, "09:45", "11:15"),
            c(17, "11:30", "13:00"),
            c(17, "14:15", "15:45"),
            c(17, "16:00", "17:30"),
            c(18, "08:00", "09:30"),
            c(18, "09:45", "13:00"),
            c(18, "14:15", "15:45"),
            c(18, "16:00", "17:30"),
            c(19, "08:00", "09:30"),
            c(19, "09:45", "11:15"),
            c(19, "11:30", "13:00"),
            c(20, "09:45", "13:00"),
            c(20, "14:15", "15:45"),
            c(20, "16:00", "17:30"),
        ];
        let lundi: Vec<&Cours> = week.iter().filter(|x| x.start.day() == 16).collect();
        assert_eq!(stats(lundi), Stats { seances: 5, minutes: 450 });
        assert_eq!(stats(&week), Stats { seances: 22, minutes: 22 * 90 });
    }

    #[test]
    fn stats_cas_limites() {
        assert_eq!(stats(&[c(1, "14:25", "17:40")]), Stats { seances: 2, minutes: 180 });
        assert_eq!(stats(&[c(1, "08:00", "11:00")]), Stats { seances: 2, minutes: 165 });
        assert_eq!(stats(&[c(1, "09:30", "11:30")]), Stats { seances: 1, minutes: 120 }); // examen 2 h
        assert_eq!(stats(&[c(1, "14:00", "15:00")]), Stats { seances: 1, minutes: 60 });
        assert_eq!(stats(&[]), Stats::default());
    }

    #[test]
    fn rooms() {
        assert_eq!(short_room("PAU E211 SALLE POLYVALENTE (TD ET INFO)"), "E211");
        assert_eq!(short_room("Amphi Bleu"), "Amphi Bleu");
        assert_eq!(short_room("PAU E211 SALLE, PAU E105 TP"), "E211, E105");
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::from_category("CM"), Kind::Cm);
        assert_eq!(Kind::from_category("TD"), Kind::Td);
        assert_eq!(Kind::from_category("TP"), Kind::Tp);
        assert_eq!(Kind::from_category("Examen"), Kind::Exam);
        assert_eq!(Kind::from_category("Indisponibilité"), Kind::Indispo);
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(90), "1 h 30");
        assert_eq!(fmt_duration(45), "45 min");
        assert_eq!(fmt_duration(120), "2 h");
    }
}
