//! Journal des plantages, lisible sans PC : les paniques et les erreurs de
//! rendu sont écrites dans `crash.txt` (dossier de configuration), et le
//! menu affiche la dernière au lancement suivant.

use bevy::prelude::*;
use bevy::render::error_handler::{RenderError, RenderErrorPolicy};

fn path() -> Option<std::path::PathBuf> {
    crate::game::config_dir().map(|d| d.join("crash.txt"))
}

/// Ajoute une ligne au journal.
pub fn record(message: &str) {
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{message}");
    }
}

/// Les paniques sont notées avant le comportement habituel.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record(&format!("panique : {info}"));
        default(info);
    }));
}

/// Erreur GPU : notée, puis ignorée sur téléphone (les pilotes mobiles
/// rejettent parfois une ressource sans que le jeu soit inutilisable) ;
/// sur ordinateur, on quitte comme Bevy par défaut.
pub fn on_render_error(error: &RenderError, main: &mut World, _render: &mut World) -> RenderErrorPolicy {
    record(&format!("erreur de rendu {:?} : {}", error.ty, error.description));
    if crate::touch::is_touch_platform() {
        RenderErrorPolicy::Ignore
    } else {
        main.write_message(AppExit::error());
        RenderErrorPolicy::StopRendering
    }
}

/// Au démarrage : affiche le dernier plantage puis archive le journal.
pub fn show_last(mut toasts: ResMut<crate::ui::Toasts>) {
    let Some(p) = path() else { return };
    let Ok(text) = std::fs::read_to_string(&p) else { return };
    let _ = std::fs::rename(&p, p.with_file_name("crash-old.txt"));
    let Some(first) = text.lines().find(|l| !l.trim().is_empty()) else { return };
    let short: String = first.chars().take(220).collect();
    toasts.push(format!("Dernier plantage : {short}"));
}
