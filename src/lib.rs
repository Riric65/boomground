//! BoomGround — destruction de voxels (style Teardown), en bac à sable ou
//! en mode normal avec boutique.
//!
//! Contrôles : ZQSD/WASD, souris, clic gauche pour agir, 1…9/0 ou molette
//! pour changer d'arme, R pour recharger, B pour la boutique, Échap pour le
//! menu.

pub mod chat;
pub mod clipboard;
pub mod crashlog;
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
pub mod rpc;
pub mod rubble;
pub mod servers;
pub mod shop;
pub mod sim;
pub mod touch;
pub mod ui;
pub mod vehicle;
pub mod vitals;
pub mod voice;
pub mod vmesh;
pub mod weapon;
pub mod world;

use bevy::camera::visibility::RenderLayers;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::window::{PresentMode, Window, WindowPlugin, WindowResolution};

use game::{simulating, GameState};
use net::client::NetClient;
use world::VoxelWorld;

/// Lance le client (fenêtre, rendu, jeu).
pub fn run() {
    crashlog::install_panic_hook();
    App::new()
        .insert_resource(bevy::audio::DefaultSpatialScale(bevy::audio::SpatialScale::new(0.03)))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.70, 0.90)))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(game::load_settings())
        .init_resource::<VoxelWorld>()
        .init_resource::<debris::Rng>()
        .init_resource::<net::mirror::Mirror>()
        .init_resource::<fx::ParticleDensity>()
        .init_resource::<chat::Chat>()
        .init_resource::<servers::ServerList>()
        .init_resource::<voice::VoiceInbox>()
        .init_resource::<voice::Voice>()
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
        .init_resource::<touch::TouchState>()
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
                // Le terrain de jeu occupe tout l'écran, sans barre de titre
                // ni bord : indispensable sur un téléphone.
                decorations: !touch::is_touch_platform(),
                ..default()
            }),
            ..default()
        }))
        // Erreurs GPU : journalisées, et non fatales sur téléphone.
        .insert_resource(bevy::render::error_handler::RenderErrorHandler(crashlog::on_render_error))
        .add_systems(
            Startup,
            (
                ui::setup_font,
                fx::setup_sounds,
                fx::setup_fire,
                fx::setup_particles,
                music::setup,
                rpc::setup.run_if(desktop),
                world::setup_voxel_assets,
                world::build_level,
                player::spawn_player,
                weapon::setup_viewmodels,
                setup_scene,
                hud::setup,
                touch::setup,
                menu::setup,
                shop::setup,
                ui::setup_toasts,
                chat::setup,
                voice::setup_indicator.run_if(desktop),
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
                // Le tactile est lu **avant** tout le reste : le regard, le
                // déplacement et le tir consomment ces valeurs la même
                // image. Rien ne tourne sur ordinateur : les zones tactiles
                // sont des nœuds plein écran « pickables » qui avalerait la
                // souris si elles restaient présentes.
                touch::pointer.run_if(touch::on_touch),
                touch::layout.run_if(touch::on_touch),
                ui::text_fields,
                ui::switches,
                menu::actions,
                menu::flow,
                menu::settings_panel,
                menu::lan_list,
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
                player::move_player.run_if(simulating),
                vitals::lifecycle,
                vitals::update_ragdolls,
                vitals::feedback,
                vitals::death_camera,
                vehicle::camera,
                fx::apply_shake,
                weapon::select,
                weapon::act,
                weapon::update_bombs.run_if(simulating),
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
                debris::update_debris.run_if(simulating),
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
            (
                net::mirror::smooth,
                fx::update_puffs,
                fx::animate_flames,
                music::update,
                hud::loading_screen,
                hud::damage_overlay,
                rpc::update.run_if(desktop),
                vehicle::controls,
                vehicle::engine_sound,
                voice::update.run_if(desktop),
                voice::indicator.run_if(desktop),
                menu::world_list,
                menu::solo_pause,
                menu::server_list_ui,
                menu::map_size_highlight,
                window_icon.run_if(desktop),
                touch::clear.run_if(touch::on_touch),
                ui::soft_keyboard.run_if(touch::on_touch),
                ui_scale,
            ),
        )
        .add_systems(PostStartup, crashlog::show_last)
        .add_systems(Update, chat::update.after(ui::text_fields))
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

/// Icône de la fenêtre (barre de titre, barre des tâches).
fn window_icon(mut done: Local<bool>, mut tries: Local<u32>) {
    if *done || *tries > 600 {
        return;
    }
    *tries += 1;
    const RGBA: &[u8] = include_bytes!("../assets/icon/window.rgba");
    let ok = bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let mut any = false;
        for window in windows.windows.values() {
            if let Ok(icon) = winit::window::Icon::from_rgba(RGBA.to_vec(), 64, 64) {
                window.set_window_icon(Some(icon));
                any = true;
            }
        }
        any
    });
    *done = ok;
}

/// Échelle de l'interface : suit la taille de la fenêtre (de 1280×720 à la
/// 4K), la mise en page de référence étant 1440×810.
fn ui_scale(window: Query<&Window, With<bevy::window::PrimaryWindow>>, mut scale: ResMut<bevy::ui::UiScale>) {
    let Ok(w) = window.single() else { return };
    // Téléphone en paysage : écran plus petit, on descend plus bas.
    let min = if touch::is_touch_platform() { 0.4 } else { 0.55 };
    let s = (w.width() / 1440.0).min(w.height() / 810.0).clamp(min, 2.4);
    if (scale.0 - s).abs() > 0.01 {
        scale.0 = s;
    }
}

/// Condition de système : fonctions propres à l'ordinateur (Discord, micro,
/// icône de fenêtre), sans objet sur téléphone.
fn desktop() -> bool {
    !touch::is_touch_platform()
}

/// Point d'entrée Android : `#[bevy_main]` exporte `android_main`, que
/// l'activité Java appelle après avoir chargé `libboomground.so`.
#[cfg(target_os = "android")]
#[bevy::prelude::bevy_main]
fn main() {
    run();
}
