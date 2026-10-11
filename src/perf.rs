//! Limitation de la fréquence d'images.
//!
//! Deux mécanismes complémentaires :
//! - `Window::present_mode = AutoVsync` (voir `main`), qui synchronise les
//!   presentations sur le rafraîchissement de l'écran ;
//! - ce module, qui borne la boucle principale à la fréquence de l'écran
//!   (détectée via [`Monitor::refresh_rate_millihertz`], ou forcée par
//!   `BOOM_FPS=144`).

use std::time::Duration;

use bevy::prelude::*;
use bevy::window::{Monitor, Window};

/// Fréquence cible. `0` = illimité.
#[derive(Resource)]
pub struct FpsLimit {
    pub fps: f32,
}

impl Default for FpsLimit {
    fn default() -> Self {
        // `BOOM_FPS` force une valeur, sinon on attend la détection.
        let fps = std::env::var("BOOM_FPS")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        Self { fps }
    }
}

/// Relit la fréquence de l'écran une fois la fenêtre créée.
pub fn detect_refresh(
    mut limit: ResMut<FpsLimit>,
    monitors: Query<&Monitor>,
    windows: Query<&Window>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    // Il faut que la fenêtre existe et soit size avant de choisir l'écran.
    if windows.iter().all(|w| w.size().x < 1.0) {
        return;
    }
    if limit.fps > 0.0 {
        *done = true;
        return;
    }
    if let Some(hz) = monitors
        .iter()
        .filter_map(|m| m.refresh_rate_millihertz)
        .max()
        .filter(|hz| *hz > 0)
    {
        limit.fps = hz as f32 / 1000.0;
        *done = true;
    }
}

/// Dort le temps qu'il manque pour ne pas dépasser la fréquence cible.
///
/// Un réglage à 0 signifie « laisser le détecteur parler » ; sinon ilprime
/// sur la détection de l'écran.
pub fn limit(
    time: Res<Time>,
    limit: Res<FpsLimit>,
    settings: Res<crate::game::Settings>,
) {
    let target = if settings.fps_limit > 1.0 {
        settings.fps_limit
    } else {
        limit.fps
    };
    if target <= 1.0 {
        return;
    }
    let min_delta = Duration::from_secs_f64(1.0 / target as f64);
    let elapsed = time.delta();
    if elapsed < min_delta {
        std::thread::sleep(min_delta - elapsed);
    }
}