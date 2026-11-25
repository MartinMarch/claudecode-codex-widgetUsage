#!/usr/bin/env bash
# Builds Agent Gauge from source and installs it for the current user.
# macOS → ~/Applications/Agent Gauge.app   Linux → ~/.local/bin/agent-gauge (+ .desktop entry)
set -euo pipefail
cd "$(dirname "$0")/.."

need() { command -v "$1" >/dev/null 2>&1 || { echo "Falta $1. $2"; exit 1; }; }
need node "Instala Node 20 o superior: https://nodejs.org"
need cargo "Instala Rust: curl https://sh.rustup.rs -sSf | sh"

if [[ "$(uname)" == "Linux" ]] && ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
  echo "Faltan las librerías de WebKitGTK. En Debian/Ubuntu:"
  echo "  sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev build-essential"
  exit 1
fi

echo "Instalando dependencias…"
npm ci --no-audit --no-fund
echo "Compilando (la primera vez tarda unos minutos)…"

case "$(uname)" in
  Darwin)
    npx tauri build --bundles app
    APP="src-tauri/target/release/bundle/macos/Agent Gauge.app"
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/Agent Gauge.app"
    cp -R "$APP" "$HOME/Applications/"
    open "$HOME/Applications/Agent Gauge.app"
    echo "Listo: ~/Applications/Agent Gauge.app (activa 'Abrir al iniciar sesión' desde el icono de la barra de menús)."
    ;;
  Linux)
    npx tauri build --no-bundle
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" "$HOME/.local/share/icons"
    install -m 755 src-tauri/target/release/agent-gauge "$HOME/.local/bin/agent-gauge"
    cp src-tauri/icons/128x128.png "$HOME/.local/share/icons/agent-gauge.png"
    cat > "$HOME/.local/share/applications/agent-gauge.desktop" <<DESK
[Desktop Entry]
Name=Agent Gauge
Comment=Uso de Claude Code y Codex
Exec=$HOME/.local/bin/agent-gauge
Icon=$HOME/.local/share/icons/agent-gauge.png
Type=Application
Categories=Development;
DESK
    nohup "$HOME/.local/bin/agent-gauge" >/dev/null 2>&1 &
    echo "Listo: ~/.local/bin/agent-gauge"
    ;;
  *) echo "Sistema no soportado por este script. En Windows usa scripts/install.ps1"; exit 1 ;;
esac
