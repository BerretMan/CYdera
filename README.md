<div align="center">

# Cydera

**All-in-one student companion for CY Tech — Linux desktop and Android.**

[![CI](https://github.com/BerretMan/CYdera/actions/workflows/ci.yml/badge.svg)](https://github.com/BerretMan/CYdera/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/BerretMan/CYdera?include_prereleases&label=release)](https://github.com/BerretMan/CYdera/releases)
![Status](https://img.shields.io/badge/status-beta-orange)
![Vibe coded](https://img.shields.io/badge/vibe%20coded-Claude%20(Anthropic)-D97757)

![Rust](https://img.shields.io/badge/Rust-2024%20edition-000000?logo=rust&logoColor=white)
![egui](https://img.shields.io/badge/UI-egui%20%2F%20eframe-4B8BBE)
![wgpu](https://img.shields.io/badge/render-wgpu-5C2D91)
![Tokio](https://img.shields.io/badge/async-Tokio-463F3F)
![iroh](https://img.shields.io/badge/P2P-iroh%20(QUIC)-7B61FF)
![rustls](https://img.shields.io/badge/TLS-rustls-2E7D32)
![Linux](https://img.shields.io/badge/Linux-x86__64-FCC624?logo=linux&logoColor=black)
![Android](https://img.shields.io/badge/Android-8.0%2B-3DDC84?logo=android&logoColor=white)
![GitHub Actions](https://img.shields.io/badge/CI%2FCD-GitHub%20Actions-2088FF?logo=githubactions&logoColor=white)

</div>

> [!NOTE]
> **This project is vibe coded.** Most of the source code, tests, CI/CD pipelines and documentation were written by an AI assistant (Claude, by Anthropic), under human direction, review and testing. It is beta software: please report any issue through the [issue tracker](https://github.com/BerretMan/CYdera/issues).

> [!IMPORTANT]
> Cydera is an unofficial student project. It is not affiliated with, endorsed by, or supported by CY Tech or CY Cergy Paris Université.

## Contents

- [Overview](#overview)
- [Features](#features)
- [Installation](#installation)
- [Updates](#updates)
- [Building from source](#building-from-source)
- [Project structure](#project-structure)
- [Release process](#release-process)
- [Security and privacy](#security-and-privacy)
- [License](#license)

## Overview

Cydera brings together the day-to-day tools of a CY Tech student in a single native application: the Celcat timetable, the university webmail, the Crous restaurant menus, a revision stopwatch and a grade-average calculator. The same Rust code base runs on Linux desktop and on Android. Paired devices synchronise their data directly with each other over an encrypted peer-to-peer connection, with no intermediate server.

The user interface is in French, as is the target audience.

## Features

| Area | Description |
|---|---|
| **Timetable** | Celcat schedule with day, week and agenda views, offline cache and upcoming-exam tracking. |
| **Mail** | Student webmail inbox (university SSO), message reading. |
| **Restaurants** | Crous menus and live attendance. |
| **Revisions** | Per-subject study stopwatch and statistics. |
| **Grades** | ECTS-sheet profiles (units, coefficients, thresholds), weighted averages and threshold alerts. |
| **Synchronisation** | End-to-end encrypted peer-to-peer sync between paired devices (iroh / QUIC). |
| **Updates** | Automatic detection of new GitHub releases, with in-app installation. |

## Installation

Prebuilt packages are published on the [Releases page](https://github.com/BerretMan/CYdera/releases). Beta versions are marked as *Pre-release*.

| Platform | Asset | Requirements |
|---|---|---|
| Linux | `cydera-linux-x86_64.tar.gz` | x86_64, glibc 2.35 or later (e.g. Ubuntu 22.04+, Debian 12+, Fedora 36+) |
| Android | `cydera.apk` | Android 8.0 (API 26) or later, ARM64 or ARMv7 |

### Linux

```bash
tar xzf cydera-linux-x86_64.tar.gz
./cydera-linux-x86_64/install.sh
```

The script installs the application for the current user only, with no administrator rights:

| Item | Location |
|---|---|
| Binary | `~/.local/bin/cydera` |
| Icon | `~/.local/share/icons/hicolor/512x512/apps/cydera.png` |
| Desktop entry | `~/.local/share/applications/cydera.desktop` |

To uninstall, run `./cydera-linux-x86_64/install.sh --desinstaller`. Configuration and data are kept.

### Android

1. Download `cydera.apk` on the device.
2. Open the file and allow installation from that source when prompted.

### Verifying downloads

Each release includes a `SHA256SUMS` file and a GitHub build-provenance attestation, which proves the files were built by this repository's CI.

```bash
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify cydera.apk -R BerretMan/CYdera
```

## Updates

At startup, and then every six hours, Cydera queries the GitHub Releases API. When a newer version is available, a banner offers to install it, postpone it or skip that version. The *À propos* screen also lets you check manually or turn off automatic checks.

| Platform | Update mechanism |
|---|---|
| Linux | The archive is downloaded and checked against `SHA256SUMS`. The running binary is then atomically replaced, and the application offers to restart. |
| Android | The APK is downloaded through the system `DownloadManager`, then handed to the system package installer. The first update asks for permission to install apps from Cydera. |

Pre-release builds are offered newer pre-releases and stable releases. Stable builds are offered stable releases only.

## Building from source

### Prerequisites

- Rust 1.93 or later ([rustup](https://rustup.rs)).
- On Debian/Ubuntu: `sudo apt install pkg-config libxkbcommon-dev libwayland-dev`.

### Desktop

```bash
cargo build --release -p cydera     # binary: target/release/cydera
cargo test --workspace
cargo run -- --dump 7               # headless mode: print the next 7 days of courses
```

### Android

Additional requirements:

- the Android SDK (platform 34, build-tools 34.0.0) and NDK in `~/Android/Sdk`;
- `cargo install cargo-ndk`;
- `rustup target add aarch64-linux-android armv7-linux-androideabi`.

```bash
apps/android/build-apk.sh debug     # output: target/apk/debug/cydera.apk
```

Every supported environment variable is documented in [`.env.example`](.env.example).

## Project structure

```
crates/
  cydera-core/    business logic: Celcat, webmail, Crous, grades, journal, updates
  cydera-sync/    peer-to-peer synchronisation engine (iroh)
  cydera-ui/      egui user interface shared by desktop and Android
apps/
  desktop/        Linux binary (window and headless modes)
  android/        NativeActivity entry point and JNI platform services
packaging/linux/  desktop entry and user installer
.github/          CI, release and security-audit workflows, Dependabot
```

## Release process

The version in the root `Cargo.toml` is the single source of truth. The Android `versionName` and `versionCode` are derived from it at build time.

```bash
scripts/version.sh 0.1.0-beta.2     # bump the version, commit, create the annotated tag
git push --follow-tags              # triggers the Release workflow
```

The release workflow does the following:

1. It checks that the tag matches the crate version.
2. It builds the Linux archive and the signed APK.
3. It generates `SHA256SUMS` and provenance attestations.
4. It publishes the GitHub release, marked as a pre-release for `-beta.N` versions.

## Security and privacy

- Credentials are stored only on the user's devices. They are sent only to the university's own services and, at pairing time and on explicit request, to the user's other devices.
- Device synchronisation is end-to-end encrypted. A peer is accepted only if it is on an explicit allowlist, checked before any data is read.
- Release signing keys are kept in a restricted GitHub environment. Each release job decodes the key into a temporary file and deletes it at the end of the job.
- Dependencies are audited weekly against the RustSec advisory database.

## License

No license has been selected yet. Until one is added, all rights are reserved by the author.
