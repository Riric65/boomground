//! Serveur BoomGround.
//!
//! ```bash
//! cargo run --bin boomground-server          # port 28431
//! BOOM_PORT=3000 cargo run --bin boomground-server
//! ```

fn main() {
    boomground::net::server::run();
}