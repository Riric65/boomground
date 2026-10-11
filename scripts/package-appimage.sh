#!/usr/bin/env sh
# Construit dist/BoomGround-x86_64.AppImage (Linux).
# L'AppImage se lance avec --portable : pas d'écran d'installation, elle est
# déjà un fichier autonome.
#
# Usage : sh scripts/package-appimage.sh
set -eu
cd "$(dirname "$0")/.."

cargo build --release
mkdir -p dist

appdir=target/BoomGround.AppDir
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin"
cp target/release/boomground "$appdir/usr/bin/boomground"
cp assets/icon/boomground.png "$appdir/boomground.png"

cat > "$appdir/boomground.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=BoomGround
Comment=Destruction de voxels en bac à sable ou en mode boutique
Exec=boomground
Icon=boomground
Categories=Game;
Terminal=false
DESKTOP

cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/boomground" --portable "$@"
APPRUN
chmod +x "$appdir/AppRun"

tool=target/appimagetool
if [ ! -x "$tool" ]; then
  curl -fsSL -o "$tool" \
    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi

# --appimage-extract-and-run : fonctionne sans FUSE (runners CI, conteneurs).
ARCH=x86_64 "$tool" --appimage-extract-and-run "$appdir" dist/BoomGround-x86_64.AppImage
ls -lh dist
