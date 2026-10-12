//! Conduite côté client : commandes, caméra de poursuite, moteur sonore.
//!
//! Le véhicule lui-même est un corps rigide simulé par le serveur (voir
//! `sim`) ; le client envoie ses commandes et suit le reflet du corps.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::fx::{SfxBank, SfxKind};
use crate::game::{GameState, Phase};
use crate::net::client::{Lobby, NetClient};
use crate::net::mirror::{Mirror, NetBody};
use crate::net::protocol::Packet;
use crate::player::{Body, Head, Player};

/// Voiture la plus proche d'un point (index, distance), à moins de `reach`.
pub fn nearest_car(world: &crate::world::VoxelWorld, from: Vec3, reach: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, &min) in world.cars.iter().enumerate() {
        let c = world.cell_center(min + IVec3::new(2, 3, 5));
        let d = c.distance(from);
        // Une voiture déjà en pièces n'a plus de voxels à son emplacement.
        if d < reach && best.is_none_or(|(_, bd)| d < bd) && world.is_solid(min + IVec3::new(2, 1, 5)) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Envoie les commandes de conduite (20 fois par seconde), sort du véhicule
/// avec `F`, klaxonne avec `H`.
#[allow(clippy::too_many_arguments)]
pub fn controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    game: Res<GameState>,
    mut net: ResMut<NetClient>,
    bank: Res<SfxBank>,
    mut commands: Commands,
    mirror: Res<Mirror>,
    bodies: Query<&NetBody>,
    mut timer: Local<f32>,
) {
    if game.driving == 0 || game.phase != Phase::Playing || game.typing {
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        net.send_raw(Packet::Exit);
        return;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        let at = mirror
            .body_entity(game.driving)
            .and_then(|e| bodies.get(e).ok())
            .map(|b| b.pos)
            .unwrap_or(Vec3::ZERO);
        crate::fx::play_at(&mut commands, &bank, SfxKind::Horn, 1.0, at);
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.05;
    let axis = |plus: &[KeyCode], minus: &[KeyCode]| -> i8 {
        i8::from(keys.any_pressed(plus.iter().copied())) - i8::from(keys.any_pressed(minus.iter().copied()))
    };
    let throttle = axis(&[KeyCode::KeyW, KeyCode::ArrowUp], &[KeyCode::KeyS, KeyCode::ArrowDown]);
    // Gauche = rotation positive autour de Y.
    let steer = axis(&[KeyCode::KeyA, KeyCode::ArrowLeft], &[KeyCode::KeyD, KeyCode::ArrowRight]);
    net.send_raw(Packet::Drive {
        throttle,
        steer,
        brake: keys.pressed(KeyCode::Space),
    });
}

/// Caméra de poursuite : derrière et au-dessus de la voiture ; le joueur
/// suit le véhicule (écoute, positions envoyées).
#[allow(clippy::too_many_arguments)]
pub fn camera(
    time: Res<Time>,
    game: Res<GameState>,
    lobby: Res<Lobby>,
    mirror: Res<Mirror>,
    world: Res<crate::world::VoxelWorld>,
    bodies: Query<&NetBody>,
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
    mut head: Query<&mut Transform, (With<Head>, Without<Player>)>,
    mut smooth: Local<Option<Vec3>>,
) {
    let _ = lobby;
    if game.driving == 0 {
        *smooth = None;
        return;
    }
    let Some(entity) = mirror.body_entity(game.driving) else { return };
    let Ok(b) = bodies.get(entity) else { return };
    let Ok((mut pt, mut body)) = player.single_mut() else { return };
    let Ok(mut h) = head.single_mut() else { return };
    pt.translation = b.pos - Vec3::Y * 0.6;
    body.vel = Vec3::ZERO;

    let fwd = b.rot * Vec3::NEG_Z;
    let fwd_h = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
    let target = b.pos + Vec3::Y * 1.2;
    let want = target - fwd_h * 9.0 + Vec3::Y * 3.4;
    // Lissage + pas de caméra dans un mur.
    let k = 1.0 - (-time.delta_secs() * 7.0).exp();
    let cur = smooth.get_or_insert(want);
    *cur = cur.lerp(want, k);
    let dir = *cur - target;
    let dist = dir.length().max(0.01);
    let cam = match world.raycast(target, dir / dist, dist) {
        Some(hit) => target + dir / dist * (hit.point.distance(target) - 0.4).max(0.8),
        None => *cur,
    };
    let rot_inv = pt.rotation.inverse();
    let look = Transform::from_translation(cam).looking_at(target, Vec3::Y);
    h.translation = rot_inv * (cam - pt.translation);
    h.rotation = rot_inv * look.rotation;
}

/// Moteur : une boucle spatialisée dont la hauteur suit la vitesse.
#[derive(Component)]
pub struct EngineSound;

pub fn engine_sound(
    game: Res<GameState>,
    mirror: Res<Mirror>,
    bank: Res<SfxBank>,
    bodies: Query<&NetBody>,
    mut commands: Commands,
    mut engine: Query<(Entity, &mut Transform, Option<&mut AudioSink>), With<EngineSound>>,
    mut last: Local<Option<Vec3>>,
    time: Res<Time>,
) {
    let driving = game.driving != 0 && game.phase == Phase::Playing;
    if !driving {
        *last = None;
        for (e, _, _) in &engine {
            commands.entity(e).despawn();
        }
        return;
    }
    let Some(pos) = mirror
        .body_entity(game.driving)
        .and_then(|e| bodies.get(e).ok())
        .map(|b| b.pos)
    else {
        return;
    };
    let speed = last.map_or(0.0, |p| p.distance(pos) / time.delta_secs().max(1e-3));
    *last = Some(pos);
    match engine.single_mut() {
        Ok((_, mut t, sink)) => {
            t.translation = pos;
            if let Some(sink) = sink {
                sink.set_speed((0.7 + speed * 0.045).clamp(0.7, 2.2));
            }
        }
        Err(_) => {
            if let Some(handle) = bank.handle(SfxKind::Engine) {
                commands.spawn((
                    EngineSound,
                    AudioPlayer(handle),
                    PlaybackSettings {
                        volume: Volume::Linear(0.55),
                        spatial: true,
                        ..PlaybackSettings::LOOP
                    },
                    Transform::from_translation(pos),
                ));
            }
        }
    }
}
