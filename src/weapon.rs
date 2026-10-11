//! Armes : la masse (dans `sledge.rs`), le Glock 17 et la C4.

use bevy::audio::AudioSource;
use std::f32::consts::FRAC_PI_2;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::debris::Rng;
use crate::fx::{self, SfxKind};
use crate::game::{GameState, Weapon, GLOCK_MAG};
use crate::player::{Head, WorldCamera};
use crate::rubble::Chunks;
use crate::world::{self, VoxelAssets, VoxelWorld};

/// Portée du Glock.
const GUN_REACH: f32 = 60.0;
/// Rayon du trou de balle (visible : on perce au moins un voxel entier).
const GUN_BLAST: f32 = 0.34;
/// Durée de la traçante.
const TRACER_TIME: f32 = 0.05;
/// Durée de recharge.
const RELOAD_TIME: f32 = 1.1;
/// Portée de pose de la C4.
pub const C4_REACH: f32 = 6.0;
/// Rayon de l'explosion.
const C4_BLAST: f32 = 3.2;
/// Délai avant détonation une fois armée.
const C4_FUSE: f32 = 3.2;

/// Modèle du Glock 17 (visible dans la main).
#[derive(Component)]
pub struct GlockView;

/// C4 tenue en main.
#[derive(Component)]
pub struct C4View;

/// C4 posée dans le monde.
#[derive(Component)]
pub struct PlacedC4 {
    pub fuse: f32,
    pub armed: bool,
    pub last_beep: f32,
}

/// Curseur de recharge.
#[derive(Component)]
pub struct Reloading(pub f32);

/// Traçante de balle : une traînée qui va du canon au point d'impact.
#[derive(Component)]
pub struct Tracer {
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
}

/// Recul du Glock dans le modèle en main.
#[derive(Component)]
pub struct Recoil(pub f32);

/// Mesh et matériau de la C4 posée dans le monde.
#[derive(Resource)]
pub struct C4Assets {
    pub body: Handle<Mesh>,
    pub light: Handle<StandardMaterial>,
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    head: Query<Entity, With<Head>>,
) {
    let Ok(head) = head.single() else {
        return;
    };

    let gunmetal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.14, 0.15, 0.16),
        metallic: 0.6,
        perceptual_roughness: 0.35,
        ..default()
    });
    let poly = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.09, 0.10),
        perceptual_roughness: 0.7,
        ..default()
    });
    let c4_body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.80, 0.76, 0.30),
        perceptual_roughness: 0.8,
        ..default()
    });
    let c4_light = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.1, 0.1),
        emissive: Color::srgb(1.0, 0.1, 0.1).into(),
        perceptual_roughness: 0.4,
        ..default()
    });

    let slide = meshes.add(Cuboid::new(0.042, 0.048, 0.17));
    let grip = meshes.add(Cuboid::new(0.040, 0.115, 0.055));
    let trigger = meshes.add(Cuboid::new(0.02, 0.02, 0.03));
    let barrel = meshes.add(Cylinder::new(0.008, 0.045));
    let brick = meshes.add(Cuboid::new(0.17, 0.05, 0.10));
    let led = meshes.add(Cuboid::new(0.03, 0.03, 0.01));
    commands.insert_resource(C4Assets {
        body: brick.clone(),
        light: c4_light.clone(),
    });

    commands.entity(head).with_children(|parent| {
        // --- Glock 17 ---
        parent
            .spawn((
                GlockView,
                Reloading(0.0),
                Recoil(0.0),
                Transform::from_xyz(0.20, -0.22, -0.40),
                Visibility::Hidden,
                children![
                    (
                        Mesh3d(slide.clone()),
                        MeshMaterial3d(gunmetal.clone()),
                        Transform::from_xyz(0.0, 0.03, -0.03),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(grip.clone()),
                        MeshMaterial3d(poly.clone()),
                        Transform::from_xyz(0.0, -0.06, 0.035)
                            .with_rotation(Quat::from_rotation_x(-0.22)),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(trigger.clone()),
                        MeshMaterial3d(poly.clone()),
                        Transform::from_xyz(0.0, -0.02, 0.015),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(barrel),
                        MeshMaterial3d(gunmetal),
                        Transform::from_xyz(0.0, 0.03, -0.13)
                            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                ],
            ));

        // --- C4 ---
        parent
            .spawn((
                C4View,
                Transform::from_xyz(0.24, -0.26, -0.40),
                Visibility::Hidden,
                children![
                    (
                        Mesh3d(brick),
                        MeshMaterial3d(c4_body),
                        Transform::from_xyz(0.0, 0.0, 0.0),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                    (
                        Mesh3d(led),
                        MeshMaterial3d(c4_light),
                        Transform::from_xyz(0.0, 0.035, -0.045),
                        RenderLayers::layer(1),
                        NotShadowCaster,
                    ),
                ],
            ));
    });
}

/// Change d'arme (1/2/3 ou molette) et affiche le bon modèle.
pub fn select(
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut game: ResMut<GameState>,
    mut glock: Query<&mut Visibility, With<GlockView>>,
    mut c4: Query<&mut Visibility, (With<C4View>, Without<GlockView>)>,
    mut sledge: Query<&mut Visibility, (With<crate::sledge::Sledge>, Without<GlockView>, Without<C4View>)>,
    mut reloading: Query<&mut Reloading>,
) {
    if !game.in_game() {
        return;
    }

    let mut wanted = game.weapon;
    if keys.just_pressed(KeyCode::Digit1) {
        wanted = Weapon::Sledge;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        wanted = Weapon::Glock;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        wanted = Weapon::C4;
    }
    if wheel.delta.y.abs() > 0.5 {
        wanted = game.weapon.next();
    }
    if wanted != game.weapon {
        game.weapon = wanted;
        // On annule la recharge en cours.
        for mut r in &mut reloading {
            r.0 = 0.0;
        }
    }

    let show_glock = game.weapon == Weapon::Glock;
    let show_c4 = game.weapon == Weapon::C4;
    // La masse doit disparaître quand on tient autre chose.
    if let Ok(mut v) = sledge.single_mut() {
        *v = if game.weapon == Weapon::Sledge {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut v) = glock.single_mut() {
        *v = if show_glock {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut v) = c4.single_mut() {
        *v = if show_c4 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Tir du Glock 17.
#[allow(clippy::too_many_arguments)]
pub fn fire_glock(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions>,
    mut game: ResMut<GameState>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut world: ResMut<VoxelWorld>,
    mut chunks: ResMut<Chunks>,
    assets: Res<VoxelAssets>,
    mut rng: ResMut<Rng>,
    mut audio: ResMut<Assets<AudioSource>>,
    mut shake: ResMut<fx::Shake>,
    mut commands: Commands,
    // Un seul emprunt pour les deux composants portés par le modèle du Glock
    // (Bevy limite le nombre de paramètres système à 16).
    mut glock: Query<(&mut Recoil, &mut Reloading)>,
    mut placed: Query<(Entity, &mut PlacedC4, &Transform)>,
    net: Option<ResMut<crate::net::client::NetClient>>,
) {
    if !game.in_game() || game.weapon != Weapon::Glock {
        return;
    }

    let Ok((mut recoil, mut reload)) = glock.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    if reload.0 > 0.0 {
        reload.0 -= dt;
        if reload.0 <= 0.0 {
            game.ammo = GLOCK_MAG;
        }
        return;
    }

    let camera_pos = camera
        .single()
        .map(|c| c.translation())
        .unwrap_or(Vec3::ZERO);

    // Rechargement manuel.
    if keys.just_pressed(KeyCode::KeyR) && game.ammo < GLOCK_MAG {
        reload.0 = RELOAD_TIME;
        fx::play_at(&mut commands, &mut audio, SfxKind::Reload, 0.7, camera_pos);
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if cursor.grab_mode != CursorGrabMode::Locked {
        return;
    }

    if game.ammo == 0 {
        reload.0 = RELOAD_TIME;
        fx::play_at(&mut commands, &mut audio, SfxKind::Reload, 0.7, camera_pos);
        return;
    }
    game.ammo -= 1;

    let Ok(cam) = camera.single() else { return };
    let origin = cam.translation();
    let dir = cam.transform_vector(Vec3::NEG_Z);

    if let Some(mut net) = net {
        crate::net::client::send_hit(
            &mut net,
            crate::net::protocol::HitKind::Bullet,
            origin,
            dir,
            GUN_BLAST,
        );
    }

    // Éclat de bouche + recul.
    let muzzle = origin + dir * 0.35;
    fx::flash(&mut commands, muzzle, 0.35);
    recoil.0 = 1.0;
    fx::impact_sfx(&mut commands, &mut audio, &mut shake, SfxKind::Shot, origin);

    if let Some(hit) = world.raycast(origin, dir, GUN_REACH) {
        // Traçante du canon jusqu'à l'impact.
        commands.spawn((
            Tracer {
                from: muzzle,
                to: hit.point,
                t: 0.0,
            },
            Mesh3d(assets.tracer_cube.clone()),
            MeshMaterial3d(assets.tracer_material.clone()),
            Transform::from_translation(muzzle),
            NotShadowCaster,
        ));
        fx::flash(&mut commands, hit.point, 0.2);

        world::damage(
            &mut world,
            &mut chunks,
            &mut commands,
            &assets,
            &mut rng,
            hit.point,
            GUN_BLAST,
            6,
            hit.normal * 4.0,
        );
        for (_, mut c4, transform) in &mut placed {
            // Une balle assez proche peut amorcer la C4.
            if transform.translation.distance(hit.point) < 0.5 {
                c4.armed = true;
            }
        }
    }
}

/// Anime les traçantes et le recul du Glock.
pub fn gun_anim(
    time: Res<Time>,
    mut commands: Commands,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform), Without<GlockView>>,
    mut guns: Query<(&mut Recoil, &mut Transform), With<GlockView>>,
) {
    let dt = time.delta_secs();

    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.t += dt / TRACER_TIME;
        let t = tracer.t.min(1.0);
        let (from, to) = (tracer.from, tracer.to);
        transform.translation = from.lerp(to, t);
        let length = from.distance(to).max(0.3);
        let dir = (to - from).normalize_or_zero();
        // Le cube est étiré dans son axe Y, qu'on aligne sur la direction.
        transform.scale = Vec3::new(0.035, length * t.max(0.25), 0.035);
        // On aligne l'axe Y du cube sur la direction du tir.
        let yaw = -dir.x.atan2(-dir.z);
        let pitch = (dir.x * dir.x + dir.z * dir.z)
            .sqrt()
            .max(1e-5)
            .atan2(dir.y);
        transform.rotation = Quat::from_euler(EulerRot::ZYX, pitch - std::f32::consts::FRAC_PI_2, yaw, 0.0);
        if tracer.t >= 1.0 {
            commands.entity(entity).despawn();
        }
    }

    for (mut recoil, mut transform) in &mut guns {
        recoil.0 = (recoil.0 - dt * 7.0).max(0.0);
        let r = recoil.0;
        transform.translation = Vec3::new(0.20 + r * 0.01, -0.22 + r * 0.02, -0.40 + r * 0.07);
        transform.rotation = Quat::from_euler(EulerRot::XYZ, -r * 0.35, 0.0, r * 0.08);
    }
}

/// Pose et amorçage de la C4.
#[allow(clippy::too_many_arguments)]
pub fn c4_system(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions>,
    mut game: ResMut<GameState>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut world: ResMut<VoxelWorld>,
    mut chunks: ResMut<Chunks>,
    assets: Res<VoxelAssets>,
    c4_assets: Res<C4Assets>,
    mut rng: ResMut<Rng>,
    mut audio: ResMut<Assets<AudioSource>>,
    mut shake: ResMut<fx::Shake>,
    net: Option<ResMut<crate::net::client::NetClient>>,
    mut commands: Commands,
    mut placed: Query<(Entity, &mut PlacedC4, &Transform)>,
) {
    if !game.in_game() {
        return;
    }

    let dt = time.delta_secs();
    let mut boom: Option<Vec3> = None;

    for (entity, mut c4, transform) in &mut placed {
        if c4.armed {
            c4.fuse -= dt;
            c4.last_beep -= dt;
            if c4.last_beep <= 0.0 {
                // Le tic s'accélère, et il vient de la C4 posée.
                c4.last_beep = (c4.fuse / 6.0).max(0.12);
                fx::play_at(
                    &mut commands,
                    &mut audio,
                    SfxKind::Beep,
                    0.5,
                    transform.translation,
                );
            }
            if c4.fuse <= 0.0 {
                boom = Some(transform.translation);
                commands.entity(entity).despawn();
            }
        }
    }

    if let Some(at) = boom {
        detonate(at, &mut world, &mut chunks, &mut commands, &assets, &mut rng);
        fx::flash(&mut commands, at, 2.5);
        fx::impact_sfx(&mut commands, &mut audio, &mut shake, SfxKind::Boom, at);
        return;
    }

    if game.weapon != Weapon::C4 || cursor.grab_mode != CursorGrabMode::Locked {
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(cam) = camera.single() else { return };
    let origin = cam.translation();
    let dir = cam.transform_vector(Vec3::NEG_Z);

    // Déjà posée et à portée ? On arme la plus proche.
    let mut existing = None;
    let mut best = C4_REACH;
    for (entity, _, transform) in &placed {
        let distance = transform.translation.distance(origin);
        if distance < best {
            best = distance;
            existing = Some(entity);
        }
    }
    if let Some(entity) = existing {
        if let Ok((_, mut c4, transform)) = placed.get_mut(entity) {
            c4.armed = true;
            // Le bip part de la C4 elle-même : on l'entend d'autant plus
            // qu'on est loin.
            fx::play_at(
                &mut commands,
                &mut audio,
                SfxKind::Beep,
                0.5,
                transform.translation,
            );
        }
        return;
    }

    if game.c4 == 0 {
        return;
    }
    game.c4 -= 1;

    let hit = world
        .raycast(origin, dir, C4_REACH)
        .map(|h| h.point + h.normal * 0.08)
        .unwrap_or(origin + dir * 2.0);

    if let Some(mut net) = net {
        net.send_raw(crate::net::protocol::Packet::C4Placed { at: hit });
    }
    place_c4(&mut commands, hit, &c4_assets);
}

/// Fait apparaître une C4 posée (utilisé aussi par le client réseau).
pub fn place_c4(commands: &mut Commands, at: Vec3, assets: &C4Assets) -> Entity {
    commands
        .spawn((
            PlacedC4 {
                fuse: C4_FUSE,
                armed: false,
                last_beep: 0.0,
            },
            Mesh3d(assets.body.clone()),
            MeshMaterial3d(assets.light.clone()),
            Transform::from_translation(at)
                .with_scale(Vec3::splat(0.22))
                .with_rotation(Quat::from_rotation_y(0.6)),
            NotShadowCaster,
        ))
        .id()
}

/// Explosion : gros trou, gravats, débris.
pub fn detonate(
    at: Vec3,
    world: &mut VoxelWorld,
    chunks: &mut Chunks,
    commands: &mut Commands,
    assets: &VoxelAssets,
    rng: &mut Rng,
) {
    world::damage(
        world,
        chunks,
        commands,
        assets,
        rng,
        at,
        C4_BLAST,
        24,
        Vec3::Y * 6.0,
    );
}