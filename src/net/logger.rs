//! Journal du serveur, dans le style des serveurs Minecraft :
//!
//! ```text
//! [14:03:22] [Server thread/INFO]: Alex a rejoint la partie
//! ```
//!
//! Chaque ligne va sur la sortie standard **et** dans `logs/latest.log`
//! (l'ancien `latest.log` est archivé sous `logs/AAAA-MM-JJ-N.log`).
//! L'heure est celle de la machine du serveur.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Niveau d'une ligne.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

static MIN_LEVEL: Mutex<Level> = Mutex::new(Level::Info);

/// Prépare le fichier de journal dans `dir` (archive l'ancien).
pub fn init(dir: &Path, level: Level) {
    *MIN_LEVEL.lock().unwrap() = level;
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let latest = dir.join("latest.log");
    if latest.exists() {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let mut n = 1;
        let archive = loop {
            let p: PathBuf = dir.join(format!("{date}-{n}.log"));
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        let _ = fs::rename(&latest, archive);
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&latest) {
        *FILE.lock().unwrap() = Some(f);
    }
}

/// Écrit une ligne.
pub fn log(level: Level, msg: &str) {
    if level < *MIN_LEVEL.lock().unwrap() {
        return;
    }
    let now = chrono::Local::now().format("%H:%M:%S");
    let tag = match level {
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    };
    let line = format!("[{now}] [Server thread/{tag}]: {msg}");
    println!("{line}");
    if let Some(f) = FILE.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

pub fn info(msg: impl AsRef<str>) {
    log(Level::Info, msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    log(Level::Warn, msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    log(Level::Error, msg.as_ref());
}

pub fn debug(msg: impl AsRef<str>) {
    log(Level::Debug, msg.as_ref());
}
