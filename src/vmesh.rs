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
use bevy::mesh::{Indices, MeshVertexAttribute, VertexAttributeValues, VertexFormat};
use bevy::prelude::*;

use crate::world::{mat, palette_linear, VoxelAssets, VoxelWorld, MESH_CHUNK, VOXEL};

/// Taille d'un morceau, en cellules.
pub const CHUNK: i32 = MESH_CHUNK;

/// Nombre maximal de morceaux reconstruits par frame.
#[allow(dead_code)]
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
    /// Morceaux proches (≤ 70 m) pas encore maillés : sert à l'écran de
    /// chargement.
    pub pending_near: usize,
    /// Morceaux à mailler dans la distance d'affichage.
    pub pending_all: usize,
    /// Le premier balayage de la carte a eu lieu.
    pub scanned: bool,
}

/// Attributs compacts : 24 octets par sommet au lieu de 48. Les identifiants
/// et noms sont ceux des attributs standard, seul le format change.
const NORMAL_S8: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_Normal", 1, VertexFormat::Snorm8x4);
const UV_U16: MeshVertexAttribute = MeshVertexAttribute::new("Vertex_Uv", 2, VertexFormat::Unorm16x2);
const COLOR_U8: MeshVertexAttribute = MeshVertexAttribute::new("Vertex_Color", 5, VertexFormat::Unorm8x4);
/// Les UV sont stockés divisés par cette valeur (`uv_transform` du matériau
/// la compense).
pub const UV_SCALE: f32 = 64.0;

/// Luminosité selon l'occlusion ambiante d'un coin (0 = très enfoncé).
const AO_LEVELS: [f32; 4] = [0.50, 0.68, 0.84, 1.0];

/// Occlusion ambiante des quatre coins d'une face, dans l'ordre des coins
/// `(−u,−v) (+u,−v) (+u,+v) (−u,+v)`, empaquetée sur 8 bits. `air` est la
/// cellule vide qui touche la face.
fn ao_bits(world: &impl Cells, air: IVec3, du: IVec3, dv: IVec3) -> u32 {
    let occ = |c: IVec3| -> u32 {
        let m = world.solid(c);
        (m && pass_of(world.mat(c)) == 0) as u32
    };
    let mut bits = 0;
    for (k, (su, sv)) in [(-1, -1), (1, -1), (1, 1), (-1, 1)].into_iter().enumerate() {
        let s1 = occ(air + du * su);
        let s2 = occ(air + dv * sv);
        let c = occ(air + du * su + dv * sv);
        let level = if s1 == 1 && s2 == 1 { 0 } else { 3 - (s1 + s2 + c) };
        bits |= level << (2 * k);
    }
    bits
}

/// Ce que le maillage a besoin de savoir d'une grille de voxels : le
/// monde, ou la grille d'un corps rigide.
pub trait Cells {
    fn dims(&self) -> IVec3;
    fn origin(&self) -> Vec3;
    fn solid(&self, c: IVec3) -> bool;
    fn mat(&self, c: IVec3) -> u8;
}

impl Cells for VoxelWorld {
    fn dims(&self) -> IVec3 {
        self.dims
    }
    fn origin(&self) -> Vec3 {
        self.origin
    }
    #[inline]
    fn solid(&self, c: IVec3) -> bool {
        self.is_solid(c)
    }
    #[inline]
    fn mat(&self, c: IVec3) -> u8 {
        self.material(c)
    }
}

impl Cells for crate::sim::BodyGrid {
    fn dims(&self) -> IVec3 {
        self.dims
    }
    fn origin(&self) -> Vec3 {
        Vec3::ZERO
    }
    #[inline]
    fn solid(&self, c: IVec3) -> bool {
        self.is_solid(c)
    }
    #[inline]
    fn mat(&self, c: IVec3) -> u8 {
        self.material(c)
    }
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
fn opaque(world: &impl Cells, c: IVec3) -> bool {
    world.solid(c) && pass_of(world.mat(c)) == 0
}

/// Cellule solide appartenant à la passe `pass` (≥ 1).
#[inline]
fn in_pass(world: &impl Cells, c: IVec3, pass: u8) -> bool {
    world.solid(c) && pass_of(world.mat(c)) == pass
}

/// Construit le maillage d'un morceau par fusion gloutonne (`glass_pass` :
/// uniquement les vitres, sinon uniquement l'opaque).
pub fn build(world: &impl Cells, chunk_corner: IVec3, glass_pass: bool) -> Mesh {
    build_pass(world, chunk_corner, glass_pass as u8)
}

/// Comme [`build`], pour une passe : 0 opaque, 1 vitres, 2 lampes.
pub fn build_pass(world: &impl Cells, chunk_corner: IVec3, pass: u8) -> Mesh {
    let glass_pass = pass != 0;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[i8; 4]> = Vec::new();
    let mut uvs: Vec<[u16; 2]> = Vec::new();
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let min = chunk_corner.max(IVec3::ZERO);
    let mut max = (chunk_corner + IVec3::splat(CHUNK)).min(world.dims());

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
        let mut mask = vec![0u32; width * height];
        let mut du = IVec3::ZERO;
        du[u] = 1;
        let mut dv = IVec3::ZERO;
        dv[v] = 1;

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

                    // Le dessous du monde ne se voit jamais.
                    if axis == 1 && slice == -1 {
                        continue;
                    }
                    let value: u32 = if glass_pass {
                        if in_pass(world, here, pass) && !world.solid(back) {
                            ((world.mat(here) as u32 + 1) << 1) | 1 | (0xFF << 16)
                        } else if in_pass(world, back, pass) && !world.solid(here) {
                            ((world.mat(back) as u32 + 1) << 1) | (0xFF << 16)
                        } else {
                            0
                        }
                    } else {
                        let a = opaque(world, here);
                        let b = opaque(world, back);
                        if a && !b {
                            ((world.mat(here) as u32 + 1) << 1) | 1 | (ao_bits(world, back, du, dv) << 16)
                        } else if !a && b {
                            ((world.mat(back) as u32 + 1) << 1) | (ao_bits(world, here, du, dv) << 16)
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

                    let material = (((value & 0xFFFF) >> 1) - 1) as u8;
                    let mut color = palette_linear(material);
                    let forward = value & 1 == 1;
                    let ao = [
                        ((value >> 16) & 3) as usize,
                        ((value >> 18) & 3) as usize,
                        ((value >> 20) & 3) as usize,
                        ((value >> 22) & 3) as usize,
                    ];

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
                    let origin = world.origin();
                    let p0 = corner(&min, &origin, axis, u, v, i as i32, j as i32, plane);
                    let p1 = corner(&min, &origin, axis, u, v, (i + w) as i32, j as i32, plane);
                    let p2 = corner(&min, &origin, axis, u, v, (i + w) as i32, (j + h) as i32, plane);
                    let p3 = corner(&min, &origin, axis, u, v, i as i32, (j + h) as i32, plane);

                    let mut normal = [0i8; 4];
                    normal[axis] = if forward { 127 } else { -127 };

                    let (quad, ao) = if forward {
                        ([p0, p1, p2, p3], ao)
                    } else {
                        ([p3, p2, p1, p0], [ao[3], ao[2], ao[1], ao[0]])
                    };
                    let enc = |v: usize| ((v as f32 / UV_SCALE) * 65535.0).round() as u16;
                    let quad_uv = [[0, 0], [enc(w), 0], [enc(w), enc(h)], [0, enc(h)]];

                    let base = positions.len() as u32;
                    for k in 0..4 {
                        positions.push(quad[k]);
                        normals.push(normal);
                        uvs.push(quad_uv[k]);
                        let f = AO_LEVELS[ao[k]];
                        let q = |c: f32| (c * f * 255.0).round().clamp(0.0, 255.0) as u8;
                        colors.push([q(color[0]), q(color[1]), q(color[2]), 255]);
                    }
                    // Diagonale choisie selon l'occlusion : pas de couture.
                    if ao[0] + ao[2] >= ao[1] + ao[3] {
                        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                    } else {
                        indices.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
                    }

                    for b in 0..h {
                        for a in 0..w {
                            mask[(j + b) * width + i + a] = 0;
                        }
                    }
                }
            }
        }
    }

    // Pas de copie côté processeur : le maillage ne vit que sur la carte
    // graphique (économie de mémoire vive).
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(NORMAL_S8, VertexAttributeValues::Snorm8x4(normals));
    mesh.insert_attribute(UV_U16, VertexAttributeValues::Unorm16x2(uvs));
    mesh.insert_attribute(COLOR_U8, VertexAttributeValues::Unorm8x4(colors));
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

/// Reconstruit les morceaux touchés (budget par frame) et charge / décharge
/// les maillages selon la distance à la caméra : seuls les morceaux à portée
/// d'affichage existent en mémoire.
#[allow(clippy::too_many_arguments)]
pub fn refresh_chunks(
    time: Res<Time>,
    mut world: ResMut<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut chunks: ResMut<ChunkEntities>,
    mut existing: Query<&mut Mesh3d, With<VoxelMesh>>,
    all: Query<Entity, With<VoxelMesh>>,
    assets: Res<VoxelAssets>,
    settings: Res<crate::game::Settings>,
    game: Res<crate::game::GameState>,
    camera: Query<&GlobalTransform, With<crate::player::WorldCamera>>,
    mut scan_timer: Local<f32>,
) {
    if world.full_rebuild {
        for entity in &all {
            commands.entity(entity).despawn();
        }
        chunks.map.clear();
        world.full_rebuild = false;
        world.dirty_chunks_mut().clear();
        *scan_timer = 0.0;
        chunks.scanned = false;
    }

    let eye = camera
        .single()
        .map(|c| c.translation())
        .unwrap_or(Vec3::ZERO);
    let origin = world.origin;
    let center_of = |k: IVec3| origin + (k.as_vec3() + 0.5) * CHUNK as f32 * VOXEL;
    let radius = settings.view_distance + 24.0;
    let half_diag = CHUNK as f32 * VOXEL * 0.87;

    // Balayage : ce qui entre à portée est mis en file, ce qui sort est
    // déchargé.
    *scan_timer -= time.delta_secs();
    if *scan_timer <= 0.0 {
        *scan_timer = 0.4;
        let grid = world.chunk_grid();
        let mut unload = Vec::new();
        for x in 0..grid.x {
            for z in 0..grid.z {
                // Colonne entière trop loin : on saute vite.
                let flat = Vec3::new(
                    origin.x + (x as f32 + 0.5) * CHUNK as f32 * VOXEL - eye.x,
                    0.0,
                    origin.z + (z as f32 + 0.5) * CHUNK as f32 * VOXEL - eye.z,
                );
                let far = flat.length() > radius + 90.0 + half_diag;
                for y in 0..grid.y {
                    let k = IVec3::new(x, y, z);
                    if far {
                        if chunks.map.contains_key(&k) {
                            unload.push(k);
                        }
                        continue;
                    }
                    let d = center_of(k).distance(eye);
                    let present = chunks.map.contains_key(&k);
                    if d <= radius + half_diag {
                        if !present && world.chunk_allocated(k) {
                            world.dirty_chunks_mut().insert(k);
                        }
                    } else if present && d > radius + 90.0 + half_diag {
                        unload.push(k);
                    }
                }
            }
        }
        for k in unload {
            if let Some(pair) = chunks.map.remove(&k) {
                for e in [pair.opaque, pair.glass, pair.lamp].into_iter().flatten() {
                    commands.entity(e).despawn();
                }
            }
        }
        chunks.scanned = true;
    }

    // File d'attente : ce qui est trop loin est abandonné.
    let limit = radius + 90.0 + half_diag;
    world
        .dirty_chunks_mut()
        .retain(|k| center_of(*k).distance(eye) <= limit);
    chunks.pending_near = world
        .dirty_chunks_mut()
        .iter()
        .filter(|k| center_of(**k).distance(eye) <= 70.0)
        .count();
    chunks.pending_all = world.dirty_chunks_mut().len();
    if world.dirty_chunks_mut().is_empty() {
        return;
    }

    // Les morceaux les plus proches de la caméra d'abord, dans une
    // enveloppe de temps par image : pendant le chargement on y va franchement
    // (l'écran de chargement masque tout), ensuite 4 ms pour garder la
    // fluidité.
    let mut keys: Vec<IVec3> = world.dirty_chunks_mut().iter().copied().collect();
    let dist = |k: &IVec3| center_of(*k).distance_squared(eye);
    let take = if game.loading { 400 } else { 64 };
    if keys.len() > take {
        keys.select_nth_unstable_by(take, |a, b| dist(a).total_cmp(&dist(b)));
        keys.truncate(take);
    }
    keys.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
    let budget = std::time::Duration::from_micros(if game.loading { 45_000 } else { 4_000 });
    let started = std::time::Instant::now();
    let n = world.dims / CHUNK;
    for key in keys {
        // Au moins un morceau par image, puis on respecte le budget.
        world.dirty_chunks_mut().remove(&key);
        if !(key.cmplt(IVec3::ZERO).any() || key.cmpge(n).any()) {
            rebuild_chunk(key, &world, &mut chunks, &mut commands, &mut meshes, &mut existing, &assets);
        }
        if started.elapsed() > budget {
            break;
        }
    }
    chunks.pending_all = world.dirty_chunks_mut().len();
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
        // Cinq faces fusionnées en un seul quad chacune (le dessous du monde
        // ne se voit jamais), y compris celles posées sur le bord (-X, -Z).
        assert_eq!(mesh.count_vertices(), 5 * 4);
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

#[cfg(test)]
mod memory_probe {
    use super::*;

    fn rss_mb() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            })
            .unwrap_or(0)
            / 1024
    }

    #[test]
    #[ignore]
    fn world_and_mesh_memory() {
        let r0 = rss_mb();
        let mut w = VoxelWorld::new();
        w.rebuild(4242);
        let r1 = rss_mb();
        let keys = w.allocated_chunks();
        let mut verts = 0usize;
        let mut idx = 0usize;
        let mut meshes = Vec::new();
        for k in &keys {
            for pass in 0..3 {
                let m = build_pass(&w, *k * CHUNK, pass);
                verts += m.count_vertices();
                idx += m.indices().map_or(0, |i| i.len());
                if m.count_vertices() > 0 {
                    meshes.push(m);
                }
            }
        }
        let r2 = rss_mb();
        println!(
            "monde : +{} Mo ({} morceaux, {} voxels) · maillages : +{} Mo ({} sommets, {} indices, {} maillages)",
            r1 - r0,
            keys.len(),
            w.alive,
            r2 - r1,
            verts,
            idx,
            meshes.len()
        );
    }
}
