#!/usr/bin/env bash
# Installe Cydera pour l'utilisateur courant (aucun droit administrateur requis) :
#   ~/.local/bin/cydera, icône et lanceur dans ~/.local/share.
# Désinstallation : ./install.sh --desinstaller (les données et la config sont conservées).
set -euo pipefail
ICI=$(cd "$(dirname "$0")" && pwd)
BIN=${XDG_BIN_HOME:-$HOME/.local/bin}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
ICONE=$DATA/icons/hicolor/512x512/apps/cydera.png
LANCEUR=$DATA/applications/cydera.desktop

if [ "${1:-}" = "--desinstaller" ]; then
  rm -f "$BIN/cydera" "$ICONE" "$LANCEUR"
  echo "Cydera désinstallé (config et données conservées dans ~/.config/cydera et ~/.local/share/cydera)."
  exit 0
fi

install -Dm755 "$ICI/cydera" "$BIN/cydera"
install -Dm644 "$ICI/cydera.png" "$ICONE"
mkdir -p "$(dirname "$LANCEUR")"
# Chemin absolu : le lanceur marche même si ~/.local/bin n'est pas dans le PATH de la session graphique.
sed "s|^Exec=.*|Exec=$BIN/cydera|" "$ICI/cydera.desktop" > "$LANCEUR"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$DATA/icons/hicolor" || true

echo "Cydera installé : $BIN/cydera"
case ":$PATH:" in *":$BIN:"*) ;; *) echo "Astuce : ajoute $BIN à ton PATH pour lancer « cydera » depuis un terminal." ;; esac
echo "Les mises à jour suivantes s'installent depuis l'appli (bandeau ou « À propos »)."
