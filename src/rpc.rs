//! Discord Rich Presence : montre dans Discord ce que fait le joueur.
//!
//! - menus : « Dans les menus » ;
//! - solo : mode et nombre de voxels détruits ;
//! - multijoueur : nom du serveur, joueurs et mode.
//!
//! La connexion à Discord (si l'application est lancée) se fait dans un fil
//! séparé, avec reconnexion automatique ; sans Discord, rien ne se passe.

use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::game::{GameState, Mode, Phase};
use crate::net::client::Lobby;
use crate::world::VoxelWorld;

/// Identifiant de l'application Discord.
const APP_ID: &str = "1559210621566713896";
/// Clé de l'image dans les ressources de l'application Discord.
const LOGO: &str = "bg_logo";

#[derive(Clone, PartialEq, Debug, Default)]
struct Presence {
    details: String,
    state: String,
    /// Début de session (secondes Unix).
    start: i64,
    /// Joueurs / capacité (multijoueur).
    party: Option<(i32, i32)>,
}

#[derive(Resource)]
pub struct Rpc {
    tx: mpsc::Sender<Presence>,
    last: Presence,
    timer: f32,
    session_start: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn setup(mut commands: Commands) {
    let (tx, rx) = mpsc::channel::<Presence>();
    let _ = std::thread::Builder::new().name("boomground-discord".into()).spawn(move || {
        let mut client: Option<DiscordIpcClient> = None;
        let mut wanted = Presence::default();
        let mut sent = Presence::default();
        let mut have_wanted = false;
        loop {
            // Dernière présence demandée (on vide la file).
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(p) => {
                    wanted = p;
                    have_wanted = true;
                    while let Ok(p) = rx.try_recv() {
                        wanted = p;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if !have_wanted {
                continue;
            }
            if client.is_none() {
                if let Ok(mut c) = DiscordIpcClient::new(APP_ID) {
                    if c.connect().is_ok() {
                        client = Some(c);
                        sent = Presence::default();
                    }
                }
            }
            let Some(c) = client.as_mut() else { continue };
            if sent == wanted {
                continue;
            }
            let mut act = activity::Activity::new()
                .details(&wanted.details)
                .assets(
                    activity::Assets::new()
                        .large_image(LOGO)
                        .large_text(concat!("BoomGround v", env!("CARGO_PKG_VERSION"))),
                )
                .timestamps(activity::Timestamps::new().start(wanted.start));
            if !wanted.state.is_empty() {
                act = act.state(&wanted.state);
            }
            if let Some((n, max)) = wanted.party {
                act = act.party(activity::Party::new().size([n, max]));
            }
            if c.set_activity(act).is_ok() {
                sent = wanted.clone();
            } else {
                // Discord a été fermé : on retentera.
                client = None;
            }
        }
    });
    commands.insert_resource(Rpc {
        tx,
        last: Presence::default(),
        timer: 0.0,
        session_start: now_secs(),
    });
}

fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

pub fn update(
    time: Res<Time>,
    mut rpc: ResMut<Rpc>,
    game: Res<GameState>,
    lobby: Res<Lobby>,
    world: Res<VoxelWorld>,
    mut session: Local<Option<(bool, i64)>>,
) {
    rpc.timer -= time.delta_secs();
    if rpc.timer > 0.0 {
        return;
    }
    rpc.timer = 4.0;
    let mode = match game.mode {
        Mode::Sandbox => "Bac à sable",
        Mode::Normal => "Mode normal",
    };
    let in_match = game.started && matches!(game.phase, Phase::Playing | Phase::Menu);
    // L'horloge repart à zéro à chaque nouvelle partie.
    match *session {
        Some((was, _)) if was == in_match => {}
        _ => *session = Some((in_match, now_secs())),
    }
    let start = if in_match { session.map(|s| s.1).unwrap_or(rpc.session_start) } else { rpc.session_start };

    let presence = if game.phase == Phase::Install {
        Presence {
            details: "Installation".into(),
            start,
            ..default()
        }
    } else if !in_match {
        Presence {
            details: "Dans les menus".into(),
            state: if game.phase == Phase::Lobby { "Rejoint une partie".into() } else { String::new() },
            start,
            ..default()
        }
    } else if lobby.solo || !lobby.active {
        let destroyed = world.total.saturating_sub(world.alive);
        let mut state = format!("{} voxels détruits", thousands(destroyed));
        if game.mode == Mode::Normal && game.earned > 0 {
            state.push_str(&format!(" · {} $ gagnés", thousands(game.earned)));
        }
        Presence {
            details: format!("Solo — {mode}"),
            state,
            start,
            party: None,
        }
    } else {
        let name = if lobby.server_name.is_empty() { "Multijoueur".to_string() } else { lobby.server_name.clone() };
        let n = lobby.players.len().max(1) as i32;
        let max = lobby.max_players.max(n as u8) as i32;
        Presence {
            details: name,
            state: format!("{mode} · {n}/{max} joueurs"),
            start,
            party: Some((n, max)),
        }
    };
    if presence != rpc.last {
        let _ = rpc.tx.send(presence.clone());
        rpc.last = presence;
    }
}
