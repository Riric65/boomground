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
    /// Outil de construction : pose des blocs.
    Build,
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
    Pickaxe,
    Revolver,
    Smg,
    Minigun,
    Railgun,
    Blocks,
}

pub const ALL_WEAPONS: [WeaponId; 19] = [
    WeaponId::Hammer,
    WeaponId::Sledge,
    WeaponId::WarHammer,
    WeaponId::Pickaxe,
    WeaponId::Jackhammer,
    WeaponId::Glock,
    WeaponId::Revolver,
    WeaponId::Smg,
    WeaponId::Shotgun,
    WeaponId::Rifle,
    WeaponId::Minigun,
    WeaponId::Sniper,
    WeaponId::Railgun,
    WeaponId::Rocket,
    WeaponId::Grenade,
    WeaponId::Dynamite,
    WeaponId::C4,
    WeaponId::MegaBomb,
    WeaponId::Blocks,
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
    /// Dégâts infligés à un joueur (par coup, balle ou plomb).
    pub damage: f32,
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
    damage: 0.0,
};

impl WeaponId {
    pub fn spec(self) -> &'static Spec {
        use WeaponId::*;
        match self {
            Hammer => &Spec {
                damage: 18.0,
                name: "Marteau",
                radius: 0.55,
                reach: 3.5,
                cooldown: 0.30,
                ..BASE
            },
            Sledge => &Spec {
                damage: 30.0,
                name: "Masse",
                price: 150,
                radius: 0.9,
                reach: 4.5,
                cooldown: 0.42,
                ..BASE
            },
            WarHammer => &Spec {
                damage: 55.0,
                name: "Masse de guerre",
                price: 1_200,
                radius: 1.4,
                reach: 4.8,
                cooldown: 0.70,
                ..BASE
            },
            Jackhammer => &Spec {
                damage: 6.0,
                name: "Marteau-piqueur",
                price: 2_500,
                radius: 0.6,
                reach: 3.0,
                cooldown: 0.07,
                auto: true,
                ..BASE
            },
            Glock => &Spec {
                damage: 14.0,
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
                damage: 11.0,
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
                damage: 12.0,
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
                damage: 95.0,
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
            Pickaxe => &Spec {
                name: "Pioche",
                price: 600,
                radius: 0.75,
                reach: 4.2,
                cooldown: 0.28,
                damage: 24.0,
                ..BASE
            },
            Revolver => &Spec {
                name: "Revolver .44",
                kind: Kind::Gun,
                price: 900,
                radius: 0.55,
                reach: 90.0,
                cooldown: 0.5,
                mag: 6,
                reload: 1.9,
                ammo_price: 35,
                ammo_pack: 12,
                starting_reserve: 24,
                damage: 45.0,
                ..BASE
            },
            Smg => &Spec {
                name: "Pistolet-mitrailleur",
                kind: Kind::Gun,
                price: 1_800,
                radius: 0.30,
                reach: 70.0,
                cooldown: 0.055,
                auto: true,
                mag: 40,
                spread: 0.03,
                reload: 1.4,
                ammo_price: 50,
                ammo_pack: 80,
                starting_reserve: 120,
                damage: 9.0,
                ..BASE
            },
            Minigun => &Spec {
                name: "Minigun",
                kind: Kind::Gun,
                price: 12_000,
                radius: 0.38,
                reach: 90.0,
                cooldown: 0.035,
                auto: true,
                mag: 150,
                spread: 0.05,
                reload: 3.5,
                ammo_price: 120,
                ammo_pack: 150,
                starting_reserve: 300,
                damage: 8.0,
                ..BASE
            },
            Railgun => &Spec {
                name: "Railgun",
                kind: Kind::Gun,
                price: 15_000,
                radius: 0.7,
                reach: 220.0,
                cooldown: 1.4,
                mag: 4,
                pierce: 16,
                reload: 2.5,
                ammo_price: 200,
                ammo_pack: 4,
                starting_reserve: 8,
                damage: 100.0,
                ..BASE
            },
            Blocks => &Spec {
                name: "Blocs",
                kind: Kind::Build,
                reach: 9.0,
                cooldown: 0.12,
                auto: true,
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

    /// Nom court pour `server.conf` (`railgun`, `c4`…).
    pub fn key(self) -> &'static str {
        use WeaponId::*;
        match self {
            Hammer => "hammer",
            Sledge => "sledge",
            WarHammer => "warhammer",
            Jackhammer => "jackhammer",
            Glock => "glock",
            Shotgun => "shotgun",
            Rifle => "rifle",
            Sniper => "sniper",
            Rocket => "rocket",
            Grenade => "grenade",
            Dynamite => "dynamite",
            C4 => "c4",
            MegaBomb => "megabomb",
            Pickaxe => "pickaxe",
            Revolver => "revolver",
            Smg => "smg",
            Minigun => "minigun",
            Railgun => "railgun",
            Blocks => "blocks",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let k = key.trim().to_lowercase();
        ALL_WEAPONS.iter().copied().find(|w| w.key() == k)
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
    /// Points de vie (0..=100).
    pub hp: f32,
    /// Mort : ragdoll et caméra à la troisième personne jusqu'à la
    /// réapparition.
    pub dead: bool,
    /// Secondes restantes avant la réapparition.
    pub respawn_in: f32,
    /// Matériau choisi pour l'outil de construction (indice dans
    /// `world::mat::BUILD`).
    pub build_index: usize,
    /// Les dégâts sont actifs (explosions en solo).
    pub hurt_enabled: bool,
    /// Poussée subie à la mort (le ragdoll est lancé avec).
    pub death_push: Vec3,
    /// Réapparition demandée à cette position.
    pub respawn_at: Option<Vec3>,
    /// Déplacement forcé par le serveur (correction, chargement).
    pub teleport_to: Option<Vec3>,
    /// Nom du monde en cours (fichier de sauvegarde).
    pub world_name: String,
    /// Chargement de la ville en cours (écran de chargement).
    pub loading: bool,
    pub loading_progress: f32,
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
            hp: MAX_HP,
            dead: false,
            respawn_in: 0.0,
            build_index: 0,
            hurt_enabled: true,
            death_push: Vec3::ZERO,
            respawn_at: None,
            teleport_to: None,
            world_name: String::new(),
            loading: false,
            loading_progress: 0.0,
        };
        s.reset_inventory();
        s
    }
}

/// Points de vie maximum.
pub const MAX_HP: f32 = 100.0;
/// Temps avant la réapparition (secondes).
pub const RESPAWN_TIME: f32 = 5.0;

impl GameState {
    pub fn in_game(&self) -> bool {
        self.phase == Phase::Playing
    }

    /// En jeu et vivant : peut agir.
    pub fn alive(&self) -> bool {
        self.phase == Phase::Playing && !self.dead && !self.loading
    }

    /// Matériau actuel de l'outil de construction.
    pub fn build_material(&self) -> u8 {
        crate::world::mat::BUILD[self.build_index % crate::world::mat::BUILD.len()]
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
        self.hp = MAX_HP;
        self.dead = false;
        self.respawn_at = None;
        for &id in &ALL_WEAPONS {
            let spec = id.spec();
            let owned = self.sandbox() || matches!(id, WeaponId::Hammer | WeaponId::Blocks);
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
    /// Distance d'affichage (mètres) : brouillard et plan lointain.
    pub view_distance: f32,
    /// Qualité des ombres : 0 aucune, 1 moyenne, 2 haute.
    pub shadows: u8,
    /// Intensité des secousses de caméra (0..1).
    pub shake: f32,
    /// Affiche le compteur d'images par seconde.
    pub show_fps: bool,
    /// Affiche le viseur.
    pub crosshair: bool,
    /// Balancement de la caméra à la marche.
    pub head_bob: bool,
    /// Volume des bruitages (en plus du volume général).
    pub sfx: f32,
    /// Préréglage graphique : 0 Bas, 1 Moyen, 2 Élevé, 3 Ultra, 4 Perso.
    pub quality: u8,
    /// Anticrénelage : 0 aucun, 2 ou 4 échantillons.
    pub msaa: u8,
    /// Halo lumineux.
    pub bloom: bool,
    /// Poussière et fumée : 0 aucune, 1 réduite, 2 pleine.
    pub particles: u8,
    /// Musique (0 = coupée).
    pub music: f32,
}

/// Noms des préréglages graphiques.
pub const QUALITY_NAMES: [&str; 5] = ["Bas", "Moyen", "Élevé", "Ultra", "Personnalisé"];

impl Settings {
    /// Applique un préréglage (0..=3) aux réglages graphiques.
    pub fn apply_quality(&mut self, q: u8) {
        let (view, shadows, msaa, bloom, particles) = match q {
            0 => (150.0, 0, 0, false, 1),
            1 => (250.0, 1, 2, false, 1),
            2 => (400.0, 2, 4, true, 2),
            _ => (600.0, 2, 4, true, 2),
        };
        self.quality = q.min(3);
        self.view_distance = view;
        self.shadows = shadows;
        self.msaa = msaa;
        self.bloom = bloom;
        self.particles = particles;
    }
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
            view_distance: 250.0,
            shadows: 1,
            shake: 1.0,
            show_fps: false,
            crosshair: true,
            head_bob: true,
            sfx: 1.0,
            quality: 1,
            msaa: 2,
            bloom: false,
            particles: 1,
            music: 0.5,
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
        self.view_distance = self.view_distance.clamp(80.0, 600.0);
        self.shadows = self.shadows.min(2);
        self.shake = self.shake.clamp(0.0, 1.0);
        self.sfx = self.sfx.clamp(0.0, 1.0);
        self.music = self.music.clamp(0.0, 1.0);
        self.quality = self.quality.min(4);
        self.particles = self.particles.min(2);
        if !matches!(self.msaa, 0 | 2 | 4) {
            self.msaa = 4;
        }
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
view_distance={}
shadows={}
shake={}
show_fps={}
crosshair={}
head_bob={}
sfx={}
quality={}
msaa={}
bloom={}
particles={}
music={}
",
        s.name,
        s.sensitivity,
        s.volume,
        s.fps_limit,
        s.invert_y,
        s.fullscreen,
        s.fov,
        s.view_distance,
        s.shadows,
        s.shake,
        s.show_fps,
        s.crosshair,
        s.head_bob,
        s.sfx,
        s.quality,
        s.msaa,
        s.bloom,
        s.particles,
        s.music
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
            "view_distance" => s.view_distance = v.parse().unwrap_or(s.view_distance),
            "shadows" => s.shadows = v.parse().unwrap_or(s.shadows),
            "shake" => s.shake = v.parse().unwrap_or(s.shake),
            "show_fps" => s.show_fps = v == "true",
            "crosshair" => s.crosshair = v == "true",
            "head_bob" => s.head_bob = v == "true",
            "sfx" => s.sfx = v.parse().unwrap_or(s.sfx),
            "quality" => s.quality = v.parse().unwrap_or(s.quality),
            "msaa" => s.msaa = v.parse().unwrap_or(s.msaa),
            "bloom" => s.bloom = v == "true",
            "particles" => s.particles = v.parse().unwrap_or(s.particles),
            "music" => s.music = v.parse().unwrap_or(s.music),
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
    fn normal_mode_starts_with_hammer_and_blocks() {
        let mut g = GameState {
            mode: Mode::Normal,
            ..default()
        };
        g.reset_inventory();
        assert_eq!(g.owned(), vec![WeaponId::Hammer, WeaponId::Blocks]);
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
