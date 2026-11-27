//! Cache disque des mails : derniers en-têtes de chaque dossier et corps des messages lus
//! récemment, pour consulter la boîte hors ligne. Fichier `mail-cache.json`, lisible par
//! l'utilisateur seul ; ignoré s'il appartient à un autre compte.

use crate::mail::{Body, Folder, Header};
use serde::{Deserialize, Serialize};

const FILE: &str = "mail-cache.json";
/// Corps gardés au plus (les plus récemment ouverts).
const MAX_BODIES: usize = 60;

#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct MailCache {
    user: String,
    folders: Vec<(Folder, Vec<Header>)>,
    /// Du plus ancien au plus récent.
    bodies: Vec<(u32, Body)>,
    /// Horodatage (secondes Unix) de la dernière liste reçue, par dossier.
    #[serde(default)]
    stamps: Vec<(Folder, i64)>,
}

impl MailCache {
    /// Cache du compte `user` (vide si absent, illisible ou d'un autre compte).
    pub fn load(user: &str) -> Self {
        crate::files::read(FILE)
            .and_then(|s| serde_json::from_str::<MailCache>(&s).ok())
            .filter(|c| c.user == user)
            .unwrap_or_else(|| MailCache { user: user.to_string(), ..Default::default() })
    }

    pub fn save(&self) {
        let Ok(json) = serde_json::to_string(self) else { return };
        crate::files::write_atomic(FILE, &json);
        #[cfg(unix)]
        if let Some(d) = crate::files::dir() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(d.join(FILE), std::fs::Permissions::from_mode(0o600));
        }
    }

    pub fn headers(&self, f: Folder) -> Option<&[Header]> {
        self.folders.iter().find(|(k, _)| *k == f).map(|(_, v)| v.as_slice())
    }

    /// Âge en minutes de la dernière liste reçue pour ce dossier.
    pub fn age_min(&self, f: Folder) -> Option<i64> {
        let t = self.stamps.iter().find(|(k, _)| *k == f)?.1;
        Some((chrono::Utc::now().timestamp() - t).max(0) / 60)
    }

    pub fn set_headers(&mut self, f: Folder, h: Vec<Header>) {
        let now = chrono::Utc::now().timestamp();
        match self.stamps.iter_mut().find(|(k, _)| *k == f) {
            Some(s) => s.1 = now,
            None => self.stamps.push((f, now)),
        }
        match self.folders.iter_mut().find(|(k, _)| *k == f) {
            Some(e) => e.1 = h,
            None => self.folders.push((f, h)),
        }
    }

    /// Modifie un en-tête en cache, quel que soit son dossier.
    pub fn update_header(&mut self, uid: u32, mut f: impl FnMut(&mut Header)) {
        for (_, hs) in &mut self.folders {
            hs.iter_mut().filter(|h| h.uid == uid).for_each(&mut f);
        }
    }

    /// Retire un message de la liste d'un dossier (déplacé ailleurs).
    pub fn remove_header(&mut self, folder: Folder, uid: u32) {
        if let Some((_, hs)) = self.folders.iter_mut().find(|(k, _)| *k == folder) {
            hs.retain(|h| h.uid != uid);
        }
    }

    pub fn body(&self, uid: u32) -> Option<&Body> {
        self.bodies.iter().find(|(u, _)| *u == uid).map(|(_, b)| b)
    }

    pub fn put_body(&mut self, uid: u32, b: Body) {
        self.bodies.retain(|(u, _)| *u != uid);
        self.bodies.push((uid, b));
        if self.bodies.len() > MAX_BODIES {
            self.bodies.drain(..self.bodies.len() - MAX_BODIES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(uid: u32) -> Header {
        Header { uid, from: "x".into(), subject: "s".into(), date: "d".into(), unread: true, sender: None, to: vec![], cc: vec![], ts: 0 }
    }

    #[test]
    fn lru_et_dossiers() {
        let mut c = MailCache::default();
        for i in 0..70 {
            c.put_body(i, Body::default());
        }
        assert!(c.body(5).is_none() && c.body(69).is_some());
        c.put_body(20, Body::default());
        assert_eq!(c.bodies.last().unwrap().0, 20);
        c.set_headers(Folder::Inbox, vec![h(1), h(2)]);
        c.set_headers(Folder::Trash, vec![h(3)]);
        c.update_header(2, |x| x.unread = false);
        c.remove_header(Folder::Inbox, 1);
        assert_eq!(c.headers(Folder::Inbox).unwrap(), [Header { unread: false, ..h(2) }]);
        assert_eq!(c.age_min(Folder::Trash), Some(0));
        let back: MailCache = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
