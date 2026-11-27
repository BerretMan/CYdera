//! Journal d'événements append-only : base de la synchro entre appareils.
//!
//! Chaque appareil numérote ses propres événements (`seq` = 1, 2, 3…). Le « vecteur de
//! version » (`appareil → dernier seq connu`) dit exactement ce qu'il manque à un pair :
//! la synchro se résume à échanger les événements au-delà du vecteur de l'autre.
//!
//! L'état affiché est une *projection* du journal ; elle ne dépend ni de l'ordre d'arrivée
//! ni de l'heure des appareils :
//! - une session ajoutée est visible tant qu'aucun appareil ne l'a supprimée (la suppression gagne) ;
//! - une même session reçue deux fois (même `id`) ne compte qu'une fois ;
//! - les matières personnalisées forment un ensemble (ajout seul) ;
//! - profils et notes (`crate::grades`) : registres « dernier écrit gagne » ordonnés par
//!   (`ts`, appareil), la suppression gagne ; leur projection est faite par `grades::project`.
//!
//! Compatibilité : une opération inconnue (venue d'une version plus récente) est conservée
//! telle quelle (`Op::Unknown`), retransmise aux autres pairs et ignorée à la projection.

use crate::grades::model::{Grade, Profile};
use crate::study::Session;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type DeviceId = String;
pub type VersionVector = BTreeMap<DeviceId, u64>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    SessionAdd(Session),
    SessionDel {
        id: String,
    },
    CustomAdd(String),
    /// Suppression d'une matière ajoutée à la main (ses sessions sont supprimées à part).
    CustomDel(String),
    /// Création ou nouvelle version d'un profil de notes (structure UE/EC complète).
    ProfileSave {
        profile: Profile,
        ts: i64,
    },
    ProfileDel {
        id: String,
    },
    /// Création ou modification d'une note.
    GradeSave {
        grade: Grade,
        ts: i64,
    },
    GradeDel {
        id: String,
    },
    /// Profil affiché (le même sur tous les appareils).
    ActiveProfile {
        id: String,
        ts: i64,
    },
    /// Opération d'une version plus récente de l'appli : gardée et retransmise, jamais interprétée.
    #[serde(untagged)]
    Unknown(serde_json::Value),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub dev: DeviceId,
    pub seq: u64,
    pub op: Op,
}

/// État obtenu en rejouant le journal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub sessions: Vec<Session>,
    pub custom: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Journal {
    pub device: DeviceId,
    events: Vec<Event>,
    #[serde(skip)]
    vv: VersionVector,
}

/// Identifiant aléatoire (128 bits, hexadécimal) propre à cette installation.
pub fn new_device_id() -> DeviceId {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("générateur aléatoire du système");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Journal {
    pub fn new(device: DeviceId) -> Self {
        Self { device, events: Vec::new(), vv: VersionVector::new() }
    }

    /// Recalcule le vecteur de version (après lecture du disque).
    pub fn reindex(mut self) -> Self {
        self.events.sort_by(|a, b| (&a.dev, a.seq).cmp(&(&b.dev, b.seq)));
        self.events.dedup_by(|a, b| a.dev == b.dev && a.seq == b.seq);
        self.vv.clear();
        // On ne garde que les suites contiguës 1..n par appareil.
        let mut kept = Vec::with_capacity(self.events.len());
        for e in self.events.drain(..) {
            let have = self.vv.get(&e.dev).copied().unwrap_or(0);
            if e.seq == have + 1 {
                self.vv.insert(e.dev.clone(), e.seq);
                kept.push(e);
            }
        }
        self.events = kept;
        self
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn version(&self) -> &VersionVector {
        &self.vv
    }

    /// Tous les événements connus (ordre de stockage, pas forcément trié).
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Enregistre `journal.json` dans le dossier de données.
    pub fn persist(&self) {
        if let Ok(s) = serde_json::to_string(self) {
            crate::files::write_atomic("journal.json", &s);
        }
    }

    /// Enregistre une opération locale.
    pub fn emit(&mut self, op: Op) {
        let seq = self.vv.get(&self.device).copied().unwrap_or(0) + 1;
        self.vv.insert(self.device.clone(), seq);
        self.events.push(Event { dev: self.device.clone(), seq, op });
    }

    /// Événements que le pair (décrit par `theirs`) n'a pas encore.
    pub fn since(&self, theirs: &VersionVector) -> Vec<Event> {
        self.events.iter().filter(|e| e.seq > theirs.get(&e.dev).copied().unwrap_or(0)).cloned().collect()
    }

    /// Intègre des événements reçus d'un pair. Renvoie le nombre d'événements nouveaux.
    ///
    /// Idempotent. Tolère le désordre et les doublons. Un événement n'est accepté que s'il
    /// prolonge exactement la suite connue de son appareil ; un trou est ignoré (le prochain
    /// échange le comblera). Les événements prétendant venir de *cet* appareil sont refusés :
    /// un pair ne peut pas écrire à notre place.
    pub fn apply(&mut self, mut incoming: Vec<Event>) -> usize {
        incoming.sort_by(|a, b| (&a.dev, a.seq).cmp(&(&b.dev, b.seq)));
        let mut added = 0;
        for e in incoming {
            if e.dev == self.device {
                continue;
            }
            let have = self.vv.get(&e.dev).copied().unwrap_or(0);
            if e.seq == have + 1 {
                self.vv.insert(e.dev.clone(), e.seq);
                self.events.push(e);
                added += 1;
            }
        }
        added
    }

    /// Rejoue le journal.
    pub fn state(&self) -> State {
        let mut evs: Vec<&Event> = self.events.iter().collect();
        evs.sort_by(|a, b| (&a.dev, a.seq).cmp(&(&b.dev, b.seq)));
        let deleted: BTreeSet<&str> = evs
            .iter()
            .filter_map(|e| match &e.op {
                Op::SessionDel { id } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut st = State::default();
        for e in evs {
            match &e.op {
                Op::SessionAdd(s) if !deleted.contains(s.id.as_str()) && seen.insert(s.id.clone()) => st.sessions.push(s.clone()),
                Op::CustomAdd(c) if !st.custom.contains(c) => st.custom.push(c.clone()),
                Op::CustomDel(c) => st.custom.retain(|x| x != c),
                _ => {}
            }
        }
        st.sessions.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::SessionKind;
    use chrono::NaiveDate;

    fn sess(id: &str, h: u32) -> Session {
        let d = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        Session {
            id: id.into(),
            start: d.and_hms_opt(h, 0, 0).unwrap(),
            end: d.and_hms_opt(h + 1, 0, 0).unwrap(),
            secs: 3600,
            subject: "Maths".into(),
            part: "Cours".into(),
            kind: SessionKind::Work,
        }
    }

    fn ids(j: &Journal) -> Vec<String> {
        j.state().sessions.into_iter().map(|s| s.id).collect()
    }

    /// Échange complet dans les deux sens, comme le fera le protocole réseau.
    fn sync(a: &mut Journal, b: &mut Journal) {
        let to_b = a.since(b.version());
        let to_a = b.since(a.version());
        b.apply(to_b);
        a.apply(to_a);
    }

    #[test]
    fn ajouts_hors_ligne_des_deux_cotes() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        pc.emit(Op::SessionAdd(sess("a", 8)));
        pc.emit(Op::SessionAdd(sess("b", 10)));
        tel.emit(Op::SessionAdd(sess("c", 9)));
        tel.emit(Op::CustomAdd("Anglais".into()));
        sync(&mut pc, &mut tel);
        assert_eq!(ids(&pc), ["a", "c", "b"]);
        assert_eq!(pc.state(), tel.state());
        assert_eq!(pc.state().custom, ["Anglais"]);
    }

    #[test]
    fn suppression_gagne_meme_si_elle_arrive_avant_l_ajout() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        pc.emit(Op::SessionAdd(sess("a", 8)));
        sync(&mut pc, &mut tel);
        // Le téléphone supprime pendant que le PC, hors ligne, ajoute autre chose.
        tel.emit(Op::SessionDel { id: "a".into() });
        pc.emit(Op::SessionAdd(sess("b", 10)));
        sync(&mut pc, &mut tel);
        assert_eq!(ids(&pc), ["b"]);
        assert_eq!(pc.state(), tel.state());
    }

    #[test]
    fn idempotent_et_tolerant_au_desordre() {
        let mut pc = Journal::new("pc".into());
        for i in 0..5 {
            pc.emit(Op::SessionAdd(sess(&format!("s{i}"), 8 + i)));
        }
        let mut evs = pc.since(&VersionVector::new());
        evs.reverse();
        let mut tel = Journal::new("tel".into());
        assert_eq!(tel.apply(evs.clone()), 5);
        assert_eq!(tel.apply(evs), 0);
        assert_eq!(tel.state(), pc.state());
    }

    #[test]
    fn un_trou_est_ignore_puis_comble() {
        let mut pc = Journal::new("pc".into());
        for i in 0..3 {
            pc.emit(Op::SessionAdd(sess(&format!("s{i}"), 8 + i)));
        }
        let all = pc.since(&VersionVector::new());
        let mut tel = Journal::new("tel".into());
        assert_eq!(tel.apply(vec![all[1].clone(), all[2].clone()]), 0);
        assert_eq!(tel.apply(all), 3);
    }

    #[test]
    fn refuse_les_evenements_forges_a_notre_nom() {
        let mut tel = Journal::new("tel".into());
        tel.emit(Op::CustomAdd("Reel".into()));
        let forge = Event { dev: "tel".into(), seq: 2, op: Op::CustomAdd("Faux".into()) };
        assert_eq!(tel.apply(vec![forge]), 0);
        assert_eq!(tel.state().custom, ["Reel"]);
    }

    #[test]
    fn meme_session_recue_de_deux_appareils_compte_une_fois() {
        let (mut pc, mut tel) = (Journal::new("pc".into()), Journal::new("tel".into()));
        // Migration du même fichier historique sur les deux appareils.
        pc.emit(Op::SessionAdd(sess("old", 8)));
        tel.emit(Op::SessionAdd(sess("old", 8)));
        sync(&mut pc, &mut tel);
        assert_eq!(ids(&pc), ["old"]);
        assert_eq!(pc.state(), tel.state());
    }

    #[test]
    fn trois_appareils_convergent_quel_que_soit_l_ordre() {
        let mut j = [Journal::new("a".into()), Journal::new("b".into()), Journal::new("c".into())];
        j[0].emit(Op::SessionAdd(sess("x", 8)));
        j[1].emit(Op::SessionAdd(sess("y", 9)));
        j[2].emit(Op::SessionAdd(sess("z", 10)));
        j[1].emit(Op::SessionDel { id: "x".into() });
        for (i, k) in [(0, 1), (1, 2), (2, 0), (0, 2), (1, 0)] {
            let (a, b) = if i < k {
                let (l, r) = j.split_at_mut(k);
                (&mut l[i], &mut r[0])
            } else {
                let (l, r) = j.split_at_mut(i);
                (&mut r[0], &mut l[k])
            };
            sync(a, b);
        }
        assert_eq!(j[0].state(), j[1].state());
        assert_eq!(j[1].state(), j[2].state());
        assert_eq!(ids(&j[0]), ["y", "z"]);
    }

    #[test]
    fn operation_inconnue_conservee_retransmise_et_ignoree() {
        // Événement d'une version future de l'appli (variante que l'on ne connaît pas).
        let json = r#"{"dev":"futur","seq":1,"op":{"NouveauTruc":{"x":1}}}"#;
        let e: Event = serde_json::from_str(json).unwrap();
        assert!(matches!(e.op, Op::Unknown(_)));
        let mut tel = Journal::new("tel".into());
        assert_eq!(tel.apply(vec![e.clone()]), 1);
        assert_eq!(tel.state(), State::default());
        // retransmis à l'identique à un troisième appareil
        let fwd = tel.since(&VersionVector::new());
        assert_eq!(serde_json::to_string(&fwd[0]).unwrap(), serde_json::to_string(&e).unwrap());
        // et le journal se relit depuis le disque
        let back: Journal = serde_json::from_str::<Journal>(&serde_json::to_string(&tel).unwrap()).unwrap().reindex();
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn ancien_journal_toujours_lisible() {
        let json = r#"{"device":"pc","events":[{"dev":"pc","seq":1,"op":{"CustomAdd":"Maths"}}]}"#;
        let j: Journal = serde_json::from_str::<Journal>(json).unwrap().reindex();
        assert_eq!(j.state().custom, ["Maths"]);
    }

    #[test]
    fn reindex_apres_lecture_disque() {
        let mut pc = Journal::new("pc".into());
        pc.emit(Op::CustomAdd("A".into()));
        pc.emit(Op::CustomAdd("B".into()));
        let json = serde_json::to_string(&pc).unwrap();
        let back: Journal = serde_json::from_str::<Journal>(&json).unwrap().reindex();
        assert_eq!(back.version().get("pc"), Some(&2));
        assert_eq!(back.state().custom, ["A", "B"]);
    }

    #[test]
    fn matiere_supprimee_puis_rajoutee() {
        let mut pc = Journal::new("pc".into());
        pc.emit(Op::CustomAdd("Maths".into()));
        pc.emit(Op::CustomDel("Maths".into()));
        assert!(pc.state().custom.is_empty());
        pc.emit(Op::CustomAdd("Maths".into()));
        assert_eq!(pc.state().custom, ["Maths"]);
    }
}
