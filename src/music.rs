//! Musique : trois ambiances **composées par le jeu** (synthèse générative,
//! aucun fichier ni licence), ou vos propres morceaux déposés dans le
//! dossier `music` à côté des réglages (`.wav` ou `.ogg`).
//!
//! - menu : nappes lentes et clochettes ;
//! - jeu : accords doux, arpèges et basse, puis une ambiance plus tendue.

use std::sync::{mpsc, Mutex};

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::game::{GameState, Phase, Settings};

const RATE: u32 = 22_050;

fn freq(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// Générateur déterministe.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 8) & 0xFFFF) as f32 / 65_536.0
    }
}

/// Mélange une note douce (partiels légèrement désaccordés) dans `out`.
fn add_pad(out: &mut [f32], start: f32, dur: f32, midi: f32, amp: f32) {
    let f = freq(midi);
    let a = 0.9 * dur.min(2.0);
    let begin = (start * RATE as f32) as usize;
    let n = (dur * RATE as f32) as usize;
    for i in 0..n {
        let Some(slot) = out.get_mut(begin + i) else { break };
        let t = i as f32 / RATE as f32;
        let env = (t / a).min(1.0) * ((dur - t) / a).clamp(0.0, 1.0);
        let tau = std::f32::consts::TAU;
        let s = (tau * f * t).sin()
            + 0.6 * (tau * f * 1.004 * t).sin()
            + 0.5 * (tau * f * 0.996 * t).sin()
            + 0.25 * (tau * f * 2.0 * t).sin();
        *slot += s * env * amp * 0.28;
    }
}

/// Note pincée (arpège) : sinus + harmonique, décroissance rapide.
fn add_pluck(out: &mut [f32], start: f32, midi: f32, amp: f32, decay: f32) {
    let f = freq(midi);
    let begin = (start * RATE as f32) as usize;
    let n = (decay * 5.0 * RATE as f32) as usize;
    for i in 0..n {
        let Some(slot) = out.get_mut(begin + i) else { break };
        let t = i as f32 / RATE as f32;
        let tau = std::f32::consts::TAU;
        let env = (-t / decay).exp() * (t * 200.0).min(1.0);
        *slot += ((tau * f * t).sin() + 0.35 * (tau * f * 2.0 * t).sin() + 0.12 * (tau * f * 3.0 * t).sin())
            * env
            * amp;
    }
}

/// Clochette : partiels inharmoniques.
fn add_bell(out: &mut [f32], start: f32, midi: f32, amp: f32) {
    let f = freq(midi);
    let begin = (start * RATE as f32) as usize;
    let n = (4.0 * RATE as f32) as usize;
    for i in 0..n {
        let Some(slot) = out.get_mut(begin + i) else { break };
        let t = i as f32 / RATE as f32;
        let tau = std::f32::consts::TAU;
        let env = (-t / 0.9).exp() * (t * 300.0).min(1.0);
        *slot += ((tau * f * t).sin() + 0.5 * (tau * f * 2.76 * t).sin() + 0.25 * (tau * f * 5.4 * t).sin()) * env * amp;
    }
}

/// Grosse caisse douce.
fn add_kick(out: &mut [f32], start: f32, amp: f32) {
    let begin = (start * RATE as f32) as usize;
    let n = (0.5 * RATE as f32) as usize;
    for i in 0..n {
        let Some(slot) = out.get_mut(begin + i) else { break };
        let t = i as f32 / RATE as f32;
        let f = 48.0 + 70.0 * (-t * 22.0).exp();
        *slot += (std::f32::consts::TAU * f * t).sin() * (-t * 7.0).exp() * amp;
    }
}

/// Réverbération légère : trois échos bouclés.
fn reverb(x: &mut [f32]) {
    for (delay, fb) in [(0.0297, 0.45), (0.0371, 0.42), (0.0537, 0.38), (0.0971, 0.3)] {
        let d = (delay * RATE as f32) as usize;
        for i in d..x.len() {
            x[i] += x[i - d] * fb * 0.5;
        }
    }
}

fn finish(mut x: Vec<f32>) -> Vec<f32> {
    reverb(&mut x);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-3);
    let fade = (1.5 * RATE as f32) as usize;
    let n = x.len();
    for (i, v) in x.iter_mut().enumerate() {
        let f = (i as f32 / fade as f32).min(1.0) * ((n - i) as f32 / fade as f32).min(1.0);
        *v = *v / peak * 0.8 * f;
    }
    x
}

/// Une ambiance : `kind` 0 menu, 1 jeu calme, 2 jeu tendu.
fn compose(kind: u8) -> Vec<f32> {
    let mut rng = Rng(0xC0FFEE + kind as u32 * 7919);
    let (bpm, root, chords, bars): (f32, f32, [[f32; 4]; 4], usize) = match kind {
        // La mineur : Am – F – C – G
        0 => (58.0, 45.0, [[0.0, 7.0, 12.0, 15.0], [-4.0, 3.0, 8.0, 12.0], [3.0, 10.0, 15.0, 19.0], [-2.0, 5.0, 10.0, 14.0]], 16),
        // Ré mineur : Dm – Bb – F – C
        1 => (78.0, 50.0, [[0.0, 7.0, 12.0, 17.0], [-4.0, 3.0, 8.0, 12.0], [3.0, 10.0, 15.0, 19.0], [-2.0, 5.0, 10.0, 14.0]], 16),
        // Mi mineur : Em – C – D – Bm
        _ => (96.0, 40.0, [[0.0, 7.0, 12.0, 15.0], [-4.0, 3.0, 8.0, 12.0], [-2.0, 5.0, 10.0, 14.0], [-5.0, 2.0, 7.0, 10.0]], 16),
    };
    let beat = 60.0 / bpm;
    let bar = beat * 4.0;
    let total = bar * bars as f32;
    let mut out = vec![0.0f32; (total * RATE as f32) as usize + (3.0 * RATE as f32) as usize];
    let minor = [0.0, 2.0, 3.0, 5.0, 7.0, 8.0, 10.0, 12.0, 15.0, 17.0, 19.0];
    for b in 0..bars {
        let chord = chords[b % 4];
        let t0 = b as f32 * bar;
        // Nappe.
        for &n in &chord {
            add_pad(&mut out, t0, bar + 1.2, root + 12.0 + n, if kind == 0 { 0.9 } else { 0.7 });
        }
        // Basse.
        add_pad(&mut out, t0, bar, root - 12.0 + chord[0], 0.9);
        match kind {
            0 => {
                // Clochettes éparses.
                for k in 0..4 {
                    if rng.next() < 0.55 {
                        let note = root + 24.0 + minor[(rng.next() * minor.len() as f32) as usize % minor.len()];
                        add_bell(&mut out, t0 + k as f32 * beat + rng.next() * beat * 0.5, note, 0.16);
                    }
                }
            }
            1 => {
                // Arpège en croches.
                for k in 0..8 {
                    let n = chord[[0usize, 1, 2, 3, 2, 1, 2, 3][k]];
                    add_pluck(&mut out, t0 + k as f32 * beat * 0.5, root + 24.0 + n, 0.22, 0.28);
                }
                add_kick(&mut out, t0, 0.35);
                add_kick(&mut out, t0 + 2.0 * beat, 0.25);
            }
            _ => {
                // Pouls grave et accents.
                for k in 0..4 {
                    add_kick(&mut out, t0 + k as f32 * beat, if k % 2 == 0 { 0.6 } else { 0.35 });
                }
                for k in 0..8 {
                    if rng.next() < 0.6 {
                        let n = chord[(rng.next() * 4.0) as usize % 4];
                        add_pluck(&mut out, t0 + k as f32 * beat * 0.5, root + 12.0 + n, 0.2, 0.18);
                    }
                }
            }
        }
    }
    finish(out)
}

/// Pistes disponibles.
#[derive(Resource)]
pub struct Music {
    /// `[menu, calme, tendu]` synthétisées.
    synth: Vec<Handle<AudioSource>>,
    /// Morceaux de l'utilisateur (priment sur la synthèse).
    user: Vec<Handle<AudioSource>>,
    rx: Option<Mutex<mpsc::Receiver<Vec<Vec<f32>>>>>,
    current: Option<Entity>,
    /// 0 menu, 1 jeu.
    playing_for: u8,
    counter: usize,
}

#[derive(Component)]
pub struct MusicPlayer;

fn user_tracks(assets: &mut Assets<AudioSource>) -> Vec<Handle<AudioSource>> {
    let Some(dir) = crate::game::config_dir().map(|d| d.join("music")) else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let ok = p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_lowercase().as_str(), "wav" | "ogg"));
        if !ok {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&p) {
            out.push(assets.add(AudioSource { bytes: bytes.into() }));
        }
    }
    out
}

pub fn setup(mut commands: Commands, mut assets: ResMut<Assets<AudioSource>>) {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new().name("boomground-musique".into()).spawn(move || {
        let tracks = vec![compose(0), compose(1), compose(2)];
        let _ = tx.send(tracks);
    });
    commands.insert_resource(Music {
        synth: Vec::new(),
        user: user_tracks(&mut assets),
        rx: Some(Mutex::new(rx)),
        current: None,
        playing_for: 255,
        counter: 0,
    });
}

/// Choisit, lance et règle le volume de la musique.
pub fn update(
    mut commands: Commands,
    mut music: ResMut<Music>,
    settings: Res<Settings>,
    game: Res<GameState>,
    mut assets: ResMut<Assets<AudioSource>>,
    mut sinks: Query<&mut AudioSink, With<MusicPlayer>>,
    alive: Query<Entity, With<MusicPlayer>>,
) {
    // Pistes synthétisées prêtes ?
    if let Some(rx) = &music.rx {
        let got = rx.lock().ok().and_then(|r| r.try_recv().ok());
        if let Some(tracks) = got {
            let handles: Vec<_> = tracks
                .into_iter()
                .map(|samples| assets.add(crate::fx::wav_rate(&samples, RATE)))
                .collect();
            music.synth = handles;
            music.rx = None;
        }
    }

    let volume = Volume::Linear(settings.music * 0.45);
    for mut sink in &mut sinks {
        sink.set_volume(volume);
    }

    // La piste en cours a disparu (fin de lecture) ?
    if let Some(e) = music.current {
        if alive.get(e).is_err() {
            music.current = None;
        }
    }
    let wants = u8::from(game.phase == Phase::Playing);
    if settings.music <= 0.0 {
        if let Some(e) = music.current.take() {
            commands.entity(e).despawn();
        }
        return;
    }
    if music.current.is_some() && music.playing_for != wants {
        if let Some(e) = music.current.take() {
            commands.entity(e).despawn();
        }
    }
    if music.current.is_some() {
        return;
    }
    let handle = if !music.user.is_empty() {
        music.counter += 1;
        music.user[music.counter % music.user.len()].clone()
    } else if music.synth.len() == 3 {
        music.counter += 1;
        let idx = if wants == 0 {
            0
        } else if music.counter % 3 == 0 {
            2
        } else {
            1
        };
        music.synth[idx].clone()
    } else {
        return;
    };
    let e = commands
        .spawn((
            MusicPlayer,
            AudioPlayer(handle),
            PlaybackSettings {
                volume,
                ..PlaybackSettings::DESPAWN
            },
        ))
        .id();
    music.current = Some(e);
    music.playing_for = wants;
}
