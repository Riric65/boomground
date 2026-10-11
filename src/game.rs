//! État global de la partie : mode, phase, arsenal, argent, seed.

use bevy::prelude::*;

/// Phase courante.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Premier lancement : écran d'installation.
    Install,
    /// Menu principal (ou pause).
    #[default]
    Menu,
    /// Lobby multijoueur : on attend que l'hôte lance la partie.
    Lobby,
    /// En jeu.
    Playing,
}

/// Mode de jeu.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Tout est débloqué, munitions et bombes infinies.
    #[default]
    Sandbox,
    /// On commence avec un marteau : chaque voxel détruit rapporte de
    /// l'argent, qui sert à acheter armes, marteaux et bombes.
    Normal,
}

/// Famille d'arme : décide de l'animation et de la façon d'agir.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Melee,
    Gun,
    /// Lancée à la main (rebondit ou colle).
    Thrown,
    /// Posée sur une surface.
    Placed,
}

/// Toutes les armes du jeu, dans l'ordre de la barre d'armes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum WeaponId {
    #[default]
    Hammer,
    Sledge,
    WarHammer,
    Jackhammer,
    Glock,
    Shotgun,
    Rifle,
    Sniper,
    Rocket,
    Grenade,
    Dynamite,
    C4,
    MegaBomb,
}

pub const ALL_WEAPONS: [WeaponId; 13] = [
    WeaponId::Hammer,
    WeaponId::Sledge,
    WeaponId::WarHammer,
    WeaponId::Jackhammer,
    WeaponId::Glock,
    WeaponId::Shotgun,
    WeaponId::Rifle,
    WeaponId::Sniper,
    WeaponId::Rocket,
    WeaponId::Grenade,
    WeaponId::Dynamite,
    WeaponId::C4,
    WeaponId::MegaBomb,
];

/// Caractéristiques d'une arme.
pub struct Spec {
    pub name: &'static str,
    pub kind: Kind,
    /// Prix d'achat (pour une bombe : prix de l'unité).
    pub price: u32,
    /// Rayon de destruction, en mètres.
    pub radius: f32,
    /// Portée (corps à corps, tir, pose).
    pub reach: f32,
    /// Temps minimal entre deux actions.
    pub cooldown: f32,
    /// Tir continu tant que le bouton est enfoncé.
    pub auto: bool,
    /// Chargeur (armes à feu).
    pub mag: u32,
    /// Plombs par tir (fusil à pompe).
    pub pellets: u32,
    /// Dispersion (radians).
    pub spread: f32,
    /// Voxels traversés par une balle (sniper).
    pub pierce: u32,
    /// Durée de recharge.
    pub reload: f32,
    /// Délai avant explosion (bombes).
    pub fuse: f32,
    /// Prix d'un paquet de munitions, et sa taille.
    pub ammo_price: u32,
    pub ammo_pack: u32,
    /// Munitions de réserve données à l'achat.
    pub starting_reserve: u32,
}

const BASE: Spec = Spec {
    name: "",
    kind: Kind::Melee,
    price: 0,
    radius: 0.5,
    reach: 4.0,
    cooldown: 0.4,
    auto: false,
    mag: 0,
    pellets: 1,
    spread: 0.0,
    pierce: 0,
    reload: 1.0,
    fuse: 0.0,
    ammo_price: 0,
    ammo_pack: 0,
    starting_reserve: 0,
};

impl WeaponId {
    pub fn spec(self) -> &'static Spec {
        use WeaponId::*;
        match self {
            Hammer => &Spec {
                name: "Marteau",
                radius: 0.55,
                reach: 3.5,
                cooldown: 0.30,
                ..BASE
            },
            Sledge => &Spec {
                name: "Masse",
                price: 150,
                radius: 0.9,
                reach: 4.5,
                cooldown: 0.42,
                ..BASE
            },
            WarHammer => &Spec {
                name: "Masse de guerre",
                price: 1_200,
                radius: 1.4,
                reach: 4.8,
                cooldown: 0.70,
                ..BASE
            },
            Jackhammer => &Spec {
                name: "Marteau-piqueur",
                price: 2_500,
                radius: 0.6,
                reach: 3.0,
                cooldown: 0.07,
                auto: true,
                ..BASE
            },
            Glock => &Spec {
                name: "Glock 17",
                kind: Kind::Gun,
                price: 400,
                radius: 0.34,
                reach: 80.0,
                cooldown: 0.12,
                mag: 17,
                reload: 1.1,
                ammo_price: 20,
                ammo_pack: 34,
                starting_reserve: 51,
                ..BASE
            },
            Shotgun => &Spec {
                name: "Fusil à pompe",
                kind: Kind::Gun,
                price: 1_200,
                radius: 0.32,
                reach: 30.0,
                cooldown: 0.75,
                mag: 6,
                pellets: 9,
                spread: 0.07,
                reload: 1.6,
                ammo_price: 40,
                ammo_pack: 12,
                starting_reserve: 18,
                ..BASE
            },
            Rifle => &Spec {
                name: "Fusil d'assaut",
                kind: Kind::Gun,
                price: 3_000,
                radius: 0.36,
                reach: 100.0,
                cooldown: 0.09,
                auto: true,
                mag: 30,
                spread: 0.012,
                reload: 1.5,
                ammo_price: 60,
                ammo_pack: 60,
                starting_reserve: 90,
                ..BASE
            },
            Sniper => &Spec {
                name: "Sniper .50",
                kind: Kind::Gun,
                price: 4_500,
                radius: 0.5,
                reach: 160.0,
                cooldown: 1.0,
                mag: 5,
                pierce: 14,
                reload: 2.0,
                ammo_price: 80,
                ammo_pack: 10,
                starting_reserve: 10,
                ..BASE
            },
            Rocket => &Spec {
                name: "Lance-roquettes",
                kind: Kind::Gun,
                price: 8_000,
                radius: 2.8,
                reach: 160.0,
                cooldown: 0.4,
                mag: 1,
                reload: 1.8,
                ammo_price: 150,
                ammo_pack: 3,
                starting_reserve: 4,
                ..BASE
            },
            Grenade => &Spec {
                name: "Grenade",
                kind: Kind::Thrown,
                price: 40,
                radius: 2.0,
                reach: 16.0,
                cooldown: 0.6,
                fuse: 2.5,
                ..BASE
            },
            Dynamite => &Spec {
                name: "Dynamite",
                kind: Kind::Thrown,
                price: 90,
                radius: 2.8,
                reach: 14.0,
                cooldown: 0.6,
                fuse: 3.5,
                ..BASE
            },
            C4 => &Spec {
                name: "C4",
                kind: Kind::Placed,
                price: 200,
                radius: 3.4,
                reach: 6.0,
                cooldown: 0.3,
                ..BASE
            },
            MegaBomb => &Spec {
                name: "Méga-bombe",
                kind: Kind::Placed,
                price: 2_500,
                radius: 7.0,
                reach: 6.0,
                cooldown: 1.0,
                fuse: 8.0,
                ..BASE
            },
        }
    }

    pub fn kind(self) -> Kind {
        self.spec().kind
    }

    pub fn is_bomb(self) -> bool {
        matches!(self.kind(), Kind::Thrown | Kind::Placed)
    }

    /// Identifiant réseau.
    pub fn tag(self) -> u8 {
        ALL_WEAPONS.iter().position(|&w| w == self).unwrap_or(0) as u8
    }

    pub fn from_tag(tag: u8) -> Self {
        ALL_WEAPONS.get(tag as usize).copied().unwrap_or_default()
    }
}

/// Ce que le joueur possède pour une arme.
#[derive(Clone, Copy, Default)]
pub struct Slot {
    pub owned: bool,
    /// Balles dans le chargeur (armes à feu).
    pub mag: u32,
    /// Munitions de réserve (armes à feu) ou nombre de bombes.
    pub reserve: u32,
}

#[derive(Resource)]
pub struct GameState {
    pub phase: Phase,
    pub mode: Mode,
    pub weapon: WeaponId,
    pub slots: [Slot; ALL_WEAPONS.len()],
    pub money: u32,
    /// Total gagné depuis le début de la partie.
    pub earned: u32,
    /// Seed de la carte (0 = à tirer au hasard).
    pub seed: u64,
    /// Une partie a déjà été lancée (le menu propose « reprendre »).
    pub started: bool,
}

impl Default for GameState {
    fn default() -> Self {
        let mut s = Self {
            phase: Phase::Menu,
            mode: Mode::Sandbox,
            weapon: WeaponId::Hammer,
            slots: [Slot::default(); ALL_WEAPONS.len()],
            money: 0,
            earned: 0,
            seed: 0,
            started: false,
        };
        s.reset_inventory();
        s
    }
}

impl GameState {
    pub fn in_game(&self) -> bool {
        self.phase == Phase::Playing
    }

    pub fn sandbox(&self) -> bool {
        self.mode == Mode::Sandbox
    }

    pub fn slot(&self, id: WeaponId) -> &Slot {
        &self.slots[id.tag() as usize]
    }

    pub fn slot_mut(&mut self, id: WeaponId) -> &mut Slot {
        &mut self.slots[id.tag() as usize]
    }

    /// Armes utilisables, dans l'ordre de la barre.
    pub fn owned(&self) -> Vec<WeaponId> {
        ALL_WEAPONS
            .iter()
            .copied()
            .filter(|&w| self.slot(w).owned)
            .collect()
    }

    /// Inventaire de départ selon le mode.
    pub fn reset_inventory(&mut self) {
        self.money = 0;
        self.earned = 0;
        for &id in &ALL_WEAPONS {
            let spec = id.spec();
            let owned = self.sandbox() || id == WeaponId::Hammer;
            *self.slot_mut(id) = Slot {
                owned,
                mag: if owned { spec.mag } else { 0 },
                reserve: if owned && spec.kind == Kind::Gun {
                    spec.starting_reserve
                } else if owned && id.is_bomb() {
                    5
                } else {
                    0
                },
            };
        }
        self.weapon = WeaponId::Hammer;
    }

    /// Une bombe est-elle disponible ? (infini en bac à sable)
    pub fn has_bomb(&self, id: WeaponId) -> bool {
        self.sandbox() || self.slot(id).reserve > 0
    }

    /// Consomme une bombe.
    pub fn use_bomb(&mut self, id: WeaponId) -> bool {
        if self.sandbox() {
            return true;
        }
        let slot = self.slot_mut(id);
        if slot.reserve == 0 {
            return false;
        }
        slot.reserve -= 1;
        true
    }

    /// Ajoute l'argent rapporté par une destruction (mode normal).
    pub fn reward(&mut self, value: u32) {
        if self.mode == Mode::Normal {
            self.money += value;
            self.earned += value;
        }
    }

    /// Achat d'une arme (ou d'une bombe). Renvoie un message d'erreur.
    pub fn buy(&mut self, id: WeaponId) -> Result<(), &'static str> {
        let spec = id.spec();
        if !id.is_bomb() && self.slot(id).owned {
            return Err("déjà possédé");
        }
        if self.money < spec.price {
            return Err("pas assez d'argent");
        }
        self.money -= spec.price;
        let slot = self.slot_mut(id);
        if id.is_bomb() {
            slot.owned = true;
            slot.reserve += 1;
        } else {
            slot.owned = true;
            slot.mag = spec.mag;
            slot.reserve = spec.starting_reserve;
        }
        Ok(())
    }

    /// Achat d'un paquet de munitions.
    pub fn buy_ammo(&mut self, id: WeaponId) -> Result<(), &'static str> {
        let spec = id.spec();
        if spec.kind != Kind::Gun {
            return Err("pas de munitions");
        }
        if !self.slot(id).owned {
            return Err("achetez d'abord l'arme");
        }
        if self.money < spec.ammo_price {
            return Err("pas assez d'argent");
        }
        self.money -= spec.ammo_price;
        self.slot_mut(id).reserve += spec.ammo_pack;
        Ok(())
    }
}

/// Réglages du joueur, modifiables depuis le menu.
#[derive(Resource)]
pub struct Settings {
    /// Pseudo affiché en multijoueur.
    pub name: String,
    /// Sensibilité de la souris (radian par pixel).
    pub sensitivity: f32,
    /// Volume général (0 = silencieux).
    pub volume: f32,
    /// Limite d'images (0 = celle de l'écran).
    pub fps_limit: f32,
    /// Inversion de l'axe vertical.
    pub invert_y: bool,
    /// Plein écran (fenêtre sans bordure).
    pub fullscreen: bool,
    /// Champ de vision (degrés).
    pub fov: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: default_name(),
            sensitivity: 0.0022,
            volume: 0.8,
            fps_limit: 0.0,
            invert_y: false,
            fullscreen: false,
            fov: 75.0,
        }
    }
}

/// Pseudo par défaut : le nom de la session, sinon « Joueur ».
fn default_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .map(|n| n.chars().take(16).collect::<String>())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Joueur".to_string())
}

impl Settings {
    pub fn clamp(&mut self) {
        self.sensitivity = self.sensitivity.clamp(0.0005, 0.01);
        self.volume = self.volume.clamp(0.0, 1.0);
        if self.fps_limit > 0.0 {
            self.fps_limit = self.fps_limit.clamp(30.0, 360.0);
        }
        self.fov = self.fov.clamp(60.0, 110.0);
        self.name = self.name.chars().filter(|c| !c.is_control()).take(16).collect();
        if self.name.trim().is_empty() {
            self.name = default_name();
        }
    }
}

/// Dossier de configuration selon le système (Windows, macOS, Linux).
pub fn config_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        env("APPDATA").map(|p| p.join("BoomGround"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support/BoomGround"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|p| p.join(".config")))
            .map(|p| p.join("boomground"))
    }
}

/// Enregistre les réglages (simple fichier `clé=valeur`).
pub fn save_settings(s: &Settings) {
    let Some(dir) = config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let body = format!(
        "name={}
sensitivity={}
volume={}
fps_limit={}
invert_y={}
fullscreen={}
fov={}
",
        s.name, s.sensitivity, s.volume, s.fps_limit, s.invert_y, s.fullscreen, s.fov
    );
    let _ = std::fs::write(dir.join("settings.txt"), body);
}

/// Relit les réglages enregistrés (valeurs par défaut sinon).
pub fn load_settings() -> Settings {
    let mut s = Settings::default();
    let Some(path) = config_dir().map(|d| d.join("settings.txt")) else {
        return s;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return s;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "name" => s.name = v.to_string(),
            "sensitivity" => s.sensitivity = v.parse().unwrap_or(s.sensitivity),
            "volume" => s.volume = v.parse().unwrap_or(s.volume),
            "fps_limit" => s.fps_limit = v.parse().unwrap_or(s.fps_limit),
            "invert_y" => s.invert_y = v == "true",
            "fullscreen" => s.fullscreen = v == "true",
            "fov" => s.fov = v.parse().unwrap_or(s.fov),
            _ => {}
        }
    }
    s.clamp();
    s
}

/// Condition de système : on ne joue pas tant qu'on est dans le menu.
pub fn in_game(state: Res<GameState>) -> bool {
    state.phase == Phase::Playing
}

/// Seed aléatoire courte (lisible et facile à partager).
pub fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    100_000 + x % 900_000
}

/// Transforme un texte saisi en seed : un nombre est pris tel quel, un mot
/// est haché (« paris » donne toujours la même ville).
pub fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(n) = text.parse::<u64>() {
        return Some(n.max(1));
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(h % 1_000_000_000 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_mode_starts_with_hammer_only() {
        let mut g = GameState {
            mode: Mode::Normal,
            ..default()
        };
        g.reset_inventory();
        assert_eq!(g.owned(), vec![WeaponId::Hammer]);
        assert!(g.buy(WeaponId::Sledge).is_err());
        g.money = 1000;
        assert!(g.buy(WeaponId::Sledge).is_ok());
        assert_eq!(g.money, 850);
        assert!(g.buy(WeaponId::Sledge).is_err(), "pas deux fois");
        assert!(g.buy(WeaponId::C4).is_ok());
        assert!(g.buy(WeaponId::C4).is_ok());
        assert_eq!(g.slot(WeaponId::C4).reserve, 2);
    }

    #[test]
    fn sandbox_has_everything() {
        let g = GameState::default();
        assert_eq!(g.owned().len(), ALL_WEAPONS.len());
        assert!(g.has_bomb(WeaponId::MegaBomb));
    }

    #[test]
    fn seeds_parse() {
        assert_eq!(parse_seed("42"), Some(42));
        assert_eq!(parse_seed("paris"), parse_seed("paris"));
        assert_eq!(parse_seed("   "), None);
        for &w in &ALL_WEAPONS {
            assert_eq!(WeaponId::from_tag(w.tag()), w);
        }
    }
}
