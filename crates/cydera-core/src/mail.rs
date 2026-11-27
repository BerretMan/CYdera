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

#[derive(Clone, Debug)]
pub struct Header {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub unread: bool,
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
fn parse_search(body: &Value) -> Vec<Header> {
    let Some(ms) = body.pointer("/SearchResponse/m").and_then(Value::as_array) else { return vec![] };
    ms.iter()
        .filter_map(|m| {
            let uid =
                m.get("id").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())))?.parse().ok()?;
            let from = m
                .get("e")
                .and_then(Value::as_array)
                .and_then(|es| es.iter().find(|e| e.get("t").and_then(Value::as_str) == Some("f")))
                .and_then(|e| e.get("p").or_else(|| e.get("a")).and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            let date = m
                .get("d")
                .and_then(Value::as_i64)
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
                .unwrap_or_default();
            Some(Header {
                uid,
                from: clean_from(&from),
                subject: m.get("su").and_then(Value::as_str).unwrap_or("(sans objet)").to_string(),
                date,
                unread: m.get("f").and_then(Value::as_str).is_some_and(|f| f.contains('u')),
            })
        })
        .collect()
}

async fn search(s: &Session, count: u32) -> Result<Vec<Header>, Error> {
    let body = soap(
        s,
        "SearchRequest",
        json!({
            "_jsns": "urn:zimbraMail",
            "types": "message",
            "query": "in:inbox",
            "sortBy": "dateDesc",
            "limit": count,
        }),
    )
    .await?;
    Ok(parse_search(&body))
}

async fn raw_message(s: &Session, uid: u32) -> Result<String, Error> {
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
    Ok(extract_text(&mail).unwrap_or_else(|| "(message sans contenu texte)".into()))
}

/// Derniers `count` messages de la boîte de réception, du plus récent au plus ancien.
/// `host` : adresse du webmail (ex. `mail.etu.cyu.fr`) ; `user` / `pass` : identifiants ENT.
pub async fn list(host: &str, user: &str, pass: &str, count: u32) -> Result<Vec<Header>, Error> {
    let s = session(host, user, pass, false).await?;
    match search(&s, count).await {
        Err(Error::Expired) => search(&session(host, user, pass, true).await?, count).await,
        r => r,
    }
}

/// Corps texte d'un message (sans le marquer comme lu).
pub async fn body(host: &str, user: &str, pass: &str, uid: u32) -> Result<String, Error> {
    let s = session(host, user, pass, false).await?;
    match raw_message(&s, uid).await {
        Err(Error::Expired) => raw_message(&session(host, user, pass, true).await?, uid).await,
        r => r,
    }
}

fn extract_text(m: &mailparse::ParsedMail) -> Option<String> {
    let find = |mime: &str| -> Option<String> {
        let mut stack = vec![m];
        while let Some(p) = stack.pop() {
            if p.subparts.is_empty() && p.ctype.mimetype.eq_ignore_ascii_case(mime) {
                return p.get_body().ok();
            }
            stack.extend(p.subparts.iter().rev());
        }
        None
    };
    find("text/plain").or_else(|| find("text/html").map(|h| html_to_text(&h)))
}

fn html_to_text(h: &str) -> String {
    let mut out = String::new();
    let (mut in_tag, mut skip) = (false, false);
    let lower = h.to_lowercase();
    let b = h.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !in_tag && b[i] == b'<' {
            in_tag = true;
            let tag = &lower[i..];
            if tag.starts_with("<style") || tag.starts_with("<script") {
                skip = true;
            } else if tag.starts_with("</style") || tag.starts_with("</script") {
                skip = false;
            } else if tag.starts_with("<br")
                || tag.starts_with("</p")
                || tag.starts_with("</div")
                || tag.starts_with("</tr")
                || tag.starts_with("</li")
            {
                out.push('\n');
            }
        } else if in_tag && b[i] == b'>' {
            in_tag = false;
        } else if !in_tag && !skip {
            let ch = h[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        i += 1;
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut res = String::new();
    let mut blank = 0;
    for l in out.lines().map(str::trim) {
        blank = if l.is_empty() { blank + 1 } else { 0 };
        if blank <= 1 {
            res.push_str(l);
            res.push('\n');
        }
    }
    res.trim().to_string()
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
             "e": [{"t": "t", "a": "moi@etu.cyu.fr"}, {"t": "f", "a": "scol@cyu.fr", "p": "Scolarité CY"}]},
            {"id": "4300", "su": "Lu", "d": 1764144000000i64, "f": "", "e": [{"t": "f", "a": "x@y.fr"}]}
        ]}});
        let h = parse_search(&body);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].uid, h[0].from.as_str(), h[0].subject.as_str(), h[0].unread), (4321, "Scolarité CY", "Rappel examen", true));
        assert_eq!((h[1].from.as_str(), h[1].unread), ("x@y.fr", false));
        assert!(!h[0].date.is_empty());
    }
}
