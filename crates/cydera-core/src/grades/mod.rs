//! Moyennes CY Tech : profils (structure UE/EC d'une fiche ECTS), notes saisies, calcul.
//!
//! Profils et notes vivent dans le journal de synchro (`crate::journal`), comme les sessions
//! de révision : ils passent d'un appareil à l'autre par le moteur P2P. `Book` en est la
//! projection côté interface.

pub mod calc;
#[cfg(feature = "ects-pdf")]
pub mod ects;
pub mod model;
pub mod templates;

use crate::journal::{Event, Journal, Op};
use model::{Grade, Profile};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

/// Ordre « dernier écrit gagne » : horodatage, puis appareil et numéro (départage déterministe).
type Key<'a> = (i64, &'a str, u64);

fn key(e: &Event, ts: i64) -> Key<'_> {
    (ts, e.dev.as_str(), e.seq)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    /// Triés par nom.
    pub profiles: Vec<Profile>,
    pub grades: Vec<Grade>,
    pub active: Option<String>,
}

/// Rejoue les opérations de notes du journal. Indépendant de l'ordre d'arrivée.
pub fn project(j: &Journal) -> Projection {
    let evs = j.events();
    let mut del_p = BTreeSet::new();
    let mut del_g = BTreeSet::new();
    for e in evs {
        match &e.op {
            Op::ProfileDel { id } => {
                del_p.insert(id.as_str());
            }
            Op::GradeDel { id } => {
                del_g.insert(id.as_str());
            }
            _ => {}
        }
    }
    let mut profiles: BTreeMap<&str, (Key, &Profile)> = BTreeMap::new();
    let mut grades: BTreeMap<&str, (Key, &Grade)> = BTreeMap::new();
    let mut active: Option<(Key, &str)> = None;
    for e in evs {
        match &e.op {
            Op::ProfileSave { profile, ts } if !del_p.contains(profile.id.as_str()) => {
                let k = key(e, *ts);
                if profiles.get(profile.id.as_str()).is_none_or(|(old, _)| k > *old) {
                    profiles.insert(&profile.id, (k, profile));
                }
            }
            Op::GradeSave { grade, ts } if !del_g.contains(grade.id.as_str()) => {
                let k = key(e, *ts);
                if grades.get(grade.id.as_str()).is_none_or(|(old, _)| k > *old) {
                    grades.insert(&grade.id, (k, grade));
                }
            }
            Op::ActiveProfile { id, ts } => {
                let k = key(e, *ts);
                if active.is_none_or(|(old, _)| k > old) {
                    active = Some((k, id));
                }
            }
            _ => {}
        }
    }
    let mut out = Projection {
        profiles: profiles.into_values().map(|(_, p)| p.clone()).collect(),
        // les notes d'un profil supprimé disparaissent avec lui
        grades: grades.into_values().filter(|(_, g)| !del_p.contains(g.profile.as_str())).map(|(_, g)| g.clone()).collect(),
        active: active.map(|(_, id)| id.to_string()),
    };
    out.profiles.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    if out.active.as_ref().is_some_and(|a| !out.profiles.iter().any(|p| &p.id == a)) {
        out.active = None;
    }
    out
}

/// Horodatage des opérations (ms depuis l'époque Unix).
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Identifiant aléatoire pour un profil ou une note.
pub fn new_id() -> String {
    crate::journal::new_device_id()
}

/// Profils et notes, adossés au journal partagé avec le chrono et le moteur de synchro.
pub struct Book {
    journal: Arc<Mutex<Journal>>,
    state: Projection,
}

impl Book {
    pub fn new(journal: Arc<Mutex<Journal>>) -> Self {
        let mut b = Self { journal, state: Projection::default() };
        b.reload();
        b
    }

    fn jr(&self) -> MutexGuard<'_, Journal> {
        self.journal.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// À appeler quand la synchro a apporté des événements.
    pub fn reload(&mut self) {
        let st = project(&self.jr());
        self.state = st;
    }

    fn emit(&mut self, op: Op) {
        {
            let mut j = self.jr();
            j.emit(op);
            j.persist();
        }
        self.reload();
    }

    pub fn profiles(&self) -> &[Profile] {
        &self.state.profiles
    }

    /// Profil affiché : celui choisi, sinon le premier.
    pub fn active(&self) -> Option<&Profile> {
        let id = self.state.active.as_deref();
        self.state.profiles.iter().find(|p| Some(p.id.as_str()) == id).or(self.state.profiles.first())
    }

    pub fn grades(&self, profile: &str) -> Vec<Grade> {
        self.state.grades.iter().filter(|g| g.profile == profile).cloned().collect()
    }

    pub fn save_profile(&mut self, mut profile: Profile) {
        profile.ensure_ids();
        self.emit(Op::ProfileSave { profile, ts: now_ms() });
    }

    /// Ajoute un profil (template ou import) avec un nouvel identifiant et l'affiche.
    pub fn add_profile(&mut self, mut profile: Profile) -> String {
        profile.id = new_id();
        let id = profile.id.clone();
        self.save_profile(profile);
        self.set_active(&id);
        id
    }

    pub fn delete_profile(&mut self, id: &str) {
        self.emit(Op::ProfileDel { id: id.to_string() });
    }

    pub fn set_active(&mut self, id: &str) {
        if self.state.active.as_deref() != Some(id) {
            self.emit(Op::ActiveProfile { id: id.to_string(), ts: now_ms() });
        }
    }

    pub fn save_grade(&mut self, grade: Grade) {
        self.emit(Op::GradeSave { grade, ts: now_ms() });
    }

    pub fn delete_grade(&mut self, id: &str) {
        self.emit(Op::GradeDel { id: id.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::VersionVector;
    use model::{Ec, Semester, Ue};

    fn prof(id: &str, name: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            year: String::new(),
            semesters: vec![Semester {
                name: "S1".into(),
                ues: vec![Ue {
                    id: "s1.ue1".into(),
                    code: "UE1".into(),
                    name: "".into(),
                    ects: 1.0,
                    coef: 1.0,
                    ecs: vec![Ec { id: "s1.ue1.ec1".into(), code: "EC1".into(), name: "".into(), coef: 1.0, seuil: Some(6.0) }],
                }],
            }],
        }
    }

    fn grade(id: &str, v: f32) -> Grade {
        Grade { id: id.into(), profile: "p".into(), ec: "s1.ue1.ec1".into(), label: String::new(), value: v, out_of: 20.0, weight: 1.0 }
    }

    fn sync(a: &mut Journal, b: &mut Journal) {
        let to_b = a.since(b.version());
        let to_a = b.since(a.version());
        b.apply(to_b);
        a.apply(to_a);
    }

    #[test]
    fn edition_concurrente_dernier_ecrit_gagne_des_deux_cotes() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        pc.emit(Op::ProfileSave { profile: prof("p", "ING2"), ts: 1 });
        pc.emit(Op::GradeSave { grade: grade("g", 10.0), ts: 2 });
        sync(&mut pc, &mut tel);
        // modifications hors ligne : le téléphone écrit plus tard
        pc.emit(Op::GradeSave { grade: grade("g", 12.0), ts: 3 });
        tel.emit(Op::GradeSave { grade: grade("g", 14.0), ts: 4 });
        sync(&mut pc, &mut tel);
        assert_eq!(project(&pc), project(&tel));
        assert_eq!(project(&pc).grades[0].value, 14.0);
    }

    #[test]
    fn meme_horodatage_departage_deterministe() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        pc.emit(Op::GradeSave { grade: grade("g", 12.0), ts: 5 });
        tel.emit(Op::GradeSave { grade: grade("g", 14.0), ts: 5 });
        sync(&mut pc, &mut tel);
        assert_eq!(project(&pc), project(&tel));
    }

    #[test]
    fn suppression_gagne_et_entraine_les_notes_du_profil() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        pc.emit(Op::ProfileSave { profile: prof("p", "ING2"), ts: 1 });
        pc.emit(Op::GradeSave { grade: grade("g1", 10.0), ts: 2 });
        pc.emit(Op::GradeSave { grade: grade("g2", 11.0), ts: 2 });
        pc.emit(Op::ActiveProfile { id: "p".into(), ts: 2 });
        sync(&mut pc, &mut tel);
        tel.emit(Op::GradeDel { id: "g1".into() });
        pc.emit(Op::GradeSave { grade: grade("g1", 18.0), ts: 99 }); // modifiée après, mais supprimée ailleurs
        sync(&mut pc, &mut tel);
        let st = project(&pc);
        assert_eq!(st.grades.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["g2"]);
        pc.emit(Op::ProfileDel { id: "p".into() });
        let st = project(&pc);
        assert!(st.profiles.is_empty() && st.grades.is_empty() && st.active.is_none());
    }

    #[test]
    fn traverse_la_synchro_complete() {
        let mut pc = Journal::new("pc".into());
        pc.emit(Op::ProfileSave { profile: prof("p", "ING2 GI-a"), ts: 1 });
        pc.emit(Op::GradeSave { grade: grade("g", 15.0), ts: 2 });
        let json = serde_json::to_string(&pc.since(&VersionVector::new())).unwrap();
        let mut tel = Journal::new("tel".into());
        tel.apply(serde_json::from_str(&json).unwrap());
        assert_eq!(project(&tel), project(&pc));
        assert_eq!(project(&tel).profiles[0].name, "ING2 GI-a");
    }
}
