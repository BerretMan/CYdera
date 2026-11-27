#!/usr/bin/env bash
# Construit l'APK Cydera : ./build-apk.sh [debug|release]
# Prérequis : SDK/NDK (~/Android/Sdk), cargo-ndk, cibles rustup Android.
# Release : clé ~/cydera.jks (créée au premier lancement), mot de passe dans $CYDERA_KS_PASS.
set -euo pipefail
MODE=${1:-debug}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | tail -1)}
BT=$ANDROID_HOME/build-tools/34.0.0

# Version : seule source = [workspace.package] du Cargo.toml racine.
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -1)
[[ $VERSION =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-beta\.([0-9]+))?$ ]] \
  || { echo "Version « $VERSION » invalide (attendu X.Y.Z ou X.Y.Z-beta.N)"; exit 1; }
# versionCode croissant : X*1_000_000 + Y*10_000 + Z*100 + (N pour une pré-version, 99 pour la finale).
MAJ=${BASH_REMATCH[1]} MIN=${BASH_REMATCH[2]} PAT=${BASH_REMATCH[3]} PRE=${BASH_REMATCH[5]:-}
SUFFIX=${PRE:-99}
(( MIN < 100 && PAT < 100 && SUFFIX <= 99 )) && { [ -z "$PRE" ] || (( PRE < 99 )); } \
  || { echo "Version « $VERSION » hors limites pour le versionCode"; exit 1; }
VERSION_CODE=$(( MAJ * 1000000 + MIN * 10000 + PAT * 100 + SUFFIX ))
echo "Cydera $VERSION (versionCode $VERSION_CODE), mode $MODE"
OUT=$ROOT/target/apk/$MODE
rm -rf "$OUT" && mkdir -p "$OUT/lib"

FLAGS=(); [ "$MODE" = release ] && FLAGS+=(--release)
TARGETS=${CYDERA_ABIS:-"arm64-v8a armeabi-v7a"}
NDK_T=(); for t in $TARGETS; do NDK_T+=(-t "$t"); done
(cd "$ROOT" && cargo ndk "${NDK_T[@]}" -o "$OUT/lib" build -p cydera-android "${FLAGS[@]}")

# Ressources + manifeste
"$BT/aapt2" compile --dir "$HERE/res" -o "$OUT/res.zip"
"$BT/aapt2" link -o "$OUT/base.apk" -I "$ANDROID_HOME/platforms/android-34/android.jar" \
  --manifest "$HERE/AndroidManifest.xml" "$OUT/res.zip" --auto-add-overlay \
  --version-name "$VERSION" --version-code "$VERSION_CODE" --replace-version
# Bibliothèques natives (non compressées + alignées : requis pour hasCode=false / minSdk≥23)
(cd "$OUT" && zip -q -r -0 base.apk lib)
"$BT/zipalign" -f -p 4 "$OUT/base.apk" "$OUT/aligned.apk"

if [ "$MODE" = release ]; then
  KS=${CYDERA_KS:-$HOME/cydera.jks}
  [ -f "$KS" ] || { echo "Clé absente : keytool -genkey -v -keystore $KS -alias cydera -keyalg RSA -keysize 2048 -validity 10000"; exit 1; }
  "$BT/apksigner" sign --ks "$KS" --ks-key-alias cydera --ks-pass env:CYDERA_KS_PASS --out "$OUT/cydera.apk" "$OUT/aligned.apk"
else
  KS=$HOME/.android/debug.keystore
  [ -f "$KS" ] || keytool -genkey -v -keystore "$KS" -alias androiddebugkey -storepass android -keypass android -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null
  "$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --out "$OUT/cydera.apk" "$OUT/aligned.apk"
fi
echo "APK : $OUT/cydera.apk"
