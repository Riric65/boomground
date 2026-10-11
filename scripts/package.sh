#!/usr/bin/env sh
# Construit LE fichier à distribuer sous Linux ou macOS :
#   dist/BoomGround-Setup-Linux        (exécutable : double-clic)
#   dist/BoomGround-macOS.zip          (BoomGround.app)
# Au premier lancement, le jeu propose de s'installer lui-même.
#
# Usage : sh scripts/package.sh
set -eu
cd "$(dirname "$0")/.."

cargo build --release
mkdir -p dist

case "$(uname -s)" in
  Darwin)
    app=dist/BoomGround.app
    rm -rf "$app"
    mkdir -p "$app/Contents/MacOS"
    cp target/release/boomground "$app/Contents/MacOS/BoomGround"
    version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>BoomGround</string>
  <key>CFBundleIdentifier</key><string>io.boomground.game</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleExecutable</key><string>BoomGround</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    codesign --force --deep -s - "$app" || true
    (cd dist && rm -f BoomGround-macOS.zip && ditto -c -k --keepParent BoomGround.app BoomGround-macOS.zip)
    rm -rf "$app"
    ;;
  *)
    cp target/release/boomground dist/BoomGround-Setup-Linux
    chmod +x dist/BoomGround-Setup-Linux
    ;;
esac
ls -lh dist
