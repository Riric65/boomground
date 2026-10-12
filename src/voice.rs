//! Voix de proximité : micro → suppression de bruit (RNNoise) → réglage
//! automatique du gain + limiteur → µ-law 16 kHz → serveur → joueurs
//! proches, mixés en stéréo avec atténuation par la distance.
//!
//! Tout le son passe par deux fils dédiés (capture, lecture) pour ne jamais
//! gêner le rendu. Sans micro ou sans sortie audio, la voix se désactive
//! sans bruit.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nnnoiseless::DenoiseState;

use crate::game::{GameState, Phase, Settings};
use crate::net::client::{Lobby, NetClient, RemotePlayer};
use crate::net::protocol::Packet;
use crate::player::WorldCamera;

/// Fréquence du flux de voix transmis.
const WIRE_RATE: f32 = 16_000.0;
/// Trame de 20 ms à 16 kHz.
const WIRE_FRAME: usize = 320;
/// Portée d'écoute (mètres) : le serveur n'envoie que ce qui est à portée.
const RANGE: f32 = 40.0;

// ---------------------------------------------------------------------------
// µ-law
// ---------------------------------------------------------------------------

fn mulaw_encode(x: f32) -> u8 {
    const MU: f32 = 255.0;
    let x = x.clamp(-1.0, 1.0);
    let y = x.signum() * (1.0 + MU * x.abs()).ln() / (1.0 + MU).ln();
    ((y * 127.0).round() as i8) as u8
}

fn mulaw_decode(b: u8) -> f32 {
    const MU: f32 = 255.0;
    let y = (b as i8) as f32 / 127.0;
    y.signum() * ((1.0 + MU).powf(y.abs()) - 1.0) / MU
}

// ---------------------------------------------------------------------------
// Mixeur de lecture
// ---------------------------------------------------------------------------

struct Speaker {
    buf: VecDeque<f32>,
    /// Position fractionnaire de lecture dans `buf` (rééchantillonnage).
    phase: f32,
    gain: f32,
    pan: f32,
    last_rx: Instant,
    /// Tampon d'amorçage : on attend quelques trames avant de jouer.
    primed: bool,
}

#[derive(Default)]
struct Mixer {
    speakers: HashMap<u16, Speaker>,
    out_rate: f32,
}

/// État partagé entre le jeu et les fils audio.
#[derive(Resource)]
pub struct Voice {
    mixer: Arc<Mutex<Mixer>>,
    /// Trames capturées prêtes à envoyer.
    captured: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Le micro parle en ce moment (activation vocale ou touche).
    speaking: Arc<std::sync::atomic::AtomicBool>,
    /// Le fil de capture doit transmettre (porte ouverte côté jeu).
    transmit: Arc<std::sync::atomic::AtomicBool>,
    seq: u16,
    started: bool,
    pub heard: HashSet<u16>,
    /// Message d'erreur matériel déjà affiché.
    warned: bool,
}

/// Trames de voix reçues, à jouer.
#[derive(Resource, Default)]
pub struct VoiceInbox {
    frames: Vec<(u16, Vec<u8>)>,
}

impl VoiceInbox {
    pub fn push(&mut self, item: (u16, Vec<u8>)) {
        self.frames.push(item);
    }
}

fn start_output(mixer: Arc<Mutex<Mixer>>) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("boomground-voix-sortie".into())
        .spawn(move || {
            let result = (|| -> Result<cpal::Stream, String> {
                let host = cpal::default_host();
                let device = host.default_output_device().ok_or("aucune sortie audio")?;
                let supported = device.default_output_config().map_err(|e| e.to_string())?;
                let config: cpal::StreamConfig = supported.config();
                let channels = config.channels as usize;
                if let Ok(mut m) = mixer.lock() {
                    m.out_rate = config.sample_rate as f32;
                }
                let mix = mixer.clone();
                let stream = device
                    .build_output_stream(
                        &config,
                        move |data: &mut [f32], _| {
                            let Ok(mut m) = mix.lock() else {
                                data.fill(0.0);
                                return;
                            };
                            let step = WIRE_RATE / m.out_rate.max(8000.0);
                            data.fill(0.0);
                            for sp in m.speakers.values_mut() {
                                if !sp.primed {
                                    if sp.buf.len() >= WIRE_FRAME * 2 {
                                        sp.primed = true;
                                    } else {
                                        continue;
                                    }
                                }
                                let (l, r) = (sp.gain * (1.0 - sp.pan.max(0.0)), sp.gain * (1.0 + sp.pan.min(0.0)));
                                for frame in data.chunks_mut(channels) {
                                    if sp.buf.len() < 2 {
                                        sp.primed = false;
                                        break;
                                    }
                                    let a = sp.buf[0];
                                    let b = sp.buf[1];
                                    let s = a + (b - a) * sp.phase;
                                    sp.phase += step;
                                    while sp.phase >= 1.0 && sp.buf.len() > 1 {
                                        sp.phase -= 1.0;
                                        sp.buf.pop_front();
                                    }
                                    frame[0] += s * l;
                                    if channels > 1 {
                                        frame[1] += s * r;
                                    }
                                    for extra in frame.iter_mut().skip(2) {
                                        *extra += s * 0.5 * (l + r);
                                    }
                                }
                            }
                            // Écrêtage doux : jamais de saturation dure.
                            for v in data.iter_mut() {
                                *v = (*v * 0.9).tanh();
                            }
                        },
                        |e| eprintln!("voix (sortie) : {e}"),
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                stream.play().map_err(|e| e.to_string())?;
                Ok(stream)
            })();
            match result {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    // Le flux vit tant que ce fil vit.
                    let _keep = stream;
                    loop {
                        std::thread::sleep(Duration::from_secs(3600));
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?
}

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    raw: Arc<Mutex<VecDeque<f32>>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if let Ok(mut q) = raw.lock() {
                    for frame in data.chunks(channels) {
                        // Mono : moyenne des canaux.
                        let sum: f32 = frame.iter().map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s)).sum();
                        q.push_back(sum / channels as f32);
                    }
                    // Jamais plus d'une seconde de retard.
                    while q.len() > 96_000 {
                        q.pop_front();
                    }
                }
            },
            |e| eprintln!("voix (micro) : {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn start_input(
    out: mpsc::Sender<Vec<u8>>,
    speaking: Arc<std::sync::atomic::AtomicBool>,
    transmit: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("boomground-voix-micro".into())
        .spawn(move || {
            let raw: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
            let setup = (|| -> Result<(cpal::Stream, f32), String> {
                let host = cpal::default_host();
                let device = host.default_input_device().ok_or("aucun microphone")?;
                let supported = device.default_input_config().map_err(|e| e.to_string())?;
                let format = supported.sample_format();
                let config: cpal::StreamConfig = supported.config();
                let rate = config.sample_rate as f32;
                let stream = match format {
                    cpal::SampleFormat::F32 => build_input::<f32>(&device, &config, raw.clone())?,
                    cpal::SampleFormat::I16 => build_input::<i16>(&device, &config, raw.clone())?,
                    cpal::SampleFormat::U16 => build_input::<u16>(&device, &config, raw.clone())?,
                    other => return Err(format!("format audio non pris en charge : {other:?}")),
                };
                stream.play().map_err(|e| e.to_string())?;
                Ok((stream, rate))
            })();
            let (stream, in_rate) = match setup {
                Ok(v) => {
                    let _ = ready_tx.send(Ok(()));
                    v
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _keep = stream;

            // Traitement : 48 kHz → trames de 480 → RNNoise → AGC/limiteur
            // → 16 kHz → µ-law.
            let mut denoise = DenoiseState::new();
            let mut resampled: VecDeque<f32> = VecDeque::new();
            let mut phase = 0.0f32;
            let step = in_rate / 48_000.0;
            let mut gain = 1.0f32;
            let mut hang = 0u32;
            let mut wire: Vec<f32> = Vec::new();
            let mut last = 0.0f32;
            loop {
                std::thread::sleep(Duration::from_millis(5));
                // Rééchantillonnage linéaire vers 48 kHz.
                if let Ok(mut q) = raw.lock() {
                    while q.len() > 2 {
                        let a = q[0];
                        let b = q[1];
                        resampled.push_back(a + (b - a) * phase.min(1.0));
                        phase += step;
                        while phase >= 1.0 && q.len() > 1 {
                            phase -= 1.0;
                            q.pop_front();
                        }
                        if resampled.len() > 48_000 {
                            break;
                        }
                    }
                }
                while resampled.len() >= DenoiseState::FRAME_SIZE {
                    let mut input = [0.0f32; DenoiseState::FRAME_SIZE];
                    for v in input.iter_mut() {
                        // RNNoise attend des valeurs à l'échelle i16.
                        *v = resampled.pop_front().unwrap_or(0.0) * 32_767.0;
                    }
                    let mut output = [0.0f32; DenoiseState::FRAME_SIZE];
                    let vad = denoise.process_frame(&mut output, &input);
                    // Porte : parole détectée (avec maintien de 250 ms).
                    if vad > 0.55 {
                        hang = 25;
                    } else if hang > 0 {
                        hang -= 1;
                    }
                    let open = hang > 0;
                    // AGC vers -20 dBFS (RMS), +12 dB au plus, puis limiteur
                    // à -3 dBFS.
                    let rms = (output.iter().map(|s| s * s).sum::<f32>() / output.len() as f32).sqrt() / 32_767.0;
                    if open && rms > 1e-4 {
                        let want = (0.1 / rms).clamp(0.25, 4.0);
                        gain += (want - gain) * 0.05;
                    }
                    speaking.store(open, Ordering::Relaxed);
                    let send = open && transmit.load(Ordering::Relaxed);
                    // 48 kHz → 16 kHz : moyenne par trois.
                    for tri in output.chunks(3) {
                        let mean = tri.iter().sum::<f32>() / tri.len() as f32 / 32_767.0;
                        let mut v = mean * gain;
                        // Limiteur (-3 dBFS) : écrêtage doux au-delà de 0,7.
                        if v.abs() > 0.7 {
                            v = v.signum() * (0.7 + 0.3 * ((v.abs() - 0.7) / 0.3).tanh());
                        }
                        last = v;
                        wire.push(if send { v } else { 0.0 });
                    }
                    while wire.len() >= WIRE_FRAME {
                        let frame: Vec<f32> = wire.drain(..WIRE_FRAME).collect();
                        if send {
                            let bytes: Vec<u8> = frame.iter().map(|&s| mulaw_encode(s)).collect();
                            if out.send(bytes).is_err() {
                                return;
                            }
                        }
                    }
                    let _ = last;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?
}

impl Voice {
    fn new() -> Self {
        let (_tx, rx) = mpsc::channel();
        Self {
            mixer: Arc::new(Mutex::new(Mixer::default())),
            captured: Mutex::new(rx),
            speaking: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            transmit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            seq: 0,
            started: false,
            heard: HashSet::new(),
            warned: false,
        }
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

/// Démarre les fils audio, relaie micro → serveur et serveur → haut-parleur.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut voice: ResMut<Voice>,
    mut inbox: ResMut<VoiceInbox>,
    settings: Res<Settings>,
    game: Res<GameState>,
    lobby: Res<Lobby>,
    keys: Res<ButtonInput<KeyCode>>,
    mut net: ResMut<NetClient>,
    mut toasts: ResMut<crate::ui::Toasts>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    mut last_heard: Local<HashMap<u16, Instant>>,
) {
    let active = lobby.active && !lobby.solo && lobby.voice && net.is_online() && settings.voice_mode > 0;
    if !active {
        inbox.frames.clear();
        if let Ok(rx) = voice.captured.lock() {
            while rx.try_recv().is_ok() {}
        }
        voice.transmit.store(false, std::sync::atomic::Ordering::Relaxed);
        voice.heard.clear();
        return;
    }

    // Démarrage unique des fils.
    if !voice.started && !voice.warned {
        voice.started = true;
        let (tx, rx) = mpsc::channel();
        let mixer = voice.mixer.clone();
        let speaking = voice.speaking.clone();
        let transmit = voice.transmit.clone();
        let out = start_output(mixer);
        let mic = start_input(tx, speaking, transmit);
        if let Err(e) = &out {
            toasts.push(format!("Voix : sortie audio indisponible ({e})"));
        }
        match mic {
            Ok(()) => {
                if let Ok(mut r) = voice.captured.lock() {
                    *r = rx;
                }
            }
            Err(e) => {
                voice.warned = true;
                toasts.push(format!("Voix : micro indisponible ({e})"));
            }
        }
    }

    // Émission : activation vocale, ou touche G maintenue.
    let can_talk = game.phase == Phase::Playing && !game.dead && !game.typing && !game.loading;
    let ptt = keys.pressed(KeyCode::KeyG);
    let allow = can_talk && (settings.voice_mode == 1 || ptt);
    voice.transmit.store(allow, std::sync::atomic::Ordering::Relaxed);
    let mut frames = Vec::new();
    if let Ok(rx) = voice.captured.lock() {
        while let Ok(f) = rx.try_recv() {
            frames.push(f);
        }
    }
    for data in frames {
        voice.seq = voice.seq.wrapping_add(1);
        let seq = voice.seq;
        net.send_raw(Packet::Voice { from: 0, seq, data });
    }

    // Réception : décodage dans les tampons des interlocuteurs.
    let frames = std::mem::take(&mut inbox.frames);
    let now = Instant::now();
    if let Ok(mut mixer) = voice.mixer.lock() {
        for (from, data) in frames {
            let sp = mixer.speakers.entry(from).or_insert_with(|| Speaker {
                buf: VecDeque::new(),
                phase: 0.0,
                gain: 0.0,
                pan: 0.0,
                last_rx: now,
                primed: false,
            });
            sp.last_rx = now;
            // Trop de retard accumulé : on rattrape.
            if sp.buf.len() > WIRE_FRAME * 8 {
                sp.buf.drain(..WIRE_FRAME * 4);
            }
            sp.buf.extend(data.iter().map(|&b| mulaw_decode(b)));
            last_heard.insert(from, now);
        }
        // Position, atténuation et panoramique.
        let (eye, right) = camera
            .single()
            .map(|c| (c.translation(), c.right().as_vec3()))
            .unwrap_or((Vec3::ZERO, Vec3::X));
        for (id, sp) in mixer.speakers.iter_mut() {
            let Some((_, t)) = remotes.iter().find(|(r, _)| r.id == *id) else {
                sp.gain = 0.0;
                continue;
            };
            let to = (t.translation + Vec3::Y * 1.6) - eye;
            let d = to.length();
            let fall = (1.0 - d / RANGE).clamp(0.0, 1.0);
            let near = 1.0 / (1.0 + d * 0.12);
            sp.gain = (fall * fall).min(1.0) * near.max(0.25) * settings.voice_volume * 1.6;
            sp.pan = if d > 0.1 { (to / d).dot(right).clamp(-1.0, 1.0) } else { 0.0 };
        }
        mixer.speakers.retain(|_, s| now.duration_since(s.last_rx) < Duration::from_secs(5));
    }
    voice.heard = last_heard
        .iter()
        .filter(|(_, t)| now.duration_since(**t) < Duration::from_millis(500))
        .map(|(id, _)| *id)
        .collect();
    last_heard.retain(|_, t| now.duration_since(*t) < Duration::from_secs(5));
}

impl Voice {
    /// Le micro capte de la parole en ce moment.
    pub fn talking(&self) -> bool {
        self.speaking.load(std::sync::atomic::Ordering::Relaxed)
            && self.transmit.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Indicateur : qui parle (en haut à droite).
#[derive(Component)]
pub struct VoiceIndicator;

pub fn setup_indicator(mut commands: Commands, font: Res<crate::ui::UiFont>) {
    let font = font.into_inner().clone();
    commands.spawn((
        VoiceIndicator,
        Node {
            position_type: PositionType::Absolute,
            right: px(16),
            top: px(40),
            padding: UiRect::axes(px(10.0), px(4.0)),
            border_radius: BorderRadius::all(px(6.0)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        crate::ui::text(&font, "", 14.0, crate::ui::GREEN),
    ));
}

pub fn indicator(
    voice: Res<Voice>,
    lobby: Res<Lobby>,
    game: Res<GameState>,
    mut q: Query<(&mut Text, &mut Node), With<VoiceIndicator>>,
) {
    let Ok((mut text, mut node)) = q.single_mut() else { return };
    let mut parts: Vec<String> = Vec::new();
    if voice.talking() {
        parts.push("Tu parles".to_string());
    }
    let mut names: Vec<String> = voice
        .heard
        .iter()
        .map(|id| {
            lobby
                .players
                .iter()
                .find(|(p, _)| p == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| format!("Joueur {id}"))
        })
        .collect();
    names.sort();
    if !names.is_empty() {
        parts.push(format!("{} parle", names.join(", ")));
    }
    let value = parts.join(" · ");
    let show = !value.is_empty() && game.phase == Phase::Playing;
    if text.0 != value {
        text.0 = value;
    }
    let d = if show { Display::Flex } else { Display::None };
    if node.display != d {
        node.display = d;
    }
}
