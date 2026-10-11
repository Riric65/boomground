//! Arsenal : marteaux, armes à feu, bombes lancées et posées.
//!
//! - [`setup_viewmodels`] construit le modèle « en main » de chaque arme
//!   (couche de rendu 1, dessinée par-dessus le monde) ;
//! - [`select`] change d'arme (1…9, 0, molette) ;
//! - [`act`] fait agir l'arme en main ;
//! - [`update_bombs`] gère grenades, dynamite, C4, méga-bombe et roquettes ;
//! - [`blast_effects`] applique le souffle (joueur projeté, gravats,
//!   réaction en chaîne des autres bombes).

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::debris::Rng;
use crate::fx::{self, FireAssets, SfxBank, SfxKind, Shake};
use crate::game::{GameState, Kind, WeaponId};
use crate::net::client::NetClient;
use crate::net::protocol::HitKind;
use crate::player::{Body, Head, Player, WorldCamera};
use crate::rubble::{self, Chunk, Rubble};
use crate::world::{self, Damage, VoxelAssets, VoxelWorld};

/// Moment de l'impact dans un coup de marteau (0..1).
const IMPACT: f32 = 0.55;
/// Durée de l'animation de lancer.
const THROW_TIME: f32 = 0.35;
/// Durée d'une traçante.
const TRACER_TIME: f32 = 0.07;
/// Vitesse d'une roquette.
const ROCKET_SPEED: f32 = 45.0;

/// Souffle d'une destruction : réveille les gravats, projette le joueur,
/// déclenche les bombes voisines.
#[derive(Message, Clone, Copy)]
pub struct Blast {
    pub at: Vec3,
    pub radius: f32,
    /// Force de projection (0 = simple coup).
    pub force: f32,
}

/// Tout ce qu'il faut pour casser le monde, regroupé (Bevy limite le
/// nombre de paramètres d'un système).
#[derive(SystemParam)]
pub struct Destroyer<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub world: ResMut<'w, VoxelWorld>,
    pub rubble: ResMut<'w, Rubble>,
    pub assets: Res<'w, VoxelAssets>,
    pub rng: ResMut<'w, Rng>,
    pub bank: Res<'w, SfxBank>,
    pub fire: Res<'w, FireAssets>,
    pub shake: ResMut<'w, Shake>,
    pub game: ResMut<'w, GameState>,
    pub blasts: MessageWriter<'w, Blast>,
    pub net: ResMut<'w, NetClient>,
}

impl Destroyer<'_, '_> {
    /// Coup de marteau ou balle : petit trou, gravats, argent.
    pub fn hit(&mut self, at: Vec3, radius: f32, normal: Vec3, debris: usize) -> Damage {
        let d = world::damage(
            &mut self.world,
            &mut self.rubble,
            &mut self.commands,
            &self.assets,
            &mut self.rng,
            at,
            radius,
            debris,
            normal * 3.0,
        );
        self.game.reward(d.value);
        if d.glass {
            fx::play_at(&mut self.commands, &self.bank, SfxKind::Glass, 0.8, at);
        }
        if d.count > 0 {
            self.blasts.write(Blast {
                at,
                radius,
                force: 0.0,
            });
        }
        d
    }

    /// Explosion : gros trou, boule de feu, souffle. `send` : informer le
    /// serveur (explosion locale en multijoueur).
    pub fn explode(&mut self, at: Vec3, radius: f32, send: bool) {
        let d = world::damage(
            &mut self.world,
            &mut self.rubble,
            &mut self.commands,
            &self.assets,
            &mut self.rng,
            at,
            radius,
            40,
            Vec3::Y * (2.0 + radius),
        );
        self.game.reward(d.value);
        self.effects(at, radius);
        if send {
            crate::net::client::send_hit(&mut self.net, HitKind::Explosion, at, Vec3::Y, radius);
        }
    }

    /// Partie visible et audible d'une explosion (aussi pour celles des
    /// autres joueurs).
    pub fn effects(&mut self, at: Vec3, radius: f32) {
        fx::flash(&mut self.commands, at, 1.0 + radius);
        fx::fireball(&mut self.commands, &self.fire, at, radius);
        let (kind, volume) = if radius > 5.0 {
            (SfxKind::BigBoom, 1.0)
        } else {
            (SfxKind::Boom, 0.9)
        };
        fx::play_at(&mut self.commands, &self.bank, kind, volume, at);
        self.blasts.write(Blast {
            at,
            radius,
            force: 5.0 + radius * 2.5,
        });
    }

    pub fn sound(&mut self, kind: SfxKind, volume: f32, at: Vec3) {
        fx::play_at(&mut self.commands, &self.bank, kind, volume, at);
    }
}

/// Modèle en main d'une arme.
#[derive(Component)]
pub struct ViewModel {
    pub id: WeaponId,
    /// Position de repos dans l'espace de la caméra.
    pub base: Transform,
    /// Bout du canon, dans l'espace de la caméra.
    pub muzzle: Vec3,
}

/// État de l'arme en main (une seule à la fois).
#[derive(Resource, Default)]
pub struct Hands {
    pub cooldown: f32,
    /// Progression d'un coup / d'un lancer (0..1).
    pub swing: Option<f32>,
    pub fired: bool,
    pub recoil: f32,
    /// Temps de recharge restant, et total.
    pub reload: f32,
    pub reload_total: f32,
    /// Animation de sortie d'arme (1 → 0).
    pub equip: f32,
    /// Marteau-piqueur en marche.
    pub drill: bool,
}

/// Bombe (ou roquette) dans le monde.
#[derive(Component)]
pub struct Bomb {
    pub id: WeaponId,
    pub vel: Vec3,
    /// Temps restant avant l'explosion (`None` : C4 en attente).
    pub fuse: Option<f32>,
    pub stuck: bool,
    pub beep: f32,
    /// Posée par ce joueur (sinon : simple image d'une bombe distante).
    pub local: bool,
}

/// Traçante de balle : une traînée qui va du canon au point d'impact.
#[derive(Component)]
pub struct Tracer {
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
}

/// Maillages et matériaux des bombes posées dans le monde.
#[derive(Resource)]
pub struct BombAssets {
    pub grenade: (Handle<Mesh>, Handle<StandardMaterial>),
    pub dynamite: (Handle<Mesh>, Handle<StandardMaterial>),
    pub c4: (Handle<Mesh>, Handle<StandardMaterial>),
    pub mega: (Handle<Mesh>, Handle<StandardMaterial>),
    pub rocket: (Handle<Mesh>, Handle<StandardMaterial>),
}

impl BombAssets {
    fn get(&self, id: WeaponId) -> &(Handle<Mesh>, Handle<StandardMaterial>) {
        match id {
            WeaponId::Grenade => &self.grenade,
            WeaponId::Dynamite => &self.dynamite,
            WeaponId::MegaBomb => &self.mega,
            WeaponId::Rocket => &self.rocket,
            _ => &self.c4,
        }
    }
}

/// Une pièce de modèle en main.
fn part(mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, transform: Transform) -> impl Bundle {
    (
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        transform,
        RenderLayers::layer(1),
        NotShadowCaster,
    )
}

fn mat(materials: &mut Assets<StandardMaterial>, rgb: (f32, f32, f32), metal: f32, rough: f32) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(rgb.0, rgb.1, rgb.2),
        metallic: metal,
        perceptual_roughness: rough,
        ..default()
    })
}

/// Construit les modèles en main de toutes les armes et les maillages des
/// bombes.
pub fn setup_viewmodels(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    head: Query<Entity, With<Head>>,
) {
    let Ok(head) = head.single() else {
        return;
    };

    let wood = mat(&mut materials, (0.45, 0.27, 0.13), 0.0, 0.75);
    let steel = mat(&mut materials, (0.55, 0.57, 0.62), 0.6, 0.4);
    let dark = mat(&mut materials, (0.12, 0.12, 0.13), 0.5, 0.45);
    let poly = mat(&mut materials, (0.08, 0.08, 0.09), 0.0, 0.7);
    let rubber = mat(&mut materials, (0.10, 0.10, 0.12), 0.0, 0.9);
    let yellow = mat(&mut materials, (0.92, 0.70, 0.10), 0.1, 0.6);
    let olive = mat(&mut materials, (0.30, 0.36, 0.20), 0.1, 0.7);
    let red = mat(&mut materials, (0.75, 0.10, 0.08), 0.0, 0.6);
    let tan = mat(&mut materials, (0.80, 0.76, 0.40), 0.0, 0.8);
    let led = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.1, 0.1),
        emissive: LinearRgba::rgb(6.0, 0.3, 0.3),
        ..default()
    });
    let lens = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.5, 0.9),
        emissive: LinearRgba::rgb(0.1, 0.3, 0.8),
        ..default()
    });

    let cyl = |meshes: &mut Assets<Mesh>, r: f32, h: f32| meshes.add(Cylinder::new(r, h));
    let cube = |meshes: &mut Assets<Mesh>, x: f32, y: f32, z: f32| meshes.add(Cuboid::new(x, y, z));
    // Cylindre couché le long de -Z (axe du canon).
    let along_z = Quat::from_rotation_x(FRAC_PI_2);

    // Marteaux : manche vertical (main en bas), tête au sommet, penchés
    // vers l'avant et vers le centre de l'écran.
    let melee_base = |x: f32, y: f32, z: f32| {
        Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_z(0.32) * Quat::from_rotation_x(-0.45))
    };

    commands.entity(head).with_children(|p| {
        // --- Marteau ---
        let base = melee_base(0.30, -0.40, -0.52);
        p.spawn((
            ViewModel { id: WeaponId::Hammer, base, muzzle: Vec3::ZERO },
            base,
            Visibility::Hidden,
        ))
        .with_children(|p| {
            let handle = cyl(&mut meshes, 0.016, 0.36);
            let head_m = cube(&mut meshes, 0.05, 0.05, 0.15);
            let claw = cube(&mut meshes, 0.04, 0.03, 0.08);
            p.spawn(part(&handle, &wood, Transform::from_xyz(0.0, 0.16, 0.0)));
            p.spawn(part(&cyl(&mut meshes, 0.02, 0.10), &rubber, Transform::from_xyz(0.0, 0.03, 0.0)));
            p.spawn(part(&head_m, &steel, Transform::from_xyz(0.0, 0.34, -0.02)));
            p.spawn(part(&claw, &steel, Transform::from_xyz(0.0, 0.35, 0.08).with_rotation(Quat::from_rotation_x(-0.5))));
        });

        // --- Masse ---
        let base = melee_base(0.32, -0.46, -0.48);
        p.spawn((ViewModel { id: WeaponId::Sledge, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                let handle = cyl(&mut meshes, 0.022, 0.62);
                p.spawn(part(&handle, &wood, Transform::from_xyz(0.0, 0.28, 0.0)));
                p.spawn(part(&cyl(&mut meshes, 0.027, 0.14), &rubber, Transform::from_xyz(0.0, 0.05, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.11, 0.11, 0.26), &steel, Transform::from_xyz(0.0, 0.60, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.12, 0.04, 0.12), &dark, Transform::from_xyz(0.0, 0.60, 0.0)));
            });

        // --- Masse de guerre ---
        let base = melee_base(0.34, -0.50, -0.52);
        p.spawn((ViewModel { id: WeaponId::WarHammer, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cyl(&mut meshes, 0.026, 0.74), &dark, Transform::from_xyz(0.0, 0.34, 0.0)));
                p.spawn(part(&cyl(&mut meshes, 0.032, 0.18), &red, Transform::from_xyz(0.0, 0.06, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.17, 0.17, 0.34), &steel, Transform::from_xyz(0.0, 0.74, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.19, 0.06, 0.36), &dark, Transform::from_xyz(0.0, 0.74, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.14, 0.05), &steel, Transform::from_xyz(0.0, 0.88, 0.0)));
            });

        // --- Marteau-piqueur : tenu à deux mains, pointe vers l'avant ---
        let base = Transform::from_xyz(0.22, -0.34, -0.55).with_rotation(Quat::from_rotation_y(0.08));
        p.spawn((ViewModel { id: WeaponId::Jackhammer, base, muzzle: Vec3::new(0.22, -0.34, -1.0) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cyl(&mut meshes, 0.065, 0.42), &yellow, Transform::from_xyz(0.0, 0.0, 0.0).with_rotation(along_z)));
                p.spawn(part(&cyl(&mut meshes, 0.035, 0.12), &dark, Transform::from_xyz(0.0, 0.0, -0.26).with_rotation(along_z)));
                p.spawn(part(&cyl(&mut meshes, 0.012, 0.22), &steel, Transform::from_xyz(0.0, 0.0, -0.42).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.34, 0.035, 0.035), &dark, Transform::from_xyz(0.0, 0.0, 0.24)));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.05, 0.08), &rubber, Transform::from_xyz(0.16, 0.0, 0.24)));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.05, 0.08), &rubber, Transform::from_xyz(-0.16, 0.0, 0.24)));
            });

        // --- Glock 17 ---
        let base = Transform::from_xyz(0.17, -0.17, -0.38);
        p.spawn((ViewModel { id: WeaponId::Glock, base, muzzle: Vec3::new(0.17, -0.14, -0.50) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cube(&mut meshes, 0.036, 0.040, 0.17), &dark, Transform::from_xyz(0.0, 0.03, -0.03)));
                p.spawn(part(&cube(&mut meshes, 0.034, 0.11, 0.05), &poly, Transform::from_xyz(0.0, -0.04, 0.03).with_rotation(Quat::from_rotation_x(-0.22))));
                p.spawn(part(&cube(&mut meshes, 0.012, 0.02, 0.03), &poly, Transform::from_xyz(0.0, -0.005, 0.0)));
            });

        // --- Fusil à pompe ---
        let base = Transform::from_xyz(0.19, -0.20, -0.42);
        p.spawn((ViewModel { id: WeaponId::Shotgun, base, muzzle: Vec3::new(0.19, -0.17, -1.0) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cyl(&mut meshes, 0.018, 0.62), &dark, Transform::from_xyz(0.0, 0.03, -0.34).with_rotation(along_z)));
                p.spawn(part(&cyl(&mut meshes, 0.026, 0.20), &wood, Transform::from_xyz(0.0, -0.005, -0.30).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.07, 0.20), &dark, Transform::from_xyz(0.0, 0.01, -0.02)));
                p.spawn(part(&cube(&mut meshes, 0.045, 0.08, 0.26), &wood, Transform::from_xyz(0.0, -0.03, 0.20).with_rotation(Quat::from_rotation_x(0.12))));
            });

        // --- Fusil d'assaut ---
        let base = Transform::from_xyz(0.18, -0.19, -0.40);
        p.spawn((ViewModel { id: WeaponId::Rifle, base, muzzle: Vec3::new(0.18, -0.16, -0.95) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cube(&mut meshes, 0.05, 0.075, 0.40), &dark, Transform::from_xyz(0.0, 0.0, -0.10)));
                p.spawn(part(&cyl(&mut meshes, 0.012, 0.32), &dark, Transform::from_xyz(0.0, 0.02, -0.44).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.04, 0.14, 0.06), &olive, Transform::from_xyz(0.0, -0.09, -0.12).with_rotation(Quat::from_rotation_x(0.25))));
                p.spawn(part(&cube(&mut meshes, 0.035, 0.09, 0.045), &poly, Transform::from_xyz(0.0, -0.07, 0.04).with_rotation(Quat::from_rotation_x(-0.25))));
                p.spawn(part(&cube(&mut meshes, 0.04, 0.07, 0.22), &olive, Transform::from_xyz(0.0, -0.01, 0.20)));
                p.spawn(part(&cube(&mut meshes, 0.03, 0.03, 0.12), &poly, Transform::from_xyz(0.0, 0.05, -0.08)));
            });

        // --- Sniper .50 ---
        let base = Transform::from_xyz(0.18, -0.20, -0.42);
        p.spawn((ViewModel { id: WeaponId::Sniper, base, muzzle: Vec3::new(0.18, -0.18, -1.25) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cube(&mut meshes, 0.05, 0.07, 0.42), &olive, Transform::from_xyz(0.0, 0.0, -0.05)));
                p.spawn(part(&cyl(&mut meshes, 0.015, 0.62), &dark, Transform::from_xyz(0.0, 0.02, -0.56).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.04, 0.06), &dark, Transform::from_xyz(0.0, 0.02, -0.88)));
                p.spawn(part(&cyl(&mut meshes, 0.024, 0.24), &dark, Transform::from_xyz(0.0, 0.075, -0.06).with_rotation(along_z)));
                p.spawn(part(&cyl(&mut meshes, 0.02, 0.005), &lens, Transform::from_xyz(0.0, 0.075, 0.062).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.04, 0.10, 0.20), &olive, Transform::from_xyz(0.0, -0.03, 0.24)));
            });

        // --- Lance-roquettes (sur l'épaule) ---
        let base = Transform::from_xyz(0.24, -0.16, -0.30);
        p.spawn((ViewModel { id: WeaponId::Rocket, base, muzzle: Vec3::new(0.24, -0.16, -1.0) }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cyl(&mut meshes, 0.065, 1.0), &olive, Transform::from_xyz(0.0, 0.0, -0.15).with_rotation(along_z)));
                p.spawn(part(&cyl(&mut meshes, 0.075, 0.08), &dark, Transform::from_xyz(0.0, 0.0, -0.62).with_rotation(along_z)));
                p.spawn(part(&cube(&mut meshes, 0.035, 0.12, 0.05), &poly, Transform::from_xyz(0.0, -0.10, -0.10)));
                p.spawn(part(&cube(&mut meshes, 0.03, 0.05, 0.10), &dark, Transform::from_xyz(-0.07, 0.04, -0.20)));
            });

        // --- Grenade ---
        let base = Transform::from_xyz(0.22, -0.24, -0.42);
        p.spawn((ViewModel { id: WeaponId::Grenade, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&meshes.add(Sphere::new(0.045)), &olive, Transform::default()));
                p.spawn(part(&cyl(&mut meshes, 0.015, 0.03), &steel, Transform::from_xyz(0.0, 0.05, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.012, 0.07, 0.02), &steel, Transform::from_xyz(0.025, 0.03, 0.0).with_rotation(Quat::from_rotation_z(-0.3))));
            });

        // --- Dynamite ---
        let base = Transform::from_xyz(0.22, -0.25, -0.42).with_rotation(Quat::from_rotation_z(0.4));
        p.spawn((ViewModel { id: WeaponId::Dynamite, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                let stick = cyl(&mut meshes, 0.018, 0.18);
                for x in [-0.019, 0.019] {
                    p.spawn(part(&stick, &red, Transform::from_xyz(x, 0.0, 0.0)));
                }
                p.spawn(part(&stick, &red, Transform::from_xyz(0.0, 0.0, -0.03)));
                p.spawn(part(&cyl(&mut meshes, 0.004, 0.06), &tan, Transform::from_xyz(0.0, 0.11, 0.0)));
            });

        // --- C4 ---
        let base = Transform::from_xyz(0.22, -0.25, -0.42).with_rotation(Quat::from_rotation_x(0.5));
        p.spawn((ViewModel { id: WeaponId::C4, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&cube(&mut meshes, 0.15, 0.05, 0.10), &tan, Transform::default()));
                p.spawn(part(&cube(&mut meshes, 0.05, 0.02, 0.03), &dark, Transform::from_xyz(0.03, 0.035, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.015, 0.012, 0.015), &led, Transform::from_xyz(-0.04, 0.032, -0.02)));
            });

        // --- Méga-bombe ---
        let base = Transform::from_xyz(0.25, -0.34, -0.60);
        p.spawn((ViewModel { id: WeaponId::MegaBomb, base, muzzle: Vec3::ZERO }, base, Visibility::Hidden))
            .with_children(|p| {
                p.spawn(part(&meshes.add(Sphere::new(0.14)), &dark, Transform::default()));
                p.spawn(part(&cyl(&mut meshes, 0.04, 0.05), &steel, Transform::from_xyz(0.0, 0.15, 0.0)));
                p.spawn(part(&cube(&mut meshes, 0.03, 0.03, 0.01), &led, Transform::from_xyz(0.0, 0.03, -0.14)));
            });
    });

    // Bombes posées / lancées dans le monde (couche 0, avec ombres).
    let led_world = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.2, 0.15),
        emissive: LinearRgba::rgb(2.0, 0.2, 0.1),
        ..default()
    });
    commands.insert_resource(BombAssets {
        grenade: (meshes.add(Sphere::new(0.07)), olive.clone()),
        dynamite: (meshes.add(Cylinder::new(0.04, 0.24)), red.clone()),
        c4: (meshes.add(Cuboid::new(0.24, 0.07, 0.15)), tan.clone()),
        mega: (meshes.add(Sphere::new(0.32)), led_world),
        rocket: (meshes.add(Capsule3d::new(0.06, 0.36)), olive.clone()),
    });
}

/// Change d'arme : 1…9 puis 0 parmi les armes possédées, ou molette.
pub fn select(
    keys: Res<ButtonInput<KeyCode>>,
    wheel: Res<AccumulatedMouseScroll>,
    shop: Res<crate::shop::ShopState>,
    mut game: ResMut<GameState>,
    mut hands: ResMut<Hands>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    if !game.in_game() || shop.open {
        return;
    }
    // Windows envoie la molette à la fenêtre survolée même si elle n'a pas
    // le focus : on l'ignore tant que le jeu n'est pas au premier plan.
    if window.single().is_ok_and(|w| !w.focused) {
        return;
    }
    let owned = game.owned();
    if owned.is_empty() {
        return;
    }
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    let mut wanted = game.weapon;
    for (i, key) in digits.iter().enumerate() {
        if keys.just_pressed(*key) {
            if let Some(&w) = owned.get(i) {
                wanted = w;
            }
        }
    }
    // Pavé numérique / touches au-delà de 10 armes : Q et E font défiler.
    let index = owned.iter().position(|&w| w == game.weapon).unwrap_or(0);
    let step = if wheel.delta.y < -0.5 || keys.just_pressed(KeyCode::KeyE) {
        1
    } else if wheel.delta.y > 0.5 || keys.just_pressed(KeyCode::KeyQ) {
        -1
    } else {
        0
    };
    if step != 0 {
        let n = owned.len() as i32;
        wanted = owned[((index as i32 + step).rem_euclid(n)) as usize];
    }
    if !owned.contains(&wanted) {
        wanted = owned[0];
    }
    if wanted != game.weapon {
        game.weapon = wanted;
        *hands = Hands {
            equip: 1.0,
            cooldown: 0.2,
            ..default()
        };
    }
}

/// Direction dispersée dans un cône.
fn spread_dir(dir: Vec3, right: Vec3, up: Vec3, spread: f32, rng: &mut Rng) -> Vec3 {
    if spread <= 0.0 {
        return dir;
    }
    let a = rng.f32() * PI * 2.0;
    let r = rng.f32().sqrt() * spread;
    (dir + (right * a.cos() + up * a.sin()) * r).normalize()
}

/// L'arme en main agit.
#[allow(clippy::too_many_arguments)]
pub fn act(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions>,
    shop: Res<crate::shop::ShopState>,
    mut hands: ResMut<Hands>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    bomb_assets: Res<BombAssets>,
    viewmodels: Query<&ViewModel>,
    mut bombs: Query<&mut Bomb>,
    mut d: Destroyer,
) {
    let dt = time.delta_secs();
    hands.cooldown = (hands.cooldown - dt).max(0.0);
    hands.equip = (hands.equip - dt * 4.0).max(0.0);
    if !d.game.in_game() || shop.open {
        hands.drill = false;
        return;
    }
    let Ok(cam) = camera.single() else { return };
    let origin = cam.translation();
    let dir = cam.forward().as_vec3();
    let right = cam.right().as_vec3();
    let up = cam.up().as_vec3();

    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    let id = d.game.weapon;
    let spec = id.spec();
    let pressed = grabbed && hands.equip < 0.5 && buttons.pressed(MouseButton::Left);
    let clicked = grabbed && hands.equip < 0.5 && buttons.just_pressed(MouseButton::Left);
    let trigger = if spec.auto { pressed } else { clicked };
    let online = d.net.is_online();

    match spec.kind {
        Kind::Melee if id == WeaponId::Jackhammer => {
            hands.drill = pressed;
            if pressed && hands.cooldown <= 0.0 {
                hands.cooldown = spec.cooldown;
                if let Some(hit) = d.world.raycast(origin, dir, spec.reach) {
                    d.hit(hit.point, spec.radius, hit.normal, 2);
                    d.sound(SfxKind::Drill, 0.6, hit.point);
                    d.shake.add(0.06);
                    if online {
                        crate::net::client::send_hit(&mut d.net, HitKind::Sledge, origin, dir, spec.radius);
                    }
                }
            }
        }
        Kind::Melee => {
            if let Some(t) = hands.swing {
                let next = t + dt / spec.cooldown;
                if !hands.fired && next >= IMPACT {
                    hands.fired = true;
                    if let Some(hit) = d.world.raycast(origin, dir, spec.reach) {
                        d.hit(hit.point, spec.radius, hit.normal, 10);
                        d.sound(SfxKind::Hit, 0.9, hit.point);
                        d.shake.add(0.18 + spec.radius * 0.12);
                        if online {
                            crate::net::client::send_hit(&mut d.net, HitKind::Sledge, origin, dir, spec.radius);
                        }
                    }
                }
                hands.swing = if next >= 1.0 { None } else { Some(next) };
            } else if trigger {
                hands.swing = Some(0.0);
                hands.fired = false;
                d.sound(SfxKind::Swing, 0.5, origin);
            }
        }
        Kind::Gun => {
            let sandbox = d.game.sandbox();
            // Recharge en cours.
            if hands.reload > 0.0 {
                hands.reload -= dt;
                if hands.reload <= 0.0 {
                    let slot = d.game.slot_mut(id);
                    let need = spec.mag - slot.mag;
                    let take = if sandbox { need } else { need.min(slot.reserve) };
                    slot.mag += take;
                    if !sandbox {
                        slot.reserve -= take;
                    }
                }
                return;
            }
            let slot = *d.game.slot(id);
            let can_reload = slot.mag < spec.mag && (sandbox || slot.reserve > 0);
            if keys.just_pressed(KeyCode::KeyR) && can_reload {
                hands.reload = spec.reload;
                hands.reload_total = spec.reload;
                d.sound(SfxKind::Reload, 0.7, origin);
                return;
            }
            if !trigger || hands.cooldown > 0.0 {
                return;
            }
            if slot.mag == 0 {
                if can_reload {
                    hands.reload = spec.reload;
                    hands.reload_total = spec.reload;
                    d.sound(SfxKind::Reload, 0.7, origin);
                } else if clicked {
                    d.sound(SfxKind::Empty, 0.6, origin);
                }
                return;
            }
            d.game.slot_mut(id).mag -= 1;
            hands.cooldown = spec.cooldown;
            hands.recoil = 1.0;

            let muzzle_local = viewmodels
                .iter()
                .find(|v| v.id == id)
                .map(|v| v.muzzle)
                .unwrap_or(Vec3::new(0.2, -0.15, -0.8));
            let muzzle = cam.transform_point(muzzle_local);
            fx::flash(&mut d.commands, muzzle, 0.35);
            let heavy = matches!(id, WeaponId::Sniper | WeaponId::Shotgun | WeaponId::Rocket);
            d.sound(if heavy { SfxKind::HeavyShot } else { SfxKind::Shot }, 0.8, origin);
            d.shake.add(if heavy { 0.3 } else { 0.1 });

            if id == WeaponId::Rocket {
                let (mesh, material) = bomb_assets.get(id).clone();
                d.commands.spawn((
                    Bomb {
                        id,
                        vel: dir * ROCKET_SPEED,
                        fuse: Some(5.0),
                        stuck: false,
                        beep: 0.0,
                        local: true,
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_translation(origin + dir * 0.9)
                        .with_rotation(Quat::from_rotation_arc(Vec3::Y, dir)),
                    PointLight {
                        intensity: 40_000.0,
                        color: Color::srgb(1.0, 0.6, 0.2),
                        range: 8.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                ));
                return;
            }

            for _ in 0..spec.pellets {
                let pdir = spread_dir(dir, right, up, spec.spread, &mut d.rng);
                let mut from = origin;
                let mut travelled = 0.0;
                let mut end = origin + pdir * spec.reach.min(120.0);
                // Le sniper traverse plusieurs épaisseurs.
                for _ in 0..=spec.pierce.min(8) {
                    let Some(hit) = d.world.raycast(from, pdir, spec.reach - travelled) else {
                        break;
                    };
                    d.hit(hit.point, spec.radius, hit.normal, 3);
                    end = hit.point;
                    travelled += hit.point.distance(from) + spec.radius;
                    from = hit.point + pdir * spec.radius;
                    if spec.pierce == 0 {
                        fx::flash(&mut d.commands, hit.point + hit.normal * 0.1, 0.15);
                        break;
                    }
                }
                if online {
                    crate::net::client::send_hit(&mut d.net, HitKind::Bullet, origin, pdir, spec.radius);
                }
                d.commands.spawn((
                    Tracer {
                        from: muzzle,
                        to: end,
                        t: 0.0,
                    },
                    Mesh3d(d.assets.tracer_cube.clone()),
                    MeshMaterial3d(d.assets.tracer_material.clone()),
                    Transform::from_translation(muzzle).with_scale(Vec3::ZERO),
                    NotShadowCaster,
                ));
            }
        }
        Kind::Thrown => {
            if let Some(t) = hands.swing {
                let next = t + dt / THROW_TIME;
                hands.swing = if next >= 1.0 { None } else { Some(next) };
                return;
            }
            if !trigger || hands.cooldown > 0.0 {
                return;
            }
            if !d.game.use_bomb(id) {
                if clicked {
                    d.sound(SfxKind::Empty, 0.6, origin);
                }
                return;
            }
            hands.cooldown = spec.cooldown;
            hands.swing = Some(0.0);
            d.sound(SfxKind::Swing, 0.4, origin);
            let (mesh, material) = bomb_assets.get(id).clone();
            let speed = if id == WeaponId::Grenade { 17.0 } else { 14.0 };
            d.commands.spawn((
                Bomb {
                    id,
                    vel: dir * speed + Vec3::Y * 2.5,
                    fuse: Some(spec.fuse),
                    stuck: false,
                    beep: 0.0,
                    local: true,
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(origin + dir * 0.5 + right * 0.15 - up * 0.1),
            ));
        }
        Kind::Placed => {
            if let Some(t) = hands.swing {
                let next = t + dt / THROW_TIME;
                hands.swing = if next >= 1.0 { None } else { Some(next) };
            }
            // Clic droit : la C4 saute (toutes celles qu'on a posées).
            if id == WeaponId::C4 && grabbed && buttons.just_pressed(MouseButton::Right) {
                let mut k = 0;
                for mut bomb in &mut bombs {
                    if bomb.local && bomb.id == WeaponId::C4 && bomb.fuse.is_none() {
                        bomb.fuse = Some(0.05 + k as f32 * 0.12);
                        k += 1;
                    }
                }
                if k > 0 {
                    d.sound(SfxKind::Beep, 0.6, origin);
                }
                return;
            }
            if !trigger || hands.cooldown > 0.0 || hands.swing.is_some() {
                return;
            }
            if !d.game.has_bomb(id) {
                if clicked {
                    d.sound(SfxKind::Empty, 0.6, origin);
                }
                return;
            }
            d.game.use_bomb(id);
            hands.cooldown = spec.cooldown;
            hands.swing = Some(0.0);

            let (at, rotation, stuck) = match d.world.raycast(origin, dir, spec.reach) {
                Some(hit) if hit.normal != Vec3::ZERO => {
                    let r = Quat::from_rotation_arc(Vec3::Y, hit.normal);
                    let lift = if id == WeaponId::MegaBomb { 0.3 } else { 0.04 };
                    (hit.point + hit.normal * lift, r, true)
                }
                _ => (origin + dir * 1.5, Quat::IDENTITY, false),
            };
            spawn_placed(&mut d.commands, &bomb_assets, id, at, rotation, stuck, true);
            d.sound(SfxKind::Beep, 0.5, at);
            if online {
                d.net.send_raw(crate::net::protocol::Packet::BombPlaced { at, weapon: id.tag() });
            }
        }
    }
}

/// Fait apparaître une bombe posée (aussi pour celles des autres joueurs).
pub fn spawn_placed(
    commands: &mut Commands,
    assets: &BombAssets,
    id: WeaponId,
    at: Vec3,
    rotation: Quat,
    stuck: bool,
    local: bool,
) {
    let (mesh, material) = assets.get(id).clone();
    let mut e = commands.spawn((
        Bomb {
            id,
            vel: Vec3::ZERO,
            fuse: if id == WeaponId::C4 { None } else { Some(id.spec().fuse) },
            stuck,
            beep: 0.0,
            local,
        },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(at).with_rotation(rotation),
    ));
    if id == WeaponId::MegaBomb {
        e.insert(PointLight {
            intensity: 0.0,
            color: Color::srgb(1.0, 0.1, 0.05),
            range: 6.0,
            shadow_maps_enabled: false,
            ..default()
        });
    }
}

/// Vol, rebonds, mèche, bips et explosion des bombes.
pub fn update_bombs(
    time: Res<Time>,
    mut bombs: Query<(Entity, &mut Bomb, &mut Transform, Option<&mut PointLight>)>,
    mut d: Destroyer,
) {
    let dt = time.delta_secs().min(0.05);
    let mut booms: Vec<(Vec3, f32, bool)> = Vec::new();

    for (entity, mut bomb, mut transform, light) in &mut bombs {
        let half = Vec3::splat(match bomb.id {
            WeaponId::MegaBomb => 0.3,
            WeaponId::Rocket => 0.08,
            _ => 0.07,
        });

        // Le support a été détruit : la bombe tombe.
        if bomb.stuck {
            let p = transform.translation;
            if !d.world.overlaps(p - half * 2.2, p + half * 2.2) {
                bomb.stuck = false;
            }
        }

        if !bomb.stuck {
            let gravity = if bomb.id == WeaponId::Rocket { 1.5 } else { 22.0 };
            bomb.vel.y -= gravity * dt;
            let mut pos = transform.translation;
            for axis in [1, 0, 2] {
                let delta = bomb.vel[axis] * dt;
                if delta == 0.0 {
                    continue;
                }
                if d.world.sweep(&mut pos, half, axis, delta) {
                    match bomb.id {
                        WeaponId::Rocket => bomb.fuse = Some(0.0),
                        WeaponId::Dynamite => {
                            bomb.vel = Vec3::ZERO;
                            bomb.stuck = true;
                        }
                        _ => {
                            bomb.vel[axis] *= -0.35;
                            if axis == 1 {
                                bomb.vel.x *= 0.7;
                                bomb.vel.z *= 0.7;
                                if bomb.vel.y.abs() < 1.0 {
                                    bomb.vel.y = 0.0;
                                }
                            }
                        }
                    }
                }
            }
            transform.translation = pos;
            if bomb.id == WeaponId::Rocket {
                if let Some(dir) = bomb.vel.try_normalize() {
                    transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
                }
            } else if bomb.vel.length_squared() > 1.0 {
                transform.rotate_local_x(dt * 9.0);
            }
            if pos.y < -10.0 {
                d.commands.entity(entity).despawn();
                continue;
            }
        }

        let Some(fuse) = bomb.fuse.as_mut() else {
            continue;
        };
        *fuse -= dt;
        let remaining = *fuse;

        // Bips de plus en plus rapides (C4, méga-bombe).
        if matches!(bomb.id, WeaponId::MegaBomb | WeaponId::Dynamite) && bomb.local {
            bomb.beep -= dt;
            if bomb.beep <= 0.0 {
                bomb.beep = (remaining / 8.0).clamp(0.08, 0.7);
                if bomb.id == WeaponId::MegaBomb {
                    d.sound(SfxKind::Beep, 0.8, transform.translation);
                }
            }
        }
        if let Some(mut light) = light {
            if bomb.id == WeaponId::MegaBomb {
                light.intensity = if bomb.beep > 0.05 { 0.0 } else { 60_000.0 };
            }
        }

        if remaining <= 0.0 {
            booms.push((transform.translation, bomb.id.spec().radius, bomb.local));
            d.commands.entity(entity).despawn();
        }
    }

    for (at, radius, local) in booms {
        if local {
            d.explode(at, radius, true);
        } else {
            d.effects(at, radius);
        }
    }
}

/// Souffle des explosions : gravats réveillés, joueur projeté, secousse,
/// réaction en chaîne.
pub fn blast_effects(
    mut blasts: MessageReader<Blast>,
    mut rubble_blocks: Query<(&Transform, &mut Chunk)>,
    mut player: Query<(&Transform, &mut Body), With<Player>>,
    mut bombs: Query<(&Transform, &mut Bomb)>,
    mut shake: ResMut<Shake>,
) {
    for blast in blasts.read() {
        rubble::wake_near(&mut rubble_blocks, blast.at, blast.radius, blast.force);
        if blast.force <= 0.0 {
            continue;
        }

        if let Ok((transform, mut body)) = player.single_mut() {
            let center = transform.translation + Vec3::Y * 0.9;
            let d = center - blast.at;
            let dist = d.length();
            let reach = blast.radius * 2.4;
            if dist < reach {
                // Projeté, mais pas en orbite : la poussée est plafonnée.
                let falloff = 1.0 - dist / reach;
                let push = (blast.force * falloff * 0.7).min(13.0);
                body.vel += d.normalize_or(Vec3::Y) * push + Vec3::Y * push * 0.35;
                // Plusieurs C4 en chaîne ne s'additionnent pas à l'infini.
                body.vel = body.vel.clamp_length_max(16.0);
                body.grounded = false;
            }
            shake.add((blast.radius * 0.6 / (dist * 0.3 + 1.0)).min(1.0));
        }

        // Les bombes prises dans l'explosion sautent à leur tour.
        for (transform, mut bomb) in &mut bombs {
            if bomb.local && transform.translation.distance(blast.at) < blast.radius * 1.1 {
                let soon = 0.12;
                bomb.fuse = Some(bomb.fuse.map_or(soon, |f| f.min(soon)));
            }
        }
    }
}

/// Supprime bombes et traçantes quand la carte est régénérée.
pub fn clear_on_rebuild(
    world: Res<VoxelWorld>,
    mut seen: Local<u32>,
    mut commands: Commands,
    bombs: Query<Entity, Or<(With<Bomb>, With<Tracer>)>>,
) {
    if *seen == world.generation {
        return;
    }
    *seen = world.generation;
    for e in &bombs {
        commands.entity(e).despawn();
    }
}

/// Anime les traçantes.
pub fn update_tracers(
    time: Res<Time>,
    mut commands: Commands,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.t += dt / TRACER_TIME;
        let (from, to) = (tracer.from, tracer.to);
        let length = from.distance(to);
        let Some(dir) = (to - from).try_normalize() else {
            commands.entity(entity).despawn();
            continue;
        };
        // Un trait lumineux de quelques mètres qui file vers l'impact.
        let head = tracer.t.min(1.0);
        let tail = (tracer.t - 0.45).clamp(0.0, 1.0);
        let a = from.lerp(to, tail);
        let b = from.lerp(to, head);
        let seg = a.distance(b).max(0.01);
        transform.translation = (a + b) * 0.5;
        transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
        transform.scale = Vec3::new(0.025, seg.min(length), 0.025);
        if tracer.t >= 1.45 {
            commands.entity(entity).despawn();
        }
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Angle du coup de marteau : armé en arrière, frappe, retour.
fn swing_angle(t: f32) -> f32 {
    const BACK: f32 = 0.85;
    const STRIKE: f32 = -1.25;
    if t < 0.3 {
        lerp(0.0, BACK, ease(t / 0.3))
    } else if t < IMPACT {
        lerp(BACK, STRIKE, ease((t - 0.3) / (IMPACT - 0.3)))
    } else {
        lerp(STRIKE, 0.0, ease((t - IMPACT) / (1.0 - IMPACT)))
    }
}

/// Positionne le modèle en main : coup, recul, recharge, lancer, marche.
pub fn animate_hands(
    time: Res<Time>,
    game: Res<GameState>,
    shop: Res<crate::shop::ShopState>,
    mut hands: ResMut<Hands>,
    body: Query<&Body, With<Player>>,
    mut models: Query<(&ViewModel, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    hands.recoil = (hands.recoil - dt * 6.0).max(0.0);
    let (bob, speed) = body
        .single()
        .map(|b| (b.bob, if b.grounded { Vec2::new(b.vel.x, b.vel.z).length() } else { 0.0 }))
        .unwrap_or((0.0, 0.0));
    let sway = (speed / 5.0).min(1.4);

    for (model, mut transform, mut visibility) in &mut models {
        let shown = game.in_game() && !shop.open && model.id == game.weapon;
        let wanted = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !shown {
            continue;
        }

        let mut t = model.base;
        // Balancement de marche.
        t.translation += Vec3::new((bob).sin() * 0.012, -(bob).sin().abs() * 0.010, 0.0) * sway;
        // Sortie d'arme : elle monte depuis le bas de l'écran.
        t.translation.y -= hands.equip * 0.3;
        t.rotation *= Quat::from_rotation_x(-hands.equip * 0.6);

        match model.id.kind() {
            Kind::Melee if model.id == WeaponId::Jackhammer => {
                if hands.drill {
                    let e = time.elapsed_secs();
                    t.translation += Vec3::new((e * 83.0).sin(), (e * 97.0).cos(), (e * 71.0).sin()) * 0.006;
                }
            }
            Kind::Melee => {
                let s = hands.swing.unwrap_or(0.0);
                let angle = swing_angle(s);
                t.rotation *= Quat::from_rotation_x(angle);
                // La main avance pendant la frappe.
                t.translation.z -= (-angle).max(0.0) * 0.08;
                t.translation.x -= (-angle).max(0.0) * 0.05;
            }
            Kind::Gun => {
                let r = hands.recoil;
                t.translation += Vec3::new(0.0, r * 0.015, r * 0.06);
                t.rotation *= Quat::from_rotation_x(r * 0.22);
                if hands.reload > 0.0 && hands.reload_total > 0.0 {
                    let p = 1.0 - hands.reload / hands.reload_total;
                    let s = (p * PI).sin();
                    t.translation.y -= s * 0.10;
                    t.rotation *= Quat::from_rotation_x(-s * 0.5) * Quat::from_rotation_z(s * 0.5);
                }
            }
            Kind::Thrown | Kind::Placed => {
                if let Some(s) = hands.swing {
                    // Armé en arrière, puis bras tendu vers l'avant.
                    let back = (s / 0.35).min(1.0);
                    let fwd = ((s - 0.35) / 0.3).clamp(0.0, 1.0);
                    let ret = ((s - 0.7) / 0.3).clamp(0.0, 1.0);
                    t.translation += Vec3::new(0.0, back * 0.06 - fwd * 0.08, back * 0.10 - fwd * 0.30) * (1.0 - ret);
                    // La bombe vient de partir : la main revient vide puis
                    // la suivante réapparaît.
                    if fwd > 0.5 && ret < 0.9 {
                        t.translation.y -= 0.5;
                    }
                }
            }
        }
        *transform = t;
    }
}
