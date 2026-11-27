//! Mises à jour : dernière version publiée sur GitHub Releases, téléchargement et, sur PC,
//! remplacement de l'exécutable en place.
//!
//! Les assets d'une release ont des noms fixes (`cydera.apk`, `cydera-linux-x86_64.tar.gz`,
//! `SHA256SUMS`) : voir `.github/workflows/release.yml`.
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Dépôt GitHub public qui publie les releases.
pub const DEPOT: &str = "BerretMan/CYdera";
pub const ASSET_APK: &str = "cydera.apk";
pub const ASSET_LINUX: &str = "cydera-linux-x86_64.tar.gz";
pub const ASSET_SOMMES: &str = "SHA256SUMS";

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub nom: String,
    pub url: String,
    pub taille: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: semver::Version,
    pub tag: String,
    /// Notes de version (Markdown brut de GitHub).
    pub notes: String,
    /// Page web de la release.
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, nom: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.nom == nom)
    }

    /// Paquet à installer sur cette plateforme (APK sur Android, archive sur Linux).
    pub fn paquet(&self) -> Option<&Asset> {
        self.asset(if cfg!(target_os = "android") { ASSET_APK } else { ASSET_LINUX })
    }
}

static SIMULEE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Simule une version installée plus ancienne (tests ; sur Android, extra `update_from` de l'intent).
pub fn simuler_version(v: String) {
    let _ = SIMULEE.set(v);
}

/// Version de l'appli en cours. `CYDERA_UPDATE_FROM` (ou `simuler_version`) simule une version
/// plus ancienne pour tester tout le parcours sur une vraie release.
pub fn version_actuelle() -> String {
    SIMULEE
        .get()
        .cloned()
        .or_else(|| std::env::var("CYDERA_UPDATE_FROM").ok())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Choisit, dans la réponse JSON de `GET /repos/…/releases`, la version la plus récente
/// strictement supérieure à `actuelle`. Les brouillons sont ignorés ; les pré-versions ne sont
/// proposées qu'à une pré-version (une beta reçoit les betas, une stable seulement des stables).
pub fn choisir(json: &str, actuelle: &str) -> Result<Option<Release>, String> {
    let actuelle =
        semver::Version::parse(actuelle.trim_start_matches('v')).map_err(|e| format!("Version actuelle « {actuelle} » invalide : {e}"))?;
    let liste: Vec<ApiRelease> = serde_json::from_str(json).map_err(|e| format!("Réponse GitHub illisible : {e}"))?;
    let beta = !actuelle.pre.is_empty();
    Ok(liste
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| semver::Version::parse(r.tag_name.trim_start_matches('v')).ok().map(|v| (v, r)))
        .filter(|(v, _)| (beta || v.pre.is_empty()) && *v > actuelle)
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(version, r)| Release {
            version,
            tag: r.tag_name,
            notes: r.body.unwrap_or_default(),
            page: r.html_url,
            assets: r.assets.into_iter().map(|a| Asset { nom: a.name, url: a.browser_download_url, taille: a.size }).collect(),
        }))
}

fn client(timeout_total: Option<Duration>) -> Result<reqwest::Client, String> {
    let b = crate::net::client_builder()
        .user_agent(concat!("Cydera/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30));
    let b = match timeout_total {
        Some(t) => b.timeout(t),
        None => b,
    };
    b.build().map_err(|e| format!("Client HTTP : {e}"))
}

async fn get(url: &str, timeout_total: Option<Duration>) -> Result<reqwest::Response, String> {
    let r = client(timeout_total)?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| format!("GitHub injoignable : {e}"))?;
    match r.status().as_u16() {
        200..=299 => Ok(r),
        403 | 429 => Err("Trop de requêtes vers GitHub : réessaie dans une heure.".into()),
        404 => Err("Aucune release trouvée sur GitHub.".into()),
        s => Err(format!("Réponse GitHub inattendue (HTTP {s}).")),
    }
}

/// Interroge GitHub : `Some(release)` si une version plus récente que `actuelle` est publiée.
pub async fn derniere(actuelle: &str) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{DEPOT}/releases?per_page=10");
    let texte = get(&url, Some(Duration::from_secs(20))).await?.text().await.map_err(|e| format!("Lecture de la réponse GitHub : {e}"))?;
    choisir(&texte, actuelle)
}

/// Télécharge `url` vers `dest` (via `dest.part`, renommé à la fin).
/// `progres(reçus, total)` est appelé à chaque morceau.
pub async fn telecharger(url: &str, dest: &Path, progres: impl Fn(u64, Option<u64>)) -> Result<(), String> {
    let mut r = get(url, None).await?;
    let total = r.content_length();
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Création de {} : {e}", dir.display()))?;
    }
    let part = dest.with_extension("part");
    let mut f = std::fs::File::create(&part).map_err(|e| format!("Écriture de {} : {e}", part.display()))?;
    let mut recus = 0u64;
    while let Some(bloc) = r.chunk().await.map_err(|e| format!("Téléchargement interrompu : {e}"))? {
        f.write_all(&bloc).map_err(|e| format!("Écriture de {} : {e}", part.display()))?;
        recus += bloc.len() as u64;
        progres(recus, total);
    }
    f.sync_all().map_err(|e| format!("Écriture de {} : {e}", part.display()))?;
    drop(f);
    if total.is_some_and(|t| t != recus) {
        let _ = std::fs::remove_file(&part);
        return Err("Téléchargement incomplet : réessaie.".into());
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Renommage de {} : {e}", part.display()))
}

/// Texte d'un petit fichier distant (ex. `SHA256SUMS`).
pub async fn texte(url: &str) -> Result<String, String> {
    get(url, Some(Duration::from_secs(30))).await?.text().await.map_err(|e| format!("Lecture de {url} : {e}"))
}

/// Somme SHA-256 attendue pour `nom` dans un fichier au format de `sha256sum`.
pub fn somme_attendue<'a>(sommes: &'a str, nom: &str) -> Option<&'a str> {
    sommes.lines().find_map(|l| {
        let (somme, fichier) = l.trim().split_once(char::is_whitespace)?;
        (fichier.trim_start().trim_start_matches('*') == nom).then_some(somme)
    })
}

#[cfg(not(target_os = "android"))]
pub use pc::*;

/// Installation sur PC : vérification SHA-256, extraction du binaire, remplacement atomique.
#[cfg(not(target_os = "android"))]
mod pc {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    pub fn sha256_fichier(p: &Path) -> Result<String, String> {
        let mut f = std::fs::File::open(p).map_err(|e| format!("Lecture de {} : {e}", p.display()))?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h).map_err(|e| format!("Lecture de {} : {e}", p.display()))?;
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Vérifie `fichier` contre l'entrée `nom` de `sommes`.
    pub fn verifier(fichier: &Path, sommes: &str, nom: &str) -> Result<(), String> {
        let attendue = somme_attendue(sommes, nom).ok_or_else(|| format!("{nom} absent de {ASSET_SOMMES}"))?;
        let obtenue = sha256_fichier(fichier)?;
        if obtenue.eq_ignore_ascii_case(attendue) {
            Ok(())
        } else {
            Err("Fichier téléchargé corrompu (somme SHA-256 différente) : réessaie.".into())
        }
    }

    /// Exécutable en cours. Après un premier remplacement, Linux suffixe l'ancien chemin de
    /// « (deleted) » : on le retire pour viser le nouveau fichier.
    pub fn exe_actuel() -> Result<PathBuf, String> {
        let p = std::env::current_exe().map_err(|e| format!("Exécutable introuvable : {e}"))?;
        let s = p.to_string_lossy();
        Ok(match s.strip_suffix(" (deleted)") {
            Some(v) => PathBuf::from(v),
            None => p,
        })
    }

    /// Extrait le binaire `cydera` de l'archive et remplace `cible` de façon atomique
    /// (fichier temporaire dans le même dossier puis `rename`, permis même si `cible` tourne).
    pub fn installer_archive(archive: &Path, cible: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        let dir = cible.parent().ok_or("Dossier de l'exécutable introuvable")?;
        let tmp = dir.join(".cydera.new");
        let f = std::fs::File::open(archive).map_err(|e| format!("Lecture de {} : {e}", archive.display()))?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(f));
        let mut trouve = false;
        for entree in tar.entries().map_err(|e| format!("Archive illisible : {e}"))? {
            let mut entree = entree.map_err(|e| format!("Archive illisible : {e}"))?;
            let chemin = entree.path().map_err(|e| format!("Archive illisible : {e}"))?.into_owned();
            if !entree.header().entry_type().is_file() || chemin.file_name().is_none_or(|n| n != "cydera") {
                continue;
            }
            let mut out = std::fs::File::create(&tmp).map_err(|e| impossible(dir, e))?;
            std::io::copy(&mut entree, &mut out).map_err(|e| impossible(dir, e))?;
            out.sync_all().map_err(|e| impossible(dir, e))?;
            trouve = true;
            break;
        }
        if !trouve {
            return Err("Archive sans binaire « cydera ».".into());
        }
        let mut magic = [0u8; 4];
        let ok = std::fs::File::open(&tmp).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic)).is_ok() && magic == *b"\x7fELF";
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            return Err("Le binaire de l'archive n'est pas un exécutable Linux.".into());
        }
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| impossible(dir, e))?;
        std::fs::rename(&tmp, cible).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            impossible(dir, e)
        })
    }

    fn impossible(dir: &Path, e: std::io::Error) -> String {
        format!("Impossible de remplacer l'exécutable dans {} ({e}) : installe la mise à jour à la main.", dir.display())
    }

    /// Parcours complet sur PC : télécharge l'archive et les sommes, vérifie, remplace
    /// l'exécutable en cours. Renvoie le chemin du nouvel exécutable (à relancer).
    pub async fn installer_pc(r: &Release, progres: impl Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
        let paquet = r.asset(ASSET_LINUX).ok_or("Cette release ne contient pas d'archive Linux.")?;
        let sommes = r.asset(ASSET_SOMMES).ok_or("Cette release ne contient pas de SHA256SUMS.")?;
        let sommes = texte(&sommes.url).await?;
        let dir = crate::platform::cache_dir().ok_or("Dossier de cache introuvable")?.join("cydera").join("maj");
        let archive = dir.join(ASSET_LINUX);
        telecharger(&paquet.url, &archive, progres).await?;
        let res = verifier(&archive, &sommes, ASSET_LINUX).and_then(|()| {
            let cible = exe_actuel()?;
            installer_archive(&archive, &cible).map(|()| cible)
        });
        let _ = std::fs::remove_file(&archive);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, draft: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","body":"notes {tag}","html_url":"https://github.com/{DEPOT}/releases/tag/{tag}","draft":{draft},"prerelease":false,
               "assets":[{{"name":"{ASSET_APK}","browser_download_url":"https://x/{tag}/{ASSET_APK}","size":12}}]}}"#
        )
    }

    fn liste(rs: &[(&str, bool)]) -> String {
        format!("[{}]", rs.iter().map(|(t, d)| rel(t, *d)).collect::<Vec<_>>().join(","))
    }

    #[test]
    fn beta_recoit_la_beta_suivante_et_la_stable() {
        let j = liste(&[("v0.1.0-beta.2", false), ("v0.1.0-beta.1", false)]);
        assert_eq!(choisir(&j, "0.1.0-beta.1").unwrap().unwrap().tag, "v0.1.0-beta.2");
        let j = liste(&[("v0.1.0", false), ("v0.1.0-beta.3", false)]);
        assert_eq!(choisir(&j, "0.1.0-beta.1").unwrap().unwrap().tag, "v0.1.0");
    }

    #[test]
    fn stable_ignore_les_betas_et_les_brouillons() {
        let j = liste(&[("v0.3.0-beta.1", false), ("v0.2.0", true), ("v0.1.1", false)]);
        let r = choisir(&j, "0.1.0").unwrap().unwrap();
        assert_eq!(r.tag, "v0.1.1");
        assert_eq!(r.asset(ASSET_APK).unwrap().taille, 12);
    }

    #[test]
    fn rien_de_plus_recent() {
        let j = liste(&[("v0.1.0-beta.1", false), ("pas-une-version", false)]);
        assert_eq!(choisir(&j, "0.1.0-beta.1").unwrap(), None);
        assert_eq!(choisir("[]", "0.1.0").unwrap(), None);
        assert!(choisir("{", "0.1.0").is_err());
        assert!(choisir("[]", "abc").is_err());
    }

    #[test]
    fn sommes_format_sha256sum() {
        let s = "abc123  cydera.apk\ndef456 *cydera-linux-x86_64.tar.gz\n";
        assert_eq!(somme_attendue(s, ASSET_APK), Some("abc123"));
        assert_eq!(somme_attendue(s, ASSET_LINUX), Some("def456"));
        assert_eq!(somme_attendue(s, "autre"), None);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn installe_le_binaire_de_l_archive() {
        let dir = std::env::temp_dir().join(format!("cydera-maj-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Archive comme celle de la CI : cydera-linux-x86_64/{cydera, install.sh}
        let archive = dir.join(ASSET_LINUX);
        {
            let gz = flate2::write::GzEncoder::new(std::fs::File::create(&archive).unwrap(), flate2::Compression::fast());
            let mut t = tar::Builder::new(gz);
            for (nom, contenu) in
                [("cydera-linux-x86_64/install.sh", &b"#!/bin/sh\n"[..]), ("cydera-linux-x86_64/cydera", &b"\x7fELFnouveau"[..])]
            {
                let mut h = tar::Header::new_gnu();
                h.set_size(contenu.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                t.append_data(&mut h, nom, contenu).unwrap();
            }
            t.into_inner().unwrap().finish().unwrap();
        }
        let somme = sha256_fichier(&archive).unwrap();
        verifier(&archive, &format!("{somme}  {ASSET_LINUX}\n"), ASSET_LINUX).unwrap();
        assert!(verifier(&archive, &format!("{}  {ASSET_LINUX}\n", "0".repeat(64)), ASSET_LINUX).is_err());

        let cible = dir.join("cydera");
        std::fs::write(&cible, b"ancien").unwrap();
        installer_archive(&archive, &cible).unwrap();
        assert_eq!(std::fs::read(&cible).unwrap(), b"\x7fELFnouveau");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&cible).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!dir.join(".cydera.new").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
