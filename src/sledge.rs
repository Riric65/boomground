//! La masse (sledge) : animation de frappe, visée au rayon, destruction.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::debris::{spawn_debris, Rng};
use crate::fx::{self, SfxKind};
use crate::game::{in_game, GameState, Weapon};
use crate::player::WorldCamera;
use crate::rubble::{self, Chunks};
use crate::world::{VoxelAssets, VoxelWorld};

/// Durée totale d'un coup, en secondes.
const SWING_TIME: f32 = 0.42;
/// Moment de l'impact dans le coup (0..1).
const IMPACT: f32 = 0.58;
/// Portée du coup en mètres.
const REACH: f32 = 4.5;
/// Rayon de la boule de destruction, en mètres.
const BLAST: f32 = 0.9;
/// Nombre maximum de débris par coup.
const MAX_DEBRIS: usize = 12;

/// Inclinaison du poignet (positif = tête en l'air).
/// Les angles sont choisis pour que la masse reste cadrée pendant tout le coup.
const IDLE: f32 = 0.42;
const RAISED: f32 = 0.98;
const HIT: f32 = -0.26;

/// Position du poignet dans l'espace de la caméra : bas-droite, près de
/// l'œil, pour que le manche remonte vers le centre comme dans Teardown.
const PIVOT: Vec3 = Vec3::new(0.42, -0.52, -0.20);
/// Lacet : la tête part vers la gauche de l'écran.
const YAW: f32 = 0.45;
/// Roulis : léger, pour que la tête ne soit pas parfaitement horizontale.
const ROLL: f32 = 0.10;

#[derive(Component)]
pub struct Sledge {
    /// Progression du coup en cours (`None` = au repos).
    t: Option<f32>,
    fired: bool,
}

/// Construit la masse et l'attache à la tête du joueur.
pub fn spawn_sledge(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    head: Query<Entity, With<crate::player::Head>>,
) {
    let Ok(head) = head.single() else {
        return;
    };

    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.27, 0.13),
        perceptual_roughness: 0.75,
        ..default()
    });
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.52, 0.55, 0.60),
        metallic: 0.5,
        perceptual_roughness: 0.45,
        ..default()
    });

    let rubber = materials.add(StandardMaterial {
        base_color: Color::srgb(0.10, 0.10, 0.12),
        perceptual_roughness: 0.9,
        ..default()
    });

    let handle = meshes.add(Cylinder::new(0.028, 1.0));
    let grip = meshes.add(Cylinder::new(0.036, 0.26));
    let head_mesh = meshes.add(Cuboid::new(0.38, 0.17, 0.17));
    let collar = meshes.add(Cuboid::new(0.1, 0.1, 0.16));
    let face = meshes.add(Cylinder::new(0.085, 0.05));
    let wedge = meshes.add(Cuboid::new(0.3, 0.08, 0.16));

    commands.entity(head).with_children(|parent| {
        parent.spawn((
            Sledge {
                t: None,
                fired: false,
            },
            Transform::from_translation(PIVOT),
            Visibility::default(),
            children![
                (
                    Mesh3d(handle),
                    MeshMaterial3d(wood),
                    // Le cylindre est aligné sur Y : on le couche sur -Z.
                    Transform::from_xyz(0.0, 0.0, -0.5)
                        .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                // Poignée, juste sous la main.
                (
                    Mesh3d(grip),
                    MeshMaterial3d(rubber.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.13)
                        .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                // Tête : corps + bague + coin supérieur effilé + face de frappe.
                (
                    Mesh3d(head_mesh),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(0.0, 0.0, -1.02),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(collar),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.9),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(wedge),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(0.0, 0.11, -1.01)
                        .with_rotation(Quat::from_rotation_x(-0.2)),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(face.clone()),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(0.21, 0.0, -1.02)
                        .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(face.clone()),
                    MeshMaterial3d(steel.clone()),
                    Transform::from_xyz(-0.21, 0.0, -1.02)
                        .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                    RenderLayers::layer(1),
                    NotShadowCaster,
                ),
            ],
        ));
    });
}

/// Courbe du coup : relevé -> frappe -> retour au repos.
fn swing_angle(t: f32) -> f32 {
    if t < 0.25 {
        lerp(IDLE, RAISED, ease(t / 0.25))
    } else if t < IMPACT {
        lerp(RAISED, HIT, ease((t - 0.25) / (IMPACT - 0.25)))
    } else {
        lerp(HIT, IDLE, ease((t - IMPACT) / (1.0 - IMPACT)))
    }
}

/// Orientation de la masse : le manche part du coin bas-droit et la tête
/// arrive au centre de l'écran, inclinée comme dans Teardown.
fn rest_rotation(pitch: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, YAW, pitch, ROLL)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Smoothstep : adoucit les deux extrémités du mouvement.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Anime la masse et gère la destruction.
#[allow(clippy::too_many_arguments)]
pub fn swing(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions>,
    game: Res<GameState>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut query: Query<(&mut Transform, &mut Sledge), With<Sledge>>,
    mut world: ResMut<VoxelWorld>,
    mut chunks: ResMut<Chunks>,
    assets: Res<VoxelAssets>,
    mut rng: ResMut<Rng>,
    mut shake: ResMut<fx::Shake>,
    mut audio: ResMut<Assets<bevy::audio::AudioSource>>,
    net: Option<ResMut<crate::net::client::NetClient>>,
    mut commands: Commands,
) {
    let Ok((mut transform, mut sledge)) = query.single_mut() else {
        return;
    };

    // La masse n'est pilotable que si elle est en main.
    let active = game.weapon == Weapon::Sledge && game.in_game();
    let _ = in_game;
    let grabbed = cursor.grab_mode == CursorGrabMode::Locked;
    let mut impact = false;

    if !active {
        sledge.t = None;
        transform.rotation = rest_rotation(IDLE);
        transform.translation = PIVOT;
        return;
    }

    let camera_pos = camera
        .single()
        .map(|c| c.translation())
        .unwrap_or(Vec3::ZERO);

    if let Some(t) = sledge.t {
        let next = t + time.delta_secs() / SWING_TIME;
        if !sledge.fired && t < IMPACT && next >= IMPACT {
            sledge.fired = true;
            impact = true;
        }
        let was_start = t <= 0.0;
        sledge.t = if next >= 1.0 { None } else { Some(next) };
        if was_start {
            fx::play_at(&mut commands, &mut audio, SfxKind::Swing, 0.5, camera_pos);
        }
    } else if grabbed && buttons.just_pressed(MouseButton::Left) {
        sledge.t = Some(0.0);
        sledge.fired = false;
    }

    let t = sledge.t.unwrap_or(0.0);
    transform.rotation = rest_rotation(swing_angle(t));
    transform.translation = PIVOT;

    if impact {
        let Ok(cam) = camera.single() else { return };
        let origin = cam.translation();
        let dir = cam.transform_vector(Vec3::NEG_Z);

        if let Some(mut net) = net {
            crate::net::client::send_hit(
                &mut net,
                crate::net::protocol::HitKind::Sledge,
                origin,
                dir,
                BLAST,
            );
        }

        let Some(hit) = world.raycast(origin, dir, REACH) else {
            return;
        };

        // Les voxels cassés deviennent des blocs qui tombent — rien ne
        // disparaît pour de bon.
        let destroyed = world.carve(hit.point, BLAST);
        fx::play_at(&mut commands, &mut audio, SfxKind::Hit, 0.9, hit.point);
        shake.add(0.35);

        for &cell in &destroyed {
            rubble::break_voxel(&mut chunks, &mut commands, &assets, &world, cell, hit.point, &mut rng);
        }

        let n = destroyed.len().min(MAX_DEBRIS);
        for _ in 0..n {
            let cell = destroyed[rng.usize(destroyed.len())];
            let pos = world.cell_center(cell);
            let mat = world.material(cell);
            let away = (pos - hit.point).normalize_or_zero();
            // Les débris sont éjectés hors du mur, avec du radial et du hasard.
            let vel = hit.normal * (2.5 + rng.f32() * 3.0)
                + away * (1.5 + rng.f32() * 2.0)
                + Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 2.5;
            spawn_debris(&mut commands, &assets, &mut rng, pos, mat, vel);
        }
    }
}
