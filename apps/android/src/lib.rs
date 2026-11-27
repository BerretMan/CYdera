//! Point d'entrée Android : `NativeActivity` → `android_main` → eframe.
#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use cydera_core::platform::{self, Dirs};
use cydera_ui::{app::App, ui::theme};

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("cydera"));
    // `panic = "abort"` en release : sans ce hook, la cause du crash serait perdue.
    cydera_core::net::crypto_init();
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

    if let Some(dir) = app.internal_data_path() {
        platform::init(Dirs::under(dir));
    }
    // Marges système (barre d'état, encoche, navigation, clavier) via JNI :
    // `decorView.getRootWindowInsets()`. Renvoie des pixels physiques [haut, droite, bas, gauche].
    let a = app.clone();
    cydera_ui::insets::set_provider(move || system_insets(&a).unwrap_or([0.0; 4]));
    // Presse-papiers système (coller un ticket d'appairage, copier une salle…).
    let (ag, as_) = (app.clone(), app.clone());
    cydera_ui::clipboard::set_provider(
        move || clipboard_get(&ag),
        move |t| {
            if clipboard_set(&as_, t).is_none() {
                log::warn!("presse-papiers : écriture impossible");
            }
        },
    );
    // Ticket d'appairage passé au lancement (`am start --es ticket cydera1:…`) : pré-rempli, jamais auto-validé.
    if let Some(t) = launch_extra(&app, "ticket") {
        cydera_ui::p2p::set_startup_ticket(t);
    }
    // Test de la mise à jour : `am start … --es update_from 0.0.1` simule une version plus ancienne.
    if let Some(v) = launch_extra(&app, "update_from") {
        cydera_core::update::simuler_version(v);
    }
    // Mise à jour intégrée : téléchargement par `DownloadManager`, puis installeur système.
    let ai = app.clone();
    cydera_ui::update::set_installer(move |url, rappel| {
        let a = ai.clone();
        std::thread::spawn(move || maj::installer(&a, &url, &*rappel));
    });
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime tokio");
    let handle = rt.handle().clone();

    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    let res = eframe::run_native(
        "Cydera",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(handle, cc.egui_ctx.clone())))
        }),
    );
    if let Err(e) = res {
        log::error!("eframe: {e}");
    }
}

fn system_insets(app: &AndroidApp) -> Option<[f32; 4]> {
    use jni::objects::JObject;
    use std::mem::ManuallyDrop;
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Option<[f32; 4]>> {
        // `activity_as_ptr` est une référence globale : ne pas la libérer en fin de portée.
        let activity = ManuallyDrop::new(unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) });
        let window = env.call_method(&*activity, jni::jni_str!("getWindow"), jni::jni_sig!("()Landroid/view/Window;"), &[])?.l()?;
        let decor = env.call_method(&window, jni::jni_str!("getDecorView"), jni::jni_sig!("()Landroid/view/View;"), &[])?.l()?;
        let insets =
            env.call_method(&decor, jni::jni_str!("getRootWindowInsets"), jni::jni_sig!("()Landroid/view/WindowInsets;"), &[])?.l()?;
        if insets.is_null() {
            return Ok(None);
        }
        let mut v = [0.0f32; 4];
        for (i, name) in [
            jni::jni_str!("getSystemWindowInsetTop"),
            jni::jni_str!("getSystemWindowInsetRight"),
            jni::jni_str!("getSystemWindowInsetBottom"),
            jni::jni_str!("getSystemWindowInsetLeft"),
        ]
        .into_iter()
        .enumerate()
        {
            v[i] = env.call_method(&insets, name, jni::jni_sig!("()I"), &[])?.i()? as f32;
        }
        Ok(Some(v))
    })
    .ok()
    .flatten()
}

/// Lit un extra texte (`--es clé valeur`) de l'intent de lancement.
fn launch_extra(app: &AndroidApp, cle: &str) -> Option<String> {
    use jni::objects::{JObject, JString, JValue};
    use std::mem::ManuallyDrop;
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
        let activity = ManuallyDrop::new(unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) });
        let intent = env.call_method(&*activity, jni::jni_str!("getIntent"), jni::jni_sig!("()Landroid/content/Intent;"), &[])?.l()?;
        if intent.is_null() {
            return Ok(None);
        }
        let key = JString::from_str(env, cle)?;
        let v = env
            .call_method(
                &intent,
                jni::jni_str!("getStringExtra"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&key)],
            )?
            .l()?;
        if v.is_null() {
            return Ok(None);
        }
        let js = env.cast_local::<JString>(v)?;
        Ok(Some(js.try_to_string(env)?))
    })
    .ok()
    .flatten()
}

fn clipboard_manager<'a>(env: &mut jni::Env<'a>, app: &AndroidApp) -> jni::errors::Result<jni::objects::JObject<'a>> {
    use jni::objects::{JObject, JString, JValue};
    use std::mem::ManuallyDrop;
    let activity = ManuallyDrop::new(unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) });
    let name = JString::from_str(env, "clipboard")?;
    env.call_method(
        &*activity,
        jni::jni_str!("getSystemService"),
        jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

/// Texte du presse-papiers (`ClipData.Item.coerceToText`).
fn clipboard_get(app: &AndroidApp) -> Option<String> {
    use jni::objects::{JObject, JString, JValue};
    use std::mem::ManuallyDrop;
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
        let cm = clipboard_manager(env, app)?;
        if cm.is_null() {
            return Ok(None);
        }
        let clip = env.call_method(&cm, jni::jni_str!("getPrimaryClip"), jni::jni_sig!("()Landroid/content/ClipData;"), &[])?.l()?;
        if clip.is_null() || env.call_method(&clip, jni::jni_str!("getItemCount"), jni::jni_sig!("()I"), &[])?.i()? < 1 {
            return Ok(None);
        }
        let item = env
            .call_method(&clip, jni::jni_str!("getItemAt"), jni::jni_sig!("(I)Landroid/content/ClipData$Item;"), &[JValue::Int(0)])?
            .l()?;
        let activity = ManuallyDrop::new(unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) });
        let cs = env
            .call_method(
                &item,
                jni::jni_str!("coerceToText"),
                jni::jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                &[JValue::Object(&activity)],
            )?
            .l()?;
        if cs.is_null() {
            return Ok(None);
        }
        let s = env.call_method(&cs, jni::jni_str!("toString"), jni::jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        let js = env.cast_local::<JString>(s)?;
        Ok(Some(js.try_to_string(env)?))
    })
    .map_err(|e| log::warn!("presse-papiers (lecture) : {e}"))
    .ok()
    .flatten()
}

fn clipboard_set(app: &AndroidApp, text: &str) -> Option<()> {
    use jni::objects::{JString, JValue};
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let cm = clipboard_manager(env, app)?;
        let label = JString::from_str(env, "Cydera")?;
        let txt = JString::from_str(env, text)?;
        let clip = env
            .call_static_method(
                jni::jni_str!("android/content/ClipData"),
                jni::jni_str!("newPlainText"),
                jni::jni_sig!("(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"),
                &[JValue::Object(&label), JValue::Object(&txt)],
            )?
            .l()?;
        env.call_method(&cm, jni::jni_str!("setPrimaryClip"), jni::jni_sig!("(Landroid/content/ClipData;)V"), &[JValue::Object(&clip)])?;
        Ok(())
    })
    .map_err(|e| log::warn!("presse-papiers (écriture) : {e}"))
    .ok()
}

/// Mise à jour de l'APK sans code Java : `DownloadManager` télécharge (avec sa propre
/// notification), on interroge son statut chaque seconde (une `NativeActivity` n'a pas de
/// `BroadcastReceiver`), puis on ouvre l'installeur système sur l'URI `content://` obtenue.
mod maj {
    use android_activity::AndroidApp;
    use cydera_ui::update::Etat;
    use jni::objects::{JObject, JString, JValue};
    use std::mem::ManuallyDrop;
    use std::time::{Duration, Instant};

    const MIME: &str = "application/vnd.android.package-archive";
    const FICHIER: &str = "cydera-maj.apk";
    // Constantes de `DownloadManager` / `Intent` (évite des lectures de champs statiques).
    const STATUT_REUSSI: i32 = 8;
    const STATUT_ECHEC: i32 = 16;
    const VISIBLE_ET_TERMINE: i32 = 1;
    const FLAG_LECTURE_URI: i32 = 0x0000_0001;
    const FLAG_NOUVELLE_TACHE: i32 = 0x1000_0000;
    const DELAI_MAX: Duration = Duration::from_secs(30 * 60);

    fn vm(app: &AndroidApp) -> jni::JavaVM {
        unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut _) }
    }

    fn activite<'a>(env: &mut jni::Env<'a>, app: &AndroidApp) -> ManuallyDrop<JObject<'a>> {
        // Référence globale détenue par android-activity : ne pas la libérer.
        ManuallyDrop::new(unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) })
    }

    fn gestionnaire<'a>(env: &mut jni::Env<'a>, app: &AndroidApp) -> jni::errors::Result<JObject<'a>> {
        let activity = activite(env, app);
        let nom = JString::from_str(env, "download")?;
        env.call_method(
            &*activity,
            jni::jni_str!("getSystemService"),
            jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&nom)],
        )?
        .l()
    }

    /// Met le téléchargement en file ; renvoie son identifiant.
    fn lancer(app: &AndroidApp, url: &str) -> jni::errors::Result<i64> {
        vm(app).attach_current_thread(|env| -> jni::errors::Result<i64> {
            let activity = activite(env, app);
            let dm = gestionnaire(env, app)?;
            let dossier = JString::from_str(env, "Download")?;
            let nom = JString::from_str(env, FICHIER)?;
            // Supprime l'APK d'une mise à jour précédente (sinon DownloadManager suffixe « -1 »).
            let dir = env
                .call_method(
                    &*activity,
                    jni::jni_str!("getExternalFilesDir"),
                    jni::jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
                    &[JValue::Object(&dossier)],
                )?
                .l()?;
            if !dir.is_null() {
                let ancien = env.new_object(
                    jni::jni_str!("java/io/File"),
                    jni::jni_sig!("(Ljava/io/File;Ljava/lang/String;)V"),
                    &[JValue::Object(&dir), JValue::Object(&nom)],
                )?;
                env.call_method(&ancien, jni::jni_str!("delete"), jni::jni_sig!("()Z"), &[])?;
            }
            let jurl = JString::from_str(env, url)?;
            let uri = env
                .call_static_method(
                    jni::jni_str!("android/net/Uri"),
                    jni::jni_str!("parse"),
                    jni::jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                    &[JValue::Object(&jurl)],
                )?
                .l()?;
            let req = env.new_object(
                jni::jni_str!("android/app/DownloadManager$Request"),
                jni::jni_sig!("(Landroid/net/Uri;)V"),
                &[JValue::Object(&uri)],
            )?;
            let mime = JString::from_str(env, MIME)?;
            env.call_method(
                &req,
                jni::jni_str!("setMimeType"),
                jni::jni_sig!("(Ljava/lang/String;)Landroid/app/DownloadManager$Request;"),
                &[JValue::Object(&mime)],
            )?;
            let titre = JString::from_str(env, "Mise à jour de Cydera")?;
            env.call_method(
                &req,
                jni::jni_str!("setTitle"),
                jni::jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/DownloadManager$Request;"),
                &[JValue::Object(&titre)],
            )?;
            env.call_method(
                &req,
                jni::jni_str!("setNotificationVisibility"),
                jni::jni_sig!("(I)Landroid/app/DownloadManager$Request;"),
                &[JValue::Int(VISIBLE_ET_TERMINE)],
            )?;
            env.call_method(
                &req,
                jni::jni_str!("setDestinationInExternalFilesDir"),
                jni::jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Landroid/app/DownloadManager$Request;"),
                &[JValue::Object(&activity), JValue::Object(&dossier), JValue::Object(&nom)],
            )?;
            env.call_method(
                &dm,
                jni::jni_str!("enqueue"),
                jni::jni_sig!("(Landroid/app/DownloadManager$Request;)J"),
                &[JValue::Object(&req)],
            )?
            .j()
        })
    }

    /// (statut, octets reçus, taille totale ou -1).
    fn statut(app: &AndroidApp, id: i64) -> jni::errors::Result<(i32, i64, i64)> {
        vm(app).attach_current_thread(|env| -> jni::errors::Result<(i32, i64, i64)> {
            let dm = gestionnaire(env, app)?;
            let q = env.new_object(jni::jni_str!("android/app/DownloadManager$Query"), jni::jni_sig!("()V"), &[])?;
            let ids = env.new_long_array(1)?;
            ids.set_region(env, 0, &[id])?;
            env.call_method(
                &q,
                jni::jni_str!("setFilterById"),
                jni::jni_sig!("([J)Landroid/app/DownloadManager$Query;"),
                &[JValue::Object(&ids)],
            )?;
            let c = env
                .call_method(
                    &dm,
                    jni::jni_str!("query"),
                    jni::jni_sig!("(Landroid/app/DownloadManager$Query;)Landroid/database/Cursor;"),
                    &[JValue::Object(&q)],
                )?
                .l()?;
            if c.is_null() {
                return Ok((STATUT_ECHEC, 0, -1));
            }
            let mut r = (STATUT_ECHEC, 0, -1);
            if env.call_method(&c, jni::jni_str!("moveToFirst"), jni::jni_sig!("()Z"), &[])?.z()? {
                let col = |env: &mut jni::Env, nom: &str| -> jni::errors::Result<i32> {
                    let n = JString::from_str(env, nom)?;
                    env.call_method(&c, jni::jni_str!("getColumnIndex"), jni::jni_sig!("(Ljava/lang/String;)I"), &[JValue::Object(&n)])?.i()
                };
                let (cs, cr, ct) = (col(env, "status")?, col(env, "bytes_so_far")?, col(env, "total_size")?);
                r.0 = env.call_method(&c, jni::jni_str!("getInt"), jni::jni_sig!("(I)I"), &[JValue::Int(cs)])?.i()?;
                r.1 = env.call_method(&c, jni::jni_str!("getLong"), jni::jni_sig!("(I)J"), &[JValue::Int(cr)])?.j()?;
                r.2 = env.call_method(&c, jni::jni_str!("getLong"), jni::jni_sig!("(I)J"), &[JValue::Int(ct)])?.j()?;
            }
            env.call_method(&c, jni::jni_str!("close"), jni::jni_sig!("()V"), &[])?;
            Ok(r)
        })
    }

    /// Ouvre l'installeur système sur le fichier téléchargé.
    fn ouvrir_installeur(app: &AndroidApp, id: i64) -> jni::errors::Result<bool> {
        vm(app).attach_current_thread(|env| -> jni::errors::Result<bool> {
            let activity = activite(env, app);
            let dm = gestionnaire(env, app)?;
            let uri = env
                .call_method(&dm, jni::jni_str!("getUriForDownloadedFile"), jni::jni_sig!("(J)Landroid/net/Uri;"), &[JValue::Long(id)])?
                .l()?;
            if uri.is_null() {
                return Ok(false);
            }
            let action = JString::from_str(env, "android.intent.action.VIEW")?;
            let intent = env.new_object(
                jni::jni_str!("android/content/Intent"),
                jni::jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&action)],
            )?;
            let mime = JString::from_str(env, MIME)?;
            env.call_method(
                &intent,
                jni::jni_str!("setDataAndType"),
                jni::jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&uri), JValue::Object(&mime)],
            )?;
            env.call_method(
                &intent,
                jni::jni_str!("addFlags"),
                jni::jni_sig!("(I)Landroid/content/Intent;"),
                &[JValue::Int(FLAG_LECTURE_URI | FLAG_NOUVELLE_TACHE)],
            )?;
            env.call_method(
                &*activity,
                jni::jni_str!("startActivity"),
                jni::jni_sig!("(Landroid/content/Intent;)V"),
                &[JValue::Object(&intent)],
            )?;
            Ok(true)
        })
    }

    pub fn installer(app: &AndroidApp, url: &str, rappel: &dyn Fn(Etat)) {
        let id = match lancer(app, url) {
            Ok(id) => id,
            Err(e) => {
                log::warn!("mise à jour : DownloadManager : {e}");
                return rappel(Etat::Echec("Téléchargement impossible.".into()));
            }
        };
        let debut = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            match statut(app, id) {
                Ok((STATUT_REUSSI, ..)) => break,
                Ok((STATUT_ECHEC, ..)) => {
                    return rappel(Etat::Echec("Le téléchargement a échoué : vérifie la connexion et réessaie.".into()));
                }
                Ok((_, recus, total)) => rappel(Etat::Progres(recus.max(0) as u64, (total > 0).then_some(total as u64))),
                Err(e) => {
                    log::warn!("mise à jour : statut : {e}");
                    return rappel(Etat::Echec("Suivi du téléchargement impossible.".into()));
                }
            }
            if debut.elapsed() > DELAI_MAX {
                return rappel(Etat::Echec("Téléchargement trop long : réessaie plus tard.".into()));
            }
        }
        match ouvrir_installeur(app, id) {
            Ok(true) => rappel(Etat::Lance),
            Ok(false) => rappel(Etat::Echec("Fichier téléchargé introuvable.".into())),
            Err(e) => {
                log::warn!("mise à jour : installeur : {e}");
                rappel(Etat::Echec("Impossible d'ouvrir l'installeur Android.".into()));
            }
        }
    }
}
