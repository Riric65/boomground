//! Point d'entrée : le jeu, son installation et le serveur dédié tiennent
//! dans ce seul exécutable.
//!
//! - `BoomGround` : le jeu (propose de s'installer au premier lancement) ;
//! - `BoomGround --server` : serveur dédié, sans fenêtre ;
//! - `BoomGround --uninstall` : désinstallation ;
//! - `BoomGround --portable` : joue sans proposer l'installation.

// Pas de console à côté de la fenêtre du jeu sous Windows (en version
// publiée) : on double-clique, le jeu s'ouvre, c'est tout.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--uninstall") || has("/uninstall") {
        if has("--quiet") || has("/S") {
            let _ = boomground::install::uninstall();
        } else {
            boomground::install::uninstall_interactive();
        }
        return;
    }
    if has("--server") {
        attach_console();
        boomground::net::server::run_dedicated();
        return;
    }
    boomground::run();
}

/// Lancé depuis un terminal, le serveur dédié y écrit son journal (sous
/// Windows, un programme « fenêtré » n'a pas de console par défaut).
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process: u32) -> i32;
    }
    unsafe {
        AttachConsole(u32::MAX);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
