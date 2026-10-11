//! Rendu du monde : **greedy meshing** par morceaux.
//!
//! Au lieu d'une entité par voxel (des milliers), la carte est découpée en
//! morceaux de 8×8 cellules, chacune rendue par **un seul maillage fusionné**
//! dont les faces identiques voisines sont réunies en un quad. Résultat : une
//! trentaine d'entités et de draw calls pour toute la ville, et la RAM ne
//! contient qu'un bitset + un octet de matériau par cellule.
//!
//! Quand on casse un voxel, seuls les morceaux touchés sont reconstruits.

use std::collections::HashMap;

use bevy::mesh::Indices;
use bevy::prelude::*;

use crate::world::{VoxelWorld, PALETTE, VOXEL};

/// Taille d'un morceau, en cellules.
pub const CHUNK: i32 = 8;

/// Marque l'entité qui porte le maillage d'un morceau.
#[derive(Component)]
pub struct VoxelMesh;

/// Table des morceaux vivante : clef → entité.
#[derive(Resource, Default)]
pub struct ChunkEntities {
    map: HashMap<IVec3, Entity>,
}

impl ChunkEntities {
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// Construit le maillage d'un morceau par fusion gloutonne.
pub fn build(world: &VoxelWorld, chunk_corner: IVec3) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let min = chunk_corner.max(IVec3::ZERO);
    let max = (chunk_corner + IVec3::splat(CHUNK)).min(world.dims);

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
        let mut mask = vec![0u16; width * (max_v - min_v) as usize];

        for slice in min[axis]..max[axis] {
            // 1. Masque de ce plan de cellules.
            for j in min_v..max_v {
                for i in min_u..max_u {
                    let mut cell = [0i32; 3];
                    cell[axis] = slice;
                    cell[u] = i;
                    cell[v] = j;
                    let here = IVec3::from_array(cell);

                    let mut next = [0i32; 3];
                    next[axis] = slice + 1;
                    next[u] = i;
                    next[v] = j;
                    let back = IVec3::from_array(next);

                    let value = if world.is_solid(here) && !world.is_solid(back) {
                        ((world.material(here) as u16 + 1) << 1) | 1
                    } else if !world.is_solid(here) && world.is_solid(back) {
                        (world.material(back) as u16 + 1) << 1
                    } else {
                        0
                    };
                    mask[(j - min_v) as usize * width + (i - min_u) as usize] = value;
                }
            }

            // 2. Fusion des rectangles de même matériau et même sens.
            for j in 0..(max_v - min_v) as usize {
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
                    'outer: while j + h < (max_v - min_v) as usize {
                        for k in 0..w {
                            if mask[(j + h) * width + i + k] != value {
                                break 'outer;
                            }
                        }
                        h += 1;
                    }

                    let material = ((value >> 1) - 1) as usize % PALETTE.len();
                    let color = PALETTE[material];
                    let forward = value & 1 == 1;

                    // Le plan de la face : bord de la cellule si la face
                    // regarde vers -axis, bord opposé sinon.
                    let plane = slice as f32 + if forward { 1.0 } else { 0.0 };
                    // Les sommets sont en coordonnées monde : on ajoute
                    // l'origine de la grille, sinon la carte rendue est
                    // décalée par rapport à la physique.
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
                    let quad_uv = if forward {
                        [
                            [0.0, 0.0],
                            [w as f32, 0.0],
                            [w as f32, h as f32],
                            [0.0, h as f32],
                        ]
                    } else {
                        [
                            [0.0, h as f32],
                            [w as f32, h as f32],
                            [w as f32, 0.0],
                            [0.0, 0.0],
                        ]
                    };

                    let base = positions.len() as u32;
                    for k in 0..4 {
                        positions.push(quad[k]);
                        normals.push(normal);
                        uvs.push(quad_uv[k]);
                        colors.push([color.0, color.1, color.2, 1.0]);
                    }
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);

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

/// Reconstruit les morceaux touchés, au plus `budget` par frame.
pub fn refresh_chunks(
    mut world: ResMut<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut chunks: ResMut<ChunkEntities>,
    mut existing: Query<(Entity, &mut Mesh3d), With<VoxelMesh>>,
    assets: Res<crate::world::VoxelAssets>,
) {
    let Some(box_) = world.take_dirty_box() else {
        return;
    };
    // Un peu de marge : les faces voisines doivent être régénérées aussi.
    let min = box_.0 - IVec3::splat(1);
    let max = box_.1 + IVec3::splat(1);

    let lo = (min.max(IVec3::ZERO) / CHUNK).min(world.dims / CHUNK);
    let hi = (max.min(world.dims) / CHUNK).min(world.dims / CHUNK);

    // Up to 8 chunks per frame keeps the cost constant even on a big C4.
    let mut budget = 8;
    for cz in lo.z..=hi.z {
        for cy in lo.y..=hi.y {
            for cx in lo.x..=hi.x {
                if budget == 0 {
                    return;
                }
                budget -= 1;
                let corner = IVec3::new(cx * CHUNK, cy * CHUNK, cz * CHUNK);
                let mesh = build(&world, corner);
                let key = corner / CHUNK;
                match chunks.map.get(&key) {
                    Some(entity) => {
                        if mesh.count_vertices() == 0 {
                            commands.entity(*entity).despawn();
                            chunks.map.remove(&key);
                        } else if let Ok((_, mut handle)) = existing.get_mut(*entity) {
                            handle.0 = meshes.add(mesh);
                        }
                    }
                    None => {
                        if mesh.count_vertices() > 0 {
                            let entity = commands
                                .spawn((
                                    VoxelMesh,
                                    Mesh3d(meshes.add(mesh)),
                                    MeshMaterial3d(assets.voxel_material.clone()),
                                    Transform::IDENTITY,
                                ))
                                .id();
                            chunks.map.insert(key, entity);
                        }
                    }
                }
            }
        }
    }
}

/// `F5` : on jette tous les morceaux et on reconstruit la carte.
pub fn rebuild_all(
    mut commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    mut chunks: ResMut<ChunkEntities>,
    entities: Query<Entity, With<VoxelMesh>>,
    world: Res<VoxelWorld>,
    assets: Res<crate::world::VoxelAssets>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    chunks.clear();
    build_all(commands, meshes, chunks, world, assets);
}

/// Au démarrage : construit tous les morceaux non vides.
pub fn build_all(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunks: ResMut<ChunkEntities>,
    world: Res<VoxelWorld>,
    assets: Res<crate::world::VoxelAssets>,
) {
    chunks.clear();
    for cz in 0..world.dims.z / CHUNK {
        for cy in 0..world.dims.y / CHUNK {
            for cx in 0..world.dims.x / CHUNK {
                let corner = IVec3::new(cx * CHUNK, cy * CHUNK, cz * CHUNK);
                let mesh = build(&world, corner);
                let mesh_count = mesh.count_vertices();
                if mesh_count == 0 {
                    continue;
                }
                let entity = commands
                    .spawn((
                        VoxelMesh,
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(assets.voxel_material.clone()),
                        Transform::IDENTITY,
                    ))
                    .id();
                chunks.map.insert(corner / CHUNK, entity);
                if std::env::var("BOOM_VERBOSE").is_ok() {
                    info!("chunk {corner:?} -> {mesh_count} vertices");
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::VoxelWorld;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn slab_is_fused_into_three_quads() {
        let mut world = VoxelWorld::new();
        world.fill(IVec3::ZERO, IVec3::new(8, 1, 8), 0);
        let mesh = build(&world, IVec3::ZERO);
        // Le morceau remplit la grille : seules les 3 faces tournées vers
        // l'extérieur existent, chacune fusionnée en un seul quad.
        assert_eq!(mesh.count_vertices(), 3 * 4);
    }

    #[test]
    fn top_face_is_on_the_upper_plane() {
        let mut world = VoxelWorld::new();
        world.fill(IVec3::ZERO, IVec3::new(2, 1, 2), 0);
        let mesh = build(&world, IVec3::ZERO);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions manquantes");
        };
        // Le dessus doit être à y = 1 voxel, pas au milieu du voxel.
        assert!(positions
            .iter()
            .any(|p| (p[1] - crate::world::VOXEL).abs() < 1e-5));
    }

    #[test]
    fn single_cube_has_six_faces() {
        let mut world = VoxelWorld::new();
        world.set(IVec3::new(2, 2, 2), true, 3);
        let mesh = build(&world, IVec3::ZERO);
        assert_eq!(mesh.count_vertices(), 6 * 4);
    }

    #[test]
    fn empty_chunk_has_no_face() {
        let world = VoxelWorld::new();
        let mesh = build(&world, IVec3::new(16, 0, 16));
        assert_eq!(mesh.count_vertices(), 0);
    }
}

