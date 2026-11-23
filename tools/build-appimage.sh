#!/usr/bin/env bash
# Monta o AppImage a partir de binários já compilados.
#
#   tools/build-appimage.sh [BINDIR] [SAÍDA]
#
# BINDIR padrão: target/x86_64-unknown-linux-musl/release (binários estáticos, rodam em
# qualquer distro). Precisa do appimagetool no PATH ou em $APPIMAGETOOL.
# Com APPDIR_ONLY=1, só monta o AppDir (útil para testar sem o appimagetool).
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
if [[ -x "$BINDIR/gamesir-bridge-app" ]]; then
    install -m755 "$BINDIR/gamesir-bridge-app" "$APPDIR/usr/bin/"
fi

# Conteúdo que o `setup` copia para o sistema (via pkexec + install.sh).
P="$APPDIR/usr/share/gamesir-bridge/payload"
mkdir -p "$P/bin"
install -m755 "$BINDIR/gamesir-bridged" "$BINDIR/gamesir-bridge" "$P/bin/"
install -m755 "$ROOT/install.sh" "$ROOT/uninstall.sh" "$P/"
cp -r "$ROOT/data" "$ROOT/profiles" "$P/"
rm -rf "$P/data/appimage"
install -m644 "$ROOT/LICENSE" "$P/"

install -Dm644 "$ROOT/data/appimage/$ID.desktop" "$APPDIR/$ID.desktop"
install -Dm644 "$ROOT/data/appimage/$ID.desktop" "$APPDIR/usr/share/applications/$ID.desktop"
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
