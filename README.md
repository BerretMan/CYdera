> 🤖 **Projet vibecodé.** Cydera est écrit en « vibe coding » avec Claude, l'IA d'Anthropic. Lucas donne l'idée, décrit les fonctionnalités, teste et valide sur ses appareils ; le code (Rust, CI/CD, packaging) est très majoritairement écrit par l'IA. C'est une beta : attends-toi à des bugs, et signale-les dans les [Issues](https://github.com/BerretMan/CYdera/issues).

# Cydera

L'appli tout-en-un des étudiants de CY Tech, sur **Linux** et **Android** (version beta).

- **Emploi du temps Celcat** : vues jour, semaine et agenda, mise en cache hors ligne, examens à venir.
- **Mails étudiants** : boîte de réception du webmail CY (connexion SSO), lecture des messages.
- **Restos U** : menus du Crous et fréquentation en direct.
- **Chrono de révisions** : sessions par matière et statistiques.
- **Moyennes** : modèles de fiche ECTS (UE, EC, coefficients, notes seuil) et alertes.
- **Synchronisation entre appareils** : PC et téléphone appairés, en pair à pair chiffré (iroh/QUIC), sans serveur.
- **Mises à jour automatiques** : l'appli détecte les nouvelles versions publiées ici et propose de les installer.

Les identifiants restent sur l'appareil. Ils ne sont envoyés qu'aux services de l'université et, si tu le demandes lors de l'appairage, à tes propres appareils.

> Projet étudiant **non officiel**, sans lien avec CY Tech ni CY Cergy Paris Université.

## Installation

Les fichiers sont dans les [Releases GitHub](https://github.com/BerretMan/CYdera/releases). Choisis la plus récente (les versions beta sont marquées « Pre-release »).

### Linux (x86_64)

```bash
tar xzf cydera-linux-x86_64.tar.gz
./cydera-linux-x86_64/install.sh          # installe dans ~/.local (sans sudo)
```

Cydera apparaît ensuite dans le menu des applications. Pour désinstaller : `./cydera-linux-x86_64/install.sh --desinstaller` (la configuration et les données sont conservées).

### Android (8.0 et plus)

1. Sur le téléphone, télécharge `cydera.apk` depuis la release.
2. Ouvre le fichier. Android demande d'autoriser l'installation depuis le navigateur : accepte.

### Mises à jour

Au lancement, puis toutes les 6 heures, Cydera interroge les releases GitHub. Si une version plus récente existe, un bandeau la propose. Trois choix : **Installer**, **Plus tard** ou **Ignorer cette version**. L'écran « À propos » permet aussi de vérifier à la main et de couper la vérification automatique.

- **Linux** : l'appli télécharge l'archive, vérifie sa somme SHA-256, remplace son exécutable puis propose de redémarrer.
- **Android** : l'appli télécharge l'APK (une notification montre la progression), puis ouvre l'installeur système. La première fois, Android demande d'autoriser Cydera à installer des applis.

Une beta reçoit les betas suivantes. Une version stable ne reçoit que des versions stables.

### Vérifier un téléchargement

Chaque release contient `SHA256SUMS` et une attestation de provenance GitHub (le fichier a été construit par la CI de ce dépôt) :

```bash
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify cydera.apk -R BerretMan/CYdera
```

## Compiler depuis les sources

Prérequis : Rust 1.93 ou plus récent (`rustup`). Sous Debian/Ubuntu, installe aussi `pkg-config libxkbcommon-dev libwayland-dev`.

```bash
cargo build --release -p cydera        # → target/release/cydera
cargo run -- --dump 7                  # mode console : affiche les cours des 7 prochains jours
cargo test --workspace
```

APK Android : il faut le SDK et le NDK Android dans `~/Android/Sdk`, `cargo install cargo-ndk` et les cibles `rustup target add aarch64-linux-android armv7-linux-androideabi`. Ensuite :

```bash
apps/android/build-apk.sh debug        # → target/apk/debug/cydera.apk
```

Les variables d'environnement reconnues sont documentées dans [`.env.example`](.env.example).

## Publier une version (mainteneurs)

```bash
scripts/version.sh 0.1.0-beta.2   # met à jour Cargo.toml, commite et crée le tag v0.1.0-beta.2
git push --follow-tags            # le workflow Release construit et publie l'archive, l'APK et SHA256SUMS
```

La version de `Cargo.toml` est la seule source : le `versionName` et le `versionCode` de l'APK en sont dérivés. La CI vérifie que le tag lui correspond.

## Contributeurs

| | Rôle |
|---|---|
| [**Lucas** (BerretMan)](https://github.com/BerretMan) | Idée, conception, tests et maintenance |
| [**Claude**](https://claude.ai) (Anthropic) | Développement assisté par IA : code, tests, CI/CD, documentation |

Les commits écrits avec l'IA portent la mention `Co-Authored-By: Claude`.

## Licence

*À définir.*
