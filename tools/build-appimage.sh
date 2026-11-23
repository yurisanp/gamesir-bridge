#!/usr/bin/env bash
# Monta o AppImage a partir de binários já compilados.
#
#   tools/build-appimage.sh [BINDIR] [SAÍDA]
#
# BINDIR padrão: target/x86_64-unknown-linux-musl/release (binários estáticos, rodam em
# qualquer distro). Precisa do appimagetool no PATH ou em $APPIMAGETOOL.
# Com APPDIR_ONLY=1, só monta o AppDir (útil para testar sem o appimagetool).
#
# O app gráfico não é estático (usa GTK4): compile com a glibc do sistema
# (`cargo build --release -p gamesir-bridge-app`) e passe o caminho em APP_BIN. Sem ele,
# sai um AppImage só com a linha de comando.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINDIR="${1:-$ROOT/target/x86_64-unknown-linux-musl/release}"
VERSION="$("$BINDIR/gamesir-bridged" --version | awk '{print $2}')"
ARCH="${ARCH:-x86_64}"
OUT="${2:-$ROOT/dist/GameSir_Bridge-$VERSION-$ARCH.AppImage}"
TOOL="${APPIMAGETOOL:-appimagetool}"
WORK="$(mktemp -d)"
APPDIR="$WORK/GameSirBridge.AppDir"
ID=io.github.yurisanp.GameSirBridge

mkdir -p "$APPDIR/usr/bin" "$(dirname "$OUT")"
install -m755 "$ROOT/data/appimage/AppRun" "$APPDIR/AppRun"
install -m755 "$BINDIR/gamesir-bridged" "$BINDIR/gamesir-bridge" "$APPDIR/usr/bin/"
APP_BIN="${APP_BIN:-$ROOT/target/release/gamesir-bridge-app}"
DESKTOP="$ROOT/data/appimage/$ID.desktop"
if [[ -x "$APP_BIN" ]]; then
    "$ROOT/tools/bundle-gtk.sh" "$APPDIR" "$APP_BIN"
else
    echo "aviso: $APP_BIN não existe; o AppImage vai só com a linha de comando" >&2
    # Sem interface, o atalho abre um terminal com o `setup` e o `status`.
    DESKTOP="$WORK/$ID.desktop"
    sed -e 's/^Terminal=false/Terminal=true/' -e 's/^Exec=.*/Exec=gamesir-bridge/' \
        "$ROOT/data/appimage/$ID.desktop" >"$DESKTOP"
fi

# Conteúdo que o `setup` copia para o sistema (via pkexec + install.sh).
P="$APPDIR/usr/share/gamesir-bridge/payload"
mkdir -p "$P/bin"
install -m755 "$BINDIR/gamesir-bridged" "$BINDIR/gamesir-bridge" "$P/bin/"
install -m755 "$ROOT/install.sh" "$ROOT/uninstall.sh" "$P/"
cp -r "$ROOT/data" "$ROOT/profiles" "$P/"
rm -rf "$P/data/appimage"
install -m644 "$ROOT/LICENSE" "$P/"

install -Dm644 "$DESKTOP" "$APPDIR/$ID.desktop"
install -Dm644 "$DESKTOP" "$APPDIR/usr/share/applications/$ID.desktop"
install -Dm644 "$ROOT/data/icons/$ID.svg" "$APPDIR/$ID.svg"
install -Dm644 "$ROOT/data/icons/$ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
install -Dm644 "$ROOT/data/metainfo/$ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
ln -sf "$ID.svg" "$APPDIR/.DirIcon"

if [[ -n "${APPDIR_ONLY:-}" ]]; then
    echo "$APPDIR"
    exit 0
fi

UPDATE=()
if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    # Permite atualizar com AppImageUpdate / Gear Lever.
    UPDATE=(-u "gh-releases-zsync|${GITHUB_REPOSITORY%%/*}|${GITHUB_REPOSITORY##*/}|latest|GameSir_Bridge-*-$ARCH.AppImage.zsync")
fi
ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "${UPDATE[@]}" "$APPDIR" "$OUT"
rm -rf "$WORK"
echo "$OUT"
