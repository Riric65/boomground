//! BoomGround — destruction de voxels (style Teardown), en bac à sable ou
//! en mode normal avec boutique.
//!
//! Contrôles : ZQSD/WASD, souris, clic gauche pour agir, 1…9/0 ou molette
//! pour changer d'arme, R pour recharger, B pour la boutique, Échap pour le
//! menu.

pub mod debris;
pub mod debug;
pub mod fx;
pub mod game;
pub mod hud;
pub mod install;
pub mod map;
pub mod menu;
pub mod music;
pub mod net;
pub mod perf;
pub mod player;
pub mod rubble;
pub mod shop;
pub mod sim;
pub mod ui;
pub mod vitals;
pub mod vmesh;
pub mod weapon;
pub mod world;

use bevy::camera::visibility::RenderLayers;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::window::{PresentMode, Window, WindowPlugin, WindowResolution};

use game::{in_game, GameState};
use net::client::NetClient;
use world::VoxelWorld;

/// Lance le client (fenêtre, rendu, jeu).
pub fn run() {
    App::new()
        .insert_resource(bevy::audio::DefaultSpatialScale(bevy::audio::SpatialScale::new(0.1)))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.70, 0.90)))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(game::load_settings())
        .init_resource::<VoxelWorld>()
        .init_resource::<debris::Rng>()
        .init_resource::<net::mirror::Mirror>()
        .init_resource::<fx::ParticleDensity>()
        .init_resource::<net::client::WorldLoading>()
        .init_resource::<vmesh::ChunkEntities>()
        .init_resource::<GameState>()
        .init_resource::<fx::Shake>()
        .insert_resource(fx::ShakeNoise(42))
        .init_resource::<perf::FpsLimit>()
        .init_resource::<hud::FpsMeter>()
        .init_resource::<NetClient>()
        .init_resource::<weapon::Hands>()
        .init_resource::<shop::ShopState>()
        .init_resource::<net::client::Lobby>()
        .init_resource::<net::lan::LanBrowser>()
        .init_resource::<ui::Toasts>()
        .add_message::<weapon::Blast>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("BoomGround {}", install::VERSION),
                resolution: WindowResolution::new(1440, 810),
                present_mode: PresentMode::AutoVsync,
                // Les tests automatiques ne volent pas le clavier.
                focused: !debug::enabled(),
                window_level: if debug::enabled() {
                    bevy::window::WindowLevel::AlwaysOnBottom
                } else {
                    bevy::window::WindowLevel::Normal
                },
                ..default()
            }),
            ..default()
        }))
        .add_systems(
            Startup,
            (
                ui::setup_font,
                fx::setup_sounds,
                fx::setup_fire,
                fx::setup_particles,
                music::setup,
                world::setup_voxel_assets,
                world::build_level,
                player::spawn_player,
                weapon::setup_viewmodels,
                setup_scene,
                hud::setup,
                menu::setup,
                shop::setup,
                ui::setup_toasts,
                menu::initial_phase,
                debug::setup,
            )
                .chain(),
        )
        // Interface : menu, lobby, boutique, tests automatiques.
        .add_systems(
            Update,
            (
                debug::force_playing.run_if(debug::enabled),
                debug::autopilot.run_if(debug::enabled),
                debug::overview.run_if(debug::enabled),
                ui::text_fields,
                ui::switches,
                menu::actions,
                menu::flow,
                menu::settings_panel,
                menu::lan_list,
                menu::world_list,
                menu::solo_pause,
                menu::lobby_screen,
                menu::status_text,
                shop::toggle,
                shop::interact,
                shop::refresh,
                menu::visibility,
                ui::hover_buttons,
                ui::update_toasts,
            )
                .chain(),
        )
        // Joueur et armes. Tout ce qui lit les entrées tourne après
        // l'autopilot de test, sinon le clic synthétisé serait perdu.
        .add_systems(
            Update,
            (
                world::handle_reset,
                player::respawn_on_rebuild,
                menu::cinematic_camera,
                rubble::clear_on_rebuild,
                weapon::clear_on_rebuild,
                debug::pose.run_if(debug::enabled),
                player::look,
                player::move_player.run_if(in_game),
                vitals::lifecycle,
                vitals::update_ragdolls,
                vitals::death_camera,
                fx::apply_shake,
                weapon::select,
                weapon::act,
                weapon::update_bombs.run_if(in_game),
                weapon::blast_effects,
                weapon::update_tracers,
                weapon::animate_hands,
            )
                .chain()
                .after(ui::update_toasts),
        )
        // Monde : gravats, débris, effets, rendu, réseau, HUD.
        .add_systems(
            Update,
            (
                debris::update_debris.run_if(in_game),
                fx::fade_flashes,
                fx::update_fireballs,
                net::client::send_state,
                net::client::receive,
                net::client::update_nametags,
                net::client::animate_remotes,
                vmesh::refresh_chunks,
                hud::update,
            )
                .chain()
                .after(weapon::animate_hands),
        )
        // La boucle ne tourne jamais plus vite que l'écran.
        .add_systems(
            Update,
            (net::mirror::smooth, fx::update_puffs, fx::animate_flames, music::update, hud::loading_screen),
        )
        .add_systems(Last, (perf::detect_refresh, perf::limit).chain())
        .run();
}

/// Lumière du soleil et ambiance.
fn setup_scene(mut commands: Commands) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.80, 0.95),
        // Suffisamment fort pour que les faces à l'ombre restent lisibles.
        brightness: 700.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        // Ombres nettes de près, jusqu'à 140 m (la carte fait 153 m).
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 14.0,
            maximum_distance: 140.0,
            ..default()
        }
        .build(),
        Transform::from_translation(Vec3::new(-18.0, 30.0, 12.0)).looking_at(Vec3::ZERO, Vec3::Y),
        // Le soleil éclaire le monde (couche 0) et les armes (couche 1).
        RenderLayers::from_layers(&[0, 1]),
    ));

    // Contre-jour sans ombre : évite les faces totalement noires.
    commands.spawn((
        DirectionalLight {
            illuminance: 1800.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(Vec3::new(20.0, 12.0, -18.0)).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, 1]),
    ));
}
