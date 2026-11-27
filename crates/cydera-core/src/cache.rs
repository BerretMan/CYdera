use crate::model::Cours;
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Cours connus + date de dernière synchro de chaque semaine (clé = lundi).
#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub cours: Vec<Cours>,
    pub weeks: BTreeMap<NaiveDate, i64>,
}

pub fn monday(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Store {
    fn path() -> Option<PathBuf> {
        crate::platform::cache_dir().map(|d| d.join("cydera").join("cours.json"))
    }

    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }

    /// Remplace les cours de `[from, to)` par `new` et marque ces semaines comme fraîches.
    pub fn merge(&mut self, from: NaiveDate, to: NaiveDate, new: Vec<Cours>) {
        self.cours.retain(|c| c.date() < from || c.date() >= to);
        self.cours.extend(new);
        self.cours.sort_by_key(|c| c.start);
        self.cours.dedup();
        let ts = now_ms();
        let mut w = monday(from);
        while w < to {
            self.weeks.insert(w, ts);
            w += Duration::days(7);
        }
    }

    /// Âge (minutes) de la semaine contenant `d`, `None` si jamais synchronisée.
    pub fn week_age_min(&self, d: NaiveDate) -> Option<i64> {
        self.weeks.get(&monday(d)).map(|ts| (now_ms() - ts) / 60_000)
    }

    pub fn last_sync_min(&self) -> Option<i64> {
        self.weeks.values().max().map(|ts| (now_ms() - ts) / 60_000)
    }

    pub fn day(&self, d: NaiveDate) -> impl Iterator<Item = &Cours> {
        self.cours.iter().filter(move |c| c.date() == d)
    }

    pub fn has_day(&self, d: NaiveDate) -> bool {
        self.day(d).next().is_some()
    }
}
