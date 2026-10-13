//! Joueur : caméra FPS, déplacements, gravité et collisions.
//!
//! Collisions : une seule boîte (0,6 × 1,7 × 0,6 m) déplacée axe par axe
//! contre les voxels **et** les blocs de gravats, avec des pas courts (pas
//! de traversée de mur), montée automatique des marches d'un voxel, et
//! accrochage au sol en descente. Un objet qui chevauche déjà le joueur ne
//! le téléporte plus : il ne peut que s'en dégager.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr};
use bevy::post_process::bloom::Bloom;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, Msaa, Tonemapping};

fn grading() -> ColorGrading {
    ColorGrading {
        global: ColorGradingGlobal {
            exposure: 0.35,
            post_saturation: 1.22,
            ..default()
        },
        ..default()
    }
}
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::rubble::{self, Chunk};
use crate::world::VoxelWorld;

/// Hauteur des yeux.
pub const EYE: f32 = 1.6;
/// Demi-largeur de la boîte de collision.
const HALF_W: f32 = 0.3;
/// Hauteur de la boîte de collision.
pub const HEIGHT: f32 = 1.7;
/// Hauteur de marche franchie sans sauter : 1,5 voxel (0,6 m) plus une petite
/// marge, pour monter les trottoirs et les escaliers d'un seul tenant.
pub const STEP: f32 = 0.62;
const WALK: f32 = 5.0;
const RUN: f32 = 8.5;
const FLY: f32 = 16.0;
const GRAVITY: f32 = 22.0;
const JUMP: f32 = 7.5;

/// Anticrénelage au démarrage. `BOOM_MSAA=0` le désactive (utile sur un
/// rendu logiciel, où le multi-échantillonnage coûte très cher).
fn msaa() -> bool {
    !std::env::var("BOOM_MSAA").is_ok_and(|v| v == "0")
}

#[derive(Component)]
pub struct Player;

/// Le « crâne » : porte la caméra, reçoit le pitch.
#[derive(Component)]
pub struct Head;

#[derive(Component)]
pub struct WorldCamera;

/// Caméra « vue en main » : ne rend que la couche 1 (les armes), par-dessus.
#[derive(Component)]
pub struct ViewModelCamera;

#[derive(Component, Default)]
pub struct Body {
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    /// Vol libre (bac à sable, touche V).
    pub fly: bool,
    /// Phase du balancement de marche (pour le « head bob »).
    pub bob: f32,
    /// Enfoncement après un atterrissage.
    pub land: f32,
    /// Lissage de la caméra après une marche montée.
    pub step: f32,
}

pub fn spawn_player(mut commands: Commands, world: Res<VoxelWorld>) {
    commands.spawn((
        Player,
        Body::default(),
        Transform::from_translation(world.spawn),
        Visibility::default(),
        children![(
            Head,
            Transform::from_xyz(0.0, EYE, 0.0),
            Visibility::default(),
            children![
                // Caméra du monde (celle qu'on utilise pour les rayons).
                (
                    WorldCamera,
                    // L'oreille du joueur : les sons 3D sont panés et
                    // atténués par rapport à cette caméra.
                    SpatialListener::default(),
                    Camera3d::default(),
                    Hdr,
                    if msaa() { Msaa::Sample4 } else { Msaa::Off },
                    Tonemapping::TonyMcMapface,
                    grading(),
                    Bloom {
                        intensity: 0.12,
                        ..Bloom::NATURAL
                    },
                    Projection::from(PerspectiveProjection {
                        fov: 75f32.to_radians(),
                        far: 600.0,
                        ..default()
                    }),
                    DistanceFog {
                        color: Color::srgb(0.62, 0.74, 0.90),
                        falloff: FogFalloff::Linear {
                            start: 90.0,
                            end: 260.0,
                        },
                        ..default()
                    },
                ),
                // Caméra « vue en main » : n'affiche que les armes (couche
                // 1), par-dessus l'image du monde **sans l'effacer** (avant,
                // elle repeignait l'écran avec la couleur du ciel) et avec
                // son propre tampon de profondeur, pour que l'arme ne
                // traverse jamais un mur.
                (
                    ViewModelCamera,
                    Camera3d::default(),
                    Hdr,
                    Msaa::Sample4,
                    Tonemapping::TonyMcMapface,
                    grading(),
                    Camera {
                        order: 1,
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    Projection::from(PerspectiveProjection {
                        fov: 68f32.to_radians(),
                        near: 0.01,
                        ..default()
                    }),
                    RenderLayers::layer(1),
                ),
            ],
        )],
    ));
}

/// Point d'apparition d'un joueur : **côté serveur**, on le tire au hasard
/// sur un trottoir de la ville (`salt` rend le tirage différent pour chaque
/// joueur, et reproductible pour un même couple *seed*, *sel*).
///
/// Le repli sur l'ancien point central sert aux tout premiers appels, quand
/// la carte n'est pas encore générée ou ne contient aucun trottoir libre.
pub fn spawn_point(world: &VoxelWorld, id: Option<u16>) -> Vec3 {
    let salt = id.unwrap_or(0) as u64 + 1;
    crate::map::random_sidewalk(world, salt).unwrap_or(world.spawn)
}

/// Point d'apparition tiré au hasard à chaque appel (serveur) : un trottoir
/// différent à chaque connexion et à chaque réapparition.
pub fn random_spawn(world: &VoxelWorld) -> Vec3 {
    let salt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    crate::map::random_sidewalk(world, salt).unwrap_or(world.spawn)
}

/// Remet le joueur au point d'apparition quand la carte est régénérée.
pub fn respawn_on_rebuild(
    world: Res<VoxelWorld>,
    net: Res<crate::net::client::NetClient>,
    mut seen: Local<u32>,
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
) {
    if *seen == world.generation {
        return;
    }
    *seen = world.generation;
    if let Ok((mut t, mut body)) = player.single_mut() {
        t.translation = spawn_point(&world, net.id);
        body.vel = Vec3::ZERO;
    }
}

/// Regard à la souris + capture du curseur.
pub fn look(
    settings: Res<crate::game::Settings>,
    game: Res<crate::game::GameState>,
    shop: Res<crate::shop::ShopState>,
    mouse: Res<AccumulatedMouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    touch: Res<crate::touch::TouchState>,
    mut cursor: Single<&mut CursorOptions>,
    mut player: Query<(&mut Transform, &mut Body), With<Player>>,
    mut head: Query<&mut Transform, (With<Head>, Without<Player>)>,
) {
    if shop.open || !game.alive() {
        return;
    }
    // Sur mobile : pas de curseur, le regard vient du glissement du doigt.
    if crate::touch::is_touch_platform() {
        let delta = touch.look;
        if delta == Vec2::ZERO {
            return;
        }
        let Ok((mut transform, mut body)) = player.single_mut() else {
            return;
        };
        let Ok(mut head_transform) = head.single_mut() else {
            return;
        };
        let invert = if settings.invert_y { -1.0 } else { 1.0 };
        body.yaw -= delta.x;
        body.pitch = (body.pitch - delta.y * invert).clamp(-FRAC_PI_2 + 0.02, FRAC_PI_2 - 0.02);
        transform.rotation = Quat::from_rotation_y(body.yaw);
        head_transform.rotation = Quat::from_rotation_x(body.pitch);
        return;
    }
    if buttons.just_pressed(MouseButton::Left) && cursor.grab_mode == CursorGrabMode::None {
        grab(&mut cursor, true);
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
    body.pitch = (body.pitch - delta.y * sensitivity * invert).clamp(-FRAC_PI_2 + 0.02, FRAC_PI_2 - 0.02);

    transform.rotation = Quat::from_rotation_y(body.yaw);
    head_transform.rotation = Quat::from_rotation_x(body.pitch);
}

/// Capture (ou libère) le curseur. Sous Linux (X11), `Locked` n'existe pas :
/// on enferme le curseur dans la fenêtre à la place.
pub fn grab(cursor: &mut CursorOptions, on: bool) {
    if on {
        cursor.grab_mode = if cfg!(target_os = "linux") {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::Locked
        };
        cursor.visible = false;
    } else {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Déplacements + gravité + collisions (voxels et gravats).
#[allow(clippy::too_many_arguments)]
pub fn move_player(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    world: Res<VoxelWorld>,
    game: Res<crate::game::GameState>,
    settings: Res<crate::game::Settings>,
    shop: Res<crate::shop::ShopState>,
    net: Res<crate::net::client::NetClient>,
    touch: Res<crate::touch::TouchState>,
    mut query: Query<(&mut Transform, &mut Body), With<Player>>,
    mut heads: Query<&mut Transform, (With<Head>, Without<Player>)>,
    mut blocks: Query<(&Transform, &mut Chunk), (Without<Player>, Without<Head>)>,
    water: Res<crate::water::WaterMirror>,
    mut push: ResMut<crate::nature::PlayerPush>,
) {
    let Ok((mut transform, mut body)) = query.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    if game.dead || game.loading {
        return;
    }

    // Vol libre : uniquement en bac à sable.
    if keys.just_pressed(KeyCode::KeyV) && game.sandbox() && game.fly_allowed && controls_open(&game, &shop) {
        body.fly = !body.fly;
        body.vel = Vec3::ZERO;
    }
    if !game.sandbox() {
        body.fly = false;
    }

    // Direction voulue dans le repère du joueur. Les touches physiques
    // WASD correspondent à ZQSD sur un clavier AZERTY.
    let forward = Vec3::new(-body.yaw.sin(), 0.0, -body.yaw.cos());
    let right = Vec3::new(body.yaw.cos(), 0.0, -body.yaw.sin());
    let mut wish = Vec3::ZERO;
    let controls = !shop.open && game.phase == crate::game::Phase::Playing && !game.typing && game.driving == 0;
    if controls {
        if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
            wish += forward;
        }
        if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
            wish -= forward;
        }
        if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
            wish += right;
        }
        if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
            wish -= right;
        }
        // Joystick tactile : x = droite, y vers le bas de l'écran (= reculer).
        let m = touch.stick;
        if m.length() > 0.08 {
            wish += right * m.x - forward * m.y;
        }
    }
    let running = keys.pressed(KeyCode::ShiftLeft);

    let mut pos = transform.translation;

    if body.fly {
        let mut v = wish.normalize_or_zero();
        if controls && keys.pressed(KeyCode::Space) {
            v += Vec3::Y;
        }
        if controls && keys.any_pressed([KeyCode::ControlLeft, KeyCode::KeyC]) {
            v -= Vec3::Y;
        }
        let speed = if running { FLY * 2.0 } else { FLY };
        body.vel = v * speed;
        // Le vol respecte les murs, le sol et les limites de la carte.
        let half = Vec3::new(HALF_W, HEIGHT * 0.5, HALF_W);
        let mut center = pos + Vec3::Y * HEIGHT * 0.5;
        for axis in [0, 2, 1] {
            let delta = body.vel[axis] * dt;
            if delta != 0.0 && world.sweep_with(&mut center, half, axis, delta, &[]) {
                body.vel[axis] = 0.0;
            }
        }
        let size = world.dims.as_vec3() * crate::world::VOXEL;
        let lo = world.origin + Vec3::new(HALF_W + 0.6, 0.0, HALF_W + 0.6);
        let hi = world.origin + Vec3::new(size.x - HALF_W - 0.6, 0.0, size.z - HALF_W - 0.6);
        center.x = center.x.clamp(lo.x, hi.x);
        center.z = center.z.clamp(lo.z, hi.z);
        center.y = center.y.clamp(crate::world::VOXEL + HEIGHT * 0.5, 70.0);
        pos = center - Vec3::Y * HEIGHT * 0.5;
        transform.translation = pos;
        body.grounded = false;
        return;
    }

    let speed = if running { RUN } else { WALK };
    let target = wish.normalize_or_zero() * speed;
    // Accélération : franche au sol, faible en l'air (les explosions
    // peuvent donc projeter le joueur).
    let accel = if body.grounded { 14.0 } else { 2.5 };
    let k = (accel * dt).min(1.0);
    body.vel.x += (target.x - body.vel.x) * k;
    body.vel.z += (target.z - body.vel.z) * k;

    let was_grounded = body.grounded;
    let fall_speed = -body.vel.y;
    // Poussées du serveur (tornade, vague).
    body.vel += push.0;
    push.0 = Vec3::ZERO;
    // Dans l'eau : on flotte presque, on est freiné, Espace fait remonter.
    let in_water = water.contains(world.world_to_cell(pos + Vec3::Y * 0.9));
    if in_water {
        body.vel.y -= GRAVITY * 0.12 * dt;
        let k = (-2.5 * dt).exp();
        body.vel *= k;
        if controls && (keys.pressed(KeyCode::Space) || touch.jump) {
            body.vel.y = (body.vel.y + 16.0 * dt).min(3.2);
        }
    } else {
        body.vel.y -= GRAVITY * dt;
    }
    let mut jumped = false;
    // Saut : barre d'espace au clavier, bouton tactile sur mobile.
    if body.grounded && controls && (keys.pressed(KeyCode::Space) || touch.jump) {
        body.vel.y = JUMP;
        body.grounded = false;
        jumped = true;
    }

    // Gravats proches : boîtes solides pour le joueur.
    let half = Vec3::new(HALF_W, HEIGHT * 0.5, HALF_W);
    let mut center = pos + Vec3::Y * HEIGHT * 0.5;
    let mut boxes: Vec<(Vec3, Vec3)> = Vec::new();
    for (t, chunk) in blocks.iter() {
        if t.translation.distance_squared(center) < 25.0 {
            boxes.push(rubble::aabb(t, &chunk));
        }
    }

    // Coincé dans un voxel (porte, régénération, gravats posés) : on prend
    // la place libre la plus proche, d'abord à côté et juste au-dessus (avant
    // on montait tout droit, jusqu'au toit s'il y avait un mur au-dessus).
    if world.overlaps(center - half, center + half) {
        let mut best: Option<(f32, Vec3)> = None;
        for dy in 0..=6 {
            for dx in -6..=6 {
                for dz in -6..=6 {
                    let off = Vec3::new(dx as f32, dy as f32, dz as f32) * 0.15;
                    let cost = off.length_squared() + off.y * off.y;
                    if best.is_some_and(|(b, _)| cost >= b) {
                        continue;
                    }
                    let c = center + off;
                    if !world.overlaps(c - half, c + half) {
                        best = Some((cost, c));
                    }
                }
            }
        }
        match best {
            Some((_, c)) => center = c,
            None => {
                for _ in 0..40 {
                    center.y += 0.1;
                    if !world.overlaps(center - half, center + half) {
                        break;
                    }
                }
            }
        }
    }

    // Horizontal, avec montée de marche.
    let mut pushed: Vec<(usize, f32)> = Vec::new();
    for axis in [0, 2] {
        let delta = body.vel[axis] * dt;
        if delta == 0.0 {
            continue;
        }
        let before = center;
        if !world.sweep_with(&mut center, half, axis, delta, &boxes) {
            continue;
        }
        let blocked_at = center;
        let mut stepped = false;
        if was_grounded || body.grounded {
            // On essaie la même avancée, une marche plus haut.
            // Un plafond bas peut écourter la montée : on garde ce qu'on a
            // pu gagner au lieu d'abandonner.
            let mut up = before;
            world.sweep_with(&mut up, half, 1, STEP, &boxes);
            if up.y - before.y > 0.05 {
                let mut fwd = up;
                world.sweep_with(&mut fwd, half, axis, delta, &boxes);
                let gained = (fwd[axis] - before[axis]).abs() - (blocked_at[axis] - before[axis]).abs();
                // Seuil minuscule : à haute fréquence d'image l'avancée par
                // image est faible et un seuil fixe bloquait les marches.
                if gained > 0.001 {
                    world.sweep_with(&mut fwd, half, 1, -STEP, &boxes);
                    // La caméra rattrape la marche en douceur.
                    body.step = (body.step + (fwd.y - before.y).max(0.0)).min(STEP);
                    center = fwd;
                    stepped = true;
                }
            }
        }
        if !stepped {
            pushed.push((axis, delta.signum()));
            body.vel[axis] = 0.0;
        }
    }

    // Vertical.
    body.grounded = false;
    if world.sweep_with(&mut center, half, 1, body.vel.y * dt, &boxes) {
        if body.vel.y <= 0.0 {
            body.grounded = true;
        }
        body.vel.y = 0.0;
    } else if was_grounded && !jumped && body.vel.y <= 0.0 {
        // Accrochage au sol : en descendant un escalier on reste collé aux
        // marches au lieu de « sautiller ».
        let mut probe = center;
        if world.sweep_with(&mut probe, half, 1, -STEP, &boxes) {
            center = probe;
            body.grounded = true;
            body.vel.y = 0.0;
        }
    }

    // Le joueur bouscule les petits gravats qu'il pousse.
    for (axis, sign) in pushed {
        let mut probe_min = center - half;
        let mut probe_max = center + half;
        probe_min[axis] += sign * 0.06;
        probe_max[axis] += sign * 0.06;
        for (t, mut chunk) in blocks.iter_mut() {
            let (bmin, bmax) = rubble::aabb(t, &chunk);
            let touching = bmin.cmplt(probe_max).all() && bmax.cmpgt(probe_min).all();
            let small = chunk.half.x * chunk.half.y * chunk.half.z * 8.0 < 0.2;
            if touching && small {
                chunk.vel[axis] += sign * 2.0;
                chunk.vel.y += 0.6;
                chunk.sleeping = false;
                chunk.rest = 0.0;
            }
        }
    }

    pos = center - Vec3::Y * HEIGHT * 0.5;

    // Filet de sécurité : hors de la carte, on réapparaît.
    if pos.y < -10.0 || !world.in_bounds_xz(pos) {
        pos = spawn_point(&world, net.id);
        body.vel = Vec3::ZERO;
    }

    transform.translation = pos;

    // Head bob + enfoncement à l'atterrissage : ça donne du poids.
    let horizontal = Vec2::new(body.vel.x, body.vel.z).length();
    if body.grounded {
        body.bob += horizontal * dt * 2.2;
        if !was_grounded && fall_speed > 4.0 {
            body.land = (fall_speed * 0.012).min(0.18);
        }
    }
    body.land = (body.land - dt * 0.9).max(0.0);
    body.step = (body.step - dt * 3.0).max(0.0);
    let intensity = if body.grounded && settings.head_bob {
        (horizontal / WALK).clamp(0.0, 1.4)
    } else {
        0.0
    };
    if let Ok(mut head) = heads.single_mut() {
        head.translation.y = EYE - body.land - body.step + (body.bob * 2.0).sin().abs() * 0.04 * intensity;
        head.translation.x = body.bob.sin() * 0.02 * intensity;
    }
}

/// Les touches de jeu sont actives (pas de boutique, pas de menu).
fn controls_open(game: &crate::game::GameState, shop: &crate::shop::ShopState) -> bool {
    !shop.open && game.phase == crate::game::Phase::Playing
}
