//! Points de vie, mort et ragdoll.
//!
//! - Solo : les explosions blessent le joueur (voir [`solo_damage`]).
//! - En ligne : le serveur est l'autorité (`Hurt`, `Died`, `Respawned`).
//! - À la mort, le joueur devient un ragdoll (squelette à particules et
//!   contraintes de distance, collisions contre les voxels) et la caméra
//!   passe en troisième personne, bloquée, jusqu'à la réapparition.

use bevy::prelude::*;

use crate::game::{GameState, MAX_HP};
use crate::net::client::NetClient;
use crate::player::{Body, Head, Player, WorldCamera};
use crate::world::VoxelWorld;

/// Particules : 0 tête, 1 poitrine, 2 bassin, 3-4 mains, 5-6 pieds.
const N: usize = 7;
/// Contraintes de distance : (a, b, longueur).
const LINKS: [(usize, usize, f32); 10] = [
    (0, 1, 0.32),
    (1, 2, 0.62),
    (1, 3, 0.66),
    (1, 4, 0.66),
    (2, 5, 0.86),
    (2, 6, 0.86),
    (0, 2, 0.92),
    (3, 4, 0.95),
    (5, 6, 0.34),
    (1, 5, 1.46),
];
const GRAVITY: f32 = 22.0;

/// Un personnage mort.
#[derive(Component)]
pub struct Ragdoll {
    pub p: [Vec3; N],
    prev: [Vec3; N],
    /// 0 : le joueur local ; sinon l'identifiant du joueur distant.
    pub owner: u16,
    pub local: bool,
    age: f32,
}

/// Enfant visuel d'un ragdoll : membre entre deux particules, ou tête.
#[derive(Component)]
pub struct Segment {
    a: usize,
    b: usize,
    thickness: f32,
}

/// Crée le ragdoll d'un joueur à `feet`, lancé avec la vitesse `push`.
pub fn spawn_ragdoll(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    feet: Vec3,
    push: Vec3,
    color: Color,
    owner: u16,
    local: bool,
) {
    let rest = [
        Vec3::new(0.0, 1.75, 0.0),
        Vec3::new(0.0, 1.43, 0.0),
        Vec3::new(0.0, 0.82, 0.0),
        Vec3::new(-0.36, 1.05, 0.0),
        Vec3::new(0.36, 1.05, 0.0),
        Vec3::new(-0.14, 0.0, 0.0),
        Vec3::new(0.14, 0.0, 0.0),
    ];
    let dt = 1.0 / 60.0;
    let mut p = [Vec3::ZERO; N];
    let mut prev = [Vec3::ZERO; N];
    for i in 0..N {
        p[i] = feet + rest[i];
        // Les membres partent un peu dans tous les sens.
        let wobble = Vec3::new((i as f32 * 1.7).sin(), (i as f32 * 2.3).cos().abs(), (i as f32 * 0.9).sin()) * 1.4;
        prev[i] = p[i] - (push + wobble) * dt;
    }

    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let shirt = materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.7,
        ..default()
    });
    let jeans = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.24, 0.42),
        perceptual_roughness: 0.85,
        ..default()
    });
    let skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.93, 0.78, 0.64),
        perceptual_roughness: 0.8,
        ..default()
    });
    commands
        .spawn((
            Ragdoll { p, prev, owner, local, age: 0.0 },
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .with_children(|c| {
            for (a, b, t, m) in [
                (1, 2, 0.46, &shirt),
                (1, 3, 0.2, &shirt),
                (1, 4, 0.2, &shirt),
                (2, 5, 0.26, &jeans),
                (2, 6, 0.26, &jeans),
            ] {
                c.spawn((
                    Segment { a, b, thickness: t },
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(m.clone()),
                    Transform::default(),
                ));
            }
            c.spawn((
                Segment { a: 0, b: 0, thickness: 0.42 },
                Mesh3d(cube.clone()),
                MeshMaterial3d(skin),
                Transform::default(),
            ));
        });
}

/// Physique des ragdolls + mise à jour des membres.
pub fn update_ragdolls(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut commands: Commands,
    mut ragdolls: Query<(Entity, &mut Ragdoll, &Children)>,
    mut segments: Query<(&Segment, &mut Transform)>,
) {
    let frame = time.delta_secs().min(0.05);
    for (entity, mut r, children) in &mut ragdolls {
        r.age += frame;
        if r.age > 30.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Deux sous-pas : plus stable que d'un seul gros pas.
        let dt = frame * 0.5;
        for _ in 0..2 {
            let old = r.p;
            for i in 0..N {
                let v = (r.p[i] - r.prev[i]) * 0.992;
                let mut next = r.p[i] + v + Vec3::NEG_Y * GRAVITY * dt * dt;
                // Contraintes de distance.
                r.prev[i] = r.p[i];
                std::mem::swap(&mut r.p[i], &mut next);
            }
            for _ in 0..4 {
                for &(a, b, len) in &LINKS {
                    let d = r.p[b] - r.p[a];
                    let dist = d.length().max(1e-4);
                    let corr = d * ((dist - len) / dist * 0.5);
                    r.p[a] += corr;
                    r.p[b] -= corr;
                }
            }
            // Collisions avec les voxels, axe par axe.
            for i in 0..N {
                let from = old[i];
                let to = r.p[i];
                let mut cur = from;
                let mut vel = to - from;
                let half = Vec3::splat(0.08);
                for axis in 0..3 {
                    let delta = vel[axis];
                    if delta != 0.0 && world.sweep(&mut cur, half, axis, delta) {
                        vel[axis] = 0.0;
                        // Frottement sur les deux autres axes.
                        for o in 0..3 {
                            if o != axis {
                                vel[o] *= 0.82;
                            }
                        }
                    } else {
                        cur[axis] = from[axis] + delta;
                    }
                }
                r.p[i] = cur;
                r.prev[i] = cur - vel;
            }
        }
        // Rendu.
        for &child in children {
            let Ok((seg, mut t)) = segments.get_mut(child) else {
                continue;
            };
            if seg.a == seg.b {
                t.translation = r.p[seg.a];
                t.scale = Vec3::splat(seg.thickness);
                t.rotation = Quat::IDENTITY;
            } else {
                let (a, b) = (r.p[seg.a], r.p[seg.b]);
                let d = b - a;
                let len = d.length().max(0.05);
                t.translation = (a + b) * 0.5;
                t.rotation = Quat::from_rotation_arc(Vec3::Y, d / len);
                t.scale = Vec3::new(seg.thickness, len, seg.thickness * 0.8);
            }
        }
    }
}

/// Transitions : début de mort (ragdoll), compte à rebours, réapparition.
#[allow(clippy::too_many_arguments)]
pub fn lifecycle(
    time: Res<Time>,
    net: Res<NetClient>,
    world: Res<VoxelWorld>,
    mut game: ResMut<GameState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut was_dead: Local<bool>,
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
    mut head: Query<&mut Transform, (With<Head>, Without<Player>)>,
    ragdolls: Query<(Entity, &Ragdoll)>,
) {
    if !game.in_game() {
        return;
    }
    let Ok((mut transform, mut body)) = player.single_mut() else {
        return;
    };

    if game.dead && !*was_dead {
        let color = net
            .id
            .map(crate::net::client::player_color)
            .unwrap_or(Color::srgb(0.85, 0.55, 0.2));
        let push = game.death_push;
        spawn_ragdoll(
            &mut commands,
            &mut meshes,
            &mut materials,
            transform.translation,
            push,
            color,
            0,
            true,
        );
        body.vel = Vec3::ZERO;
    }
    *was_dead = game.dead;

    if game.dead {
        game.respawn_in = (game.respawn_in - time.delta_secs()).max(0.0);
        // Solo : réapparition locale. En ligne : le serveur décide.
        if !net.is_online() && game.respawn_in <= 0.0 {
            game.respawn_at = Some(crate::player::spawn_point(&world, None));
        }
    }

    if let Some(pos) = game.teleport_to.take() {
        transform.translation = pos;
        body.vel = Vec3::ZERO;
    }

    if let Some(pos) = game.respawn_at.take() {
        game.dead = false;
        game.hp = MAX_HP;
        *was_dead = false;
        transform.translation = pos;
        body.vel = Vec3::ZERO;
        if let Ok(mut h) = head.single_mut() {
            h.translation = Vec3::new(0.0, crate::player::EYE, 0.0);
            h.rotation = Quat::from_rotation_x(body.pitch);
        }
        for (e, r) in &ragdolls {
            if r.local {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Caméra à la troisième personne, bloquée derrière le ragdoll.
pub fn death_camera(
    game: Res<GameState>,
    world: Res<VoxelWorld>,
    ragdolls: Query<&Ragdoll>,
    player: Query<(&Transform, &Body), With<Player>>,
    mut head: Query<&mut Transform, (With<Head>, Without<Player>, Without<WorldCamera>)>,
) {
    if !game.dead {
        return;
    }
    let Some(r) = ragdolls.iter().find(|r| r.local) else {
        return;
    };
    let Ok((pt, body)) = player.single() else { return };
    let Ok(mut h) = head.single_mut() else { return };
    let target = r.p[1];
    // Derrière la direction du regard à la mort, un peu en hauteur.
    let back = Vec3::new(body.yaw.sin(), 0.0, body.yaw.cos());
    let want = target + back * 3.4 + Vec3::Y * 1.8;
    // Ne pas traverser les murs.
    let dir = want - target;
    let dist = dir.length().max(0.01);
    let cam = match world.raycast(target, dir / dist, dist) {
        Some(hit) => target + dir / dist * (hit.point.distance(target) - 0.3).max(0.4),
        None => want,
    };
    let rot_inv = pt.rotation.inverse();
    let look = Transform::from_translation(cam).looking_at(target, Vec3::Y);
    h.translation = rot_inv * (cam - pt.translation);
    h.rotation = rot_inv * look.rotation;
}
