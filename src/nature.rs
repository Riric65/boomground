//! Côté client : affichage de l'eau, de la pluie et des tornades, et
//! poussées reçues du serveur (vent, vague).

use bevy::prelude::*;
use std::collections::HashMap;

use crate::water::WaterMirror;
use crate::world::VoxelWorld;

/// Identifiant de « matériau » de l'eau (couleur de la palette).
pub const WATER_MAT: u8 = 23;

/// Météo reçue du serveur.
#[derive(Resource, Default)]
pub struct Weather {
    /// Pluie, 0..1.
    pub rain: f32,
}

/// Tornades reçues du serveur : `(id, position au sol, rayon)`.
#[derive(Resource, Default)]
pub struct Tornadoes(pub Vec<(u32, Vec3, f32)>);

/// Vitesse à ajouter au joueur (poussées du serveur), consommée par le
/// déplacement.
#[derive(Resource, Default)]
pub struct PlayerPush(pub Vec3);

/// Vue « voxels » de l'eau, pour réutiliser le maillage des voxels.
struct WaterView<'a> {
    water: &'a WaterMirror,
    world: &'a VoxelWorld,
}

impl crate::vmesh::Cells for WaterView<'_> {
    fn dims(&self) -> IVec3 {
        self.world.dims
    }
    fn origin(&self) -> Vec3 {
        self.world.origin
    }
    fn solid(&self, c: IVec3) -> bool {
        self.water.contains(c) && !self.world.is_solid(c)
    }
    fn mat(&self, _c: IVec3) -> u8 {
        WATER_MAT
    }
}

#[derive(Component)]
pub struct WaterMesh;

/// Maillages de l'eau, par morceau (translucides).
pub fn water_meshes(
    mut commands: Commands,
    mut water: ResMut<WaterMirror>,
    world: Res<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
    mut shown: Local<HashMap<IVec3, Entity>>,
) {
    if world.full_rebuild {
        for (_, e) in shown.drain() {
            commands.entity(e).despawn();
        }
    }
    if water.dirty.is_empty() {
        return;
    }
    let mat = material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 1.0, 1.0, 0.62),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.08,
                reflectance: 0.6,
                ..default()
            })
        })
        .clone();
    // Quelques morceaux par image (une vague en salit beaucoup).
    let keys: Vec<IVec3> = water.dirty.iter().copied().take(24).collect();
    for k in &keys {
        water.dirty.remove(k);
    }
    for key in keys {
        let view = WaterView { water: &water, world: &world };
        let mesh = crate::vmesh::build_pass(&view, key * crate::vmesh::CHUNK, 0);
        if let Some(e) = shown.remove(&key) {
            commands.entity(e).despawn();
        }
        if mesh.count_vertices() == 0 {
            continue;
        }
        let e = commands
            .spawn((
                WaterMesh,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::IDENTITY,
                bevy::light::NotShadowCaster,
            ))
            .id();
        shown.insert(key, e);
    }
}

#[derive(Component)]
pub struct RainDrop;

/// Pluie : gouttes étirées qui tombent autour de la caméra, ciel et
/// brouillard assombris.
#[allow(clippy::too_many_arguments)]
pub fn rain(
    time: Res<Time>,
    weather: Res<Weather>,
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<crate::player::WorldCamera>>,
    mut drops: Query<(Entity, &mut Transform), With<RainDrop>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut clear: ResMut<ClearColor>,
    mut fog: Query<&mut DistanceFog>,
    mut seed: Local<u32>,
) {
    let mut rand = || {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let rain = weather.rain;
    // Ciel gris quand il pleut.
    let sky = Color::srgb(0.55, 0.70, 0.90).mix(&Color::srgb(0.42, 0.45, 0.50), rain);
    if clear.0 != sky {
        clear.0 = sky;
    }
    for mut f in &mut fog {
        f.color = Color::srgb(0.62, 0.74, 0.90).mix(&Color::srgb(0.45, 0.48, 0.52), rain);
    }
    let Ok(cam) = camera.single() else { return };
    let eye = cam.translation();
    let wanted = (rain * 700.0) as usize;
    let count = drops.iter().len();
    if count > wanted {
        for (e, _) in drops.iter().take(count - wanted) {
            commands.entity(e).despawn();
        }
    } else if count < wanted {
        let (mesh, mat) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Cuboid::new(0.02, 0.55, 0.02)),
                    materials.add(StandardMaterial {
                        base_color: Color::srgba(0.75, 0.82, 0.92, 0.45),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        ..default()
                    }),
                )
            })
            .clone();
        for _ in 0..(wanted - count).min(60) {
            let p = eye + Vec3::new(rand() * 40.0 - 20.0, rand() * 22.0 - 4.0, rand() * 40.0 - 20.0);
            commands.spawn((
                RainDrop,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(p),
                bevy::light::NotShadowCaster,
            ));
        }
    }
    let dt = time.delta_secs();
    for (_, mut t) in &mut drops {
        t.translation.y -= 24.0 * dt;
        t.translation.x += 1.5 * dt;
        let rel = t.translation - eye;
        if rel.y < -6.0 || rel.x.abs() > 22.0 || rel.z.abs() > 22.0 {
            t.translation = eye + Vec3::new(rand() * 40.0 - 20.0, 14.0 + rand() * 6.0, rand() * 40.0 - 20.0);
        }
    }
}

/// Une bouffée de l'entonnoir d'une tornade.
#[derive(Component)]
pub struct Funnel {
    id: u32,
    /// Hauteur relative (0 = sol, 1 = sommet) et phase.
    h: f32,
    phase: f32,
}

/// Entonnoirs des tornades : bouffées de poussière en spirale qui tournent.
pub fn tornadoes(
    time: Res<Time>,
    list: Res<Tornadoes>,
    mut commands: Commands,
    mut puffs: Query<(Entity, &Funnel, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut known: Local<Vec<u32>>,
) {
    let (mesh, mat) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.42, 0.40, 0.38, 0.55),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 1.0,
                    ..default()
                }),
            )
        })
        .clone();
    // Nouvelles tornades : on crée leurs bouffées.
    for (id, _, _) in &list.0 {
        if !known.contains(id) {
            known.push(*id);
            for i in 0..70 {
                let h = i as f32 / 70.0;
                commands.spawn((
                    Funnel { id: *id, h, phase: i as f32 * 2.399 },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_scale(Vec3::ZERO),
                    bevy::light::NotShadowCaster,
                ));
            }
        }
    }
    known.retain(|id| list.0.iter().any(|(t, _, _)| t == id));
    let now = time.elapsed_secs();
    for (e, f, mut t) in &mut puffs {
        let Some((_, pos, radius)) = list.0.iter().find(|(id, _, _)| *id == f.id) else {
            commands.entity(e).despawn();
            continue;
        };
        // Entonnoir : étroit en bas, large en haut, qui tourne vite.
        let height = 34.0 * f.h;
        let r = radius * (0.25 + f.h * 1.4);
        let a = f.phase + now * (5.0 - f.h * 2.5);
        let target = *pos + Vec3::new(a.cos() * r, height, a.sin() * r);
        t.translation = t.translation.lerp(target, 0.35);
        t.scale = Vec3::splat(1.2 + f.h * 3.5);
    }
}
