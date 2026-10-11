//! Installateur de BoomGround : un seul fichier, aucun terminal.
//!
//! Le jeu compilé est embarqué dans l'installateur (voir `build.rs`).
//!
//! - **Windows** : installe dans `%LOCALAPPDATA%\Programs\BoomGround` (pas de
//!   droits administrateur), crée les raccourcis Bureau + menu Démarrer et
//!   une entrée « Applications installées » pour la désinstallation.
//! - **Linux** : installe dans `~/.local/share/boomground`, ajoute le jeu au
//!   menu des applications et la commande `boomground`.
//! - **macOS** : crée `~/Applications/BoomGround.app`.
//!
//! `--uninstall` (ou `/uninstall`) désinstalle ; `--quiet` (ou `/S`) ne pose
//! aucune question.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

/// Le jeu, embarqué à la compilation.
static GAME: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));

const NAME: &str = "BoomGround";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));
    let quiet = has(&["--quiet", "/S", "/s", "-q"]);
    let result = if has(&["--uninstall", "/uninstall", "-u"]) {
        uninstall(quiet)
    } else {
        install(quiet)
    };
    if let Err(e) = result {
        if quiet {
            eprintln!("{NAME} : {e}");
        } else {
            dialog::error(&e);
        }
        std::process::exit(1);
    }
}

fn install(quiet: bool) -> Result<(), String> {
    if GAME.is_empty() {
        return Err("Cet installateur a été construit sans le jeu.\n\
                    Compilez d'abord `cargo build --release`, puis l'installateur."
            .into());
    }
    let dir = platform::install_dir()?;
    if !quiet
        && !dialog::ask(&format!(
            "Installer {NAME} {VERSION} ?\n\nDossier : {}\n\nAucun droit administrateur n'est nécessaire.",
            dir.display()
        ))
    {
        return Ok(());
    }
    let exe = platform::install(&dir)?;
    if !quiet && dialog::ask(&format!("{NAME} est installé !\n\nLancer le jeu maintenant ?")) {
        platform::launch(&exe);
    }
    Ok(())
}

fn uninstall(quiet: bool) -> Result<(), String> {
    if !quiet && !dialog::ask(&format!("Désinstaller {NAME} ?")) {
        return Ok(());
    }
    platform::uninstall()?;
    if !quiet {
        dialog::info(&format!("{NAME} a été désinstallé."));
    }
    Ok(())
}

/// Écrit un fichier en signalant clairement l'échec le plus courant (jeu
/// encore ouvert).
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Impossible de créer {} : {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| {
        format!(
            "Impossible d'écrire {} : {e}\n\nSi {NAME} est ouvert, fermez-le puis relancez l'installation.",
            path.display()
        )
    })
}

#[cfg_attr(windows, allow(dead_code))]
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "dossier personnel introuvable".to_string())
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\BoomGround";

    pub fn install_dir() -> Result<PathBuf, String> {
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs").join(NAME))
            .ok_or_else(|| "LOCALAPPDATA introuvable".to_string())
    }

    fn hidden(program: &str, args: &[&str]) -> Result<(), String> {
        let status = Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("{program} : {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{program} a échoué ({status})"))
        }
    }

    fn powershell(script: &str) -> Result<(), String> {
        hidden(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
        )
    }

    /// Chaîne PowerShell entre apostrophes.
    fn ps_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    pub fn install(dir: &Path) -> Result<PathBuf, String> {
        let exe = dir.join("BoomGround.exe");
        write_file(&exe, GAME)?;

        // L'installateur se copie à côté du jeu : il sert de désinstalleur.
        let uninstaller = dir.join("Desinstaller BoomGround.exe");
        if let Ok(me) = std::env::current_exe() {
            if me != uninstaller {
                let _ = std::fs::copy(&me, &uninstaller);
            }
        }

        // Raccourcis Bureau + menu Démarrer.
        let script = format!(
            "$w = New-Object -ComObject WScript.Shell; \
             foreach ($d in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {{ \
               $s = $w.CreateShortcut((Join-Path $d 'BoomGround.lnk')); \
               $s.TargetPath = {exe}; $s.WorkingDirectory = {dir}; $s.IconLocation = {icon}; \
               $s.Description = 'BoomGround'; $s.Save() }}",
            exe = ps_quote(&exe.to_string_lossy()),
            dir = ps_quote(&dir.to_string_lossy()),
            icon = ps_quote(&format!("{},0", exe.to_string_lossy())),
        );
        // Un raccourci raté n'empêche pas de jouer : on continue.
        let _ = powershell(&script);

        // Entrée « Applications installées » (désinstallation propre).
        let size_kb = (GAME.len() / 1024).to_string();
        let uninstall_cmd = format!("\"{}\" /uninstall", uninstaller.to_string_lossy());
        let exe_s = exe.to_string_lossy().to_string();
        let dir_s = dir.to_string_lossy().to_string();
        for (name, kind, value) in [
            ("DisplayName", "REG_SZ", NAME),
            ("DisplayVersion", "REG_SZ", VERSION),
            ("Publisher", "REG_SZ", NAME),
            ("DisplayIcon", "REG_SZ", exe_s.as_str()),
            ("InstallLocation", "REG_SZ", dir_s.as_str()),
            ("UninstallString", "REG_SZ", uninstall_cmd.as_str()),
            ("NoModify", "REG_DWORD", "1"),
            ("NoRepair", "REG_DWORD", "1"),
            ("EstimatedSize", "REG_DWORD", size_kb.as_str()),
        ] {
            let _ = hidden("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", value, "/f"]);
        }
        Ok(exe)
    }

    pub fn uninstall() -> Result<(), String> {
        let dir = install_dir()?;
        let _ = powershell(
            "foreach ($d in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) { \
               Remove-Item -LiteralPath (Join-Path $d 'BoomGround.lnk') -ErrorAction SilentlyContinue }",
        );
        let _ = hidden("reg", &["delete", UNINSTALL_KEY, "/f"]);
        std::fs::remove_file(dir.join("BoomGround.exe")).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(format!("Impossible de supprimer le jeu : {e}\n\nFermez BoomGround puis réessayez."))
            }
        })?;
        if let Some(config) = std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(NAME)) {
            let _ = std::fs::remove_dir_all(config);
        }
        // Le désinstalleur ne peut pas s'effacer lui-même pendant qu'il
        // tourne : on confie la fin du ménage à un `cmd` détaché.
        let cleanup = format!(
            "ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"",
            dir.to_string_lossy()
        );
        // `raw_arg` : cmd ne comprend pas les guillemets échappés à la Rust.
        let _ = Command::new("cmd")
            .raw_arg(format!("/C {cleanup}"))
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn();
        Ok(())
    }

    pub fn launch(exe: &Path) {
        let _ = Command::new(exe)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .creation_flags(DETACHED_PROCESS)
            .spawn();
    }
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
<rect width="64" height="64" rx="12" fill="#1c2230"/>
<circle cx="30" cy="38" r="17" fill="#2b2b2b" stroke="#f5c542" stroke-width="3"/>
<path d="M40 22 l6 -6" stroke="#f5c542" stroke-width="4" stroke-linecap="round"/>
<circle cx="49" cy="13" r="4" fill="#ff7a2f"/>
</svg>
"##;

    fn data_home() -> Result<PathBuf, String> {
        Ok(std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".local").join("share")))
    }

    pub fn install_dir() -> Result<PathBuf, String> {
        Ok(data_home()?.join("boomground"))
    }

    pub fn install(dir: &Path) -> Result<PathBuf, String> {
        let exe = dir.join("boomground");
        write_file(&exe, GAME)?;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;

        if let Ok(me) = std::env::current_exe() {
            let uninstaller = dir.join("boomground-uninstall");
            if me != uninstaller && std::fs::copy(&me, &uninstaller).is_ok() {
                let _ = std::fs::set_permissions(&uninstaller, std::fs::Permissions::from_mode(0o755));
            }
        }

        let data = data_home()?;
        let icon = data.join("icons/hicolor/scalable/apps/boomground.svg");
        write_file(&icon, ICON.as_bytes())?;
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={NAME}\nComment=Destruction de voxels\n\
             Exec=\"{}\"\nIcon=boomground\nTerminal=false\nCategories=Game;ActionGame;\n",
            exe.display()
        );
        write_file(&data.join("applications/boomground.desktop"), desktop.as_bytes())?;

        // Commande `boomground` dans le terminal (facultatif).
        let bin = home()?.join(".local/bin");
        if std::fs::create_dir_all(&bin).is_ok() {
            let link = bin.join("boomground");
            let _ = std::fs::remove_file(&link);
            let _ = std::os::unix::fs::symlink(&exe, &link);
        }
        let _ = Command::new("update-desktop-database")
            .arg(data.join("applications"))
            .status();
        Ok(exe)
    }

    pub fn uninstall() -> Result<(), String> {
        let data = data_home()?;
        let _ = std::fs::remove_file(data.join("applications/boomground.desktop"));
        let _ = std::fs::remove_file(data.join("icons/hicolor/scalable/apps/boomground.svg"));
        let _ = std::fs::remove_file(home()?.join(".local/bin/boomground"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".config"))
            .join("boomground");
        let _ = std::fs::remove_dir_all(config);
        std::fs::remove_dir_all(install_dir()?).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e.to_string())
            }
        })
    }

    pub fn launch(exe: &Path) {
        let _ = Command::new(exe).spawn();
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    pub fn install_dir() -> Result<PathBuf, String> {
        Ok(home()?.join("Applications").join("BoomGround.app"))
    }

    pub fn install(app: &Path) -> Result<PathBuf, String> {
        let exe = app.join("Contents/MacOS/BoomGround");
        write_file(&exe, GAME)?;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>{NAME}</string>
  <key>CFBundleDisplayName</key><string>{NAME}</string>
  <key>CFBundleIdentifier</key><string>io.boomground.game</string>
  <key>CFBundleVersion</key><string>{VERSION}</string>
  <key>CFBundleShortVersionString</key><string>{VERSION}</string>
  <key>CFBundleExecutable</key><string>BoomGround</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.action-games</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
        );
        write_file(&app.join("Contents/Info.plist"), plist.as_bytes())?;
        // Application non signée : on retire la quarantaine et on signe
        // localement, sinon Gatekeeper refuse de l'ouvrir.
        let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(app).status();
        let _ = Command::new("codesign").args(["--force", "--deep", "-s", "-"]).arg(app).status();
        Ok(app.to_path_buf())
    }

    pub fn uninstall() -> Result<(), String> {
        let _ = std::fs::remove_dir_all(home()?.join("Library/Application Support/BoomGround"));
        std::fs::remove_dir_all(install_dir()?).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e.to_string())
            }
        })
    }

    pub fn launch(app: &Path) {
        let _ = Command::new("open").arg(app).spawn();
    }
}

// ---------------------------------------------------------------------------
// Boîtes de dialogue natives
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod dialog {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    const MB_YESNO: u32 = 0x04;
    const MB_ICONERROR: u32 = 0x10;
    const MB_ICONQUESTION: u32 = 0x20;
    const MB_ICONINFORMATION: u32 = 0x40;
    const IDYES: i32 = 6;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn show(text: &str, kind: u32) -> i32 {
        let text = wide(text);
        let caption = wide("Installation de BoomGround");
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), kind) }
    }

    pub fn ask(text: &str) -> bool {
        show(text, MB_YESNO | MB_ICONQUESTION) == IDYES
    }

    pub fn info(text: &str) {
        show(text, MB_ICONINFORMATION);
    }

    pub fn error(text: &str) {
        show(text, MB_ICONERROR);
    }
}

#[cfg(target_os = "macos")]
mod dialog {
    use std::process::Command;

    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    pub fn ask(text: &str) -> bool {
        let script = format!(
            "display dialog \"{}\" with title \"BoomGround\" buttons {{\"Non\", \"Oui\"}} default button \"Oui\"",
            escape(text)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("Oui"))
            .unwrap_or(true)
    }

    pub fn info(text: &str) {
        let script = format!(
            "display dialog \"{}\" with title \"BoomGround\" buttons {{\"OK\"}} default button \"OK\"",
            escape(text)
        );
        let _ = Command::new("osascript").args(["-e", &script]).status();
    }

    pub fn error(text: &str) {
        info(text);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod dialog {
    use std::io::{BufRead, IsTerminal, Write};
    use std::process::Command;

    /// zenity (GNOME), kdialog (KDE), sinon le terminal, sinon « oui ».
    pub fn ask(text: &str) -> bool {
        if let Ok(s) = Command::new("zenity")
            .args(["--question", "--title=BoomGround", "--width=420", &format!("--text={text}")])
            .status()
        {
            return s.success();
        }
        if let Ok(s) = Command::new("kdialog").args(["--title", "BoomGround", "--yesno", text]).status() {
            return s.success();
        }
        if std::io::stdin().is_terminal() {
            print!("{text} [O/n] ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            let answer = line.trim().to_lowercase();
            return answer.is_empty() || answer.starts_with('o') || answer.starts_with('y');
        }
        true
    }

    pub fn info(text: &str) {
        if Command::new("zenity")
            .args(["--info", "--title=BoomGround", "--width=420", &format!("--text={text}")])
            .status()
            .is_ok()
        {
            return;
        }
        if Command::new("kdialog").args(["--title", "BoomGround", "--msgbox", text]).status().is_ok() {
            return;
        }
        println!("{text}");
    }

    pub fn error(text: &str) {
        if Command::new("zenity")
            .args(["--error", "--title=BoomGround", "--width=420", &format!("--text={text}")])
            .status()
            .is_ok()
        {
            return;
        }
        eprintln!("{text}");
    }
}
