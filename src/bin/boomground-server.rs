//! Serveur dédié BoomGround : sans fenêtre, configurable par `server.conf`
//! (créé à côté de l'exécutable au premier lancement).

fn main() {
    boomground::net::server::run_dedicated();
}
