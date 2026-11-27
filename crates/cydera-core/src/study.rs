//! Chrono de révisions : une session = un intervalle de travail sur une matière.
//!
//! Inspiré de « Chrono Semestre » (start/pause par matière et par partie, −/+ minutes,
//! reprise après crash, coupure à minuit), mais adossé aux examens de l'emploi du temps.
//! Données : `~/.local/share/cydera/revisions.json` (sessions) et `revisions-state.json`
//! (chrono en cours, checkpoint toutes les 10 s).

use crate::journal::{self, Event, Journal, Op, VersionVector};
use crate::model::now;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

pub const PARTS: [&str; 3] = ["Cours", "TD", "Annale"];
const CHECKPOINT_EVERY: u64 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    /// Chronométré.
    Work,
    /// Ajouté à la main (travail non chronométré).
    Manual,
    /// Retrait de minutes (durée négative).
    Adjust,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    /// Secondes ; négatif pour `Adjust`.
    pub secs: i64,
    pub subject: String,
    pub part: String,
    pub kind: SessionKind,
}

impl Session {
    pub fn day(&self) -> NaiveDate {
        self.start.date()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Data {
    sessions: Vec<Session>,
    /// Matières ajoutées à la main (hors examens de l'emploi du temps).
    custom: Vec<String>,
    last: Option<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct RunState {
    running: bool,
    start: NaiveDateTime,
    subject: String,
    part: String,
    checkpoint: NaiveDateTime,
}

pub struct Chrono {
    data: Data,
    /// Source de vérité pour la synchro ; `data` en est la projection (+ `last`, local).
    journal: Arc<Mutex<Journal>>,
    pub subject: Option<String>,
    pub part: String,
    start_dt: NaiveDateTime,
    started: Option<Instant>,
    last_ckpt: Instant,
    /// Dernière session supprimée (pour « Annuler »).
    pub undo: Option<Session>,
    /// Message d'info (reprise après crash…).
    pub notice: Option<String>,
}

use crate::files::{dir, write_atomic};

fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!("{:x}{:04x}", chrono::Utc::now().timestamp_micros(), N.fetch_add(1, Ordering::Relaxed) & 0xffff)
}

pub fn fmt_hms(secs: i64) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// « 2h05 » ou « 12 min ».
pub fn fmt_short(secs: i64) -> String {
    let s = secs.max(0);
    let (h, m) = (s / 3600, s % 3600 / 60);
    if h > 0 { format!("{h}h{m:02}") } else { format!("{m} min") }
}

impl Chrono {
    pub fn load() -> Self {
        let mut data: Data = dir()
            .and_then(|d| std::fs::read_to_string(d.join("revisions.json")).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let last = data.last.clone();
        let (journal, created) = Self::load_journal(&mut data);
        let mut c = Self {
            data,
            journal: Arc::new(Mutex::new(journal)),
            subject: last.as_ref().map(|l| l.0.clone()),
            part: last.map(|l| l.1).unwrap_or_else(|| PARTS[0].into()),
            start_dt: now(),
            started: None,
            last_ckpt: Instant::now(),
            undo: None,
            notice: None,
        };
        if created {
            c.save();
        }
        c.recover();
        c
    }

    /// Lit `journal.json`. Premier lancement avec une version qui a le journal : on le
    /// construit à partir des données existantes (`revisions.json`). Renvoie `true` si créé.
    fn load_journal(data: &mut Data) -> (Journal, bool) {
        let read = crate::files::read("journal.json").and_then(|s| serde_json::from_str::<Journal>(&s).ok());
        match read {
            Some(j) if !j.device.is_empty() => {
                let j = j.reindex();
                let st = j.state();
                data.sessions = st.sessions;
                data.custom = st.custom;
                (j, false)
            }
            _ => {
                let mut j = Journal::new(journal::new_device_id());
                for s in &data.sessions {
                    j.emit(Op::SessionAdd(s.clone()));
                }
                for c in &data.custom {
                    j.emit(Op::CustomAdd(c.clone()));
                }
                (j, true)
            }
        }
    }

    /// Enregistre une opération : journal (synchro) + état affiché.
    fn emit(&mut self, op: Op) {
        match &op {
            Op::SessionAdd(s) => self.data.sessions.push(s.clone()),
            Op::SessionDel { id } => self.data.sessions.retain(|s| &s.id != id),
            Op::CustomAdd(c) => self.data.custom.push(c.clone()),
            Op::CustomDel(c) => self.data.custom.retain(|x| x != c),
            _ => {} // opérations des notes : gérées par `grades::Book`
        }
        self.jr().emit(op);
    }

    fn jr(&self) -> MutexGuard<'_, Journal> {
        self.journal.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Journal partagé avec le moteur de synchro (thread réseau).
    pub fn journal_handle(&self) -> Arc<Mutex<Journal>> {
        self.journal.clone()
    }

    /// À appeler quand le moteur de synchro a intégré des événements distants :
    /// recalcule l'état affiché depuis le journal et l'enregistre.
    pub fn reload_from_journal(&mut self) {
        let st = self.jr().state();
        self.data.sessions = st.sessions;
        self.data.custom = st.custom;
        self.save();
    }

    // ---------- synchro ----------
    pub fn sync_version(&self) -> VersionVector {
        self.jr().version().clone()
    }

    /// Événements que le pair (de vecteur `theirs`) n'a pas encore.
    pub fn sync_events_since(&self, theirs: &VersionVector) -> Vec<Event> {
        self.jr().since(theirs)
    }

    /// Intègre les événements d'un pair ; renvoie le nombre d'événements nouveaux.
    pub fn sync_apply(&mut self, events: Vec<Event>) -> usize {
        let n = self.jr().apply(events);
        if n > 0 {
            self.reload_from_journal();
        }
        n
    }

    fn save(&self) {
        if let Ok(s) = serde_json::to_string(&self.data) {
            write_atomic("revisions.json", &s);
        }
        self.jr().persist();
    }

    /// Si l'app a été tuée pendant un chrono, on garde le temps jusqu'au dernier checkpoint.
    fn recover(&mut self) {
        let st: Option<RunState> =
            dir().and_then(|d| std::fs::read_to_string(d.join("revisions-state.json")).ok()).and_then(|s| serde_json::from_str(&s).ok());
        let Some(st) = st.filter(|s| s.running) else { return };
        let secs = (st.checkpoint - st.start).num_seconds();
        if secs > 0 {
            self.record(st.start, st.checkpoint, &st.subject, &st.part, SessionKind::Work);
            self.notice = Some(format!("Chrono interrompu récupéré : {} sur {}", fmt_short(secs), st.subject));
        }
        self.write_state(false);
    }

    fn write_state(&self, running: bool) {
        let st = RunState {
            running,
            start: self.start_dt,
            subject: self.subject.clone().unwrap_or_default(),
            part: self.part.clone(),
            checkpoint: now(),
        };
        if let Ok(s) = serde_json::to_string(&st) {
            write_atomic("revisions-state.json", &s);
        }
    }

    /// Enregistre `[start, end]`, coupé à minuit si besoin (une session = un jour).
    fn record(&mut self, start: NaiveDateTime, end: NaiveDateTime, subject: &str, part: &str, kind: SessionKind) {
        let mut a = start;
        while a < end {
            let midnight = (a.date() + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap();
            let b = end.min(midnight);
            let secs = (b - a).num_seconds();
            if secs > 0 {
                self.emit(Op::SessionAdd(Session {
                    id: new_id(),
                    start: a,
                    end: b,
                    secs,
                    subject: subject.into(),
                    part: part.into(),
                    kind,
                }));
            }
            a = b;
        }
        self.data.last = Some((subject.into(), part.into()));
        self.save();
    }

    // ---------- état du chrono ----------
    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    /// Durée écoulée de la session en cours.
    pub fn run_secs(&self) -> i64 {
        // Heure murale (et non `Instant`) : l'horloge monotone s'arrête quand Android
        // met le téléphone en veille, ce qui ferait perdre du temps au chrono.
        self.started.map_or(0, |_| (now() - self.start_dt).num_seconds().max(0))
    }

    /// Part de la session en cours qui tombe aujourd'hui.
    fn run_secs_today(&self) -> i64 {
        let midnight = now().date().and_hms_opt(0, 0, 0).unwrap();
        self.run_secs().min((now() - self.start_dt.max(midnight)).num_seconds().max(0))
    }

    pub fn is_running_on(&self, subject: &str) -> bool {
        self.running() && self.subject.as_deref() == Some(subject)
    }

    pub fn select(&mut self, subject: &str, part: &str, autostart: bool) {
        let was_running = self.running();
        if self.subject.as_deref() != Some(subject) || self.part != part {
            self.pause();
            self.subject = Some(subject.into());
            self.part = part.into();
            self.data.last = Some((subject.into(), part.into()));
            self.save();
        }
        if autostart || was_running {
            self.start();
        }
    }

    pub fn start(&mut self) {
        if self.running() || self.subject.is_none() {
            return;
        }
        self.start_dt = now().with_nanosecond_zero();
        self.started = Some(Instant::now());
        self.last_ckpt = Instant::now();
        self.write_state(true);
    }

    pub fn pause(&mut self) {
        if !self.running() {
            return;
        }
        let (start, end) = (self.start_dt, now().with_nanosecond_zero());
        self.started = None;
        if let Some(s) = self.subject.clone() {
            let part = self.part.clone();
            self.record(start, end, &s, &part, SessionKind::Work);
        }
        self.write_state(false);
    }

    pub fn toggle(&mut self) {
        if self.running() {
            self.pause();
        } else {
            self.start();
        }
    }

    /// À appeler à chaque frame : checkpoint périodique.
    pub fn tick(&mut self) {
        if self.running() && self.last_ckpt.elapsed().as_secs() >= CHECKPOINT_EVERY {
            self.last_ckpt = Instant::now();
            self.write_state(true);
        }
    }

    /// Ajoute du travail non chronométré (aujourd'hui, hier…). Termine à maintenant ou à 23:59:59.
    pub fn add_minutes(&mut self, minutes: i64, days_ago: i64) {
        let Some(s) = self.subject.clone() else { return };
        if minutes <= 0 {
            return;
        }
        let day = now().date() - Duration::days(days_ago);
        let midnight = day.and_hms_opt(0, 0, 0).unwrap();
        let end = if days_ago == 0 { now().with_nanosecond_zero() } else { midnight + Duration::seconds(86_399) };
        let secs = (minutes * 60).min((end - midnight).num_seconds());
        let part = self.part.clone();
        self.record(end - Duration::seconds(secs), end, &s, &part, SessionKind::Manual);
    }

    /// Retire des minutes au temps du jour de la matière sélectionnée.
    pub fn subtract_minutes(&mut self, minutes: i64) {
        let Some(s) = self.subject.clone() else { return };
        let part = self.part.clone();
        let rsec = (minutes * 60).min(self.today(&s));
        if rsec <= 0 {
            return;
        }
        let t = now().with_nanosecond_zero();
        self.emit(Op::SessionAdd(Session { id: new_id(), start: t, end: t, secs: -rsec, subject: s, part, kind: SessionKind::Adjust }));
        self.save();
    }

    pub fn delete(&mut self, id: &str) {
        if let Some(s) = self.data.sessions.iter().find(|s| s.id == id).cloned() {
            self.emit(Op::SessionDel { id: s.id.clone() });
            self.undo = Some(s);
            self.save();
        }
    }

    pub fn undo_delete(&mut self) {
        if let Some(mut s) = self.undo.take() {
            // La suppression gagne toujours sur le même `id` : on rétablit sous un nouvel `id`.
            s.id = new_id();
            self.emit(Op::SessionAdd(s));
            self.data.sessions.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
            self.save();
        }
    }

    /// Nombre de sessions enregistrées pour une matière.
    pub fn session_count(&self, subject: &str) -> usize {
        self.data.sessions.iter().filter(|s| s.subject == subject).count()
    }

    /// Supprime une matière et toutes ses sessions (synchronisé avec les autres appareils).
    pub fn delete_subject(&mut self, subject: &str) {
        if self.is_running_on(subject) {
            self.pause();
        }
        let ids: Vec<String> = self.data.sessions.iter().filter(|s| s.subject == subject).map(|s| s.id.clone()).collect();
        for id in ids {
            self.emit(Op::SessionDel { id });
        }
        self.emit(Op::CustomDel(subject.to_string()));
        if self.subject.as_deref() == Some(subject) {
            self.subject = None;
        }
        if self.data.last.as_ref().is_some_and(|l| l.0 == subject) {
            self.data.last = None;
        }
        self.undo = None;
        self.save();
    }

    pub fn add_custom(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() && !self.data.custom.iter().any(|c| c == name) {
            self.emit(Op::CustomAdd(name.to_string()));
            self.save();
        }
        if !name.is_empty() {
            let part = self.part.clone();
            self.select(name, &part, false);
        }
    }

    // ---------- totaux ----------
    pub fn sessions(&self) -> &[Session] {
        &self.data.sessions
    }

    /// Matières connues par les sessions et ajouts manuels.
    pub fn known_subjects(&self) -> Vec<String> {
        let mut v: Vec<String> = self.data.custom.clone();
        for s in &self.data.sessions {
            if !v.contains(&s.subject) {
                v.push(s.subject.clone());
            }
        }
        v
    }

    fn sum(&self, subject: Option<&str>, from: NaiveDate, to: NaiveDate) -> i64 {
        self.data
            .sessions
            .iter()
            .filter(|s| s.day() >= from && s.day() <= to && subject.is_none_or(|m| s.subject == m))
            .map(|s| s.secs)
            .sum::<i64>()
    }

    fn live(&self, subject: Option<&str>) -> i64 {
        match (&self.subject, self.running()) {
            (Some(cur), true) if subject.is_none_or(|m| m == cur) => self.run_secs_today(),
            _ => 0,
        }
    }

    pub fn today(&self, subject: &str) -> i64 {
        let d = now().date();
        (self.sum(Some(subject), d, d) + self.live(Some(subject))).max(0)
    }

    pub fn today_all(&self) -> i64 {
        let d = now().date();
        (self.sum(None, d, d) + self.live(None)).max(0)
    }

    pub fn week(&self, subject: Option<&str>) -> i64 {
        let d = now().date();
        let mon = crate::cache::monday(d);
        (self.sum(subject, mon, d) + self.live(subject)).max(0)
    }

    pub fn total(&self, subject: Option<&str>) -> i64 {
        (self.sum(subject, NaiveDate::MIN, NaiveDate::MAX) + self.live(subject)).max(0)
    }

    /// Secondes travaillées sur les `n` derniers jours (le dernier = aujourd'hui).
    pub fn last_days(&self, n: i64) -> Vec<(NaiveDate, i64)> {
        let today = now().date();
        (0..n)
            .rev()
            .map(|i| {
                let d = today - Duration::days(i);
                let live = if i == 0 { self.live(None) } else { 0 };
                (d, (self.sum(None, d, d) + live).max(0))
            })
            .collect()
    }

    /// Total par matière sur la semaine en cours, décroissant.
    pub fn week_by_subject(&self) -> Vec<(String, i64)> {
        let mut m: BTreeMap<String, i64> = BTreeMap::new();
        for s in self.known_subjects() {
            let t = self.week(Some(&s));
            if t > 0 {
                m.insert(s, t);
            }
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by_key(|(_, t)| -t);
        v
    }
}

trait ZeroNanos {
    fn with_nanosecond_zero(self) -> Self;
}
impl ZeroNanos for NaiveDateTime {
    fn with_nanosecond_zero(self) -> Self {
        use chrono::Timelike;
        self.with_nanosecond(0).unwrap_or(self)
    }
}
