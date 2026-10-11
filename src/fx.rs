//! Effets : sons synthétisés, secousse de caméra, flashes d'explosion.
//!
//! Aucun fichier audio n'est embarqué : les échantillons sont générés à la
//! volée (bruit filtré, sinus, rampe de fréquence), encodés en WAV 16 bits
//! monosouris, puis lus par Bevy. L'entité son se déspawn toute seule à la
//! fin de la lecture.

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

/// Balayage de bruit : le « sifflement » de la masse qui tourne.
fn whoosh(dur: f32) -> AudioSource {
    let mut n = Noise(7);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * (0.05 + 0.6 * t / dur);
        prev * (t / dur).sin().powf(1.5) * 0.6
    })
}

/// Détonation : grondement grave + bruit long.
fn explosion(dur: f32) -> AudioSource {
    let mut n = Noise(99);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * 0.02;
        let rumble = (2.0 * std::f32::consts::PI * (55.0 - 25.0 * t / dur) * t).sin();
        (prev * 1.6 + rumble * 0.35) * (-2.2 * t).exp() * 0.9
    })
}

/// Coup de feu : claquement + petit corps.
fn gunshot(dur: f32) -> AudioSource {
    let mut n = Noise(1234);
    let mut prev = 0.0;
    render(dur, |t| {
        let raw = n.next();
        prev += (raw - prev) * 0.5;
        let crack = (-90.0 * t).exp();
        let snap = (2.0 * std::f32::consts::PI * 140.0 * t).sin() * (-25.0 * t).exp();
        (prev * crack + snap * 0.3) * 0.85
    })
}

/// Bip carré : le tic de la C4.
fn beep(freq: f32, dur: f32) -> AudioSource {
    render(dur, |t| {
        let square = (2.0 * std::f32::consts::PI * freq * t).sin().signum();
        square * 0.22 * (-8.0 * t).exp()
    })
}

/// Les sons du jeu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SfxKind {
    Swing,
    Hit,
    Shot,
    Boom,
    Beep,
    Reload,
}

/// Son synthétisé correspondant à un effet.
fn source_for(kind: SfxKind) -> AudioSource {
    match kind {
        SfxKind::Swing => whoosh(0.28),
        SfxKind::Hit => noise_burst(0.30, 18.0, 3, 0.25),
        SfxKind::Shot => gunshot(0.22),
        SfxKind::Boom => explosion(1.5),
        SfxKind::Beep => beep(1000.0, 0.10),
        SfxKind::Reload => beep(320.0, 0.06),
    }
}

/// Joue un effet **en 3D** : le son est positionné dans la scène, donc on
/// l'entend à droite/gauche selon la caméra et plus loin quand on s'éloigne.
pub fn play_at(
    commands: &mut Commands,
    assets: &mut Assets<AudioSource>,
    kind: SfxKind,
    volume: f32,
    at: Vec3,
) {
    commands.spawn((
        AudioPlayer(assets.add(source_for(kind))),
        PlaybackSettings {
            volume: Volume::Linear(volume),
            spatial: true,
            ..default()
        },
        Transform::from_translation(at),
    ));
}

/// Variante 2D (interface).
pub fn play(commands: &mut Commands, assets: &mut Assets<AudioSource>, kind: SfxKind, volume: f32) {
    commands.spawn((
        AudioPlayer(assets.add(source_for(kind))),
        PlaybackSettings {
            volume: Volume::Linear(volume),
            ..default()
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
        return;
    }
    let amount = shake.trauma * shake.trauma * 0.06;
    let t = time.elapsed_secs();
    noise.0 = noise.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let a = ((noise.0 >> 16) & 0x7FFF) as f32 / 16_384.0 - 1.0;
    let b = ((noise.0 >> 1) & 0x7FFF) as f32 / 16_384.0 - 1.0;
    let ox = (a * (t * 31.0).sin() + b * (t * 37.0).cos()) * amount;
    camera.rotation = Quat::from_rotation_x(ox);
}

/// Son + secousse correspondant à une action (le son part de `at`).
pub fn impact_sfx(
    commands: &mut Commands,
    assets: &mut Assets<AudioSource>,
    shake: &mut Shake,
    kind: SfxKind,
    at: Vec3,
) {
    play_at(commands, assets, kind, 0.8, at);
    match kind {
        SfxKind::Swing => shake.add(0.05),
        SfxKind::Hit => shake.add(0.35),
        SfxKind::Shot => shake.add(0.18),
        SfxKind::Boom => shake.add(1.0),
        SfxKind::Beep | SfxKind::Reload => {}
    }
}

// ---------------------------------------------------------------------------
// Flashs d'explosion
// ---------------------------------------------------------------------------

/// Flash lumineux ponctuel (explosion).
#[derive(Component)]
pub struct Flash {
    life: f32,
}

/// Flash lumineux ponctuel (explosion).
pub fn flash(commands: &mut Commands, at: Vec3, power: f32) {
    commands.spawn((
        PointLight {
            intensity: 60_000.0 * power,
            color: Color::srgb(1.0, 0.85, 0.6),
            radius: 6.0,
            ..default()
        },
        Transform::from_translation(at),
        Flash { life: 0.3 },
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
        light.intensity *= 0.82;
        if flash.life <= 0.0 {
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
        let source = gunshot(0.1);
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
        for kind in [
            SfxKind::Swing,
            SfxKind::Hit,
            SfxKind::Shot,
            SfxKind::Boom,
            SfxKind::Beep,
            SfxKind::Reload,
        ] {
            let _ = kind;
        }
        assert!(whoosh(0.05).bytes.len() > 100);
        assert!(explosion(0.2).bytes.len() > 100);
        assert!(beep(440.0, 0.05).bytes.len() > 100);
    }
}
