//! Journal du serveur, dans le style des serveurs Minecraft :
//!
//! ```text
//! [14:03:22] [Server thread/INFO]: Alex a rejoint la partie
//! ```
//!
//! Chaque ligne va sur la sortie standard **et** dans `logs/latest.log`
//! (l'ancien `latest.log` est archivé sous `logs/AAAA-MM-JJ-N.log`).
//! L'heure est en UTC.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Jour civil (année, mois, jour) à partir d'un nombre de jours depuis 1970.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn now_parts() -> (i64, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    (days, rem / 3600, (rem / 60) % 60, rem % 60)
}

/// Prépare le fichier de journal dans `dir` (archive l'ancien).
pub fn init(dir: &Path, level: Level) {
    *MIN_LEVEL.lock().unwrap() = level;
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let latest = dir.join("latest.log");
    if latest.exists() {
        let (days, ..) = now_parts();
        let (y, m, d) = civil(days);
        let mut n = 1;
        let archive = loop {
            let p: PathBuf = dir.join(format!("{y:04}-{m:02}-{d:02}-{n}.log"));
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
    let (_, h, m, s) = now_parts();
    let tag = match level {
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    };
    let line = format!("[{h:02}:{m:02}:{s:02}] [Server thread/{tag}]: {msg}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_000), (2024, 10, 4));
    }
}
