//! Voix de proximité (provisoire : file d'entrée).

use bevy::prelude::*;

/// Trames de voix reçues, à jouer.
#[derive(Resource, Default)]
pub struct VoiceInbox {
    frames: Vec<(u16, Vec<u8>)>,
}

impl VoiceInbox {
    pub fn push(&mut self, item: (u16, Vec<u8>)) {
        self.frames.push(item);
    }
}
