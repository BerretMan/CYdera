# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Cydera is a CY Tech all-in-one app (Celcat timetable, student IMAP mail, Crous restaurant menus, revision stopwatch, grade averages) written in Rust with egui, running on Linux desktop and Android, with encrypted peer-to-peer sync between paired devices. It is a Rust port of an Obsidian Dataview script (`example.md`, local only, not in the public repo). Code, comments, UI strings and error messages are in **French**: keep it that way.

## Commands

```bash
cargo build                       # desktop only (default-members = apps/desktop)
cargo build --release -p cydera   # release desktop binary → target/release/cydera
cargo run -- --dump [days]        # headless: scrape Celcat and print courses (default 14 days)
cargo run -- --mail               # headless: list the webmail inbox (with message ids)
cargo run -- --mail-body <id>     # headless: print one message's text (does not mark it read)

cargo test --workspace                            # all tests (apps/android compiles to an empty lib off-Android)
# `-p cydera-ui` alone fails to build (eframe has no winit backend without the desktop crate's features): add `-p cydera`
cargo test -p cydera-core journal                 # one crate / filter by test name
cargo test -p cydera-sync --test pairing          # one integration test file
cargo test -p cydera-sync --test internet_sync -- --ignored   # real iroh relays, needs internet

cargo run -p cydera-sync --example pc_peer        # standalone PC sync peer for testing pairing/sync with the phone/emulator

# regenerate the built-in grade templates from the ECTS sheet fixture (crates/cydera-core/tests/fixtures/,
# NOT published: the school's PDF is gitignored, tests that need it skip themselves when it's missing)
cargo test -p cydera-core --features ects-pdf dump_templates -- --ignored

apps/android/build-apk.sh debug|release           # → target/apk/<mode>/cydera.apk
CYDERA_ABIS=x86_64 apps/android/build-apk.sh debug  # emulator-only build

cargo fmt --all                                   # rustfmt.toml: max_width 140 (CI runs --check)
cargo clippy --workspace --all-targets -- -D warnings   # CI is strict, keep it at zero warnings

scripts/version.sh 0.1.0-beta.2                   # bump version, commit, annotated tag; then `git push --follow-tags`
CYDERA_UPDATE_FROM=0.0.1 cargo run                # pretend to be an old version to test the update flow
```

Android prerequisites: SDK+NDK in `~/Android/Sdk`, `cargo-ndk`, rustup targets `aarch64-linux-android`, `armv7-linux-androideabi` (+ `x86_64-linux-android` for the emulator). Release signing uses `~/cydera.jks` with the password in `$CYDERA_KS_PASS`.

After changing the desktop app: build release, copy to `~/.local/bin/cydera`, relaunch. The launcher template is `packaging/linux/cydera.desktop` (`Exec=cydera`, rewritten to an absolute path by `packaging/linux/install.sh`, which also installs `crates/cydera-ui/assets/icon.png` as the `cydera` icon): update both if assets move.

## Versioning, CI/CD and releases

- **Single version source:** `[workspace.package] version` in the root `Cargo.toml` (`X.Y.Z` or `X.Y.Z-beta.N`, no other pre-release labels). `build-apk.sh` derives `versionName` and `versionCode` (`X*1e6 + Y*1e4 + Z*100 + N`, 99 for a final release) and passes them to `aapt2 link --replace-version`; the manifest has no version.
- **Workflows** (`.github/workflows/`): `ci.yml` (desktop: fmt, clippy `-D warnings`, tests, release build on ubuntu-22.04; Android: arm64 debug APK, Android clippy, versionName check), `release.yml` (tag `v*` must equal the Cargo version; builds `cydera-linux-x86_64.tar.gz` and the signed `cydera.apk`, then `SHA256SUMS`, provenance attestations, GitHub release, pre-release when the version has `-`), `audit.yml` (RustSec, weekly). The Rust version is pinned by `RUST_VERSION` in ci.yml and release.yml (no rust-toolchain.toml: a second local toolchain would invalidate `target/` and disk is tight).
- **Signing:** the release job decodes the `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASS` secrets of the GitHub environment `release` into a temporary keystore (`CYDERA_KS`), deleted at the end. It must be the same key as `~/cydera.jks`, or Android refuses updates over installed copies.
- **Release asset names are fixed** (`cydera.apk`, `cydera-linux-x86_64.tar.gz`, `SHA256SUMS`): the in-app updater looks them up by name.
- **In-app updates:** `core::update` reads `GET /repos/BerretMan/CYdera/releases` (not `/releases/latest`, which skips pre-releases), picks the highest semver above the running version (pre-releases only offered to pre-release builds), downloads, and on desktop verifies SHA-256, extracts the binary and atomically renames it over `current_exe()` (strip the ` (deleted)` suffix). `cydera_ui::update::Maj` holds the state (own mpsc channel, polled from `App::poll`; auto check at startup then every 6 h if `Config::maj_auto`), `ui::update` draws the banner, the "Mise à jour" page (`top_page` id `update`) and the About section. Android installs through the host provider `cydera_ui::update::set_installer`: JNI `DownloadManager` (polled every second) then `ACTION_VIEW` on the `content://` URI (`REQUEST_INSTALL_PACKAGES` permission). Test on Android with `am start … --es update_from 0.0.1`.
- **Never commit** keystores, `.env`, the ECTS PDFs, or `example.md` (personal ids); see `.gitignore`. `Config::default()` has empty credentials on purpose (public repo); an empty `username` sends the user to the login screen.

## Architecture

Cargo workspace (edition 2024, resolver 3):

- **`crates/cydera-core`**: business logic, no UI and no OS assumptions. `celcat` (login token + cookie scraping → `model::Cours`), `cache::Store` (courses cached per week keyed by Monday), `mail` (webmail scraping: SSO chain mail.etu.cyu.fr → Shibboleth IdP idp.cyu.fr → CAS authentification.cyu.fr with the ENT/Celcat credentials, incl. the IdP attribute-consent form, then Zimbra SOAP JSON `SearchRequest` + raw MIME via `/service/home/~/?id=`, parsed with `mailparse`; IMAP is refused for federated accounts. `CYDERA_MAIL_DEBUG=1` traces the SSO steps without values), `resto`, `study::Chrono` (revision stopwatch), `journal` (sync event log), `grades` (see below), `files` (atomic writes in the data dir), `config`.
- **`crates/cydera-sync`**: P2P sync on iroh (QUIC). `engine::SyncEngine` (custom ALPN `cydera/sync/1`), `peers::PeerList` (allowlist of `EndpointId`s, checked before any data is read), `wire` (protocol messages), `pairing` (one-time ticket `cydera1:…`).
- **`crates/cydera-ui`**: shared egui UI for both platforms. `app.rs` holds the single big `App` state struct and every window. `p2p.rs` runs the sync engine in the background and exposes a `Status` to the UI. `ui/` contains the views (`week`, `day`, `agenda` mobile day view, `revisions`, `devices`, `mobile` tab bar, `gesture`, `theme`…). `lib.rs` re-exports core modules, so `crate::model`, `crate::config` etc. work inside the UI crate.
- **`apps/desktop`**: `cydera` binary (eframe window + headless `--dump`/`--mail` modes).
- **`apps/android`**: `cdylib` loaded by a Java-less `NativeActivity` (`hasCode=false`, package `fr.cydera.app`). `android_main` wires platform services into the shared UI through JNI.

Cross-cutting patterns that need several files to understand:

- **Host-provided services.** Core and UI never call platform APIs directly. The host installs them at startup through `OnceLock` providers: `core::platform::init(Dirs)` (mandatory on Android, where `dirs` is unavailable; desktop falls back to `dirs`), `ui::insets::set_provider` (system bars/keyboard via JNI `getRootWindowInsets`), `ui::clipboard::set_provider` (JNI `ClipboardManager`), `update::set_installer` (JNI `DownloadManager` + package installer). To add a platform feature, follow the same pattern.
- **Async without blocking egui.** `App` owns a `tokio::runtime::Handle`; network work is spawned on it and results come back over `std::sync::mpsc` channels (`SyncMsg`, `MailMsg`, resto channel) that `App::poll` drains each frame, then `ctx.request_repaint()`.
- **Course rendering.** All views share one visual style (the mobile one): `ui::card::course_card` draws a course in a rect dictated by its time slot and adapts to the rect size (one-line micro mode under 40 px, tighter fonts in narrow columns). `ui::agenda::timeline` is the time-scaled day view used by both mobile and desktop day views. Every view uses the fixed 8:00 to 18:30 range (`agenda::DAY_START`/`DAY_END`, `offset_min` clamps times into it), and overlapping courses are split into lanes by `agenda::lanes`. On mobile (< 768 px) the day and week views scale to the available height so the whole 8:00 to 18:30 day is always visible without scrolling (user requirement).
- **Responsive layout.** `app.rs::breakpoint`: Mobile < 768 px, Medium < 1280 px, Wide. The Android build uses the same code in Mobile mode (tab bar, "Plus" sheet, swipe gestures, `Key::BrowserBack` = Android back).
- **Secondary screens = window or page.** Every secondary screen (exams, revisions, resto, mails, devices, about, update, course detail, login, pair request) goes through `ui::page::show(ctx, id, title, window, open, scroll, add)`: a floating `egui::Window` on desktop, a fixed full-screen page on mobile (header with a 48 pt back button, tab bar stays visible). Each frame `App::ui` calls `page::set_area` with the page rect and `App::top_page()`; only that one page is drawn (navigation stack by priority), and Android back pops it via `App::close_top_page()`. New screens must use this helper, not `egui::Window` directly; in page mode (`page::is_page`) use ≥ 44 pt touch targets and let scroll areas fill `ui.available_height()`.
- **Sync data model.** Only revision sessions and custom subjects are synced; Celcat courses are re-downloaded by each device. `core::journal` is an append-only per-device event log with a version vector; displayed state is a projection (delete wins over add, events deduplicated by id). `Chrono` writes `journal.json` and still writes the legacy `revisions.json` alongside it. The engine is connected through `Chrono::journal_handle()` / `reload_from_journal()`.
- **Grades (`core::grades`).** A `Profile` is the UE → EC structure of a school "fiche ECTS" (UE coef/ECTS, EC coef and threshold "note seuil", EC coef 0 = not graded); `Grade`s are weighted marks of one EC (`value/out_of`, `weight`). `calc::report` computes EC, UE (EC weighted by coef), semester and year (UE weighted by coef) averages, threshold alerts and "provisional" flags. UE/EC ids are deterministic (`s2.ue2a.sae1`) so a template and an import of the same sheet match. Built-in templates (`grades/templates/*.json`, ING2 GI-a/GI-b) are generated by the PDF parser `grades::ects` (cargo feature `ects-pdf`, enabled only by `apps/desktop`; positions from `pdf-extract`'s `OutputDev`, columns found from the "ECTS"/"Coef"/"Note-seuil" headers, UEs differing only by a trailing letter = one profile per letter). The UI gets the parser and the `rfd` file dialog through host providers in `cydera_ui::files`; Android has neither, so imports happen on the PC and arrive by sync. Screen: `ui::grades` (keyboard-aware fields via `scroll_to_me`).
- **Journal ops for grades.** `ProfileSave`/`GradeSave`/`ActiveProfile` are last-writer-wins registers ordered by (`ts` ms, device, seq); `ProfileDel`/`GradeDel` win over saves; `grades::project(&Journal)` is the projection and `grades::Book` the UI-side state (shares `Chrono::journal_handle()`, persists with `Journal::persist`). `Op::Unknown` (`#[serde(untagged)]`, last variant) keeps events from newer versions so adding ops later doesn't break older peers; versions before this change don't have it, so every device had to be updated together once.
- **TLS crypto provider.** iroh pulls in `ring` and reqwest `aws-lc-rs`; rustls then panics at the first `ClientConfig::builder()`, and with `panic = "abort"` in release the app dies instantly. Every entry point must call `cydera_core::net::crypto_init()`, and HTTP clients must come from `net::client_builder()` (Android also needs `webpki-roots` through `tls_client_config()`). `cydera-sync/tests/tls_provider.rs` guards against regressions.

## Data locations

Desktop: config `~/.config/cydera/config.json`, course cache `~/.cache/cydera/cours.json`, revisions/journal `~/.local/share/cydera/`. Android: everything under the app's `files/{config,cache,data}`. `config::migrate_legacy()` moves the old `cyagartha`/`cydesktop` directories. All environment variables are listed in `.env.example`. Passwords: `CELCAT_PASSWORD` / `CYU_MAIL_PASSWORD` env vars, then files saved by the login screen, then `~/CELCAT_PASSWORD` / `~/CYU_MAIL_PASSWORD`.

## Gotchas

- eframe is `default-features = false` at the workspace level. Android enables `android-native-activity`, `default_fonts`, `wgpu`: accesskit is incompatible with native-activity, and `glow` changes the `on_exit` signature.
- egui margins are `i8`: always use `saturating_add` when adding insets (the Android keyboard is about 385 pt).
- The proportional font lacks ▲ ◌ ○ ›: draw these glyphs with the painter (the monospace font has them).
- In the agenda view, measure content height *before* the `ScrollArea` and set `item_spacing.y = 0`, or it overflows.
- On Android, the keyboard's Paste never reaches a NativeActivity, so the app shows its own "COLLER" pill (`App::clipboard_bridge` injects `Event::Paste` in `raw_input_hook`).
- `dev` profile uses `debug = "line-tables-only"` on purpose (iroh + wgpu otherwise produce several GB of symbols). Disk space is tight: remove `target/*-linux-android` and `target/apk` after Android work.
- Android emulator: run with `-gpu host -no-window` (software GPU crashes adb). Stop it with `pkill -x qemu-system-x86`, never `pkill -f emulator`. Pass long strings such as pairing tickets with `am start -n fr.cydera.app/android.app.NativeActivity --es ticket …`, because `adb shell input text` corrupts them. Disable the emulator's network before typing fake passwords so Celcat is never contacted.
- After installing a desktop build, check that a window actually opens, not just that the process exists.
- Android keyboard: insets are polled once per frame (`raw_input_hook`), so `App::ui` repaints every frame while they change (`last_insets`), mobile pages paint their background down to the tab bar or the screen bottom (`page::set_area` `bg_bottom`), and focused fields are scrolled into view without animation (`grades::keep_visible`). An animated scroll restarted on every resize made the page jump.
- Red rectangles around widgets in debug APKs/builds are egui's `debug.show_unaligned` (on with `debug_assertions`, e.g. during an animated scroll), not a layout bug; release builds don't show them.
