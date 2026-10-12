//! Liste de serveurs enregistrés (comme Minecraft) : nom + adresse ou code,
//! avec joueurs, mode, message du jour et ping interrogés en arrière-plan.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::net::protocol::{read_into, resolve_target, split_address, FrameReader, Packet};

/// Réponse d'un serveur.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub name: String,
    pub players: u8,
    pub max: u8,
    pub mode: u8,
    pub version: u16,
    pub motd: String,
    pub ping_ms: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Unknown,
    Checking,
    Online(Info),
    Offline,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub address: String,
    pub state: State,
}

#[derive(Resource)]
pub struct ServerList {
    pub entries: Vec<Entry>,
    rx: Mutex<mpsc::Receiver<(String, State)>>,
    tx: mpsc::Sender<(String, State)>,
    last_refresh: Option<Instant>,
    /// Incrémenté à chaque changement (le menu reconstruit la liste).
    pub revision: u32,
}

impl Default for ServerList {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut list = Self {
            entries: Vec::new(),
            rx: Mutex::new(rx),
            tx,
            last_refresh: None,
            revision: 0,
        };
        list.load();
        list
    }
}

fn file() -> Option<std::path::PathBuf> {
    crate::game::config_dir().map(|d| d.join("servers.txt"))
}

impl ServerList {
    fn load(&mut self) {
        let Some(path) = file() else { return };
        let Ok(text) = std::fs::read_to_string(path) else { return };
        for line in text.lines() {
            if let Some((name, address)) = line.split_once('|') {
                let (name, address) = (name.trim(), address.trim());
                if !address.is_empty() {
                    self.entries.push(Entry {
                        name: if name.is_empty() { address.to_string() } else { name.to_string() },
                        address: address.to_string(),
                        state: State::Unknown,
                    });
                }
            }
        }
    }

    fn save(&self) {
        let Some(path) = file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let body: String = self
            .entries
            .iter()
            .map(|e| format!("{}|{}\n", e.name.replace('|', " "), e.address.replace('|', " ")))
            .collect();
        let _ = std::fs::write(path, body);
    }

    pub fn add(&mut self, name: &str, address: &str) -> bool {
        let address = address.trim();
        if address.is_empty() || self.entries.iter().any(|e| e.address.eq_ignore_ascii_case(address)) {
            return false;
        }
        let name = name.trim();
        self.entries.push(Entry {
            name: if name.is_empty() { address.to_string() } else { name.to_string() },
            address: address.to_string(),
            state: State::Unknown,
        });
        self.save();
        self.revision += 1;
        self.last_refresh = None;
        true
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.save();
            self.revision += 1;
        }
    }

    /// Interroge tous les serveurs (un fil par entrée).
    pub fn refresh(&mut self) {
        self.last_refresh = Some(Instant::now());
        for e in &mut self.entries {
            if e.state != State::Checking {
                e.state = State::Checking;
            }
            let address = e.address.clone();
            let tx = self.tx.clone();
            let _ = std::thread::Builder::new().name("boomground-ping".into()).spawn(move || {
                let state = match query(&address) {
                    Some(info) => State::Online(info),
                    None => State::Offline,
                };
                let _ = tx.send((address, state));
            });
        }
        self.revision += 1;
    }

    /// Reçoit les réponses ; relance l'interrogation toutes les 8 s si
    /// `active`.
    pub fn poll(&mut self, active: bool) {
        let mut changed = false;
        if let Ok(rx) = self.rx.lock() {
            while let Ok((address, state)) = rx.try_recv() {
                for e in self.entries.iter_mut().filter(|e| e.address == address) {
                    if e.state != state {
                        e.state = state.clone();
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.revision += 1;
        }
        if active && self.last_refresh.is_none_or(|t| t.elapsed() > Duration::from_secs(8)) {
            self.refresh();
        }
    }
}

/// Une requête de statut : connexion, `Status`, lecture de `StatusInfo`.
fn query(address: &str) -> Option<Info> {
    let target = resolve_target(address);
    let (host, port) = split_address(&target);
    let target = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let addr = target.to_socket_addrs().ok()?.next()?;
    let started = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(2200)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(2200))).ok()?;
    stream.set_nodelay(true).ok();
    stream.write_all(&Packet::Status.encode()).ok()?;
    let mut reader = FrameReader::default();
    let deadline = Instant::now() + Duration::from_millis(2500);
    while Instant::now() < deadline {
        match read_into(&mut stream, &mut reader) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(_) => return None,
        }
        while let Some(frame) = reader.next() {
            if let Some(Packet::StatusInfo {
                name,
                players,
                max,
                mode,
                version,
                motd,
            }) = Packet::decode(&frame)
            {
                return Some(Info {
                    name,
                    players,
                    max,
                    mode,
                    version,
                    motd,
                    ping_ms: started.elapsed().as_millis().min(9999) as u32,
                });
            }
        }
    }
    None
}
