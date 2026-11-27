//! Mails étudiants CY (Partage Renater / Zimbra), en lecture seule, via le webmail.
//!
//! Le serveur IMAP refuse le mot de passe ENT (compte fédéré) : on se connecte donc comme un
//! navigateur, à la manière du script Obsidian pour Celcat. `https://mail.etu.cyu.fr/` renvoie vers
//! la fédération (IdP Shibboleth `idp.cyu.fr`), puis vers le CAS `authentification.cyu.fr` où l'on
//! poste identifiant + mot de passe. Les formulaires intermédiaires (session IdP, `SAMLResponse`)
//! sont soumis tels quels. Au bout, Zimbra pose le cookie `ZM_AUTH_TOKEN`, puis :
//! - liste : API SOAP JSON `SearchRequest` ;
//! - message : MIME brut via `/service/home/~/?id=…` (ne le marque pas comme lu).
//!
//! La session est gardée en mémoire (jamais sur disque) et refaite si Zimbra la déclare expirée.

use reqwest::Url;
use reqwest::cookie::{CookieStore, Jar};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("réseau : {0}")]
    Network(#[from] reqwest::Error),
    #[error("identifiant ou mot de passe ENT refusé")]
    Login,
    #[error("étape de connexion inattendue : {0}")]
    Sso(String),
    #[error("webmail : {0}")]
    Server(String),
    #[error("session webmail expirée")]
    Expired,
}

/// Dossier Zimbra affichable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Folder {
    #[default]
    Inbox,
    Sent,
    Drafts,
    Junk,
    Trash,
}

impl Folder {
    pub const ALL: [Folder; 5] = [Folder::Inbox, Folder::Sent, Folder::Drafts, Folder::Junk, Folder::Trash];

    pub fn label(self) -> &'static str {
        match self {
            Folder::Inbox => "Réception",
            Folder::Sent => "Envoyés",
            Folder::Drafts => "Brouillons",
            Folder::Junk => "Spams",
            Folder::Trash => "Corbeille",
        }
    }

    /// Requête de recherche Zimbra.
    fn query(self) -> &'static str {
        match self {
            Folder::Inbox => "in:inbox",
            Folder::Sent => "in:sent",
            Folder::Drafts => "in:drafts",
            Folder::Junk => "in:junk",
            Folder::Trash => "in:trash",
        }
    }

    /// Dossiers où l'on affiche le destinataire plutôt que l'expéditeur.
    pub fn shows_recipient(self) -> bool {
        matches!(self, Folder::Sent | Folder::Drafts)
    }

    /// Identifiant Zimbra du dossier système.
    fn zimbra_id(self) -> &'static str {
        match self {
            Folder::Inbox => "2",
            Folder::Trash => "3",
            Folder::Junk => "4",
            Folder::Sent => "5",
            Folder::Drafts => "6",
        }
    }

    pub fn empty_label(self) -> &'static str {
        match self {
            Folder::Inbox => "Boîte de réception vide.",
            Folder::Sent => "Aucun message envoyé.",
            Folder::Drafts => "Aucun brouillon.",
            Folder::Junk => "Aucun spam.",
            Folder::Trash => "La corbeille est vide.",
        }
    }
}

/// Action sur un message (`MsgActionRequest`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgAction {
    Read,
    Unread,
    /// Déplacer dans la corbeille.
    Trash,
    /// Signaler comme spam (déplace dans « Spams »).
    Spam,
    /// Pas un spam : retour dans la boîte de réception.
    NotSpam,
    /// Ramener dans un dossier (ex. restaurer depuis la corbeille).
    Move(Folder),
}

impl MsgAction {
    fn request(self, uid: u32) -> Value {
        let (op, l) = match self {
            MsgAction::Read => ("read", None),
            MsgAction::Unread => ("!read", None),
            MsgAction::Trash => ("trash", None),
            MsgAction::Spam => ("spam", None),
            MsgAction::NotSpam => ("!spam", Some(Folder::Inbox.zimbra_id())),
            MsgAction::Move(f) => ("move", Some(f.zimbra_id())),
        };
        let mut action = json!({ "id": uid.to_string(), "op": op });
        if let Some(l) = l {
            action["l"] = json!(l);
        }
        json!({ "_jsns": "urn:zimbraMail", "action": action })
    }
}

/// Requête de recherche Zimbra : le dossier, plus le texte saisi (mots-clés, « from:… »…).
fn search_query(folder: Folder, text: &str) -> String {
    let t = text.trim();
    if t.is_empty() { folder.query().to_string() } else { format!("{} ({t})", folder.query()) }
}

/// Portion de texte d'un message avec sa mise en forme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}

/// Une ligne (ou un paragraphe, à renvoyer à la ligne) du message ; vide = ligne blanche.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub spans: Vec<Span>,
    /// Niveau de citation (« > »).
    pub quote: u8,
    pub bullet: bool,
}

/// Corps d'un message mis en forme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub lines: Vec<Line>,
}

impl Body {
    /// Texte brut (pour la ligne de commande).
    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                let t: String = l.spans.iter().map(|s| s.text.as_str()).collect();
                format!("{}{}{}", "> ".repeat(l.quote as usize), if l.bullet { "• " } else { "" }, t)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Correspondant : nom affiché (peut être vide) et adresse.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub addr: String,
}

impl Contact {
    /// Le nom s'il est connu, sinon l'adresse.
    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.addr } else { &self.name }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub uid: u32,
    /// Résumé pour la liste : l'expéditeur, ou « À : … » dans Envoyés / Brouillons.
    pub from: String,
    pub subject: String,
    pub date: String,
    pub unread: bool,
    #[serde(default)]
    pub sender: Option<Contact>,
    #[serde(default)]
    pub to: Vec<Contact>,
    #[serde(default)]
    pub cc: Vec<Contact>,
    /// Date d'envoi (ms Unix), 0 si inconnue.
    #[serde(default)]
    pub ts: i64,
}

#[derive(Clone)]
struct Session {
    key: (String, String),
    client: reqwest::Client,
    jar: Arc<Jar>,
    base: Url,
}

impl Session {
    fn auth_token(&self) -> Option<String> {
        cookie_value(&self.jar, &self.base, "ZM_AUTH_TOKEN")
    }
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

fn cookie_value(jar: &Jar, url: &Url, name: &str) -> Option<String> {
    let all = jar.cookies(url)?;
    let all = all.to_str().ok()?;
    all.split(';').map(str::trim).find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=').map(str::to_string)).filter(|v| !v.is_empty())
}

/// URL sans la requête ni le fragment (pour les messages d'erreur : rien de secret).
fn bare(u: &Url) -> String {
    format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), u.path())
}

/// Session existante (même compte) ou nouvelle connexion SSO.
async fn session(host: &str, user: &str, pass: &str, fresh: bool) -> Result<Session, Error> {
    let key = (host.to_string(), user.to_string());
    if !fresh && let Some(s) = slot().lock().unwrap().clone().filter(|s| s.key == key) {
        return Ok(s);
    }
    let s = login(host, user, pass).await?;
    *slot().lock().unwrap() = Some(s.clone());
    Ok(s)
}

async fn login(host: &str, user: &str, pass: &str) -> Result<Session, Error> {
    let jar = Arc::new(Jar::default());
    let client = crate::net::client_builder()
        .cookie_provider(jar.clone())
        .redirect(reqwest::redirect::Policy::limited(20))
        .timeout(std::time::Duration::from_secs(25))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36")
        .build()?;
    let start = Url::parse(&format!("https://{}/", host.trim_end_matches('/'))).map_err(|e| Error::Sso(e.to_string()))?;
    let mut resp = client.get(start).send().await?;
    let mut posted_password = false;
    for _ in 0..10 {
        let url = resp.url().clone();
        if cookie_value(&jar, &url, "ZM_AUTH_TOKEN").is_some() {
            let mut base = url.clone();
            base.set_path("/");
            base.set_query(None);
            base.set_fragment(None);
            return Ok(Session { key: (host.to_string(), user.to_string()), client, jar, base });
        }
        let status = resp.status();
        let html = resp.text().await?;
        let Some(form) = pick_form(&html) else {
            return Err(Error::Sso(format!("{} (HTTP {}, pas de formulaire)", bare(&url), status.as_u16())));
        };
        if std::env::var_os("CYDERA_MAIL_DEBUG").is_some() {
            let names: Vec<&str> = form.fields.iter().map(|(k, _)| k.as_str()).collect();
            eprintln!(
                "[mail] {} HTTP {} → form {} {:?} mdp={}",
                bare(&url),
                status.as_u16(),
                form.action.split('?').next().unwrap_or(""),
                names,
                form.has_password
            );
        }
        let mut fields = form.fields.clone();
        if form.has_password {
            if posted_password {
                return Err(Error::Login);
            }
            posted_password = true;
            for (k, v) in fields.iter_mut() {
                match k.as_str() {
                    "username" | "j_username" => *v = user.to_string(),
                    "password" | "j_password" => *v = pass.to_string(),
                    _ => {}
                }
            }
        }
        let action = if form.action.is_empty() { url.clone() } else { url.join(&form.action).map_err(|e| Error::Sso(e.to_string()))? };
        resp = if form.method_get {
            let mut u = action;
            u.query_pairs_mut().extend_pairs(fields.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            client.get(u).send().await?
        } else {
            client.post(action).form(&fields).send().await?
        };
    }
    Err(Error::Sso("trop d'étapes de connexion".into()))
}

/// Formulaire HTML réduit à ce qu'il faut pour le soumettre.
#[derive(Debug, Default, PartialEq)]
struct Form {
    action: String,
    method_get: bool,
    fields: Vec<(String, String)>,
    has_password: bool,
}

/// Valeur d'un attribut dans une balise (guillemets doubles ou simples), entités décodées.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let i = from + i;
        from = i + name.len();
        let before = lower[..i].chars().last().unwrap_or(' ');
        if !before.is_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let q = rest.chars().next()?;
        if q == '"' || q == '\'' {
            let end = rest[1..].find(q)?;
            return Some(decode_entities(&rest[1..1 + end]));
        }
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        return Some(decode_entities(&rest[..end]));
    }
    None
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_forms(html: &str) -> Vec<Form> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("<form") {
        let start = pos + i;
        let tag_end = start + lower[start..].find('>').unwrap_or(lower.len() - start);
        let end = lower[tag_end..].find("</form").map(|e| tag_end + e).unwrap_or(lower.len());
        let tag = &html[start..tag_end];
        let mut f = Form {
            action: attr(tag, "action").unwrap_or_default(),
            method_get: attr(tag, "method").is_some_and(|m| m.eq_ignore_ascii_case("get")),
            ..Default::default()
        };
        let body = &html[tag_end..end];
        let lb = &lower[tag_end..end];
        // boutons d'envoi nommés : on garde « _eventId_proceed » (Continuer / Accepter) s'il existe
        let mut submits: Vec<(String, String)> = Vec::new();
        let mut q = 0;
        while let Some(j) = lb[q..].find("<button") {
            let s = q + j;
            let e = s + lb[s..].find('>').unwrap_or(lb.len() - s);
            let t = &body[s..e];
            q = e;
            if let Some(name) = attr(t, "name")
                && attr(t, "type").is_none_or(|ty| ty.eq_ignore_ascii_case("submit"))
            {
                submits.push((name, attr(t, "value").unwrap_or_default()));
            }
        }
        let mut p = 0;
        while let Some(j) = lb[p..].find("<input") {
            let s = p + j;
            let e = s + lb[s..].find('>').unwrap_or(lb.len() - s);
            let t = &body[s..e];
            p = e;
            let ty = attr(t, "type").unwrap_or_else(|| "text".into()).to_ascii_lowercase();
            if ty == "password" {
                f.has_password = true;
            }
            let Some(name) = attr(t, "name") else { continue };
            if ty == "submit" {
                submits.push((name, attr(t, "value").unwrap_or_default()));
                continue;
            }
            if matches!(ty.as_str(), "button" | "image" | "reset")
                || (matches!(ty.as_str(), "checkbox" | "radio") && !t.to_ascii_lowercase().contains("checked"))
            {
                continue;
            }
            f.fields.push((name, attr(t, "value").unwrap_or_default()));
        }
        let has_proceed = f.fields.iter().any(|(k, _)| k == "_eventId_proceed");
        if !has_proceed
            && let Some(b) =
                submits.iter().find(|(k, _)| k == "_eventId_proceed").or_else(|| submits.iter().find(|(k, _)| !k.contains("Reject")))
        {
            f.fields.push(b.clone());
        }
        out.push(f);
        pos = end.max(start + 5);
    }
    out
}

/// Formulaire à soumettre : celui du mot de passe s'il y en a un, sinon le premier qui a des champs.
fn pick_form(html: &str) -> Option<Form> {
    let mut forms = parse_forms(html);
    if let Some(i) = forms.iter().position(|f| f.has_password) {
        return Some(forms.swap_remove(i));
    }
    forms.into_iter().find(|f| !f.fields.is_empty())
}

/// Appel SOAP JSON Zimbra ; renvoie le contenu de `Body`.
async fn soap(s: &Session, name: &str, req: Value) -> Result<Value, Error> {
    let token = s.auth_token().ok_or(Error::Expired)?;
    let url = s.base.join(&format!("service/soap/{name}")).map_err(|e| Error::Server(e.to_string()))?;
    let payload = json!({
        "Header": { "context": { "_jsns": "urn:zimbra", "authToken": token } },
        "Body": { name: req },
    });
    let res = s.client.post(url).header("Content-Type", "application/json").body(payload.to_string()).send().await?;
    let status = res.status();
    let text = res.text().await?;
    let v: Value = serde_json::from_str(&text).map_err(|_| Error::Server(format!("réponse illisible (HTTP {})", status.as_u16())))?;
    if let Some(fault) = v.pointer("/Body/Fault") {
        let code = fault.pointer("/Detail/Error/Code").and_then(Value::as_str).unwrap_or("");
        if code.starts_with("service.AUTH_") {
            return Err(Error::Expired);
        }
        let why = fault.pointer("/Reason/Text").and_then(Value::as_str).unwrap_or(code);
        return Err(Error::Server(why.to_string()));
    }
    if status.as_u16() == 401 {
        return Err(Error::Expired);
    }
    Ok(v.get("Body").cloned().unwrap_or(Value::Null))
}

/// `SearchResponse` → en-têtes, du plus récent au plus ancien.
fn parse_search(body: &Value, folder: Folder) -> Vec<Header> {
    let Some(ms) = body.pointer("/SearchResponse/m").and_then(Value::as_array) else { return vec![] };
    ms.iter()
        .filter_map(|m| {
            let uid =
                m.get("id").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))?.parse().ok()?;
            let es: &[Value] = m.get("e").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            let contacts = |t: &str| -> Vec<Contact> {
                es.iter()
                    .filter(|e| e.get("t").and_then(Value::as_str) == Some(t))
                    .map(|e| {
                        let addr = e.get("a").and_then(Value::as_str).unwrap_or("").to_string();
                        let name = e.get("p").and_then(Value::as_str).map(clean_from).unwrap_or_default();
                        Contact { name: if name == addr { String::new() } else { name }, addr }
                    })
                    .collect()
            };
            let (sender, to, cc) = (contacts("f").into_iter().next(), contacts("t"), contacts("c"));
            let from = if folder.shows_recipient() {
                if to.is_empty() {
                    "(sans destinataire)".to_string()
                } else {
                    format!("À : {}", to.iter().map(Contact::label).collect::<Vec<_>>().join(", "))
                }
            } else {
                sender.as_ref().map(|c| c.label().to_string()).unwrap_or_default()
            };
            let ts = m.get("d").and_then(Value::as_i64).unwrap_or(0);
            let date = m
                .get("d")
                .and_then(Value::as_i64)
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
                .unwrap_or_default();
            Some(Header {
                uid,
                from,
                subject: m.get("su").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("(sans objet)").to_string(),
                date,
                unread: m.get("f").and_then(Value::as_str).is_some_and(|f| f.contains('u')),
                sender,
                to,
                cc,
                ts,
            })
        })
        .collect()
}

async fn search(s: &Session, folder: Folder, text: &str, count: u32) -> Result<Vec<Header>, Error> {
    let body = soap(
        s,
        "SearchRequest",
        json!({
            "_jsns": "urn:zimbraMail",
            "types": "message",
            "query": search_query(folder, text),
            // destinataires en plus de l'expéditeur (sinon Zimbra ne renvoie que le second)
            "recip": "2",
            "sortBy": "dateDesc",
            "limit": count,
        }),
    )
    .await?;
    Ok(parse_search(&body, folder))
}

async fn raw_message(s: &Session, uid: u32) -> Result<Body, Error> {
    let url = s.base.join(&format!("service/home/~/?id={uid}")).map_err(|e| Error::Server(e.to_string()))?;
    let res = s.client.get(url).send().await?;
    if matches!(res.status().as_u16(), 401 | 403) {
        return Err(Error::Expired);
    }
    if !res.status().is_success() {
        return Err(Error::Server(format!("message introuvable (HTTP {})", res.status().as_u16())));
    }
    let raw = res.bytes().await?;
    let mail = mailparse::parse_mail(&raw).map_err(|e| Error::Server(e.to_string()))?;
    Ok(extract_body(&mail))
}

/// Derniers `count` messages d'un dossier, du plus récent au plus ancien.
/// `host` : adresse du webmail (ex. `mail.etu.cyu.fr`) ; `user` / `pass` : identifiants ENT.
/// `text` : recherche libre dans ce dossier (vide = tout).
pub async fn list(host: &str, user: &str, pass: &str, folder: Folder, text: &str, count: u32) -> Result<Vec<Header>, Error> {
    let s = session(host, user, pass, false).await?;
    match search(&s, folder, text, count).await {
        Err(Error::Expired) => search(&session(host, user, pass, true).await?, folder, text, count).await,
        r => r,
    }
}

/// Applique une action à un message (lu / non lu, corbeille, spam…).
pub async fn action(host: &str, user: &str, pass: &str, uid: u32, act: MsgAction) -> Result<(), Error> {
    let req = act.request(uid);
    let s = session(host, user, pass, false).await?;
    let r = match soap(&s, "MsgActionRequest", req.clone()).await {
        Err(Error::Expired) => soap(&session(host, user, pass, true).await?, "MsgActionRequest", req).await,
        r => r,
    };
    r.map(|_| ())
}

/// Corps mis en forme d'un message (sans le marquer comme lu).
pub async fn body(host: &str, user: &str, pass: &str, uid: u32) -> Result<Body, Error> {
    let s = session(host, user, pass, false).await?;
    match raw_message(&s, uid).await {
        Err(Error::Expired) => raw_message(&session(host, user, pass, true).await?, uid).await,
        r => r,
    }
}

fn find_part(m: &mailparse::ParsedMail, mime: &str) -> Option<String> {
    let mut stack = vec![m];
    while let Some(p) = stack.pop() {
        if p.subparts.is_empty() && p.ctype.mimetype.eq_ignore_ascii_case(mime) {
            return p.get_body().ok();
        }
        stack.extend(p.subparts.iter().rev());
    }
    None
}

/// Corps mis en forme : le HTML s'il donne du texte (gras, liens…), sinon le texte brut.
fn extract_body(m: &mailparse::ParsedMail) -> Body {
    let html = find_part(m, "text/html").map(|h| parse_html(&h)).filter(|b| !b.lines.is_empty());
    html.or_else(|| find_part(m, "text/plain").map(|t| parse_plain(&t))).filter(|b| !b.lines.is_empty()).unwrap_or_else(|| Body {
        lines: vec![Line { spans: vec![Span { text: "(message sans contenu texte)".into(), ..Default::default() }], ..Default::default() }],
    })
}

/// Découpe un texte en morceaux, les URL `http(s)://…` devenant des liens.
fn linkify(text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min() {
        let tail = &rest[i..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'']);
        if url.len() <= "https://".len() {
            out.push((rest[..i + 4].to_string(), None));
            rest = &rest[i + 4..];
            continue;
        }
        if i > 0 {
            out.push((rest[..i].to_string(), None));
        }
        out.push((url.to_string(), Some(url.to_string())));
        rest = &rest[i + url.len()..];
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), None));
    }
    out
}

fn parse_plain(t: &str) -> Body {
    let mut lines: Vec<Line> = Vec::new();
    for raw in t.replace('\r', "").lines() {
        let mut rest = raw.trim_end();
        let mut quote = 0u8;
        while let Some(r) = rest.trim_start().strip_prefix('>') {
            quote = quote.saturating_add(1);
            rest = r;
        }
        let rest = if quote > 0 { rest.strip_prefix(' ').unwrap_or(rest) } else { rest };
        let spans: Vec<Span> = linkify(rest).into_iter().map(|(text, link)| Span { text, link, ..Default::default() }).collect();
        if spans.is_empty() && lines.last().is_none_or(|l| l.spans.is_empty()) {
            continue;
        }
        lines.push(Line { spans, quote, bullet: false });
    }
    while lines.last().is_some_and(|l| l.spans.is_empty()) {
        lines.pop();
    }
    Body { lines }
}

#[derive(Default)]
struct Builder {
    lines: Vec<Line>,
    cur: Vec<Span>,
    bold: u32,
    italic: u32,
    links: Vec<Option<String>>,
    quote: u8,
    bullet: bool,
}

impl Builder {
    fn push_text(&mut self, text: &str) {
        // espaces HTML : toute suite d'espaces/retours devient une seule espace
        let mut t = String::with_capacity(text.len());
        let mut prev_space = self.cur.last().is_none_or(|s| s.text.ends_with(' '));
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !prev_space {
                    t.push(' ');
                }
                prev_space = true;
            } else {
                t.push(if c == '\u{a0}' { ' ' } else { c });
                prev_space = false;
            }
        }
        if t.is_empty() {
            return;
        }
        let link = self.links.iter().rev().find_map(|l| l.clone());
        let pieces = if link.is_some() { vec![(t, link)] } else { linkify(&t) };
        for (text, link) in pieces {
            let (bold, italic) = (self.bold > 0, self.italic > 0);
            match self.cur.last_mut() {
                Some(s) if s.bold == bold && s.italic == italic && s.link == link => s.text.push_str(&text),
                _ => self.cur.push(Span { text, bold, italic, link }),
            }
        }
    }

    /// Termine la ligne en cours.
    fn flush(&mut self) {
        let mut spans = std::mem::take(&mut self.cur);
        if let Some(f) = spans.first_mut() {
            f.text = f.text.trim_start().to_string();
        }
        if let Some(l) = spans.last_mut() {
            l.text = l.text.trim_end().to_string();
        }
        spans.retain(|s| !s.text.is_empty());
        let bullet = std::mem::take(&mut self.bullet);
        if !spans.is_empty() {
            self.lines.push(Line { spans, quote: self.quote, bullet });
        }
    }

    /// Fin de paragraphe : ligne en cours, puis une ligne blanche (une seule d'affilée).
    fn para(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            self.lines.push(Line { quote: self.quote, ..Default::default() });
        }
    }
}

fn parse_html(h: &str) -> Body {
    let lower = h.to_ascii_lowercase();
    let mut b = Builder::default();
    let mut i = 0;
    while i < h.len() {
        let Some(lt) = h[i..].find('<').map(|k| i + k) else {
            b.push_text(&decode_entities_full(&h[i..]));
            break;
        };
        if lt > i {
            b.push_text(&decode_entities_full(&h[i..lt]));
        }
        if lower[lt..].starts_with("<!--") {
            i = lower[lt..].find("-->").map_or(h.len(), |k| lt + k + 3);
            continue;
        }
        let Some(gt) = h[lt..].find('>').map(|k| lt + k) else { break };
        let tag = &h[lt + 1..gt];
        i = gt + 1;
        let closing = tag.starts_with('/');
        let name: String =
            tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match name.as_str() {
            "style" | "script" | "head" | "title" if !closing => {
                let close = format!("</{name}");
                i = lower[i..].find(&close).map_or(h.len(), |k| i + k);
            }
            "b" | "strong" | "th" if !closing => b.bold += 1,
            "b" | "strong" | "th" => b.bold = b.bold.saturating_sub(1),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                b.para();
                if closing { b.bold = b.bold.saturating_sub(1) } else { b.bold += 1 }
            }
            "i" | "em" | "cite" if !closing => b.italic += 1,
            "i" | "em" | "cite" => b.italic = b.italic.saturating_sub(1),
            "a" if !closing => {
                let href = attr(tag, "href").filter(|u| u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:"));
                b.links.push(href);
            }
            "a" => {
                b.links.pop();
            }
            "br" => b.flush(),
            "p" => b.para(),
            "div" | "tr" | "table" | "ul" | "ol" | "pre" => b.flush(),
            "li" if !closing => {
                b.flush();
                b.bullet = true;
            }
            "li" => b.flush(),
            "td" if closing => b.push_text(" "),
            "blockquote" => {
                b.para();
                b.quote = if closing { b.quote.saturating_sub(1) } else { b.quote.saturating_add(1) };
            }
            "hr" => {
                b.flush();
                b.push_text("────────");
                b.flush();
            }
            "img" if !closing => {
                if let Some(alt) = attr(tag, "alt").filter(|a| !a.trim().is_empty()) {
                    b.push_text(&format!("[{}]", alt.trim()));
                }
            }
            _ => {}
        }
    }
    b.flush();
    while b.lines.last().is_some_and(|l| l.spans.is_empty()) {
        b.lines.pop();
    }
    while b.lines.first().is_some_and(|l| l.spans.is_empty()) {
        b.lines.remove(0);
    }
    Body { lines: b.lines }
}

/// Entités HTML : celles de `decode_entities` plus les plus courantes en français.
fn decode_entities_full(s: &str) -> String {
    let mut t = decode_entities(s);
    if t.contains('&') {
        for (k, v) in [
            ("&nbsp;", "\u{a0}"),
            ("&hellip;", "…"),
            ("&rsquo;", "’"),
            ("&lsquo;", "‘"),
            ("&laquo;", "«"),
            ("&raquo;", "»"),
            ("&ndash;", "–"),
            ("&mdash;", "—"),
            ("&euro;", "€"),
            ("&copy;", "©"),
            ("&eacute;", "é"),
            ("&egrave;", "è"),
            ("&agrave;", "à"),
            ("&ecirc;", "ê"),
            ("&ccedil;", "ç"),
            ("&bull;", "•"),
        ] {
            t = t.replace(k, v);
        }
    }
    t
}

/// « "Jean Dupont" <jd@x.fr> » → « Jean Dupont »
fn clean_from(s: &str) -> String {
    let name = s.split('<').next().unwrap_or(s).trim().trim_matches('"').trim();
    if name.is_empty() { s.trim_matches(|c| c == '<' || c == '>').to_string() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idp_session_form() {
        let html = r#"<form name="form1" action="/idp/profile/SAML2/Redirect/SSO?execution=e1s1" method="post">
            <input type="hidden" name="csrf_token" value="_abc&amp;def" />
            <input name="shib_idp_ls_supported" type="hidden" />
            <input name="_eventId_proceed" type="hidden" />
            <input type="submit" value="Continue" /></form>"#;
        let f = pick_form(html).unwrap();
        assert_eq!(f.action, "/idp/profile/SAML2/Redirect/SSO?execution=e1s1");
        assert!(!f.method_get && !f.has_password);
        assert_eq!(f.fields[0], ("csrf_token".into(), "_abc&def".into()));
        assert_eq!(f.fields.len(), 3);
        let base = Url::parse("https://idp.cyu.fr/idp/profile/SAML2/Redirect/SSO?execution=e1s1").unwrap();
        assert_eq!(base.join(&f.action).unwrap().as_str(), "https://idp.cyu.fr/idp/profile/SAML2/Redirect/SSO?execution=e1s1");
    }

    #[test]
    fn cas_login_form_preferred() {
        let html = r#"<form id="lang" action="x"><input type="hidden" name="locale" value="fr"></form>
            <form method="post" id="fm1" action="login" onsubmit="loginFormSubmission();">
            <input class="mdc-text-field__input form-control" id="username" size="25" type="text" required name="username" value="" />
            <input class="pwd" type="password" id="password" required name="password" value=""/>
            <input type="hidden" name="execution" value="e1&#x2f;s2"/>
            <input type="hidden" name="_eventId" value="submit"/>
            <input type="hidden" name="geolocation"/></form>"#;
        let f = pick_form(html).unwrap();
        assert!(f.has_password);
        assert_eq!(f.action, "login");
        let names: Vec<&str> = f.fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["username", "password", "execution", "_eventId", "geolocation"]);
        assert_eq!(f.fields[2].1, "e1/s2");
        let base = Url::parse("https://authentification.cyu.fr/cas/login?service=x").unwrap();
        assert_eq!(base.join(&f.action).unwrap().path(), "/cas/login");
    }

    #[test]
    fn consent_accepts() {
        let html = r#"<form action="/idp/profile/SAML2/Redirect/SSO?execution=e1s3" method="post">
            <input type="hidden" name="csrf_token" value="t"/>
            <input type="checkbox" name="_shib_idp_consentIds" value="mail" checked>
            <input type="radio" name="_shib_idp_consentOptions" value="_shib_idp_doNotRememberConsent">
            <input type="radio" name="_shib_idp_consentOptions" value="_shib_idp_rememberConsent" checked>
            <input type="submit" name="_eventId_AttributeReleaseRejected" value="Refuser">
            <input type="submit" name="_eventId_proceed" value="Accepter"></form>"#;
        let f = pick_form(html).unwrap();
        assert_eq!(
            f.fields,
            vec![
                ("csrf_token".into(), "t".into()),
                ("_shib_idp_consentIds".into(), "mail".into()),
                ("_shib_idp_consentOptions".into(), "_shib_idp_rememberConsent".into()),
                ("_eventId_proceed".into(), "Accepter".into()),
            ]
        );
    }

    #[test]
    fn saml_autosubmit() {
        let html = r#"<body onload="document.forms[0].submit()"><form action="https&#x3a;&#x2f;&#x2f;sp.partage.renater.fr&#x2f;Shibboleth.sso&#x2f;SAML2&#x2f;POST" method="post">
            <input type="hidden" name="RelayState" value="ss&#x3a;mem&#x3a;12"/><input type="hidden" name="SAMLResponse" value="PHNhbWw+"/></form>"#;
        let f = pick_form(html).unwrap();
        assert_eq!(f.action, "https://sp.partage.renater.fr/Shibboleth.sso/SAML2/POST");
        assert_eq!(f.fields, vec![("RelayState".into(), "ss:mem:12".into()), ("SAMLResponse".into(), "PHNhbWw+".into())]);
    }

    #[test]
    fn search_response() {
        let body = json!({"SearchResponse": {"m": [
            {"id": "4321", "su": "Rappel examen", "d": 1764230400000i64, "f": "u",
             "e": [{"t": "t", "a": "moi@etu.cyu.fr"}, {"t": "f", "a": "scol@cyu.fr", "p": "Scolarité CY"}, {"t": "c", "a": "prof@cyu.fr", "p": "Prof"}]},
            {"id": "4300", "su": "Lu", "d": 1764144000000i64, "f": "", "e": [{"t": "f", "a": "x@y.fr"}]}
        ]}});
        let h = parse_search(&body, Folder::Inbox);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].uid, h[0].from.as_str(), h[0].subject.as_str(), h[0].unread), (4321, "Scolarité CY", "Rappel examen", true));
        assert_eq!((h[1].from.as_str(), h[1].unread), ("x@y.fr", false));
        assert!(!h[0].date.is_empty());
        assert_eq!(h[0].sender, Some(Contact { name: "Scolarité CY".into(), addr: "scol@cyu.fr".into() }));
        assert_eq!(h[0].to, [Contact { name: String::new(), addr: "moi@etu.cyu.fr".into() }]);
        assert_eq!(h[0].cc[0].label(), "Prof");
        assert_eq!(h[0].ts, 1764230400000);
    }

    #[test]
    fn sent_shows_recipient() {
        let body = json!({"SearchResponse": {"m": [
            {"id": "1", "su": "Re", "d": 1764230400000i64, "e": [{"t": "f", "a": "moi@x.fr"}, {"t": "t", "a": "prof@x.fr", "p": "Prof"}]}]}});
        assert_eq!(parse_search(&body, Folder::Sent)[0].from, "À : Prof");
    }

    #[test]
    fn html_formatting() {
        let b = parse_html(
            "<html><head><style>p{}</style></head><body><p>Bonjour <b>Lucas</b>,</p><p>Voir <a href=\"https://cyu.fr/x\">le site</a> et <i>vite</i>&nbsp;!</p><ul><li>un</li><li>deux</li></ul><blockquote>cité</blockquote></body></html>",
        );
        let l = &b.lines;
        assert_eq!(l[0].spans[1], Span { text: "Lucas".into(), bold: true, ..Default::default() });
        assert_eq!(l[0].spans[2].text, ",");
        assert_eq!(l[2].spans[1].link.as_deref(), Some("https://cyu.fr/x"));
        assert!(l[2].spans[3].italic);
        assert!(l.iter().any(|x| x.bullet && x.spans[0].text == "un"));
        assert!(l.iter().any(|x| x.quote == 1 && x.spans[0].text == "cité"));
    }

    #[test]
    fn plain_links_and_quotes() {
        let b = parse_plain("Voir https://a.fr/b. Merci\n> cité\n\n\n\nfin");
        assert_eq!(b.lines[0].spans[1].link.as_deref(), Some("https://a.fr/b"));
        assert_eq!(b.lines[1].quote, 1);
        assert_eq!(b.lines.len(), 4);
    }

    #[test]
    fn requetes_zimbra() {
        assert_eq!(search_query(Folder::Inbox, "  "), "in:inbox");
        assert_eq!(search_query(Folder::Sent, "from:prof examen"), "in:sent (from:prof examen)");
        assert_eq!(MsgAction::Unread.request(12)["action"], json!({"id": "12", "op": "!read"}));
        assert_eq!(MsgAction::Move(Folder::Inbox).request(3)["action"], json!({"id": "3", "op": "move", "l": "2"}));
        assert_eq!(MsgAction::NotSpam.request(3)["action"]["l"], "2");
    }
}
