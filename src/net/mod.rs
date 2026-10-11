//! Réseau : client et serveur authoritatif (anti-triche léger).
//!
//! Le serveur ne rejoue pas toute la physique : il vérifie la *cohérence* des
//! actions (portée, cadence, destruction plausible) puis diffuse les voxels
//! réellement détruits. Le client prédit localement pour rester réactif.

pub mod client;
pub mod protocol;
pub mod server;

pub use protocol::DEFAULT_PORT;