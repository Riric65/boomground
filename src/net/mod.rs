//! Réseau : lobby, client et serveur authoritatif (anti-triche léger).
//!
//! Le serveur ne rejoue pas toute la physique : il vérifie la *cohérence* des
//! actions (portée, cadence, destruction plausible) puis diffuse les voxels
//! réellement détruits. Le client prédit localement pour rester réactif.
//!
//! Créer un lobby lance le serveur en fond dans le jeu, l'annonce sur le
//! réseau local (`lan`) et ouvre le port de la box (`upnp`).

pub mod client;
pub mod conf;
pub mod console;
pub mod lan;
pub mod logger;
pub mod mirror;
pub mod protocol;
pub mod save;
pub mod server;
pub mod upnp;

pub use protocol::DEFAULT_PORT;
