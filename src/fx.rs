//! Effets : sons synthétisés, secousse de caméra, flashes d'explosion.
//!
//! Aucun fichier audio n'est embarqué : les échantillons sont générés une
//! seule fois au démarrage (bruit filtré, sinus, rampe de fréquence),
//! encodés en WAV 16 bits mono, puis rejoués. L'entité son se supprime
//! toute seule à la fin de la lecture (avant, chaque son laissait une
//! entité derrière lui).

use std::collections::HashMap;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

/// Fréquence d'échantillonnage de synthèse.
const RATE: u32 = 44_100;

// ---------------------------------------------------------------------------
// Son
// ---------------------------------------------------------------------------

/// Encode des échantillons en WAV 16 bits PCM.
fn wav(samples: &[f32]) -> AudioSource {
    let data_bytes = samples.len() * 2;
    let mut bytes: Vec<u8> = Vec::with_capacity(44 + data_bytes);

    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes()); // octets/s
    bytes.extend_from_slice(&2u16.to_le_bytes()); // alignement
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits par échantillon
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        bytes.extend_from_slice(&((clamped * i16::MAX as f32) as i16).to_le_bytes());
    }

    AudioSource { bytes: bytes.into() }
}

/// Bruit pseudo-aléatoire déterministe (LCG, pas de dépendance `rand`).
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 8) & 0xFFFF) as f32 / 32_768.0 - 1.0
    }
}

/// Génère des échantillons à partir d'une fonction temps → amplitude.
fn render(dur: f32, mut f: impl FnMut(f32) -> f32) -> AudioSource {
    let n = (RATE as f32 * dur) as usize;
    let data: Vec<f32> = (0..n).map(|i| f(i as f32 / RATE as f32)).collect();
    wav(&data)
}

/// Impulsion de bruit filtrée et décroissante : les impacts.
fn noise_burst(dur: f32, decay: f32, seed: u32, lowpass: f32) -> AudioSource {
    let mut n = Noise(seed);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * lowpass;
        prev * (-decay * t).exp()
    })
}

/// Balayage de bruit : le « sifflement » d'une masse qui tourne.
fn whoosh(dur: f32) -> AudioSource {
    let mut n = Noise(7);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * (0.05 + 0.6 * t / dur);
        prev * (std::f32::consts::PI * t / dur).sin().powf(1.5) * 0.6
    })
}

/// Détonation : grondement grave + bruit long.
fn explosion(dur: f32, pitch: f32) -> AudioSource {
    let mut n = Noise(99);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * 0.02 * pitch;
        let rumble = (2.0 * std::f32::consts::PI * (55.0 - 25.0 * t / dur) * pitch * t).sin();
        (prev * 1.6 + rumble * 0.35) * (-2.2 * t / pitch).exp() * 0.9
    })
}

/// Coup de feu : claquement + petit corps.
fn gunshot(dur: f32, body: f32) -> AudioSource {
    let mut n = Noise(1234);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * 0.5;
        let crack = (-90.0 * t).exp();
        let snap = (2.0 * std::f32::consts::PI * body * t).sin() * (-25.0 * t).exp();
        (prev * crack + snap * 0.3) * 0.85
    })
}

/// Bip carré : le tic des bombes.
fn beep(freq: f32, dur: f32) -> AudioSource {
    render(dur, |t| {
        let square = (2.0 * std::f32::consts::PI * freq * t).sin().signum();
        square * 0.22 * (-8.0 * t).exp()
    })
}

/// Bris de verre : bruit aigu + tintements.
fn glass(dur: f32) -> AudioSource {
    let mut n = Noise(4242);
    render(dur, |t| {
        let raw = n.next();
        let ting = (2.0 * std::f32::consts::PI * 3_100.0 * t).sin() * 0.3
            + (2.0 * std::f32::consts::PI * 4_700.0 * t).sin() * 0.2;
        (raw * 0.5 + ting) * (-14.0 * t).exp() * 0.6
    })
}

/// Les sons du jeu.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SfxKind {
    Swing,
    Hit,
    Shot,
    HeavyShot,
    Boom,
    BigBoom,
    Beep,
    Reload,
    Empty,
    Buy,
    Glass,
    Drill,
}

const ALL_SFX: [SfxKind; 12] = [
    SfxKind::Swing,
    SfxKind::Hit,
    SfxKind::Shot,
    SfxKind::HeavyShot,
    SfxKind::Boom,
    SfxKind::BigBoom,
    SfxKind::Beep,
    SfxKind::Reload,
    SfxKind::Empty,
    SfxKind::Buy,
    SfxKind::Glass,
    SfxKind::Drill,
];

/// Son synthétisé correspondant à un effet.
fn source_for(kind: SfxKind) -> AudioSource {
    match kind {
        SfxKind::Swing => whoosh(0.28),
        SfxKind::Hit => noise_burst(0.30, 18.0, 3, 0.25),
        SfxKind::Shot => gunshot(0.22, 140.0),
        SfxKind::HeavyShot => gunshot(0.45, 70.0),
        SfxKind::Boom => explosion(1.5, 1.0),
        SfxKind::BigBoom => explosion(3.0, 0.6),
        SfxKind::Beep => beep(1000.0, 0.10),
        SfxKind::Reload => beep(320.0, 0.06),
        SfxKind::Empty => beep(1800.0, 0.03),
        SfxKind::Buy => beep(660.0, 0.12),
        SfxKind::Glass => glass(0.35),
        SfxKind::Drill => noise_burst(0.08, 30.0, 77, 0.6),
    }
}

/// Banque des sons, synthétisés une fois pour toutes.
#[derive(Resource, Default)]
pub struct SfxBank(HashMap<SfxKind, Handle<AudioSource>>);

impl SfxBank {
    fn get(&self, kind: SfxKind) -> Option<Handle<AudioSource>> {
        self.0.get(&kind).cloned()
    }
}

pub fn setup_sounds(mut commands: Commands, mut assets: ResMut<Assets<AudioSource>>) {
    let mut bank = SfxBank::default();
    for kind in ALL_SFX {
        bank.0.insert(kind, assets.add(source_for(kind)));
    }
    commands.insert_resource(bank);
}

/// Joue un effet **en 3D** : le son est positionné dans la scène, donc on
/// l'entend à droite/gauche selon la caméra et plus loin quand on s'éloigne.
pub fn play_at(commands: &mut Commands, bank: &SfxBank, kind: SfxKind, volume: f32, at: Vec3) {
    let Some(handle) = bank.get(kind) else { return };
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            volume: Volume::Linear(volume),
            spatial: true,
            ..PlaybackSettings::DESPAWN
        },
        Transform::from_translation(at),
    ));
}

/// Variante 2D (interface).
pub fn play(commands: &mut Commands, bank: &SfxBank, kind: SfxKind, volume: f32) {
    let Some(handle) = bank.get(kind) else { return };
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            volume: Volume::Linear(volume),
            ..PlaybackSettings::DESPAWN
        },
    ));
}

// ---------------------------------------------------------------------------
// Secousse de caméra
// ---------------------------------------------------------------------------

/// Traumatisme cumulé (0 = stable, 1 = secousse maximale).
#[derive(Resource, Default)]
pub struct Shake {
    pub trauma: f32,
}

impl Shake {
    pub fn add(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).min(1.0);
    }
}

/// Bruit lissé pour la secousse.
#[derive(Resource)]
pub struct ShakeNoise(pub u32);

/// Applique la secousse à la caméra, par-dessus le regard du joueur.
pub fn apply_shake(
    time: Res<Time>,
    mut shake: ResMut<Shake>,
    mut noise: ResMut<ShakeNoise>,
    mut cameras: Query<
        &mut Transform,
        (
            With<crate::player::WorldCamera>,
            Without<crate::player::ViewModelCamera>,
        ),
    >,
) {
    shake.trauma = (shake.trauma - time.delta_secs() * 1.8).max(0.0);
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };
    if shake.trauma <= 0.0 {
        camera.rotation = Quat::IDENTITY;
        return;
    }
    let amount = shake.trauma * shake.trauma * 0.06;
    let t = time.elapsed_secs();
    noise.0 = noise.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let a = ((noise.0 >> 16) & 0x7FFF) as f32 / 16_384.0 - 1.0;
    let b = ((noise.0 >> 1) & 0x7FFF) as f32 / 16_384.0 - 1.0;
    let ox = (a * (t * 31.0).sin() + b * (t * 37.0).cos()) * amount;
    let oz = (b * (t * 23.0).sin() - a * (t * 29.0).cos()) * amount * 0.6;
    camera.rotation = Quat::from_euler(EulerRot::XYZ, ox, 0.0, oz);
}

// ---------------------------------------------------------------------------
// Flashs d'explosion
// ---------------------------------------------------------------------------

/// Flash lumineux ponctuel.
#[derive(Component)]
pub struct Flash {
    life: f32,
}

/// Flash lumineux ponctuel (explosion, éclat de bouche).
pub fn flash(commands: &mut Commands, at: Vec3, power: f32) {
    commands.spawn((
        PointLight {
            intensity: 80_000.0 * power,
            color: Color::srgb(1.0, 0.8, 0.5),
            range: 6.0 + power * 6.0,
            radius: 0.3,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(at),
        Flash { life: 0.12 + power.min(3.0) * 0.08 },
    ));
}

/// Éteint les flashes.
pub fn fade_flashes(
    time: Res<Time>,
    mut commands: Commands,
    mut flashes: Query<(Entity, &mut Flash, &mut PointLight)>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut light) in &mut flashes {
        flash.life -= dt;
        light.intensity *= (1.0 - dt * 10.0).max(0.0);
        if flash.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Boule de feu d'une explosion : une sphère qui gonfle et s'éteint.
#[derive(Component)]
pub struct Fireball {
    life: f32,
    max: f32,
    size: f32,
    /// Vitesse de montée.
    rise: f32,
}

#[derive(Resource)]
pub struct FireAssets {
    sphere: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
}

pub fn setup_fire(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(FireAssets {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.6, 0.2, 0.85),
            emissive: LinearRgba::rgb(8.0, 3.2, 0.6),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        smoke: materials.add(StandardMaterial {
            base_color: Color::srgba(0.32, 0.31, 0.30, 0.35),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

/// Boule de feu + bouffées de fumée.
pub fn fireball(commands: &mut Commands, fire: &FireAssets, at: Vec3, radius: f32) {
    commands.spawn((
        Mesh3d(fire.sphere.clone()),
        MeshMaterial3d(fire.material.clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(0.1)),
        Fireball {
            life: 0.0,
            max: 0.3,
            size: radius * 0.8,
            rise: 0.5,
        },
        bevy::light::NotShadowCaster,
    ));
    // Quelques bouffées réparties autour du centre, qui montent et se
    // dissipent (une seule grosse sphère faisait un dôme gris).
    let puffs = (3.0 + radius) as u32;
    let mut seed = (at.x * 73.0 + at.z * 31.0 + at.y * 7.0).abs() as u32 | 1;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 1000) as f32 / 1000.0
    };
    for _ in 0..puffs {
        let offset = Vec3::new(rand() - 0.5, rand() * 0.6, rand() - 0.5) * radius * 1.2;
        commands.spawn((
            Mesh3d(fire.sphere.clone()),
            MeshMaterial3d(fire.smoke.clone()),
            Transform::from_translation(at + offset).with_scale(Vec3::splat(0.05)),
            Fireball {
                life: 0.0,
                max: 0.9 + rand() * 0.8,
                size: radius * (0.14 + rand() * 0.14),
                rise: 1.5 + rand() * 1.5,
            },
            bevy::light::NotShadowCaster,
        ));
    }
}

pub fn update_fireballs(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Fireball, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut ball, mut transform) in &mut query {
        ball.life += dt;
        let t = (ball.life / ball.max).min(1.0);
        // Gonfle vite, puis se résorbe.
        let grow = 1.0 - (1.0 - t).powi(3);
        let shrink = if t > 0.6 { 1.0 - (t - 0.6) / 0.4 } else { 1.0 };
        transform.scale = Vec3::splat((ball.size * grow * shrink).max(0.01));
        transform.translation.y += dt * ball.rise;
        if ball.life >= ball.max {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les échantillons doivent être un WAV valide, sinon le lecteur
    /// rejette le son en silence.
    #[test]
    fn wav_header_is_valid() {
        let source = gunshot(0.1, 140.0);
        let bytes = &source.bytes[..];
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        // 44100 Hz, mono, 16 bits → 0,1 s ≈ 8 820 octets de données.
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 44 + data_len);
        assert!((8_000..9_500).contains(&data_len), "durée inattendue");
    }

    #[test]
    fn every_sound_generates_samples() {
        for kind in ALL_SFX {
            assert!(source_for(kind).bytes.len() > 100, "{kind:?}");
        }
    }
}
