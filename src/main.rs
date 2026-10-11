//! Point d'entrée du client.

// Pas de console à côté de la fenêtre du jeu sous Windows (en version
// publiée) : on double-clique, le jeu s'ouvre, c'est tout.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    boomground::run();
}
