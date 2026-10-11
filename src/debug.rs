//! Mode test (`BOOM_AUTO=1`) : le jeu joue tout seul (regard, marche,
//! actions) et écrit des captures dans le dossier temporaire
//! (`<tmp>/boomground/`), ce qui permet de valider le rendu et la
//! destruction sans simulation de clavier.
//!
//! Variables utiles : `BOOM_WEAPON=hammer|sledge|glock|rifle|c4|…`,
//! `BOOM_MODE=normal`, `BOOM_SEED=1234`, `BOOM_MENU=1` (reste au menu),
//! `BOOM_SHOP=1` (ouvre la boutique), `BOOM_OVERVIEW=1` (vue du dessus),
//! `BOOM_NO_DRIFT=1` (caméra fixe), `BOOM_SERVER=hôte[:port]`.
//!
//! Inactif en fonctionnement normal.

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::CursorOptions;

use crate::game::{GameState, Mode, Phase, WeaponId, ALL_WEAPONS};

pub fn enabled() -> bool {
    std::env::var("BOOM_AUTO").is_ok_and(|v| v == "1")
}

fn env(name: &str) -> bool {
    std::env::var(name).is_ok()
}

/// Dossier des captures automatiques.
pub fn shot_dir() -> PathBuf {
    std::env::temp_dir().join("boomground")
}

/// Injecte la ressource de test si le mode est actif.
pub fn setup(mut commands: Commands) {
    if enabled() {
        std::fs::create_dir_all(shot_dir()).ok();
        commands.insert_resource(Auto::default());
    }
}

/// Vue d'ensemble de la carte (validation) : caméra haute, sans dérive.
pub fn overview(
    mut player: Query<(&mut Transform, &mut crate::player::Body), With<crate::player::Player>>,
    mut head: Query<&mut Transform, (With<crate::player::Head>, Without<crate::player::Player>)>,
) {
    if !env("BOOM_OVERVIEW") {
        return;
    }
    if let Ok((mut t, mut body)) = player.single_mut() {
        t.translation = Vec3::new(-30.0, 70.0, 60.0);
        t.rotation = Quat::from_rotation_y(-0.45);
        body.fly = true;
        body.vel = Vec3::ZERO;
    }
    if let Ok(mut t) = head.single_mut() {
        t.rotation = Quat::from_rotation_x(-0.75);
    }
}

/// Orientation / position de départ des tests : `BOOM_YAW=1.57`,
/// `BOOM_POS=x,z` (mètres).
pub fn pose(
    mut frames: Local<u32>,
    world: Res<crate::world::VoxelWorld>,
    mut player: Query<(&mut Transform, &mut crate::player::Body), With<crate::player::Player>>,
) {
    *frames += 1;
    if *frames != 3 {
        return;
    }
    let Ok((mut t, mut body)) = player.single_mut() else { return };
    if let Some(yaw) = std::env::var("BOOM_YAW").ok().and_then(|v| v.parse::<f32>().ok()) {
        body.yaw = yaw;
        t.rotation = Quat::from_rotation_y(yaw);
    }
    if let Some((x, z)) = std::env::var("BOOM_POS").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?))
    }) {
        t.translation = Vec3::new(x, world.ground_height(x, z) + 0.05, z);
    }
}

/// En mode auto, on saute le menu et on démarre directement la partie.
pub fn force_playing(
    mut game: ResMut<GameState>,
    mut world: ResMut<crate::world::VoxelWorld>,
    mut cursor: Single<&mut CursorOptions>,
    mut net: ResMut<crate::net::client::NetClient>,
    mut shop: ResMut<crate::shop::ShopState>,
    mut done: Local<bool>,
) {
    if *done || env("BOOM_MENU") {
        return;
    }
    *done = true;
    if let Ok(address) = std::env::var("BOOM_SERVER") {
        net.address = address;
        match net.connect() {
            Ok(target) => info!("connecté à {target}"),
            Err(e) => error!("connexion impossible : {e}"),
        }
    }
    game.mode = if std::env::var("BOOM_MODE").is_ok_and(|m| m == "normal") {
        Mode::Normal
    } else {
        Mode::Sandbox
    };
    if let Some(seed) = std::env::var("BOOM_SEED").ok().and_then(|s| crate::game::parse_seed(&s)) {
        game.seed = seed;
        world.rebuild(seed);
    }
    game.reset_inventory();
    if game.mode == Mode::Normal {
        game.money = 5_000;
    }
    game.started = true;
    game.phase = Phase::Playing;
    if env("BOOM_SHOP") {
        shop.open = true;
    } else {
        crate::player::grab(&mut cursor, true);
    }
}

#[derive(Resource)]
pub struct Auto {
    /// Déclenche une action toutes les 0,45 s.
    swing: Timer,
    weapon_tick: Timer,
    /// Marche pendant 1,1 s, puis s'arrête.
    walk: Timer,
    shot: Timer,
    shot_index: u32,
    right_clicks: u32,
}

impl Default for Auto {
    fn default() -> Self {
        Self {
            swing: Timer::from_seconds(0.45, TimerMode::Repeating),
            weapon_tick: Timer::from_seconds(4.0, TimerMode::Repeating),
            walk: Timer::from_seconds(
                std::env::var("BOOM_WALK")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1.1),
                TimerMode::Once,
            ),
            shot: Timer::from_seconds(3.0, TimerMode::Once),
            shot_index: 0,
            right_clicks: 0,
        }
    }
}

/// Regard qui balaye, marche, puis agit en continu.
#[allow(clippy::too_many_arguments)]
pub fn autopilot(
    auto: Option<ResMut<Auto>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse: ResMut<AccumulatedMouseMotion>,
    mut game: ResMut<GameState>,
    mut commands: Commands,
    time: Res<Time>,
) {
    let Some(mut auto) = auto else { return };
    let overview = env("BOOM_OVERVIEW");
    let on_menu = env("BOOM_MENU") || env("BOOM_SHOP");

    // Balayage lent, pour voir sous différents angles (désactivable).
    if !env("BOOM_NO_DRIFT") && !overview && !on_menu {
        mouse.delta += Vec2::new(0.6, 0.0);
    }

    // `BOOM_WEAPON=nom` force une arme, sinon on les alterne.
    if auto.weapon_tick.tick(time.delta()).just_finished() || auto.shot_index == 0 {
        let fixed = std::env::var("BOOM_WEAPON").ok();
        let wanted = fixed.as_deref().and_then(|name| {
            ALL_WEAPONS
                .iter()
                .copied()
                .find(|w| format!("{w:?}").eq_ignore_ascii_case(name))
        });
        game.weapon = match wanted {
            Some(w) => w,
            None if auto.weapon_tick.just_finished() => {
                let owned = game.owned();
                let i = owned.iter().position(|&w| w == game.weapon).unwrap_or(0);
                owned[(i + 1) % owned.len()]
            }
            None => game.weapon,
        };
        if game.weapon == WeaponId::Hammer && fixed.is_none() && game.sandbox() {
            game.weapon = WeaponId::Sledge;
        }
    }

    auto.walk.tick(time.delta());
    if auto.walk.is_finished() || overview || on_menu {
        keys.release(KeyCode::KeyW);
    } else {
        keys.press(KeyCode::KeyW);
    }

    auto.swing.tick(time.delta());
    if !on_menu && !overview && auto.swing.just_finished() {
        buttons.release(MouseButton::Left);
        buttons.press(MouseButton::Left);
        // De temps en temps, clic droit : la C4 posée saute.
        auto.right_clicks += 1;
        buttons.release(MouseButton::Right);
        if auto.right_clicks % 6 == 0 {
            buttons.press(MouseButton::Right);
        }
    }

    // Captures d'écran pour la validation visuelle.
    auto.shot.tick(time.delta());
    if auto.shot.just_finished() && auto.shot_index < 4 {
        let path = shot_dir().join(format!("shot{}.png", auto.shot_index));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        auto.shot_index += 1;
        auto.shot = Timer::from_seconds(2.5, TimerMode::Once);
    }
}
