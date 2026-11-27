//! Statistiques de révision sur une période (jour ou semaine) : totaux, répartition par
//! matière / partie / heure, et lieu déduit de l'emploi du temps (« en cours » quand une
//! session chevauche un créneau de cours, « chez soi » sinon).
//!
//! Fonctions pures : la session en cours est passée comme une session ordinaire, les cours
//! viennent du cache de l'emploi du temps.

use crate::model::{Cours, Kind};
use crate::study::{Session, SessionKind};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use std::collections::BTreeMap;

/// Bilan d'une période `[from, to]` (bornes incluses, en jours).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Total net (chronométré + ajouté − retiré), jamais négatif.
    pub total: i64,
    /// Chronométré pendant un créneau de cours.
    pub class: i64,
    /// Chronométré hors des créneaux de cours.
    pub home: i64,
    /// Ajouté à la main (pas d'heure réelle : ni en cours ni chez soi).
    pub manual: i64,
    /// Retraits (valeur positive à soustraire).
    pub removed: i64,
    /// Nombre de sessions chronométrées (une session coupée à minuit compte pour chaque jour).
    pub sessions: usize,
    /// Plus longue session chronométrée (secondes).
    pub longest: i64,
    /// Net par matière, décroissant.
    pub by_subject: Vec<(String, i64)>,
    /// Net par partie (Cours, TD, Annale…), décroissant.
    pub by_part: Vec<(String, i64)>,
    /// Un élément par jour de la période.
    pub days: Vec<DayStat>,
    /// Chronométré par heure de la journée (0–23), toute la période.
    pub hours: [i64; 24],
    /// Jours de la période dont l'emploi du temps n'a jamais été téléchargé
    /// (leur temps chronométré est compté « chez soi » faute de mieux).
    pub no_timetable: Vec<NaiveDate>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayStat {
    pub date: NaiveDate,
    pub total: i64,
    pub class: i64,
    pub home: i64,
    pub manual: i64,
    /// Net par matière ce jour-là (ordre décroissant).
    pub by_subject: Vec<(String, i64)>,
    /// Chronométré par heure (0–23).
    pub hours: [i64; 24],
}

/// Morceau de session chronométrée sur la frise d'un jour.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub subject: String,
    pub part: String,
    /// Secondes passées pendant un cours.
    pub class: i64,
}

/// Créneaux de cours fusionnés d'un jour (les indisponibilités ne comptent pas).
pub fn course_slots(courses: &[Cours], day: NaiveDate) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut v: Vec<_> =
        courses.iter().filter(|c| c.date() == day && c.kind != Kind::Indispo && c.end > c.start).map(|c| (c.start, c.end)).collect();
    v.sort();
    let mut out: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn overlap(a: NaiveDateTime, b: NaiveDateTime, slots: &[(NaiveDateTime, NaiveDateTime)]) -> i64 {
    slots.iter().map(|(s, e)| (b.min(*e) - a.max(*s)).num_seconds().max(0)).sum()
}

fn midnight(d: NaiveDate) -> NaiveDateTime {
    d.and_hms_opt(0, 0, 0).unwrap()
}

/// Coupe une session chronométrée à minuit (la session en cours peut chevaucher deux jours).
fn split_days(s: &Session) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut out = Vec::new();
    let mut a = s.start;
    while a < s.end {
        let b = s.end.min(midnight(a.date() + Duration::days(1)));
        out.push((a, b));
        a = b;
    }
    out
}

/// Ajoute à `hours` les secondes de `[a, b]` heure par heure.
fn add_hours(hours: &mut [i64; 24], a: NaiveDateTime, b: NaiveDateTime) {
    let mut t = a;
    while t < b {
        let next = (t.date().and_hms_opt(t.hour(), 0, 0).unwrap() + Duration::hours(1)).min(b);
        hours[t.hour() as usize] += (next - t).num_seconds();
        t = next;
    }
}

fn sorted(m: BTreeMap<String, i64>) -> Vec<(String, i64)> {
    let mut v: Vec<_> = m.into_iter().filter(|(_, t)| *t > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Bilan de `[from, to]`. `has_timetable(jour)` dit si l'emploi du temps de ce jour est connu.
pub fn report(
    sessions: &[Session],
    courses: &[Cours],
    from: NaiveDate,
    to: NaiveDate,
    has_timetable: impl Fn(NaiveDate) -> bool,
) -> Report {
    let ndays = (to - from).num_days().max(0) + 1;
    let mut days: Vec<DayStat> = (0..ndays).map(|i| DayStat { date: from + Duration::days(i), ..Default::default() }).collect();
    let mut day_subj: Vec<BTreeMap<String, i64>> = vec![BTreeMap::new(); days.len()];
    let mut r = Report { from, to, ..Default::default() };
    let (mut by_subject, mut by_part) = (BTreeMap::<String, i64>::new(), BTreeMap::<String, i64>::new());
    let slots: BTreeMap<NaiveDate, _> = days.iter().map(|d| (d.date, course_slots(courses, d.date))).collect();
    let idx = |d: NaiveDate| -> Option<usize> { (d >= from && d <= to).then(|| (d - from).num_days() as usize) };

    for s in sessions {
        match s.kind {
            SessionKind::Work => {
                for (a, b) in split_days(s) {
                    let Some(i) = idx(a.date()) else { continue };
                    let secs = (b - a).num_seconds();
                    let class = overlap(a, b, &slots[&a.date()]);
                    let d = &mut days[i];
                    d.class += class;
                    d.home += secs - class;
                    add_hours(&mut d.hours, a, b);
                    *day_subj[i].entry(s.subject.clone()).or_default() += secs;
                    *by_subject.entry(s.subject.clone()).or_default() += secs;
                    *by_part.entry(s.part.clone()).or_default() += secs;
                    r.sessions += 1;
                    r.longest = r.longest.max(secs);
                }
            }
            SessionKind::Manual | SessionKind::Adjust => {
                let Some(i) = idx(s.day()) else { continue };
                if s.kind == SessionKind::Manual {
                    days[i].manual += s.secs;
                } else {
                    r.removed += -s.secs;
                }
                *day_subj[i].entry(s.subject.clone()).or_default() += s.secs;
                *by_subject.entry(s.subject.clone()).or_default() += s.secs;
                *by_part.entry(s.part.clone()).or_default() += s.secs;
            }
        }
    }

    for (d, subj) in days.iter_mut().zip(day_subj) {
        d.total = subj.values().sum::<i64>().max(0);
        d.by_subject = sorted(subj);
        r.class += d.class;
        r.home += d.home;
        r.manual += d.manual;
        for h in 0..24 {
            r.hours[h] += d.hours[h];
        }
        if d.class + d.home > 0 && !has_timetable(d.date) {
            r.no_timetable.push(d.date);
        }
    }
    r.total = days.iter().map(|d| d.total).sum();
    r.by_subject = sorted(by_subject);
    r.by_part = sorted(by_part);
    r.days = days;
    r
}

/// Morceaux chronométrés d'un jour, dans l'ordre, avec leur part pendant les cours.
pub fn blocks(sessions: &[Session], courses: &[Cours], day: NaiveDate) -> Vec<Block> {
    let slots = course_slots(courses, day);
    let mut v: Vec<Block> = sessions
        .iter()
        .filter(|s| s.kind == SessionKind::Work)
        .flat_map(|s| split_days(s).into_iter().map(move |(a, b)| (s, a, b)))
        .filter(|(_, a, _)| a.date() == day)
        .map(|(s, a, b)| Block { start: a, end: b, subject: s.subject.clone(), part: s.part.clone(), class: overlap(a, b, &slots) })
        .collect();
    v.sort_by_key(|b| b.start);
    v
}

/// Nombre de jours consécutifs avec du temps de révision net, en remontant depuis `day`.
/// Un `day` encore vide (aujourd'hui, pas encore commencé) ne casse pas la série.
pub fn streak(sessions: &[Session], day: NaiveDate) -> u32 {
    let mut per_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for s in sessions {
        if s.kind == SessionKind::Work {
            for (a, b) in split_days(s) {
                *per_day.entry(a.date()).or_default() += (b - a).num_seconds();
            }
        } else {
            *per_day.entry(s.day()).or_default() += s.secs;
        }
    }
    let worked = |d: NaiveDate| per_day.get(&d).is_some_and(|t| *t > 0);
    let mut d = if worked(day) { day } else { day - Duration::days(1) };
    let mut n = 0;
    while worked(d) {
        n += 1;
        d -= Duration::days(1);
    }
    n
}

/// Lundi de la semaine de `d`.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(d: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn work(subject: &str, a: NaiveDateTime, b: NaiveDateTime) -> Session {
        Session {
            id: format!("{a}"),
            start: a,
            end: b,
            secs: (b - a).num_seconds(),
            subject: subject.into(),
            part: "TD".into(),
            kind: SessionKind::Work,
        }
    }

    fn cours(a: NaiveDateTime, b: NaiveDateTime, kind: Kind) -> Cours {
        Cours {
            start: a,
            end: b,
            matiere: "Maths".into(),
            category: "CM".into(),
            kind,
            prof: String::new(),
            salle: String::new(),
            salle_courte: String::new(),
        }
    }

    #[test]
    fn en_cours_ou_chez_soi() {
        // cours 9h–10h30 et 10h45–12h15 (pas d'indispo comptée)
        let c = vec![
            cours(dt(24, 9, 0), dt(24, 10, 30), Kind::Cm),
            cours(dt(24, 10, 45), dt(24, 12, 15), Kind::Td),
            cours(dt(24, 14, 0), dt(24, 18, 0), Kind::Indispo),
        ];
        // 10h00–11h00 : 30 + 15 min en cours, 15 min de pause « chez soi » ; 15h–16h : chez soi (indispo)
        let s = vec![work("Algo", dt(24, 10, 0), dt(24, 11, 0)), work("Algo", dt(24, 15, 0), dt(24, 16, 0))];
        let d = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let r = report(&s, &c, d, d, |_| true);
        assert_eq!(r.class, 45 * 60);
        assert_eq!(r.home, 75 * 60);
        assert_eq!(r.total, 2 * 3600);
        assert_eq!(r.sessions, 2);
        assert_eq!(r.hours[10], 3600);
        assert_eq!(r.hours[15], 3600);
        assert!(r.no_timetable.is_empty());
    }

    #[test]
    fn ajouts_retraits_et_minuit() {
        let mut s = vec![work("Algo", dt(24, 23, 30), dt(25, 0, 30))];
        let t = dt(24, 12, 0);
        s.push(Session {
            id: "m".into(),
            start: t,
            end: t + Duration::minutes(20),
            secs: 1200,
            subject: "BDD".into(),
            part: "Cours".into(),
            kind: SessionKind::Manual,
        });
        s.push(Session {
            id: "a".into(),
            start: t,
            end: t,
            secs: -600,
            subject: "Algo".into(),
            part: "TD".into(),
            kind: SessionKind::Adjust,
        });
        let (a, b) = (NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
        let r = report(&s, &[], a, b, |d| d != a);
        assert_eq!(r.days[0].total, 30 * 60 + 1200 - 600);
        assert_eq!(r.days[1].total, 30 * 60);
        assert_eq!((r.manual, r.removed), (1200, 600));
        assert_eq!(r.total, r.class + r.home + r.manual - r.removed);
        assert_eq!(r.by_subject[0], ("Algo".to_string(), 3600 - 600));
        assert_eq!(r.no_timetable, vec![a]);
        assert_eq!(blocks(&s, &[], b).len(), 1);
        assert_eq!(streak(&s, b), 2);
        assert_eq!(streak(&s, b + Duration::days(1)), 2);
        assert_eq!(streak(&s, b + Duration::days(2)), 0);
    }
}
