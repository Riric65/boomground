//! État global de la partie : phase de jeu, arme en main, munitions.

use bevy::prelude::*;

/// Phase courante.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Menu principal (solo / multijoueur).
    #[default]
    Menu,
    /// En jeu.
    Playing,
}

/// Arme sélectionnée.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Weapon {
    #[default]
    Sledge,
    Glock,
    C4,
}

impl Weapon {
    pub fn next(self) -> Self {
        match self {
            Weapon::Sledge => Weapon::Glock,
            Weapon::Glock => Weapon::C4,
            Weapon::C4 => Weapon::Sledge,
        }
    }
}

#[derive(Resource)]
pub struct GameState {
    pub phase: Phase,
    pub weapon: Weapon,
    /// Balles restantes dans le chargeur du Glock.
    pub ammo: u32,
    /// Nombre de C4 en réserve.
    pub c4: u32,
    /// Temps restant sur la C4 posée (aucune = pas amorcée).
    pub c4_timer: Option<f32>,
    /// Vrai si le joueur a retiré le pied de la C4 (on l'arme).
    pub c4_armed: bool,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            phase: Phase::Menu,
            weapon: Weapon::Sledge,
            ammo: GLOCK_MAG,
            c4: STARTING_C4,
            c4_timer: None,
            c4_armed: false,
        }
    }
}

impl GameState {
    pub fn in_game(&self) -> bool {
        self.phase == Phase::Playing
    }

    /// Remet le joueur en pleine forme (solo ou début de partie).
    pub fn resupply(&mut self) {
        self.ammo = GLOCK_MAG;
        self.c4 = STARTING_C4;
    }
}

/// Réglages du joueur, modifiables depuis le menu.
#[derive(Resource)]
pub struct Settings {
    /// Sensibilité de la souris (radian par pixel).
    pub sensitivity: f32,
    /// Volume général (0 = silencieux).
    pub volume: f32,
    /// Limite d'images (0 = celle de l'écran).
    pub fps_limit: f32,
    /// Inversion de l'axe vertical.
    pub invert_y: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sensitivity: 0.0022,
            volume: 0.8,
            fps_limit: 0.0,
            invert_y: false,
        }
    }
}

impl Settings {
    pub fn clamp(&mut self) {
        self.sensitivity = self.sensitivity.clamp(0.0005, 0.01);
        self.volume = self.volume.clamp(0.0, 1.0);
        if self.fps_limit > 0.0 {
            self.fps_limit = self.fps_limit.clamp(30.0, 360.0);
        }
    }
}

/// Condition de système : on ne joue pas tant qu'on est dans le menu.
pub fn in_game(state: Res<GameState>) -> bool {
    state.phase == Phase::Playing
}

/// Capacité du chargeur du Glock 17.
pub const GLOCK_MAG: u32 = 17;
/// C4 données au démarrage d'une partie.
pub const STARTING_C4: u32 = 3;