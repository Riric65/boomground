//! Musique : trois morceaux **libres de droits embarqués** dans
//! l'exécutable (voir `assets/music/CREDITS.md`), ou vos propres fichiers
//! déposés dans le dossier `music` à côté des réglages.
//!
//! - menu : `Exploring Town`, ambiance calme et lumineuse ;
//! - jeu calme : `Summer Sunday` ;
//! - action : `8-Bit Battle Loop`, plus nerveuse.
//!
//! Les trois sont en Ogg Vorbis, un format que le moteur sait décoder sans
//! dépendance supplémentaire, et pèsent ~2 Mo au total : le jeu reste
//! distribuable en un seul fichier.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::game::{GameState, Phase, Settings};

/// Les morceaux embarqués : `[menu, jeu calme, action]`.
const TRACKS: [&[u8]; 3] = [
    include_bytes!("../assets/music/menu.ogg"),
    include_bytes!("../assets/music/game.ogg"),
    include_bytes!("../assets/music/action.ogg"),
];

#[derive(Resource)]
pub struct Music {
    /// Les trois morceaux embarqués.
    bundled: Vec<Handle<AudioSource>>,
    /// Morceaux de l'utilisateur (ils priment sur les embarqués).
    user: Vec<Handle<AudioSource>>,
    current: Option<Entity>,
    /// 0 menu, 1 jeu.
    playing_for: u8,
    counter: usize,
}

#[derive(Component)]
pub struct MusicPlayer;

/// Les morceaux déposés par l'utilisateur dans le dossier de configuration.
fn user_tracks(assets: &mut Assets<AudioSource>) -> Vec<Handle<AudioSource>> {
    let Some(dir) = crate::game::config_dir().map(|d| d.join("music")) else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    // On trie par nom : la lecture est ensuite round-robin, donc sans tri
    // l'utilisateur entendrait ses morceaux dans le désordre à chaque
    // lancement (l'ordre de `read_dir` n'est pas garanti).
    let mut files: Vec<std::path::PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| matches!(x.to_lowercase().as_str(), "wav" | "ogg"))
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for p in files {
        if let Ok(bytes) = std::fs::read(&p) {
            out.push(assets.add(AudioSource { bytes: bytes.into() }));
        }
    }
    out
}

pub fn setup(mut commands: Commands, mut assets: ResMut<Assets<AudioSource>>) {
    let bundled = TRACKS
        .iter()
        .map(|bytes| assets.add(AudioSource { bytes: (*bytes).to_vec().into() }))
        .collect();
    commands.insert_resource(Music {
        bundled,
        user: user_tracks(&mut assets),
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
    mut sinks: Query<&mut AudioSink, With<MusicPlayer>>,
    alive: Query<Entity, With<MusicPlayer>>,
) {
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
    // Bascule menu → jeu (ou l'inverse) : on change de morceau.
    if music.current.is_some() && music.playing_for != wants {
        if let Some(e) = music.current.take() {
            commands.entity(e).despawn();
        }
    }
    if music.current.is_some() {
        return;
    }
    music.counter += 1;
    let handle = if !music.user.is_empty() {
        music.user[music.counter % music.user.len()].clone()
    } else if music.bundled.len() == TRACKS.len() {
        // Au menu on fait tourner les trois morceaux en boucle ; en jeu on
        // alterne calme et action (une sur trois est tendue).
        let idx = if wants == 0 {
            (music.counter - 1) % music.bundled.len()
        } else if music.counter % 3 == 0 {
            2
        } else {
            1
        };
        music.bundled[idx].clone()
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
