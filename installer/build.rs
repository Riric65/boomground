//! Embarque le jeu compilé dans l'installateur.
//!
//! Chemin du jeu : variable `BOOMGROUND_EXE`, sinon
//! `../target/release/boomground[.exe]`. S'il manque, l'installateur est
//! quand même construit mais refusera d'installer (message clair).

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let exe_name = if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        "boomground.exe"
    } else {
        "boomground"
    };
    let source = std::env::var_os("BOOMGROUND_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("..").join("target").join("release").join(exe_name));

    println!("cargo:rerun-if-env-changed=BOOMGROUND_EXE");
    println!("cargo:rerun-if-changed={}", source.display());

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.bin");
    match std::fs::read(&source) {
        Ok(bytes) => std::fs::write(&out, bytes).unwrap(),
        Err(_) => {
            println!(
                "cargo:warning=jeu introuvable ({}) : compilez d'abord `cargo build --release`",
                source.display()
            );
            std::fs::write(&out, []).unwrap();
        }
    }
}
