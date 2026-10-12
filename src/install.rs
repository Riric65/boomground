//! Installation : **un seul fichier** fait tout.
//!
//! Au premier lancement (depuis Téléchargements, une clé USB…), le jeu
//! propose de s'installer lui-même : il se copie dans le dossier des
//! programmes de l'utilisateur, crée ses raccourcis, autorise le
//! multijoueur dans le pare-feu (Windows), puis se relance depuis là. Pas
//! d'installateur séparé, pas de droits administrateur (sauf la question du
//! pare-feu, facultative).
//!
//! - **Windows** : `%LOCALAPPDATA%\Programs\BoomGround\BoomGround.exe`,
//!   raccourcis Bureau + menu Démarrer, entrée « Applications installées ».
//! - **Linux** : `~/.local/share/boomground/boomground`, menu des
//!   applications, commande `boomground`.
//! - **macOS** : `~/Applications/BoomGround.app`.
//!
//! `BoomGround --uninstall` désinstalle (c'est ce que lance Windows depuis
//! « Applications installées »).

use std::path::{Path, PathBuf};

pub const NAME: &str = "BoomGround";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Options cochées dans l'écran d'installation.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub desktop_shortcut: bool,
    /// Windows : règle de pare-feu pour héberger des parties.
    pub firewall: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            desktop_shortcut: true,
            firewall: true,
        }
    }
}

/// Fichier qui contient le jeu : l'AppImage elle-même (variable `APPIMAGE`)
/// quand on en vient, sinon l'exécutable courant. C'est ce qu'on copie à
/// l'installation, pour que le raccourci relance l'AppImage complète.
fn source_exe() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        if p.is_file() {
            return Ok(p);
        }
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Le jeu tourne-t-il depuis son emplacement installé ?
pub fn is_installed_copy() -> bool {
    let (Ok(me), Ok(target)) = (source_exe(), platform::exe_path()) else {
        return false;
    };
    same_file(&me, &target)
}

/// Une version est déjà installée (on proposera « mettre à jour »).
pub fn already_installed() -> bool {
    platform::exe_path().is_ok_and(|p| p.exists())
}

/// Faut-il proposer l'installation au démarrage ?
pub fn should_offer() -> bool {
    // Sur téléphone, c'est l'APK qui installe le jeu.
    if cfg!(target_os = "android") {
        return false;
    }
    if std::env::args().any(|a| a == "--portable" || a == "--no-install") {
        return false;
    }
    if crate::debug::enabled() {
        return std::env::var("BOOM_INSTALLER").is_ok();
    }
    // En développement (`cargo run`), seulement sur demande.
    if cfg!(debug_assertions) && std::env::var("BOOM_INSTALLER").is_err() {
        return false;
    }
    !is_installed_copy()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Dossier d'installation (affiché à l'écran).
pub fn install_dir() -> String {
    platform::exe_path()
        .ok()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
        .unwrap_or_default()
}

/// Copie le jeu à sa place et crée raccourcis / menus. Renvoie le chemin de
/// l'exécutable installé et d'éventuels avertissements.
pub fn install(options: Options) -> Result<(PathBuf, Vec<String>), String> {
    let me = source_exe()?;
    let target = platform::exe_path()?;
    if !same_file(&me, &target) {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Impossible de créer {} : {e}", dir.display()))?;
        }
        std::fs::copy(&me, &target).map_err(|e| {
            format!(
                "Impossible de copier le jeu : {e}\nSi BoomGround est déjà ouvert ailleurs, ferme-le puis réessaie."
            )
        })?;
    }
    let warnings = platform::integrate(&target, options)?;
    Ok((target, warnings))
}

/// Relance le jeu installé (le processus actuel doit ensuite quitter).
pub fn launch(exe: &Path) -> Result<(), String> {
    platform::launch(exe)
}

/// Désinstallation complète.
pub fn uninstall() -> Result<(), String> {
    platform::uninstall()
}

/// `--uninstall` : demande confirmation (boîte de dialogue native), puis
/// désinstalle.
pub fn uninstall_interactive() {
    if !dialog::ask(&format!("Désinstaller {NAME} ?")) {
        return;
    }
    match uninstall() {
        Ok(()) => dialog::info(&format!("{NAME} a été désinstallé.")),
        Err(e) => dialog::error(&e),
    }
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
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\BoomGround";

    pub fn exe_path() -> Result<PathBuf, String> {
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs").join(NAME).join("BoomGround.exe"))
            .ok_or_else(|| "LOCALAPPDATA introuvable".to_string())
    }

    fn hidden(program: &str, args: &[&str]) -> bool {
        Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn powershell(script: &str) -> bool {
        hidden(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
        )
    }

    /// Chaîne PowerShell entre apostrophes.
    fn ps_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    pub fn integrate(exe: &Path, options: Options) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        let dir = exe.parent().unwrap_or(Path::new("."));
        // Restes de la v1.0 (installateur séparé).
        let _ = std::fs::remove_file(dir.join("Desinstaller BoomGround.exe"));

        // Raccourcis : menu Démarrer toujours, Bureau si demandé.
        let folders = if options.desktop_shortcut {
            "@([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))"
        } else {
            "@([Environment]::GetFolderPath('Programs'))"
        };
        let script = format!(
            "$w = New-Object -ComObject WScript.Shell; \
             foreach ($d in {folders}) {{ \
               $s = $w.CreateShortcut((Join-Path $d 'BoomGround.lnk')); \
               $s.TargetPath = {exe}; $s.WorkingDirectory = {dir}; $s.IconLocation = {icon}; \
               $s.Description = 'BoomGround'; $s.Save() }}",
            exe = ps_quote(&exe.to_string_lossy()),
            dir = ps_quote(&dir.to_string_lossy()),
            icon = ps_quote(&format!("{},0", exe.to_string_lossy())),
        );
        if !powershell(&script) {
            warnings.push("Raccourcis non créés (le jeu reste lançable depuis son dossier).".into());
        }

        // Entrée « Applications installées ».
        let size_kb = std::fs::metadata(exe).map(|m| m.len() / 1024).unwrap_or(0).to_string();
        let uninstall_cmd = format!("\"{}\" --uninstall", exe.to_string_lossy());
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
            hidden("reg", &["add", UNINSTALL_KEY, "/v", name, "/t", kind, "/d", value, "/f"]);
        }

        // Pare-feu : autorise les autres joueurs à rejoindre nos lobbys.
        // Windows demande une confirmation (UAC) ; si on refuse, il posera
        // la question lui-même au premier lobby.
        if options.firewall {
            let netsh = format!(
                "/c netsh advfirewall firewall delete rule name=BoomGround >nul & \
                 netsh advfirewall firewall add rule name=BoomGround dir=in action=allow \
                 program=\"{}\" enable=yes profile=any",
                exe.to_string_lossy()
            );
            let script = format!(
                "Start-Process -FilePath cmd.exe -ArgumentList {} -Verb RunAs -WindowStyle Hidden -Wait",
                ps_quote(&netsh)
            );
            if !powershell(&script) {
                warnings.push(
                    "Pare-feu non configuré : Windows demandera l'autorisation au premier lobby.".into(),
                );
            }
        }
        Ok(warnings)
    }

    pub fn launch(exe: &Path) -> Result<(), String> {
        Command::new(exe)
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn uninstall() -> Result<(), String> {
        let exe = exe_path()?;
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        powershell(
            "foreach ($d in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) { \
               Remove-Item -LiteralPath (Join-Path $d 'BoomGround.lnk') -ErrorAction SilentlyContinue }",
        );
        hidden("reg", &["delete", UNINSTALL_KEY, "/f"]);
        // Sans droits administrateur, la règle de pare-feu reste : elle ne
        // vise plus aucun programme, elle est donc inoffensive.
        hidden("netsh", &["advfirewall", "firewall", "delete", "rule", "name=BoomGround"]);
        if let Some(config) = std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(NAME)) {
            let _ = std::fs::remove_dir_all(config);
        }
        // Le jeu ne peut pas s'effacer pendant qu'il tourne : un `cmd`
        // détaché finit le ménage deux secondes plus tard.
        let cleanup = format!("/C ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"", dir.to_string_lossy());
        Command::new("cmd")
            .raw_arg(cleanup)
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
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

    /// Icône du jeu (512 × 512).
    const ICON: &[u8] = include_bytes!("../assets/icon/boomground.png");


    fn data_home() -> Result<PathBuf, String> {
        Ok(std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".local").join("share")))
    }

    pub fn exe_path() -> Result<PathBuf, String> {
        Ok(data_home()?.join("boomground").join("boomground"))
    }

    fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, bytes).map_err(|e| format!("{} : {e}", path.display()))
    }

    pub fn integrate(exe: &Path, options: Options) -> Result<Vec<String>, String> {
        let mut warnings = Vec::new();
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        let data = data_home()?;
        write(&data.join("icons/hicolor/512x512/apps/boomground.png"), ICON)?;
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={NAME}\nComment=Destruction de voxels\n\
             Exec=\"{}\"\nIcon=boomground\nTerminal=false\nCategories=Game;ActionGame;\n",
            exe.display()
        );
        write(&data.join("applications/boomground.desktop"), desktop.as_bytes())?;
        if options.desktop_shortcut {
            let desk = home()?.join("Desktop");
            if desk.is_dir() {
                let file = desk.join("boomground.desktop");
                if write(&file, desktop.as_bytes()).is_ok() {
                    let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755));
                    let _ = Command::new("gio")
                        .args(["set", "-t", "string"])
                        .arg(&file)
                        .args(["metadata::trusted", "true"])
                        .status();
                }
            }
        }
        // Commande `boomground` dans le terminal.
        let bin = home()?.join(".local/bin");
        if std::fs::create_dir_all(&bin).is_ok() {
            let link = bin.join("boomground");
            let _ = std::fs::remove_file(&link);
            if std::os::unix::fs::symlink(exe, &link).is_err() {
                warnings.push("Commande `boomground` non créée.".into());
            }
        }
        let _ = Command::new("update-desktop-database")
            .arg(data.join("applications"))
            .status();
        Ok(warnings)
    }

    pub fn launch(exe: &Path) -> Result<(), String> {
        Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn uninstall() -> Result<(), String> {
        let data = data_home()?;
        let _ = std::fs::remove_file(data.join("applications/boomground.desktop"));
        let _ = std::fs::remove_file(data.join("icons/hicolor/512x512/apps/boomground.png"));
        let _ = std::fs::remove_file(home()?.join(".local/bin/boomground"));
        let _ = std::fs::remove_file(home()?.join("Desktop/boomground.desktop"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".config"))
            .join("boomground");
        let _ = std::fs::remove_dir_all(config);
        if let Some(dir) = exe_path()?.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
        Ok(())
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

    fn app_dir() -> Result<PathBuf, String> {
        Ok(home()?.join("Applications").join("BoomGround.app"))
    }

    pub fn exe_path() -> Result<PathBuf, String> {
        Ok(app_dir()?.join("Contents/MacOS/BoomGround"))
    }

    pub fn integrate(exe: &Path, _options: Options) -> Result<Vec<String>, String> {
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        let app = app_dir()?;
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
        std::fs::write(app.join("Contents/Info.plist"), plist).map_err(|e| e.to_string())?;
        // Application non signée : on retire la quarantaine et on signe
        // localement, sinon Gatekeeper refuse de l'ouvrir.
        let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&app).status();
        let _ = Command::new("codesign").args(["--force", "--deep", "-s", "-"]).arg(&app).status();
        Ok(Vec::new())
    }

    pub fn launch(_exe: &Path) -> Result<(), String> {
        Command::new("open")
            .arg(app_dir()?)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn uninstall() -> Result<(), String> {
        let _ = std::fs::remove_dir_all(home()?.join("Library/Application Support/BoomGround"));
        std::fs::remove_dir_all(app_dir()?).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e.to_string())
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Boîtes de dialogue natives (désinstallation, erreurs au démarrage)
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub mod dialog {
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
        let caption = wide("BoomGround");
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

#[cfg(not(windows))]
pub mod dialog {
    use std::io::{BufRead, Write};

    pub fn ask(text: &str) -> bool {
        print!("{text} [o/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        let a = line.trim().to_lowercase();
        a.starts_with('o') || a.starts_with('y')
    }

    pub fn info(text: &str) {
        println!("{text}");
    }

    pub fn error(text: &str) {
        eprintln!("{text}");
    }
}
