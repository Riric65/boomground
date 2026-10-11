//! Joueur : caméra FPS, déplacements, gravité et collisions avec les voxels.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::world::{VoxelWorld, VOXEL};

/// Point d'apparition du joueur : au milieu de la rue, face au mur de briques.
pub const SPAWN: Vec3 = Vec3::new(0.0, 1.4, 18.0);

/// Hauteur des yeux.
const EYE: f32 = 1.6;
/// Demi-largeur du cylindre de collision.
const HALF_W: f32 = 0.3;
/// Hauteur du cylindre de collision.
const HEIGHT: f32 = 1.7;
const WALK: f32 = 5.0;
const RUN: f32 = 8.5;
const GRAVITY: f32 = 22.0;
const JUMP: f32 = 7.5;


#[derive(Component)]
pub struct Player;

/// Le « crâne » : porte la caméra, reçoit le pitch.
#[derive(Component)]
pub struct Head;

#[derive(Component)]
pub struct WorldCamera;

/// Caméra « vue en main » : ne rend que la couche 1 (la masse), par-dessus.
#[derive(Component)]
pub struct ViewModelCamera;

#[derive(Component, Default)]
pub struct Body {
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    /// Phase du balancement de marche (pour le « head bob »).
    pub bob: f32,
    /// Enfoncement après un atterrissage.
    pub land: f32,
}

pub fn spawn_player(mut commands: Commands) {
    commands.spawn((
        Player,
        Body::default(),
        Transform::from_xyz(SPAWN.x, SPAWN.y, SPAWN.z),
        Visibility::default(),
        children![
            (
                Head,
                Transform::from_xyz(0.0, EYE, 0.0),
                Visibility::default(),
                children![
                    // Caméra du monde (celle qu'on utilise pour les rayons).
                    (
                        WorldCamera,
                        // L'oreille du joueur : les sons 3D sont panés
                        // et atténués par rapport à cette caméra.
                        SpatialListener::default(),
                        Camera3d::default(),
                        Projection::from(PerspectiveProjection {
                            fov: 75f32.to_radians(),
                            ..default()
                        }),
                    ),
                    // Caméra « vue en main » : n'affiche que les armes
                    // (couche 1), rendue par-dessus pour ne jamais traverser le mur.
                    (
                        ViewModelCamera,
                        Camera3d::default(),
                        Camera {
                            order: 1,
                            ..default()
                        },
                        Projection::from(PerspectiveProjection {
                            fov: 70f32.to_radians(),
                            ..default()
                        }),
                        RenderLayers::layer(1),
                    ),
                ],
            ),
        ],
    ));
}

/// Regard à la souris + capture du curseur.
pub fn look(
    settings: Res<crate::game::Settings>,
    mouse: Res<AccumulatedMouseMotion>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions>,
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
    mut head: Query<&mut Transform, (With<Head>, Without<Player>)>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    if buttons.just_pressed(MouseButton::Left)
        && cursor.grab_mode == CursorGrabMode::None
    {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }

    let delta = mouse.delta;
    if delta == Vec2::ZERO {
        return;
    }

    let Ok((mut transform, mut body)) = player.single_mut() else {
        return;
    };
    let Ok(mut head_transform) = head.single_mut() else {
        return;
    };

    let sensitivity = settings.sensitivity;
    let invert = if settings.invert_y { -1.0 } else { 1.0 };
    body.yaw -= delta.x * sensitivity;
    body.pitch = (body.pitch - delta.y * sensitivity * invert)
        .clamp(-FRAC_PI_2 + 0.02, FRAC_PI_2 - 0.02);

    transform.rotation = Quat::from_rotation_y(body.yaw);
    head_transform.rotation = Quat::from_rotation_x(body.pitch);
}

/// Déplacements + gravité + collisions voxel par voxel.
pub fn move_player(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    world: Res<VoxelWorld>,
    mut query: Query<(&mut Transform, &mut Body), With<Player>>,
    mut heads: Query<&mut Transform, (With<Head>, Without<Player>)>,
) {
    let Ok((mut transform, mut body)) = query.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);

    // Direction voulue dans le repère du joueur.
    let forward = Vec3::new(-body.yaw.sin(), 0.0, -body.yaw.cos());
    let right = Vec3::new(body.yaw.cos(), 0.0, -body.yaw.sin());
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish -= right;
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        RUN
    } else {
        WALK
    };
    let wish = wish.normalize_or_zero() * speed;
    body.vel.x = wish.x;
    body.vel.z = wish.z;

    let was_airborne = !body.grounded;
    body.vel.y -= GRAVITY * dt;
    if body.grounded && keys.pressed(KeyCode::Space) {
        body.vel.y = JUMP;
        body.grounded = false;
    }

    let mut pos = transform.translation;

    body.grounded = false;
    if sweep(&world, &mut pos, 1, body.vel.y * dt) {
        if body.vel.y <= 0.0 {
            body.grounded = true;
        }
        body.vel.y = 0.0;
    }
    if sweep(&world, &mut pos, 0, body.vel.x * dt) {
        body.vel.x = 0.0;
    }
    if sweep(&world, &mut pos, 2, body.vel.z * dt) {
        body.vel.z = 0.0;
    }

    // Filet de sécurité : hors de la carte, on réapparaît au point de départ.
    if pos.y < -10.0 {
        pos = SPAWN;
        body.vel = Vec3::ZERO;
    }

    transform.translation = pos;

    // Head bob + enfoncement à l'atterrissage : ça donne du poids.
    let speed = body.vel.length();
    if body.grounded {
        body.bob += speed * dt * 5.5;
        if was_airborne && body.vel.y > 0.0 {
            body.land = (body.vel.y * 0.12).min(0.22);
        }
    }
    body.land = (body.land - dt * 1.6).max(0.0);
    let intensity = (speed / WALK).clamp(0.0, 1.4);
    if let Ok(mut head) = heads.single_mut() {
        head.translation.y = EYE - body.land + (body.bob.sin() * 0.035 * intensity);
        head.translation.x = body.bob.cos() * 0.5 * 0.02 * intensity;
    }
}

/// Le joueur contre les blocs de gravats.
///
/// Sans ça on traversait les tas de débris. Le joueur est repoussé hors du
/// bloc par l'axe de plus faible pénétration (il peut donc se hisser dessus),
/// et un bloc touché reçoit une petite impulsion : on bouscule les tas.
/// Déplacement maximal accordé par image quand on gravit un bloc.
const MAX_STEP: f32 = 0.25;

pub fn collide_chunks(
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
    mut blocks: Query<(&Transform, &mut crate::rubble::Chunk), Without<Player>>,
) {
    let Ok((mut transform, mut body)) = player.single_mut() else {
        return;
    };
    let mut pos = transform.translation;

    // Deux passes : plusieurs blocs se touchent souvent.
    for _ in 0..2 {
        for (block_transform, mut block) in &mut blocks {
            let center = block_transform.translation;
            let half = block.half;
            if half == Vec3::ZERO {
                continue;
            }

            let player_min = Vec3::new(pos.x - HALF_W, pos.y, pos.z - HALF_W);
            let player_max = Vec3::new(pos.x + HALF_W, pos.y + HEIGHT, pos.z + HALF_W);
            let block_min = center - half;
            let block_max = center + half;

            let overlap = player_max.min(block_max) - player_min.max(block_min);
            if overlap.min_element() <= 0.0 {
                continue;
            }

            // Distance pour monter sur le dessus / passer dessous.
            let up = block_max.y - player_min.y;
            let down = player_max.y - block_min.y;

            if up <= down && up <= overlap.x && up <= overlap.z {
                // Borné : on monte sur le bloc, on ne se téléporte pas dessus.
                pos.y += up.min(MAX_STEP);
                if body.vel.y < 0.0 {
                    body.vel.y = 0.0;
                }
                body.grounded = true;
            } else if down < up && down <= overlap.x && down <= overlap.z {
                pos.y -= down;
                body.vel.y = body.vel.y.min(0.0);
            } else if overlap.x <= overlap.z {
                let sign = if pos.x < center.x { 1.0 } else { -1.0 };
                pos.x += overlap.x * sign;
                body.vel.x = 0.0;
                block.vel.x += sign * 1.2;
                block.sleeping = false;
            } else {
                let sign = if pos.z < center.z { 1.0 } else { -1.0 };
                pos.z += overlap.z * sign;
                body.vel.z = 0.0;
                block.vel.z += sign * 1.2;
                block.sleeping = false;
            }
        }
    }

    transform.translation = pos;
}

fn axis(v: Vec3, a: usize) -> f32 {
    match a {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Boîte de collision autour des pieds du joueur.
fn aabb(pos: Vec3) -> (Vec3, Vec3) {
    (
        Vec3::new(pos.x - HALF_W, pos.y, pos.z - HALF_W),
        Vec3::new(pos.x + HALF_W, pos.y + HEIGHT, pos.z + HALF_W),
    )
}

/// Déplace `pos` d'un pas sur un axe et le repousse proprement s'il entre
/// dans un voxel. Renvoie `true` s'il y a eu collision.
fn sweep(world: &VoxelWorld, pos: &mut Vec3, axis_index: usize, delta: f32) -> bool {
    if delta == 0.0 {
        return false;
    }
    set_axis(pos, axis_index, axis(*pos, axis_index) + delta);

    let (min, max) = aabb(*pos);
    // Offset de la face « basse » / « haute » par rapport à `pos`.
    let (lo, hi) = if axis_index == 1 {
        (0.0, HEIGHT)
    } else {
        (-HALF_W, HALF_W)
    };

    let mut bound: Option<f32> = None;
    world.visit_cells(min, max, |c| {
        if !world.is_solid(c) {
            return;
        }
        let cmin = axis(world.cell_min(c), axis_index);
        let face = if delta < 0.0 { cmin + VOXEL } else { cmin };
        bound = Some(match bound {
            None => face,
            // En descendant on prend la surface la plus haute,
            // en montant la surface la plus basse.
            Some(b) if delta < 0.0 => b.max(face),
            Some(b) => b.min(face),
        });
    });

    match bound {
        Some(face) => {
            set_axis(pos, axis_index, face - if delta < 0.0 { lo } else { hi });
            true
        }
        None => false,
    }
}

fn set_axis(v: &mut Vec3, a: usize, value: f32) {
    match a {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}
