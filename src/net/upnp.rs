//! Ouverture automatique du port sur la box (UPnP IGD), sans dépendance.
//!
//! Quand on crée un lobby, la box reçoit l'ordre de rediriger le port du
//! jeu vers ce PC : les joueurs sur Internet peuvent alors rejoindre avec le
//! code de partie, sans régler quoi que ce soit à la main. La redirection
//! est retirée à la fermeture du lobby.
//!
//! Étapes : découverte SSDP (multicast `239.255.255.250:1900`), lecture de
//! la description de la box (HTTP), puis appels SOAP `AddPortMapping`,
//! `GetExternalIPAddress` et `DeletePortMapping`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

/// Box trouvée sur le réseau.
#[derive(Clone, Debug)]
pub struct Gateway {
    /// `ip:port` du serveur HTTP de la box.
    host: String,
    /// Chemin de contrôle SOAP.
    control: String,
    service: String,
    /// Adresse de ce PC vue depuis la box.
    pub local_ip: Ipv4Addr,
    /// Nom de la box (« Bbox », « Freebox »…), pour les messages.
    pub name: String,
}

/// Cherche la box (environ 2 s par carte réseau au maximum).
///
/// La recherche part d'abord de la carte réseau principale : sur un PC qui
/// a aussi des cartes virtuelles (VPN, machines virtuelles…), Windows
/// envoie sinon le multicast par la mauvaise et la box ne répond jamais.
pub fn discover() -> Result<Gateway, String> {
    let mut binds: Vec<Ipv4Addr> = Vec::new();
    if let Some(ip) = crate::net::lan::local_ipv4() {
        binds.push(ip);
    }
    binds.push(Ipv4Addr::UNSPECIFIED);

    let mut locations: Vec<String> = Vec::new();
    for bind in binds {
        locations.extend(search(bind).unwrap_or_default());
        if !locations.is_empty() {
            break;
        }
    }
    if locations.is_empty() {
        return Err("aucune box UPnP trouvée sur le réseau".into());
    }
    for location in locations {
        if let Ok(gw) = gateway_from(&location) {
            return Ok(gw);
        }
    }
    Err("la box ne propose pas l'ouverture de port (UPnP)".into())
}

/// Envoie les requêtes SSDP depuis `bind` et rend les adresses de
/// description des passerelles qui répondent.
fn search(bind: Ipv4Addr) -> Result<Vec<String>, String> {
    let socket = UdpSocket::bind((bind, 0)).map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(300)))
        .map_err(|e| e.to_string())?;
    for st in [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
    ] {
        let msg = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {st}\r\n\r\n"
        );
        let _ = socket.send_to(msg.as_bytes(), "239.255.255.250:1900");
    }

    let deadline = Instant::now() + Duration::from_millis(2200);
    let mut locations: Vec<String> = Vec::new();
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        let Ok((n, _)) = socket.recv_from(&mut buf) else {
            if !locations.is_empty() {
                break;
            }
            continue;
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        let gateway = text.contains("InternetGatewayDevice") || text.contains("WANIPConnection");
        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if gateway && key.trim().eq_ignore_ascii_case("location") {
                    let url = value.trim().to_string();
                    if !locations.contains(&url) {
                        locations.push(url);
                    }
                }
            }
        }
    }
    Ok(locations)
}

/// `http://192.168.1.1:5000/desc.xml` → (`192.168.1.1:5000`, `/desc.xml`).
fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    Some((host, path.to_string()))
}

fn gateway_from(location: &str) -> Result<Gateway, String> {
    let (host, path) = split_url(location).ok_or("adresse de box invalide")?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    let body = http(&host, &request)?;
    let name = tag(&body, "friendlyName").unwrap_or_else(|| "ta box".into());

    for block in body.split("<service>").skip(1) {
        let Some(service) = tag(block, "serviceType") else { continue };
        if !SERVICES.contains(&service.as_str()) {
            continue;
        }
        let Some(control) = tag(block, "controlURL") else { continue };
        let control = if control.starts_with("http://") {
            split_url(&control).map(|(_, p)| p).unwrap_or(control)
        } else if control.starts_with('/') {
            control
        } else {
            format!("/{control}")
        };
        let local_ip = local_ip_towards(&host)?;
        return Ok(Gateway {
            host,
            control,
            service,
            local_ip,
            name,
        });
    }
    Err("service WANIPConnection introuvable".into())
}

/// Adresse locale utilisée pour joindre `host` (aucun paquet envoyé).
fn local_ip_towards(host: &str) -> Result<Ipv4Addr, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect(host).map_err(|e| e.to_string())?;
    match socket.local_addr().map_err(|e| e.to_string())? {
        SocketAddr::V4(a) => Ok(*a.ip()),
        SocketAddr::V6(_) => Err("pas d'adresse IPv4 locale".into()),
    }
}

/// Contenu d'une balise XML (première occurrence, sans espace de noms).
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

/// Requête HTTP brute (connexion fermée par la box à la fin).
fn http(host: &str, request: &str) -> Result<String, String> {
    let addr = host
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("adresse de box introuvable")?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    if response.is_empty() {
        return Err("la box n'a pas répondu".into());
    }
    Ok(String::from_utf8_lossy(&response).into_owned())
}

impl Gateway {
    fn soap(&self, action: &str, args: &str) -> Result<String, String> {
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
             <u:{action} xmlns:u=\"{service}\">{args}</u:{action}></s:Body></s:Envelope>\r\n",
            service = self.service
        );
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\n\
             SOAPAction: \"{service}#{action}\"\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            path = self.control,
            host = self.host,
            service = self.service,
            len = body.len()
        );
        let response = http(&self.host, &request)?;
        let status = response.lines().next().unwrap_or_default().to_string();
        if !status.contains(" 200") {
            let detail = tag(&response, "errorDescription")
                .or_else(|| tag(&response, "errorCode"))
                .unwrap_or(status);
            return Err(detail);
        }
        Ok(response)
    }

    /// Adresse publique de la box.
    pub fn external_ip(&self) -> Result<Ipv4Addr, String> {
        let response = self.soap("GetExternalIPAddress", "")?;
        tag(&response, "NewExternalIPAddress")
            .and_then(|ip| ip.parse().ok())
            .ok_or_else(|| "adresse publique inconnue".to_string())
    }

    /// Redirige `port` (TCP) de la box vers ce PC.
    pub fn open(&self, port: u16) -> Result<(), String> {
        let args = |lease: u32| {
            format!(
                "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort>\
                 <NewProtocol>TCP</NewProtocol><NewInternalPort>{port}</NewInternalPort>\
                 <NewInternalClient>{ip}</NewInternalClient><NewEnabled>1</NewEnabled>\
                 <NewPortMappingDescription>BoomGround</NewPortMappingDescription>\
                 <NewLeaseDuration>{lease}</NewLeaseDuration>",
                ip = self.local_ip
            )
        };
        // Bail de 24 h (la redirection disparaît même si le jeu plante) ;
        // certaines box n'acceptent que les baux permanents.
        self.soap("AddPortMapping", &args(86_400))
            .or_else(|_| self.soap("AddPortMapping", &args(0)))
            .map(|_| ())
            .map_err(|e| format!("{} refuse l'ouverture automatique ({e})", self.name))
    }

    /// Retire la redirection.
    pub fn close(&self, port: u16) {
        let _ = self.soap(
            "DeletePortMapping",
            &format!(
                "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort>\
                 <NewProtocol>TCP</NewProtocol>"
            ),
        );
    }
}

/// Adresse « publique » qui n'en est pas une : box derrière un autre NAT
/// (CGNAT de l'opérateur, box en cascade). Les joueurs d'Internet ne
/// pourront pas entrer, même avec le port ouvert.
pub fn is_shared_address(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || (o[0] == 100 && (o[1] & 0xC0) == 64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_tags() {
        assert_eq!(
            split_url("http://192.168.1.1:5000/rootDesc.xml"),
            Some(("192.168.1.1:5000".into(), "/rootDesc.xml".into()))
        );
        assert_eq!(split_url("http://box/"), Some(("box:80".into(), "/".into())));
        let xml = "<service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
                   <controlURL>/ctl/IPConn</controlURL></service>";
        assert_eq!(tag(xml, "controlURL").as_deref(), Some("/ctl/IPConn"));
    }

    #[test]
    fn shared_addresses() {
        assert!(is_shared_address(Ipv4Addr::new(100, 72, 3, 4)));
        assert!(is_shared_address(Ipv4Addr::new(192, 168, 0, 10)));
        assert!(!is_shared_address(Ipv4Addr::new(82, 64, 1, 2)));
    }
}
