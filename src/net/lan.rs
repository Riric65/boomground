//! Découverte des parties sur le réseau local (même box / même Wi-Fi).
//!
//! Le menu envoie régulièrement une question en diffusion UDP sur le port
//! [`DISCOVERY_PORT`] ; chaque lobby ouvert répond avec son nom, son nombre
//! de joueurs et son port. Les parties apparaissent toutes seules dans la
//! liste « Parties sur ton réseau », sans rien saisir.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use super::protocol::{DISCOVERY_PORT, PROTOCOL_VERSION};

const QUERY: &str = "BOOMGROUND?";
const ANSWER: &str = "BOOMGROUND!";

/// Description d'un lobby envoyée en réponse.
#[derive(Clone, Debug, PartialEq)]
pub struct LobbyInfo {
    pub name: String,
    pub port: u16,
    pub players: u8,
    pub max: u8,
    pub mode: u8,
    pub started: bool,
}

impl LobbyInfo {
    fn encode(&self) -> String {
        format!(
            "{ANSWER}|{PROTOCOL_VERSION}|{}|{}|{}|{}|{}|{}",
            self.port, self.players, self.max, self.mode, self.started as u8, self.name
        )
    }

    fn decode(text: &str) -> Option<Self> {
        let mut parts = text.splitn(8, '|');
        if parts.next()? != ANSWER {
            return None;
        }
        if parts.next()?.parse::<u16>().ok()? != PROTOCOL_VERSION {
            return None;
        }
        Some(Self {
            port: parts.next()?.parse().ok()?,
            players: parts.next()?.parse().ok()?,
            max: parts.next()?.parse().ok()?,
            mode: parts.next()?.parse().ok()?,
            started: parts.next()? == "1",
            name: parts.next()?.chars().take(40).collect(),
        })
    }
}

/// Côté serveur : répond aux questions des menus.
pub struct Responder {
    socket: Option<UdpSocket>,
}

impl Responder {
    pub fn new() -> Self {
        let socket = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).ok().and_then(|s| {
            s.set_nonblocking(true).ok()?;
            Some(s)
        });
        Self { socket }
    }

    pub fn poll(&self, info: &LobbyInfo) {
        let Some(socket) = &self.socket else { return };
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            if &buf[..n] == QUERY.as_bytes() {
                let _ = socket.send_to(info.encode().as_bytes(), from);
            }
        }
    }
}

impl Default for Responder {
    fn default() -> Self {
        Self::new()
    }
}

/// Une partie trouvée sur le réseau local.
#[derive(Clone, Debug)]
pub struct LanGame {
    pub addr: SocketAddr,
    pub info: LobbyInfo,
    seen: Instant,
}

/// Côté menu : liste des parties visibles.
#[derive(Resource)]
pub struct LanBrowser {
    socket: Option<UdpSocket>,
    pub games: Vec<LanGame>,
    last_query: Option<Instant>,
}

impl Default for LanBrowser {
    fn default() -> Self {
        let socket = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| {
            s.set_nonblocking(true).ok()?;
            s.set_broadcast(true).ok()?;
            Some(s)
        });
        Self {
            socket,
            games: Vec::new(),
            last_query: None,
        }
    }
}

/// Adresse IPv4 locale principale (celle qui sort vers Internet ; aucun
/// paquet n'est réellement envoyé).
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

impl LanBrowser {
    /// Envoie une question toutes les 1,5 s et range les réponses.
    pub fn refresh(&mut self) {
        let Some(socket) = &self.socket else { return };
        let now = Instant::now();
        if self
            .last_query
            .is_none_or(|t| now.duration_since(t) > Duration::from_millis(1500))
        {
            self.last_query = Some(now);
            let mut targets: Vec<SocketAddr> = vec![
                (Ipv4Addr::BROADCAST, DISCOVERY_PORT).into(),
                (Ipv4Addr::LOCALHOST, DISCOVERY_PORT).into(),
            ];
            // Diffusion du sous-réseau (Windows n'envoie 255.255.255.255
            // que sur une seule carte réseau).
            if let Some(ip) = local_ipv4() {
                let o = ip.octets();
                targets.push((Ipv4Addr::new(o[0], o[1], o[2], 255), DISCOVERY_PORT).into());
            }
            for target in targets {
                let _ = socket.send_to(QUERY.as_bytes(), target);
            }
        }

        let mut buf = [0u8; 512];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            let Some(info) = LobbyInfo::decode(&String::from_utf8_lossy(&buf[..n])) else {
                continue;
            };
            let addr = SocketAddr::new(from.ip(), info.port);
            // La même partie répond par plusieurs chemins (diffusion et
            // 127.0.0.1) : une seule ligne par lobby, et on garde l'adresse
            // réseau plutôt que 127.0.0.1.
            let same = |g: &LanGame| g.info.port == info.port && g.info.name == info.name;
            if let Some(game) = self.games.iter_mut().find(|g| same(g)) {
                game.info = info;
                game.seen = now;
                if !from.ip().is_loopback() {
                    game.addr = addr;
                }
            } else {
                self.games.push(LanGame {
                    addr,
                    info,
                    seen: now,
                });
            }
        }
        self.games
            .retain(|g| now.duration_since(g.seen) < Duration::from_secs(5));
        self.games.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_info_round_trip() {
        let info = LobbyInfo {
            name: "Partie de test | test".into(),
            port: 28431,
            players: 3,
            max: 16,
            mode: 1,
            started: true,
        };
        assert_eq!(LobbyInfo::decode(&info.encode()), Some(info));
        assert_eq!(LobbyInfo::decode("autre chose"), None);
    }

    #[test]
    fn browser_finds_local_responder() {
        let responder = Responder {
            socket: UdpSocket::bind("127.0.0.1:0").ok(),
        };
        let port = responder.socket.as_ref().unwrap().local_addr().unwrap().port();
        responder.socket.as_ref().unwrap().set_nonblocking(true).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        client.send_to(QUERY.as_bytes(), ("127.0.0.1", port)).unwrap();
        let info = LobbyInfo {
            name: "x".into(),
            port: 1,
            players: 1,
            max: 16,
            mode: 0,
            started: false,
        };
        std::thread::sleep(Duration::from_millis(50));
        responder.poll(&info);
        let mut buf = [0u8; 256];
        let (n, _) = client.recv_from(&mut buf).unwrap();
        assert_eq!(LobbyInfo::decode(&String::from_utf8_lossy(&buf[..n])), Some(info));
    }
}
