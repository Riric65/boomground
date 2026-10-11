#!/usr/bin/env sh
# Construit la version distribuée pour Linux ou macOS :
#   dist/BoomGround-Setup-<os>      (installateur)
#   dist/BoomGround-<os>            (version portable)
#   dist/BoomGround-Server-<os>     (serveur dédié, optionnel)
#
# Usage : sh scripts/package.sh
set -eu
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) os=macOS ;;
  *) os=Linux ;;
esac

cargo build --release --bins
BOOMGROUND_EXE="$(pwd)/target/release/boomground" cargo build --release -p boomground-setup

mkdir -p dist
cp target/release/BoomGround-Setup "dist/BoomGround-Setup-$os"
cp target/release/boomground "dist/BoomGround-$os"
cp target/release/boomground-server "dist/BoomGround-Server-$os"
chmod +x dist/*
ls -lh dist
