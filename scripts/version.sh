#!/usr/bin/env bash
# Passe le workspace à une nouvelle version, crée le commit et le tag annoté.
#   scripts/version.sh 0.1.0-beta.2     (ou 0.1.0 pour une version stable)
# La version de Cargo.toml est la seule source : l'APK (versionName/versionCode) et le
# workflow de release la relisent. Le push reste manuel : git push --follow-tags
set -euo pipefail
V=${1:?usage : scripts/version.sh X.Y.Z[-beta.N]}
V=${V#v}
[[ $V =~ ^[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$ ]] || { echo "Version « $V » invalide (attendu X.Y.Z ou X.Y.Z-beta.N)"; exit 1; }
cd "$(dirname "$0")/.."
[ -z "$(git status --porcelain)" ] || { echo "Arbre de travail non propre : commite d'abord."; exit 1; }
git rev-parse -q --verify "refs/tags/v$V" >/dev/null && { echo "Le tag v$V existe déjà."; exit 1; }
sed -i "0,/^version = \".*\"$/s//version = \"$V\"/" Cargo.toml
cargo update --workspace --quiet # met à jour Cargo.lock pour les crates du workspace
git add Cargo.toml Cargo.lock
git commit -m "Version $V"
git tag -a "v$V" -m "Cydera $V"
echo "Commit et tag v$V créés. Publie avec : git push --follow-tags"
