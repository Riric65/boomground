//! Presse-papiers du système, via les outils de la plateforme (aucune
//! dépendance) : `wl-clipboard`, `xclip` ou `xsel` sous Linux, PowerShell
//! sous Windows, `pbcopy` / `pbpaste` sous macOS.

use std::io::Write;
use std::process::{Command, Stdio};

fn quiet(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Copie `text` dans le presse-papiers.
pub fn set(text: &str) -> bool {
    let candidates: &[(&str, &[&str])] = if cfg!(windows) {
        &[("clip", &[])]
    } else if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (program, args) in candidates {
        let mut cmd = Command::new(program);
        quiet(cmd.args(*args));
        if let Ok(mut child) = cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }
    }
    false
}

/// Contenu texte du presse-papiers (première ligne utile).
pub fn get() -> Option<String> {
    let candidates: &[(&str, &[&str])] = if cfg!(windows) {
        &[("powershell", &["-NoProfile", "-Command", "Get-Clipboard"])]
    } else if cfg!(target_os = "macos") {
        &[("pbpaste", &[])]
    } else {
        &[
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ]
    };
    for (program, args) in candidates {
        let mut cmd = Command::new(program);
        quiet(cmd.args(*args));
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                let line = text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
                if line.is_some() {
                    return line;
                }
            }
        }
    }
    None
}
