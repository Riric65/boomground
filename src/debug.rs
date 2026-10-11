//! Mode test (`BOOM_AUTO=1`) : le jeu joue tout seul (regard, marche, frappes)
//! et écrit des captures dans `/tmp/opencode/boom/`, ce qui permet de valider
//! le rendu et la destruction sans simulation de clavier.
//!
//! Inactif en fonctionnement normal.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{CursorGrabMode, CursorOptions};

/// Dossier des captures automatiques.
const SHOT_DIR: &str = "/tmp/opencode/boom";

pub fn enabled() -> bool {
    std::env::var("BOOM_AUTO").is_ok_and(|v| v == "1")
}

/// Injecte la ressource de test si le mode est actif.
pub fn setup(mut commands: Commands) {
    if enabled() {
        std::fs::create_dir_all(SHOT_DIR).ok();
        commands.insert_resource(Auto::default());
    }
}

/// Ouvre le panneau multijoueur (validation) : `BOOM_MULTI=1`.
pub fn open_multi(mut view: ResMut<crate::menu::MenuView>) {
    if std::env::var("BOOM_MULTI").is_ok() {
        view.show_multi = true;
    }
}

/// Ouvre le panneau des paramètres (validation) : `BOOM_SETTINGS=1`.
pub fn open_settings(mut view: ResMut<crate::menu::MenuView>) {
    if std::env::var("BOOM_SETTINGS").is_ok() {
        view.show_settings = true;
    }
}

/// Vue d'ensemble de la carte (validation) : caméra haute, sans dérive.
pub fn overview(
    mut game: ResMut<crate::game::GameState>,
    mut player: Query<&mut Transform, With<crate::player::Player>>,
    mut head: Query<&mut Transform, (With<crate::player::Head>, Without<crate::player::Player>)>,
) {
    if std::env::var("BOOM_OVERVIEW").is_err() {
        return;
    }
    game.phase = crate::game::Phase::Playing;
    if let Ok(mut t) = player.single_mut() {
        t.translation = Vec3::new(0.0, 46.0, 4.0);
        t.rotation = Quat::from_rotation_y(0.35);
    }
    if let Ok(mut t) = head.single_mut() {
        t.rotation = Quat::from_rotation_x(-1.15);
    }
    if std::env::var("BOOM_VERBOSE").is_ok() {
        info!("OVERVIEW {:?}", player.single().map(|p| p.translation));
    }
}

/// En mode auto, on saute le menu et on démarre directement la partie.
/// `BOOM_SERVER=host[:port]` connecte aussi à un serveur de test.
pub fn force_playing(
    mut game: ResMut<crate::game::GameState>,
    mut cursor: Single<&mut CursorOptions>,
    mut net: ResMut<crate::net::client::NetClient>,
) {
    // `BOOM_MENU=1` : on reste sur le menu (pour le vérifier à l'écran).
    if game.phase == crate::game::Phase::Menu && std::env::var("BOOM_MENU").is_err() {
        if let Ok(address) = std::env::var("BOOM_SERVER") {
            net.address = address;
            match net.connect() {
                Ok(target) => info!("connecté à {target}"),
                Err(e) => error!("connexion impossible : {e}"),
            }
        }
        game.phase = crate::game::Phase::Playing;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

#[derive(Resource)]
pub struct Auto {
    /// Déclenche un coup toutes les 0,45 s.
    swing: Timer,
    weapon_tick: Timer,
    /// Marche vers le mur pendant 1,1 s, puis s'arrête.
    walk: Timer,
    shot: Timer,
    shot_index: u32,
    started: bool,
}

impl Default for Auto {
    fn default() -> Self {
        Self {
            swing: Timer::from_seconds(0.45, TimerMode::Repeating),
            weapon_tick: Timer::from_seconds(4.0, TimerMode::Repeating),
            walk: Timer::from_seconds(1.1, TimerMode::Once),
            shot: Timer::from_seconds(2.0, TimerMode::Once),
            shot_index: 0,
            started: false,
        }
    }
}

/// Regard qui balaye, marche vers le mur, puis frappe en continu.
pub fn autopilot(
    auto: Option<ResMut<Auto>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse: ResMut<AccumulatedMouseMotion>,
    mut cursor: Single<&mut CursorOptions>,
    mut game_state: ResMut<crate::game::GameState>,
    mut commands: Commands,
    time: Res<Time>,
) {
    let Some(mut auto) = auto else { return };

    if !auto.started {
        auto.started = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }

    // Balayage lent, pour voir le mur sous différents angles (désactivable).
    if std::env::var("BOOM_NO_DRIFT").is_err() && std::env::var("BOOM_OVERVIEW").is_err() {
        mouse.delta += Vec2::new(0.3, -0.08);
    }

    // `BOOM_WEAPON=glock|c4|sledge` force une arme, sinon on les alterne.
    if auto.weapon_tick.tick(time.delta()).just_finished() {
        let fixed = std::env::var("BOOM_WEAPON").ok();
        game_state.weapon = match fixed.as_deref() {
            Some("glock") => crate::game::Weapon::Glock,
            Some("c4") => crate::game::Weapon::C4,
            Some("sledge") => crate::game::Weapon::Sledge,
            _ => game_state.weapon.next(),
        };
        game_state.ammo = crate::game::GLOCK_MAG;
        game_state.c4 = 5;
    }

    auto.walk.tick(time.delta());
    if auto.walk.is_finished() || std::env::var("BOOM_OVERVIEW").is_ok() {
        keys.release(KeyCode::KeyW);
    } else {
        keys.press(KeyCode::KeyW);
    }

    // Sur le menu, on ne clique pas (sinon on lance une partie).
    let on_menu = std::env::var("BOOM_MENU").is_ok() && !game_state.in_game();
    let overview = std::env::var("BOOM_OVERVIEW").is_ok();

    auto.swing.tick(time.delta());
    if !on_menu && !overview && auto.swing.just_finished() {
        buttons.release(MouseButton::Left);
        buttons.press(MouseButton::Left);
    }

    // Captures d'écran pour la validation visuelle.
    auto.shot.tick(time.delta());
    if auto.shot.just_finished() && auto.shot_index < 3 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!("{SHOT_DIR}/shot{}.png", auto.shot_index)));
        auto.shot_index += 1;
        auto.shot = Timer::from_seconds(5.0, TimerMode::Once);
    }
}