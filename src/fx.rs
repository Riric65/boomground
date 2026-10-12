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

/// Encode des échantillons en WAV 16 bits PCM (mono, fréquence donnée).
pub fn wav_rate(samples: &[f32], rate: u32) -> AudioSource {
    let data_bytes = samples.len() * 2;
    let mut bytes: Vec<u8> = Vec::with_capacity(44 + data_bytes);

    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // taille du bloc fmt
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes()); // octets/s
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

/// WAV à la fréquence de synthèse, **normalisé** : les sons filtrés étaient
/// très faibles, d'où des effets à peine audibles.
fn wav(samples: &[f32]) -> AudioSource {
    let peak = samples.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-4);
    let gain = 0.92 / peak;
    let norm: Vec<f32> = samples.iter().map(|s| s * gain).collect();
    wav_rate(&norm, RATE)
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

/// Grondement grave et irrégulier : un bâtiment qui s'écroule.
fn rumble(dur: f32) -> AudioSource {
    let mut n = Noise(31337);
    let (mut a, mut b) = (0.0f32, 0.0f32);
    render(dur, |t| {
        let raw = n.next();
        a += (raw - a) * 0.012;
        b += (a - b) * 0.05;
        let wobble = 0.65 + 0.35 * (std::f32::consts::TAU * 7.0 * t).sin();
        let env = (t * 8.0).min(1.0) * (1.0 - t / dur).powf(1.4);
        (b * 9.0 + (std::f32::consts::TAU * 38.0 * t).sin() * 0.25) * wobble * env
    })
}

/// Impact lourd : claquement grave et éclats de gravats.
fn crash(dur: f32) -> AudioSource {
    let mut n = Noise(555);
    let mut low = 0.0f32;
    render(dur, |t| {
        let raw = n.next();
        low += (raw - low) * 0.06;
        let thump = (std::f32::consts::TAU * (62.0 - 30.0 * t) * t).sin() * (-9.0 * t).exp();
        let debris = raw * (-14.0 * t).exp() * 0.5 + low * (-5.0 * t).exp();
        thump * 1.1 + debris
    })
}

/// Crépitement : claquements aléatoires brefs.
fn crackle(dur: f32) -> AudioSource {
    let mut n = Noise(8080);
    let mut env = 0.0f32;
    render(dur, |_| {
        if n.next().abs() > 0.985 {
            env = 1.0;
        }
        env *= 0.992;
        n.next() * env
    })
}

/// Bloc posé : petit choc sourd.
fn thunk(dur: f32) -> AudioSource {
    render(dur, |t| (std::f32::consts::TAU * (150.0 - 400.0 * t) * t).sin() * (-28.0 * t).exp())
}

/// Deux battements sourds (« toum-toum »).
fn heartbeat() -> AudioSource {
    render(0.9, |t| {
        let beat = |t0: f32, amp: f32| {
            let x = t - t0;
            if x < 0.0 {
                0.0
            } else {
                (std::f32::consts::TAU * (58.0 - 20.0 * x) * x).sin() * (-14.0 * x).exp() * amp
            }
        };
        beat(0.0, 1.0) + beat(0.28, 0.75)
    })
}

/// Choc : souffle coupé + grave.
fn hurt_sound(dur: f32) -> AudioSource {
    let mut n = Noise(2024);
    let mut low = 0.0f32;
    render(dur, |t| {
        low += (n.next() - low) * 0.05;
        let thump = (std::f32::consts::TAU * (95.0 - 60.0 * t) * t).sin() * (-8.0 * t).exp();
        thump + low * 1.6 * (-4.0 * t).exp()
    })
}

/// Sifflement aigu qui décroît lentement.
fn ring(dur: f32) -> AudioSource {
    render(dur, |t| {
        let env = (-1.6 * t).exp() * (t * 200.0).min(1.0);
        ((std::f32::consts::TAU * 4_200.0 * t).sin() * 0.6 + (std::f32::consts::TAU * 4_650.0 * t).sin() * 0.4) * env * 0.35
    })
}

/// Grincement : balayage de fréquence avec harmoniques.
fn door_creak(dur: f32) -> AudioSource {
    let mut phase = 0.0f32;
    render(dur, |t| {
        let f = 180.0 + 120.0 * (std::f32::consts::TAU * 1.7 * t).sin() + 140.0 * t;
        phase += std::f32::consts::TAU * f / RATE as f32;
        let env = (t * 12.0).min(1.0) * (1.0 - t / dur);
        (phase.sin() + 0.5 * (phase * 2.0).sin() + 0.3 * (phase * 3.0).sin()) * env * 0.4
    })
}

/// Moteur : onde en dents de scie grave modulée (bouclable).
fn engine(dur: f32) -> AudioSource {
    render(dur, |t| {
        let f = 70.0;
        let saw = (t * f).fract() * 2.0 - 1.0;
        let pulse = (std::f32::consts::TAU * 28.0 * t).sin() * 0.3 + 0.7;
        saw * pulse * 0.6 + (std::f32::consts::TAU * f * 0.5 * t).sin() * 0.4
    })
}

/// Klaxon : deux notes tenues.
fn horn(dur: f32) -> AudioSource {
    render(dur, |t| {
        let env = (t * 60.0).min(1.0) * ((dur - t) * 40.0).clamp(0.0, 1.0);
        let a = (std::f32::consts::TAU * 392.0 * t).sin().signum();
        let b = (std::f32::consts::TAU * 494.0 * t).sin().signum();
        (a + b) * 0.25 * env
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
    /// Grondement d'un écroulement.
    Rumble,
    /// Choc lourd d'un bloc qui s'écrase.
    Crash,
    /// Crépitement du feu.
    Crackle,
    /// Pose d'un bloc.
    Thunk,
    /// Battement de cœur (points de vie bas).
    Heartbeat,
    /// Douleur : souffle coupé, choc violent.
    Hurt,
    /// Acouphène après une explosion proche.
    Ring,
    /// Porte qui grince.
    Door,
    /// Moteur (boucle courte, hauteur selon la vitesse).
    Engine,
    /// Klaxon.
    Horn,
    /// Éclat de verre / métal tordu : impact de véhicule.
    Scrape,
}

const ALL_SFX: [SfxKind; 23] = [
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
    SfxKind::Rumble,
    SfxKind::Crash,
    SfxKind::Crackle,
    SfxKind::Thunk,
    SfxKind::Heartbeat,
    SfxKind::Hurt,
    SfxKind::Ring,
    SfxKind::Door,
    SfxKind::Engine,
    SfxKind::Horn,
    SfxKind::Scrape,
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
        SfxKind::Rumble => rumble(3.2),
        SfxKind::Crash => crash(1.1),
        SfxKind::Crackle => crackle(0.7),
        SfxKind::Thunk => thunk(0.18),
        SfxKind::Heartbeat => heartbeat(),
        SfxKind::Hurt => hurt_sound(0.7),
        SfxKind::Ring => ring(2.4),
        SfxKind::Door => door_creak(0.7),
        SfxKind::Engine => engine(0.5),
        SfxKind::Horn => horn(0.6),
        SfxKind::Scrape => crash(0.5),
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
static SFX_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// Volume des bruitages (réglage), 0..1.
pub fn set_sfx_scale(v: f32) {
    SFX_SCALE.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn sfx_scale() -> f32 {
    f32::from_bits(SFX_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn play_at(commands: &mut Commands, bank: &SfxBank, kind: SfxKind, volume: f32, at: Vec3) {
    let volume = volume * sfx_scale();
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
    let volume = volume * sfx_scale();
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
    settings: Res<crate::game::Settings>,
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
    let amount = shake.trauma * shake.trauma * 0.06 * settings.shake;
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

// ----------------------------------------------------------------------
// Poussière, fumée, flammes
// ----------------------------------------------------------------------

/// Nombre maximal de bouffées simultanées.
const MAX_PUFFS: usize = 220;
/// Nombre maximal de flammes affichées.
pub const MAX_FLAMES: usize = 450;
/// Niveaux d'opacité d'une bouffée (du plus opaque au plus transparent).
const PUFF_LEVELS: usize = 6;

/// Maillages et matériaux des effets de particules.
#[derive(Resource)]
pub struct ParticleAssets {
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    /// `[teinte][niveau]` : 0 poussière, 1 fumée.
    puffs: [Vec<Handle<StandardMaterial>>; 2],
    flame: [Handle<StandardMaterial>; 2],
}

/// Densité des particules (0 aucune, 1 réduite, 2 pleine).
#[derive(Resource)]
pub struct ParticleDensity(pub u8);

impl Default for ParticleDensity {
    fn default() -> Self {
        Self(2)
    }
}

/// Une bouffée de poussière ou de fumée.
#[derive(Component)]
pub struct Puff {
    age: f32,
    life: f32,
    vel: Vec3,
    size0: f32,
    size1: f32,
    tint: usize,
    level: usize,
}

/// Une flamme (un voxel en feu).
#[derive(Component)]
pub struct Flame {
    pub phase: f32,
    pub base: Vec3,
    smoke: f32,
}

pub fn setup_particles(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut puffs: [Vec<Handle<StandardMaterial>>; 2] = [Vec::new(), Vec::new()];
    let tints = [Color::srgb(0.66, 0.62, 0.56), Color::srgb(0.16, 0.16, 0.17)];
    for (t, tint) in tints.iter().enumerate() {
        for l in 0..PUFF_LEVELS {
            let alpha = 0.5 * (1.0 - l as f32 / PUFF_LEVELS as f32);
            puffs[t].push(materials.add(StandardMaterial {
                base_color: tint.with_alpha(alpha),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }));
        }
    }
    let flame = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.45, 0.08),
            emissive: LinearRgba::rgb(7.0, 2.2, 0.2),
            unlit: true,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.3),
            emissive: LinearRgba::rgb(8.0, 6.0, 1.2),
            unlit: true,
            ..default()
        }),
    ];
    commands.insert_resource(ParticleAssets {
        sphere: meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap()),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        puffs,
        flame,
    });
}

/// Bouffées de poussière : `amount` ~ nombre de voxels, ou force.
pub fn dust(commands: &mut Commands, p: &ParticleAssets, density: u8, existing: usize, at: Vec3, amount: f32, smoke: bool) {
    if density == 0 || existing >= MAX_PUFFS {
        return;
    }
    let scale = if density == 1 { 0.5 } else { 1.0 };
    let n = ((amount * 0.12).clamp(2.0, 16.0) * scale) as usize;
    let seed = (at.x * 12.9898 + at.y * 78.233 + at.z * 37.719).sin() * 43758.547;
    let mut s = seed.fract().abs();
    let mut rnd = move || {
        s = (s * 9301.0 + 49297.0) % 233280.0;
        s / 233280.0
    };
    let size = (0.6 + amount.sqrt() * 0.09).min(3.5);
    for _ in 0..n {
        let a = rnd() * std::f32::consts::TAU;
        let out = 0.6 + rnd() * 2.2;
        let tint = if smoke { 1 } else { 0 };
        commands.spawn((
            Puff {
                age: 0.0,
                life: 1.6 + rnd() * 2.2,
                vel: Vec3::new(a.cos() * out, 0.5 + rnd() * 1.6, a.sin() * out),
                size0: size * (0.5 + rnd() * 0.5),
                size1: size * (2.2 + rnd() * 1.5),
                tint,
                level: 0,
            },
            Mesh3d(p.sphere.clone()),
            MeshMaterial3d(p.puffs[tint][0].clone()),
            Transform::from_translation(at + Vec3::new(rnd() - 0.5, rnd() * 0.4, rnd() - 0.5) * size).with_scale(Vec3::splat(0.2)),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Crée une flamme sur un voxel.
pub fn flame(commands: &mut Commands, p: &ParticleAssets, at: Vec3, phase: f32) -> Entity {
    commands
        .spawn((
            Flame {
                phase,
                base: at,
                smoke: phase.fract(),
            },
            Mesh3d(p.cube.clone()),
            MeshMaterial3d(p.flame[0].clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.5)),
            bevy::light::NotShadowCaster,
        ))
        .id()
}

/// Les bouffées grossissent, montent, s'éclaircissent et disparaissent.
pub fn update_puffs(
    time: Res<Time>,
    assets: Res<ParticleAssets>,
    mut commands: Commands,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut p, mut t, mut m) in &mut puffs {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let k = p.age / p.life;
        t.translation += p.vel * dt;
        p.vel *= 1.0 - (1.4 * dt).min(0.9);
        p.vel.y += 0.35 * dt;
        t.scale = Vec3::splat(p.size0 + (p.size1 - p.size0) * (1.0 - (1.0 - k).powi(2)));
        let level = ((k * PUFF_LEVELS as f32) as usize).min(PUFF_LEVELS - 1);
        if level != p.level {
            p.level = level;
            m.0 = assets.puffs[p.tint][level].clone();
        }
    }
}

/// Scintillement des flammes et fumée qui s'en échappe.
pub fn animate_flames(
    time: Res<Time>,
    bank: Res<SfxBank>,
    mut crackle_cooldown: Local<f32>,
    assets: Res<ParticleAssets>,
    density: Res<ParticleDensity>,
    mut commands: Commands,
    puffs: Query<(), With<Puff>>,
    mut flames: Query<(&mut Flame, &mut Transform), Without<Puff>>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let mut count = puffs.iter().len();
    *crackle_cooldown -= dt;
    for (mut f, mut tr) in &mut flames {
        if *crackle_cooldown <= 0.0 && (t * 3.0 + f.phase * 11.0).sin() > 0.995 {
            *crackle_cooldown = 0.35;
            play_at(&mut commands, &bank, SfxKind::Crackle, 0.45, f.base);
        }
        let a = (t * 9.0 + f.phase * 40.0).sin() * 0.5 + 0.5;
        let b = (t * 13.0 + f.phase * 17.0).cos() * 0.5 + 0.5;
        tr.scale = Vec3::new(0.28 + a * 0.2, 0.45 + b * 0.55, 0.28 + b * 0.2);
        tr.translation = f.base + Vec3::new(0.0, tr.scale.y * 0.4 - 0.1, 0.0);
        f.smoke += dt * (0.4 + f.phase.fract());
        if f.smoke > 3.0 && density.0 > 0 && count < MAX_PUFFS - 20 {
            f.smoke = 0.0;
            count += 1;
            dust(&mut commands, &assets, density.0, count, f.base + Vec3::Y * 0.5, 14.0, true);
        }
    }
}
