//! BoomGround — destruction de voxels à la masse (style Teardown).
//!
//! Contrôles : ZQSD/WASD, souris, clic gauche pour agir, 1/2/3 pour changer
//! d'arme, R pour reconstruire, Échap pour libérer le curseur.

pub mod debug;
pub mod debris;
pub mod fx;
pub mod game;
pub mod hud;
pub mod map;
pub mod menu;
pub mod net;
pub mod perf;
pub mod player;
pub mod rubble;
pub mod sledge;
pub mod weapon;
pub mod vmesh;
pub mod world;

use bevy::camera::visibility::RenderLayers;
use bevy::diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::light::{DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::window::{PresentMode, Window, WindowPlugin, WindowResolution};

use game::GameState;
use net::client::NetClient;
use world::VoxelWorld;

/// La carte vient d'être reconstruite : on relance tous les morceaux.
fn world_reset(world: Res<VoxelWorld>) -> bool {
    world.dirty && world.alive == world.total
}

/// Lance le client (fenêtre, rendu, jeu).
pub fn run() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.46, 0.66, 0.90)))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .init_resource::<VoxelWorld>()
        .init_resource::<debris::Rng>()
        .init_resource::<rubble::Chunks>()
        .init_resource::<vmesh::ChunkEntities>()
        .init_resource::<GameState>()
        .init_resource::<game::Settings>()
        .init_resource::<fx::Shake>()
        .insert_resource(fx::ShakeNoise(42))
        .init_resource::<perf::FpsLimit>()
        .init_resource::<hud::FpsMeter>()
        .init_resource::<NetClient>()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "BoomGround".into(),
                    resolution: WindowResolution::new(1600, 900),
                    // Synchronisé sur le rafraîchissement de l'écran.
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            }),
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(5),
                ..default()
            },
        ))
        .add_systems(
            Startup,
            (
                world::setup_voxel_assets,
                player::spawn_player,
                sledge::spawn_sledge,
                weapon::setup,
                world::build_level,
                vmesh::build_all,
                setup_scene,
                hud::setup,
                menu::setup,
                debug::setup,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                debug::force_playing.run_if(debug::enabled),
                debug::autopilot.run_if(debug::enabled),
                debug::open_multi.run_if(debug::enabled),
                debug::open_settings.run_if(debug::enabled),
                menu::settings_input,
                menu::toggle_visibility,
                menu::menu_input,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                player::look.run_if(game::in_game),
                player::move_player.run_if(game::in_game),
                player::collide_chunks.run_if(game::in_game),

                fx::apply_shake.run_if(game::in_game),
                debug::overview.run_if(debug::enabled),
                world::handle_reset,
                weapon::select.run_if(game::in_game),
                weapon::gun_anim,
                rubble::sync_scales,
            )
                .chain()
                // Tout ce qui consomme les entrées doit tourner APRÈS
                // l'autopilot de test, sinon le clic synthétisé peut être
                // effacé avant d'être lu (et rien ne se casse).
                .after(debug::autopilot),
        )
        .add_systems(
            Update,
            (
                sledge::swing,
                weapon::fire_glock,
                weapon::c4_system,
                rubble::update,
                debris::update_debris,
                fx::fade_flashes,
                vmesh::refresh_chunks,
                vmesh::rebuild_all.run_if(world_reset),
                net::client::send_state,
                net::client::receive,
                hud::update_stats,
            )
                .chain()
                .after(weapon::select)
                .after(rubble::sync_scales),
        )
        // La boucle ne tourne jamais plus vite que l'écran.
        .add_systems(Last, (perf::detect_refresh, perf::limit).chain())
        .run();
}

/// Lumière du soleil, ambiance et sol.
fn setup_scene(mut commands: Commands) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.80, 0.95),
        // Suffisamment fort pour que les faces à l'ombre restent lisibles
        // au lieu d'être noires.
        brightness: 900.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 7000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_translation(Vec3::new(-18.0, 28.0, 16.0)).looking_at(Vec3::ZERO, Vec3::Y),
        // Le soleil éclaire le monde (couche 0) et les armes (couche 1).
        RenderLayers::from_layers(&[0, 1]),
    ));

    // Contre-jour sans ombre : évite les faces totalement noires et donne
    // du volume aux bâtiments.
    commands.spawn((
        DirectionalLight {
            illuminance: 2200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(Vec3::new(20.0, 12.0, -18.0)).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, 1]),
    ));

    // Le sol est fait de voxels destructibles : aucun plan ici.
}