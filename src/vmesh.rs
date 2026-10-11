//! Rendu du monde : **greedy meshing** par morceaux.
//!
//! La carte est découpée en morceaux de 16³ cellules, chacun rendu par un
//! maillage opaque fusionné (faces voisines identiques réunies en un quad)
//! et, s'il contient des vitres, un second maillage translucide.
//!
//! Quand on casse un voxel, seuls les morceaux touchés sont reconstruits ;
//! ceux qui n'ont pas pu l'être dans la frame restent en file d'attente (avant,
//! une grosse explosion laissait des voxels « fantômes » visibles).

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;

use crate::world::{mat, palette_linear, VoxelAssets, VoxelWorld, MESH_CHUNK, VOXEL};

/// Taille d'un morceau, en cellules.
pub const CHUNK: i32 = MESH_CHUNK;

/// Nombre maximal de morceaux reconstruits par frame.
const BUDGET: usize = 40;

/// Marque l'entité qui porte le maillage d'un morceau.
#[derive(Component)]
pub struct VoxelMesh;

/// Entités d'un morceau : opaque, vitres et lampes.
#[derive(Default, Clone, Copy)]
struct ChunkPair {
    opaque: Option<Entity>,
    glass: Option<Entity>,
    lamp: Option<Entity>,
}

/// Table des morceaux vivante : clef → entités.
#[derive(Resource, Default)]
pub struct ChunkEntities {
    map: HashMap<IVec3, ChunkPair>,
}

/// Passe de rendu d'un matériau : 0 opaque, 1 vitres, 2 lampes.
#[inline]
fn pass_of(material: u8) -> u8 {
    match material {
        mat::GLASS => 1,
        mat::LAMP => 2,
        _ => 0,
    }
}

#[inline]
fn opaque(world: &VoxelWorld, c: IVec3) -> bool {
    world.is_solid(c) && pass_of(world.material(c)) == 0
}

/// Cellule solide appartenant à la passe `pass` (≥ 1).
#[inline]
fn in_pass(world: &VoxelWorld, c: IVec3, pass: u8) -> bool {
    world.is_solid(c) && pass_of(world.material(c)) == pass
}

/// Construit le maillage d'un morceau par fusion gloutonne (`glass_pass` :
/// uniquement les vitres, sinon uniquement l'opaque).
pub fn build(world: &VoxelWorld, chunk_corner: IVec3, glass_pass: bool) -> Mesh {
    build_pass(world, chunk_corner, glass_pass as u8)
}

/// Comme [`build`], pour une passe : 0 opaque, 1 vitres, 2 lampes.
pub fn build_pass(world: &VoxelWorld, chunk_corner: IVec3, pass: u8) -> Mesh {
    let glass_pass = pass != 0;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let min = chunk_corner.max(IVec3::ZERO);
    let mut max = (chunk_corner + IVec3::splat(CHUNK)).min(world.dims);

    // Passe vitres : rien à faire si le morceau n'en contient aucune.
    if glass_pass {
        let any = (min.x..max.x).any(|x| {
            (min.y..max.y).any(|y| (min.z..max.z).any(|z| in_pass(world, IVec3::new(x, y, z), pass)))
        });
        if !any {
            max = min;
        }
    }

    for axis in 0..3 {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let (min_u, max_u) = (min[u], max[u]);
        let (min_v, max_v) = (min[v], max[v]);
        if min_u >= max_u || min_v >= max_v {
            continue;
        }

        // Le masque code : (matériau + 1) << 1 | (1 si la face regarde vers
        // +axis). 0 = pas de face.
        let width = (max_u - min_u) as usize;
        let height = (max_v - min_v) as usize;
        let mut mask = vec![0u16; width * height];

        // Le premier morceau construit aussi le plan du bord de la grille,
        // sinon les faces extérieures de la carte manquaient.
        let first = if min[axis] == 0 { -1 } else { min[axis] };
        for slice in first..max[axis] {
            // 1. Masque de ce plan de cellules.
            for j in min_v..max_v {
                for i in min_u..max_u {
                    let mut here = IVec3::ZERO;
                    here[axis] = slice;
                    here[u] = i;
                    here[v] = j;
                    let mut back = here;
                    back[axis] = slice + 1;

                    let value = if glass_pass {
                        if in_pass(world, here, pass) && !world.is_solid(back) {
                            ((world.material(here) as u16 + 1) << 1) | 1
                        } else if in_pass(world, back, pass) && !world.is_solid(here) {
                            (world.material(back) as u16 + 1) << 1
                        } else {
                            0
                        }
                    } else {
                        let a = opaque(world, here);
                        let b = opaque(world, back);
                        if a && !b {
                            ((world.material(here) as u16 + 1) << 1) | 1
                        } else if !a && b {
                            (world.material(back) as u16 + 1) << 1
                        } else {
                            0
                        }
                    };
                    mask[(j - min_v) as usize * width + (i - min_u) as usize] = value;
                }
            }

            // 2. Fusion des rectangles de même matériau et même sens.
            for j in 0..height {
                for i in 0..width {
                    let value = mask[j * width + i];
                    if value == 0 {
                        continue;
                    }
                    let mut w = 1;
                    while i + w < width && mask[j * width + i + w] == value {
                        w += 1;
                    }
                    let mut h = 1;
                    'outer: while j + h < height {
                        for k in 0..w {
                            if mask[(j + h) * width + i + k] != value {
                                break 'outer;
                            }
                        }
                        h += 1;
                    }

                    let material = ((value >> 1) - 1) as u8;
                    let mut color = palette_linear(material);
                    let forward = value & 1 == 1;

                    // Éclairage d'appoint par orientation : les faces du
                    // dessous sont plus sombres, le dessus un peu plus clair.
                    // Ça donne du relief même à l'ombre.
                    let shade = match (axis, forward) {
                        (1, true) => 1.0,
                        (1, false) => 0.55,
                        (0, _) => 0.86,
                        _ => 0.78,
                    };
                    for c in color.iter_mut().take(3) {
                        *c *= shade;
                    }

                    let plane = slice as f32 + 1.0;
                    let origin = world.origin;
                    let p0 = corner(&min, &origin, axis, u, v, i as i32, j as i32, plane);
                    let p1 = corner(&min, &origin, axis, u, v, (i + w) as i32, j as i32, plane);
                    let p2 = corner(&min, &origin, axis, u, v, (i + w) as i32, (j + h) as i32, plane);
                    let p3 = corner(&min, &origin, axis, u, v, i as i32, (j + h) as i32, plane);

                    let mut normal = [0.0f32; 3];
                    normal[axis] = if forward { 1.0 } else { -1.0 };

                    let quad = if forward {
                        [p0, p1, p2, p3]
                    } else {
                        [p3, p2, p1, p0]
                    };
                    let quad_uv = [
                        [0.0, 0.0],
                        [w as f32, 0.0],
                        [w as f32, h as f32],
                        [0.0, h as f32],
                    ];

                    let base = positions.len() as u32;
                    for k in 0..4 {
                        positions.push(quad[k]);
                        normals.push(normal);
                        uvs.push(quad_uv[k]);
                        colors.push(color);
                    }
                    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);

                    for b in 0..h {
                        for a in 0..w {
                            mask[(j + b) * width + i + a] = 0;
                        }
                    }
                }
            }
        }
    }

    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Coin d'une face, exprimé en unités de voxel le long de `axis`.
#[allow(clippy::too_many_arguments)]
fn corner(
    min: &IVec3,
    origin: &Vec3,
    axis: usize,
    u: usize,
    v: usize,
    i: i32,
    j: i32,
    plane: f32,
) -> [f32; 3] {
    let mut cell = [0.0f32; 3];
    cell[axis] = plane;
    cell[u] = (min[u] + i) as f32;
    cell[v] = (min[v] + j) as f32;
    [
        origin.x + cell[0] * VOXEL,
        origin.y + cell[1] * VOXEL,
        origin.z + cell[2] * VOXEL,
    ]
}

/// Remplace (ou crée, ou supprime) l'entité d'un maillage.
fn apply(
    slot: &mut Option<Entity>,
    mesh: Mesh,
    pass: u8,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    existing: &mut Query<&mut Mesh3d, With<VoxelMesh>>,
    assets: &VoxelAssets,
) {
    let empty = mesh.count_vertices() == 0;
    match *slot {
        Some(entity) if empty => {
            commands.entity(entity).despawn();
            *slot = None;
        }
        Some(entity) => {
            if let Ok(mut handle) = existing.get_mut(entity) {
                handle.0 = meshes.add(mesh);
            }
        }
        None if empty => {}
        None => {
            let material = match pass {
                1 => assets.glass_material.clone(),
                2 => assets.lamp_material.clone(),
                _ => assets.voxel_material.clone(),
            };
            let mut e = commands.spawn((
                VoxelMesh,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::IDENTITY,
            ));
            if pass != 0 {
                e.insert(NotShadowCaster);
            }
            *slot = Some(e.id());
        }
    }
}

fn rebuild_chunk(
    key: IVec3,
    world: &VoxelWorld,
    chunks: &mut ChunkEntities,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    existing: &mut Query<&mut Mesh3d, With<VoxelMesh>>,
    assets: &VoxelAssets,
) {
    let corner = key * CHUNK;
    let mut pair = chunks.map.get(&key).copied().unwrap_or_default();
    let opaque_mesh = build_pass(world, corner, 0);
    apply(&mut pair.opaque, opaque_mesh, 0, commands, meshes, existing, assets);
    let glass_mesh = build_pass(world, corner, 1);
    apply(&mut pair.glass, glass_mesh, 1, commands, meshes, existing, assets);
    let lamp_mesh = build_pass(world, corner, 2);
    apply(&mut pair.lamp, lamp_mesh, 2, commands, meshes, existing, assets);
    if pair.opaque.is_none() && pair.glass.is_none() && pair.lamp.is_none() {
        chunks.map.remove(&key);
    } else {
        chunks.map.insert(key, pair);
    }
}

/// Reconstruit les morceaux touchés (budget par frame), ou toute la carte
/// après une régénération.
pub fn refresh_chunks(
    mut world: ResMut<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut chunks: ResMut<ChunkEntities>,
    mut existing: Query<&mut Mesh3d, With<VoxelMesh>>,
    all: Query<Entity, With<VoxelMesh>>,
    assets: Res<VoxelAssets>,
    camera: Query<&GlobalTransform, With<crate::player::WorldCamera>>,
) {
    if world.full_rebuild {
        for entity in &all {
            commands.entity(entity).despawn();
        }
        chunks.map.clear();
        // Les morceaux sont remaillés au fil des frames, les plus proches de
        // la caméra d'abord (voir le budget ci-dessous).
        world.full_rebuild = false;
        let keys = world.allocated_chunks();
        let dirty = world.dirty_chunks_mut();
        dirty.clear();
        dirty.extend(keys);
    }

    if world.dirty_chunks_mut().is_empty() {
        return;
    }

    // Les morceaux les plus proches de la caméra d'abord.
    let eye = camera
        .single()
        .map(|c| c.translation())
        .unwrap_or(Vec3::ZERO);
    let origin = world.origin;
    let mut keys: Vec<IVec3> = world.dirty_chunks_mut().iter().copied().collect();
    let dist = |k: &IVec3| {
        (origin + (k.as_vec3() + 0.5) * CHUNK as f32 * VOXEL).distance_squared(eye)
    };
    // Gros chantier (chargement de la carte) : budget élargi.
    let budget = if keys.len() > 400 { BUDGET * 4 } else { BUDGET };
    if keys.len() > budget {
        keys.select_nth_unstable_by(budget, |a, b| dist(a).total_cmp(&dist(b)));
        keys.truncate(budget);
    }
    for key in &keys {
        world.dirty_chunks_mut().remove(key);
    }

    let n = world.dims / CHUNK;
    for key in keys {
        if key.cmplt(IVec3::ZERO).any() || key.cmpge(n).any() {
            continue;
        }
        rebuild_chunk(key, &world, &mut chunks, &mut commands, &mut meshes, &mut existing, &assets);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::VoxelWorld;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn slab_is_fused_into_six_quads_at_grid_corner() {
        let mut world = VoxelWorld::new();
        world.fill(IVec3::new(0, 0, 0), IVec3::new(16, 1, 16), 0);
        let mesh = build(&world, IVec3::ZERO, false);
        // Six faces, chacune fusionnée en un seul quad — y compris celles
        // posées sur le bord de la grille (-X, -Y, -Z), qui manquaient avant.
        assert_eq!(mesh.count_vertices(), 6 * 4);
    }

    #[test]
    fn top_face_is_on_the_upper_plane() {
        let mut world = VoxelWorld::new();
        world.fill(IVec3::new(2, 2, 2), IVec3::new(4, 3, 4), 0);
        let mesh = build(&world, IVec3::ZERO, false);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions manquantes");
        };
        assert!(positions
            .iter()
            .any(|p| (p[1] - 3.0 * crate::world::VOXEL).abs() < 1e-5));
    }

    #[test]
    fn single_cube_has_six_faces() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(2, 2, 2), true, 3);
        let mesh = build(&world, IVec3::ZERO, false);
        assert_eq!(mesh.count_vertices(), 6 * 4);
    }

    #[test]
    fn glass_is_in_its_own_mesh() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(2, 2, 2), true, mat::GLASS);
        assert_eq!(build(&world, IVec3::ZERO, false).count_vertices(), 0);
        assert_eq!(build(&world, IVec3::ZERO, true).count_vertices(), 6 * 4);
    }

    #[test]
    fn empty_chunk_has_no_face() {
        let world = VoxelWorld::new();
        let mesh = build(&world, IVec3::new(32, 16, 32), false);
        assert_eq!(mesh.count_vertices(), 0);
    }
}
