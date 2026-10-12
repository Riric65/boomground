#!/usr/bin/env bash
# Compile BoomGround pour Android (bibliothèque partagée `cdylib`) et la
# dépose dans android/src/main/jniLibs/<abi>/libboomground.so, là où Gradle
# la trouve. Nécessite `cargo-ndk` et ANDROID_NDK_HOME.
#
# Utilisé par android/build.gradle (tâche `cargoBuild`).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT="android/src/main/jniLibs"

# abi:triple Rust. arm64 couvre tous les téléphones récents (dont les Pixel
# sous GrapheneOS) ; les autres ABI doubleraient le temps de compilation.
TARGETS=("arm64-v8a:aarch64-linux-android")

for pair in "${TARGETS[@]}"; do
    abi="${pair%%:*}"
    triple="${pair##*:}"
    echo "==> Compilation $abi ($triple)"
    # `--crate-type cdylib` uniquement ici : le bureau n'a pas à produire
    # de bibliothèque partagée en plus de l'exécutable.
    cargo ndk -t "$abi" --platform 26 rustc --lib --release --crate-type cdylib
    mkdir -p "$OUT/$abi"
    cp "target/$triple/release/libboomground.so" "$OUT/$abi/libboomground.so"
done

ls -lh "$OUT"/*/libboomground.so
