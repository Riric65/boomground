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

/// Catégorie d'arme (rangement de la barre d'armes).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cat {
    Melee,
    Pistol,
    Rifle,
    Heavy,
    Explosive,
    Tool,
}

impl Cat {
    pub const ALL: [Cat; 6] = [Cat::Melee, Cat::Pistol, Cat::Rifle, Cat::Heavy, Cat::Explosive, Cat::Tool];

    pub fn name(self) -> &'static str {
        match self {
            Cat::Melee => "Mêlée",
            Cat::Pistol => "Pistolets",
            Cat::Rifle => "Fusils",
            Cat::Heavy => "Lourdes",
            Cat::Explosive => "Explosifs",
            Cat::Tool => "Outils",
        }
    }

    pub fn index(self) -> usize {
        Cat::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }
}

/// Modèle 3D en main (les armes d'une même famille partagent la forme et
/// diffèrent par la teinte et l'échelle).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Hammer,
    Pick,
    Axe,
    Blade,
    Shovel,
    Chainsaw,
    Drill,
    Fist,
    Pistol,
    Revolver,
    Smg,
    Rifle,
    Shotgun,
    Sniper,
    Launcher,
    Minigun,
    Rail,
    Crossbow,
    Flamer,
    Ball,
    Stick,
    Brick,
    Bottle,
    Mega,
    Blocks,
    Extinguisher,
}

/// Identifiant d'une arme : indice dans [`WEAPONS`] (aussi l'identifiant
/// réseau et l'indice de l'inventaire). Les 19 premiers gardent leur ordre
/// historique : les anciennes sauvegardes restent valables.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct WeaponId(pub u8);

/// Nombre d'armes.
pub const COUNT: usize = 76;

pub const ALL_WEAPONS: [WeaponId; COUNT] = {
    let mut all = [WeaponId(0); COUNT];
    let mut i = 0;
    while i < COUNT {
        all[i] = WeaponId(i as u8);
        i += 1;
    }
    all
};

#[allow(non_upper_case_globals)]
impl WeaponId {
    pub const Hammer: WeaponId = WeaponId(0);
    pub const Sledge: WeaponId = WeaponId(1);
    pub const Warhammer: WeaponId = WeaponId(2);
    pub const Pickaxe: WeaponId = WeaponId(3);
    pub const Jackhammer: WeaponId = WeaponId(4);
    pub const Glock: WeaponId = WeaponId(5);
    pub const Revolver: WeaponId = WeaponId(6);
    pub const Smg: WeaponId = WeaponId(7);
    pub const Shotgun: WeaponId = WeaponId(8);
    pub const Rifle: WeaponId = WeaponId(9);
    pub const Minigun: WeaponId = WeaponId(10);
    pub const Sniper: WeaponId = WeaponId(11);
    pub const Railgun: WeaponId = WeaponId(12);
    pub const Rocket: WeaponId = WeaponId(13);
    pub const Grenade: WeaponId = WeaponId(14);
    pub const Dynamite: WeaponId = WeaponId(15);
    pub const C4: WeaponId = WeaponId(16);
    pub const Megabomb: WeaponId = WeaponId(17);
    pub const Blocks: WeaponId = WeaponId(18);
    pub const Axe: WeaponId = WeaponId(19);
    pub const Crowbar: WeaponId = WeaponId(20);
    pub const Machete: WeaponId = WeaponId(21);
    pub const Shovel: WeaponId = WeaponId(22);
    pub const Chainsaw: WeaponId = WeaponId(23);
    pub const Maul: WeaponId = WeaponId(24);
    pub const Wrecking: WeaponId = WeaponId(25);
    pub const Katana: WeaponId = WeaponId(26);
    pub const Drill: WeaponId = WeaponId(27);
    pub const Fist: WeaponId = WeaponId(28);
    pub const M1911: WeaponId = WeaponId(29);
    pub const Silenced: WeaponId = WeaponId(30);
    pub const Machinepistol: WeaponId = WeaponId(31);
    pub const Magnum: WeaponId = WeaponId(32);
    pub const Deagle: WeaponId = WeaponId(33);
    pub const Flare: WeaponId = WeaponId(34);
    pub const Nailgun: WeaponId = WeaponId(35);
    pub const Laserpistol: WeaponId = WeaponId(36);
    pub const Uzi: WeaponId = WeaponId(37);
    pub const Mp5: WeaponId = WeaponId(38);
    pub const Vector: WeaponId = WeaponId(39);
    pub const Ak: WeaponId = WeaponId(40);
    pub const Scar: WeaponId = WeaponId(41);
    pub const Carbine: WeaponId = WeaponId(42);
    pub const Burst: WeaponId = WeaponId(43);
    pub const Dmr: WeaponId = WeaponId(44);
    pub const Sawedoff: WeaponId = WeaponId(45);
    pub const Autoshotgun: WeaponId = WeaponId(46);
    pub const Slug: WeaponId = WeaponId(47);
    pub const Lmg: WeaponId = WeaponId(48);
    pub const Crossbow: WeaponId = WeaponId(49);
    pub const Bow: WeaponId = WeaponId(50);
    pub const Flamethrower: WeaponId = WeaponId(51);
    pub const Amr: WeaponId = WeaponId(52);
    pub const Gauss: WeaponId = WeaponId(53);
    pub const Plasma: WeaponId = WeaponId(54);
    pub const Bazooka: WeaponId = WeaponId(55);
    pub const Minimissile: WeaponId = WeaponId(56);
    pub const Grenadelauncher: WeaponId = WeaponId(57);
    pub const Cannon: WeaponId = WeaponId(58);
    pub const Tsar: WeaponId = WeaponId(59);
    pub const Thermite: WeaponId = WeaponId(60);
    pub const Laserrifle: WeaponId = WeaponId(61);
    pub const Frag: WeaponId = WeaponId(62);
    pub const Impact: WeaponId = WeaponId(63);
    pub const Molotov: WeaponId = WeaponId(64);
    pub const Napalm: WeaponId = WeaponId(65);
    pub const Sticky: WeaponId = WeaponId(66);
    pub const Firework: WeaponId = WeaponId(67);
    pub const Mine: WeaponId = WeaponId(68);
    pub const Claymore: WeaponId = WeaponId(69);
    pub const Shaped: WeaponId = WeaponId(70);
    pub const Timebomb: WeaponId = WeaponId(71);
    pub const Nuke: WeaponId = WeaponId(72);
    pub const DynamitePack: WeaponId = WeaponId(73);
    pub const Extinguisher: WeaponId = WeaponId(74);
    pub const Torch: WeaponId = WeaponId(75);
}

/// Caractéristiques d'une arme.
pub struct Spec {
    /// Clé de `server.conf` (`railgun`).
    pub key: &'static str,
    pub name: &'static str,
    /// Nom court (barre d'armes).
    pub short: &'static str,
    pub cat: Cat,
    pub look: Look,
    /// Teinte du modèle et échelle.
    pub tint: (f32, f32, f32),
    pub scale: f32,
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
    /// Rayon d'incendie (m) à l'impact ou à l'explosion (0 : aucun).
    pub incendiary: f32,
    /// Vitesse du projectile ou du lancer (m/s).
    pub speed: f32,
    /// Projectile qui explose au contact (roquette).
    pub projectile: bool,
    /// Colle à la surface touchée.
    pub sticky: bool,
    /// Explose au premier contact (grenade à impact, cocktail).
    pub contact: bool,
    /// Se déclenche à distance (clic droit).
    pub remote: bool,
    /// Mine : rayon de déclenchement par proximité (0 : aucun).
    pub trigger: f32,
    /// Éteint le feu au lieu de détruire.
    pub extinguish: bool,
    /// Son lourd.
    pub heavy: bool,
}

const BASE: Spec = Spec {
    key: "",
    name: "",
    short: "",
    cat: Cat::Melee,
    look: Look::Hammer,
    tint: (0.5, 0.5, 0.5),
    scale: 1.0,
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
    incendiary: 0.0,
    speed: 14.0,
    projectile: false,
    sticky: false,
    contact: false,
    remote: false,
    trigger: 0.0,
    extinguish: false,
    heavy: false,
};

/// Toutes les armes, dans l'ordre des identifiants.
pub static WEAPONS: [Spec; COUNT] = [
    Spec { key: "hammer", name: "Marteau", short: "Marteau", cat: Cat::Melee, look: Look::Hammer, kind: Kind::Melee, tint: (0.55, 0.57, 0.62), scale: 1.0, radius: 0.55, reach: 3.5, cooldown: 0.3, damage: 18.0, ..BASE },
    Spec { key: "sledge", name: "Masse", short: "Masse", cat: Cat::Melee, look: Look::Hammer, kind: Kind::Melee, tint: (0.5, 0.5, 0.55), scale: 1.5, price: 150, radius: 0.9, reach: 4.5, cooldown: 0.42, damage: 30.0, ..BASE },
    Spec { key: "warhammer", name: "Masse de guerre", short: "Guerre", cat: Cat::Melee, look: Look::Hammer, kind: Kind::Melee, tint: (0.35, 0.35, 0.4), scale: 2.0, price: 1200, radius: 1.4, reach: 4.8, cooldown: 0.7, damage: 55.0, ..BASE },
    Spec { key: "pickaxe", name: "Pioche", short: "Pioche", cat: Cat::Melee, look: Look::Pick, kind: Kind::Melee, tint: (0.6, 0.6, 0.65), scale: 1.0, price: 600, radius: 0.75, reach: 4.2, cooldown: 0.28, damage: 24.0, ..BASE },
    Spec { key: "jackhammer", name: "Marteau-piqueur", short: "Piqueur", cat: Cat::Melee, look: Look::Drill, kind: Kind::Melee, tint: (0.9, 0.7, 0.1), scale: 1.0, price: 2500, radius: 0.6, reach: 3.0, cooldown: 0.07, auto: true, damage: 6.0, ..BASE },
    Spec { key: "glock", name: "Glock 17", short: "Glock", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.12, 0.12, 0.13), scale: 1.0, price: 400, radius: 0.34, reach: 80.0, cooldown: 0.12, mag: 17, reload: 1.1, ammo_price: 20, ammo_pack: 34, starting_reserve: 51, damage: 14.0, ..BASE },
    Spec { key: "revolver", name: "Revolver .44", short: "Revolver", cat: Cat::Pistol, look: Look::Revolver, kind: Kind::Gun, tint: (0.55, 0.57, 0.62), scale: 1.0, price: 900, radius: 0.55, reach: 90.0, cooldown: 0.5, mag: 6, reload: 1.9, ammo_price: 35, ammo_pack: 12, starting_reserve: 24, damage: 45.0, heavy: true, ..BASE },
    Spec { key: "smg", name: "Pistolet-mitrailleur", short: "PM", cat: Cat::Rifle, look: Look::Smg, kind: Kind::Gun, tint: (0.12, 0.12, 0.13), scale: 1.0, price: 1800, radius: 0.3, reach: 70.0, cooldown: 0.055, auto: true, mag: 40, spread: 0.03, reload: 1.4, ammo_price: 50, ammo_pack: 80, starting_reserve: 120, damage: 9.0, ..BASE },
    Spec { key: "shotgun", name: "Fusil à pompe", short: "Pompe", cat: Cat::Rifle, look: Look::Shotgun, kind: Kind::Gun, tint: (0.45, 0.27, 0.13), scale: 1.0, price: 1200, radius: 0.32, reach: 30.0, cooldown: 0.75, mag: 6, pellets: 9, spread: 0.07, reload: 1.6, ammo_price: 40, ammo_pack: 12, starting_reserve: 18, damage: 11.0, heavy: true, ..BASE },
    Spec { key: "rifle", name: "Fusil d'assaut", short: "Fusil", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.3, 0.36, 0.2), scale: 1.0, price: 3000, radius: 0.36, reach: 100.0, cooldown: 0.09, auto: true, mag: 30, spread: 0.012, reload: 1.5, ammo_price: 60, ammo_pack: 60, starting_reserve: 90, damage: 12.0, ..BASE },
    Spec { key: "minigun", name: "Minigun", short: "Minigun", cat: Cat::Heavy, look: Look::Minigun, kind: Kind::Gun, tint: (0.5, 0.5, 0.55), scale: 1.0, price: 12000, radius: 0.38, reach: 90.0, cooldown: 0.035, auto: true, mag: 150, spread: 0.05, reload: 3.5, ammo_price: 120, ammo_pack: 150, starting_reserve: 300, damage: 8.0, ..BASE },
    Spec { key: "sniper", name: "Sniper .50", short: "Sniper", cat: Cat::Heavy, look: Look::Sniper, kind: Kind::Gun, tint: (0.3, 0.36, 0.2), scale: 1.0, price: 4500, radius: 0.5, reach: 160.0, cooldown: 1.0, mag: 5, pierce: 14, reload: 2.0, ammo_price: 80, ammo_pack: 10, starting_reserve: 10, damage: 95.0, heavy: true, ..BASE },
    Spec { key: "railgun", name: "Railgun", short: "Rail", cat: Cat::Heavy, look: Look::Rail, kind: Kind::Gun, tint: (0.55, 0.57, 0.62), scale: 1.0, price: 15000, radius: 0.7, reach: 220.0, cooldown: 1.4, mag: 4, pierce: 16, reload: 2.5, ammo_price: 200, ammo_pack: 4, starting_reserve: 8, damage: 100.0, heavy: true, ..BASE },
    Spec { key: "rocket", name: "Lance-roquettes", short: "Roquette", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.3, 0.36, 0.2), scale: 1.0, price: 8000, radius: 2.8, reach: 160.0, cooldown: 0.4, mag: 1, reload: 1.8, ammo_price: 150, ammo_pack: 3, starting_reserve: 4, projectile: true, speed: 45.0, heavy: true, ..BASE },
    Spec { key: "grenade", name: "Grenade", short: "Grenade", cat: Cat::Explosive, look: Look::Ball, kind: Kind::Thrown, tint: (0.3, 0.36, 0.2), scale: 1.0, price: 40, radius: 2.0, reach: 16.0, cooldown: 0.6, fuse: 2.5, speed: 17.0, ..BASE },
    Spec { key: "dynamite", name: "Dynamite", short: "Dynamite", cat: Cat::Explosive, look: Look::Stick, kind: Kind::Thrown, tint: (0.75, 0.1, 0.08), scale: 1.0, price: 90, radius: 2.8, reach: 14.0, cooldown: 0.6, fuse: 3.5, sticky: true, speed: 14.0, ..BASE },
    Spec { key: "c4", name: "C4", short: "C4", cat: Cat::Explosive, look: Look::Brick, kind: Kind::Placed, tint: (0.8, 0.76, 0.4), scale: 1.0, price: 200, radius: 3.4, reach: 6.0, cooldown: 0.3, remote: true, ..BASE },
    Spec { key: "megabomb", name: "Méga-bombe", short: "Méga", cat: Cat::Explosive, look: Look::Mega, kind: Kind::Placed, tint: (0.2, 0.2, 0.22), scale: 1.0, price: 2500, radius: 7.0, reach: 6.0, cooldown: 1.0, fuse: 8.0, ..BASE },
    Spec { key: "blocks", name: "Blocs", short: "Blocs", cat: Cat::Tool, look: Look::Blocks, kind: Kind::Build, tint: (0.8, 0.76, 0.4), scale: 1.0, reach: 9.0, cooldown: 0.12, auto: true, ..BASE },
    Spec { key: "axe", name: "Hache", short: "Hache", cat: Cat::Melee, look: Look::Axe, kind: Kind::Melee, tint: (0.55, 0.57, 0.62), scale: 1.0, price: 350, radius: 0.7, reach: 4.0, cooldown: 0.35, damage: 28.0, ..BASE },
    Spec { key: "crowbar", name: "Pied-de-biche", short: "Biche", cat: Cat::Melee, look: Look::Blade, kind: Kind::Melee, tint: (0.35, 0.4, 0.45), scale: 1.0, price: 120, radius: 0.4, reach: 3.8, cooldown: 0.18, damage: 14.0, ..BASE },
    Spec { key: "machete", name: "Machette", short: "Machette", cat: Cat::Melee, look: Look::Blade, kind: Kind::Melee, tint: (0.7, 0.72, 0.75), scale: 1.2, price: 300, radius: 0.5, reach: 3.6, cooldown: 0.14, damage: 20.0, ..BASE },
    Spec { key: "shovel", name: "Pelle", short: "Pelle", cat: Cat::Melee, look: Look::Shovel, kind: Kind::Melee, tint: (0.5, 0.5, 0.52), scale: 1.0, price: 280, radius: 1.1, reach: 4.2, cooldown: 0.4, damage: 16.0, ..BASE },
    Spec { key: "chainsaw", name: "Tronçonneuse", short: "Tronçon.", cat: Cat::Melee, look: Look::Chainsaw, kind: Kind::Melee, tint: (0.9, 0.5, 0.1), scale: 1.0, price: 3500, radius: 0.7, reach: 3.2, cooldown: 0.06, auto: true, damage: 7.0, ..BASE },
    Spec { key: "maul", name: "Maillet", short: "Maillet", cat: Cat::Melee, look: Look::Hammer, kind: Kind::Melee, tint: (0.45, 0.3, 0.15), scale: 1.7, price: 900, radius: 1.2, reach: 4.4, cooldown: 0.55, damage: 40.0, ..BASE },
    Spec { key: "wrecking", name: "Boulet", short: "Boulet", cat: Cat::Melee, look: Look::Mega, kind: Kind::Melee, tint: (0.25, 0.25, 0.28), scale: 1.0, price: 3000, radius: 2.0, reach: 5.0, cooldown: 1.1, damage: 70.0, ..BASE },
    Spec { key: "katana", name: "Katana", short: "Katana", cat: Cat::Melee, look: Look::Blade, kind: Kind::Melee, tint: (0.85, 0.85, 0.9), scale: 1.5, price: 1500, radius: 0.35, reach: 4.2, cooldown: 0.12, damage: 24.0, ..BASE },
    Spec { key: "drill", name: "Perceuse", short: "Perceuse", cat: Cat::Melee, look: Look::Drill, kind: Kind::Melee, tint: (0.2, 0.45, 0.8), scale: 0.8, price: 1800, radius: 0.45, reach: 2.8, cooldown: 0.05, auto: true, damage: 4.0, ..BASE },
    Spec { key: "fist", name: "Poing américain", short: "Poing", cat: Cat::Melee, look: Look::Fist, kind: Kind::Melee, tint: (0.6, 0.6, 0.62), scale: 1.0, price: 100, radius: 0.4, reach: 2.6, cooldown: 0.2, damage: 12.0, ..BASE },
    Spec { key: "m1911", name: "Pistolet 1911", short: "1911", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.3, 0.3, 0.32), scale: 1.0, price: 250, radius: 0.3, reach: 70.0, cooldown: 0.22, mag: 8, reload: 1.1, ammo_price: 15, ammo_pack: 16, starting_reserve: 32, damage: 15.0, ..BASE },
    Spec { key: "silenced", name: "Pistolet silencieux", short: "Silence", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.1, 0.1, 0.12), scale: 1.15, price: 500, radius: 0.28, reach: 70.0, cooldown: 0.18, mag: 12, reload: 1.2, ammo_price: 20, ammo_pack: 24, starting_reserve: 36, damage: 12.0, ..BASE },
    Spec { key: "machinepistol", name: "Pistolet automatique", short: "Auto", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.2, 0.2, 0.22), scale: 1.0, price: 1100, radius: 0.26, reach: 55.0, cooldown: 0.07, auto: true, mag: 20, spread: 0.04, reload: 1.3, ammo_price: 30, ammo_pack: 40, starting_reserve: 60, damage: 8.0, ..BASE },
    Spec { key: "magnum", name: "Magnum .500", short: "Magnum", cat: Cat::Pistol, look: Look::Revolver, kind: Kind::Gun, tint: (0.7, 0.7, 0.75), scale: 1.3, price: 2800, radius: 0.8, reach: 100.0, cooldown: 0.9, mag: 5, reload: 2.2, ammo_price: 60, ammo_pack: 10, starting_reserve: 15, damage: 70.0, heavy: true, ..BASE },
    Spec { key: "deagle", name: "Désert", short: "Désert", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.75, 0.75, 0.78), scale: 1.2, price: 1500, radius: 0.5, reach: 85.0, cooldown: 0.45, mag: 7, reload: 1.6, ammo_price: 40, ammo_pack: 14, starting_reserve: 21, damage: 38.0, heavy: true, ..BASE },
    Spec { key: "flare", name: "Pistolet de détresse", short: "Fusée", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.9, 0.3, 0.1), scale: 1.1, price: 600, radius: 0.4, reach: 60.0, cooldown: 1.2, mag: 1, reload: 1.8, ammo_price: 40, ammo_pack: 3, starting_reserve: 6, damage: 6.0, incendiary: 3.0, ..BASE },
    Spec { key: "nailgun", name: "Cloueur", short: "Cloueur", cat: Cat::Pistol, look: Look::Pistol, kind: Kind::Gun, tint: (0.9, 0.75, 0.15), scale: 1.1, price: 700, radius: 0.22, reach: 25.0, cooldown: 0.1, auto: true, mag: 30, spread: 0.03, reload: 1.4, ammo_price: 20, ammo_pack: 60, starting_reserve: 90, damage: 6.0, ..BASE },
    Spec { key: "laserpistol", name: "Pistolet laser", short: "Laser", cat: Cat::Pistol, look: Look::Rail, kind: Kind::Gun, tint: (0.2, 0.8, 1.0), scale: 0.6, price: 8000, radius: 0.25, reach: 90.0, cooldown: 0.12, auto: true, pierce: 8, mag: 40, reload: 1.5, ammo_price: 80, ammo_pack: 80, starting_reserve: 120, damage: 10.0, ..BASE },
    Spec { key: "uzi", name: "Uzi", short: "Uzi", cat: Cat::Rifle, look: Look::Smg, kind: Kind::Gun, tint: (0.2, 0.2, 0.22), scale: 0.9, price: 1500, radius: 0.28, reach: 60.0, cooldown: 0.06, auto: true, mag: 32, spread: 0.05, reload: 1.3, ammo_price: 40, ammo_pack: 64, starting_reserve: 96, damage: 7.0, ..BASE },
    Spec { key: "mp5", name: "MP5", short: "MP5", cat: Cat::Rifle, look: Look::Smg, kind: Kind::Gun, tint: (0.15, 0.15, 0.17), scale: 1.1, price: 2200, radius: 0.3, reach: 75.0, cooldown: 0.075, auto: true, mag: 30, spread: 0.025, reload: 1.4, ammo_price: 50, ammo_pack: 60, starting_reserve: 90, damage: 10.0, ..BASE },
    Spec { key: "vector", name: "Vector", short: "Vector", cat: Cat::Rifle, look: Look::Smg, kind: Kind::Gun, tint: (0.3, 0.3, 0.35), scale: 1.0, price: 2600, radius: 0.28, reach: 65.0, cooldown: 0.045, auto: true, mag: 25, spread: 0.06, reload: 1.3, ammo_price: 55, ammo_pack: 50, starting_reserve: 75, damage: 8.0, ..BASE },
    Spec { key: "ak", name: "AK-47", short: "AK-47", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.4, 0.28, 0.15), scale: 1.05, price: 3200, radius: 0.42, reach: 100.0, cooldown: 0.1, auto: true, mag: 30, spread: 0.02, reload: 1.6, ammo_price: 60, ammo_pack: 60, starting_reserve: 90, damage: 15.0, ..BASE },
    Spec { key: "scar", name: "SCAR", short: "SCAR", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.7, 0.65, 0.5), scale: 1.05, price: 3800, radius: 0.38, reach: 110.0, cooldown: 0.085, auto: true, mag: 20, spread: 0.012, reload: 1.5, ammo_price: 65, ammo_pack: 40, starting_reserve: 80, damage: 13.0, ..BASE },
    Spec { key: "carbine", name: "Carabine", short: "Carabine", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.35, 0.35, 0.38), scale: 0.9, price: 2000, radius: 0.4, reach: 100.0, cooldown: 0.14, mag: 15, spread: 0.01, reload: 1.5, ammo_price: 45, ammo_pack: 30, starting_reserve: 45, damage: 18.0, ..BASE },
    Spec { key: "burst", name: "Fusil à rafales", short: "Rafale", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.25, 0.4, 0.3), scale: 1.0, price: 2900, radius: 0.34, reach: 100.0, cooldown: 0.35, pellets: 3, mag: 30, spread: 0.012, reload: 1.6, ammo_price: 60, ammo_pack: 60, starting_reserve: 90, damage: 13.0, ..BASE },
    Spec { key: "dmr", name: "Fusil de précision", short: "Précision", cat: Cat::Rifle, look: Look::Sniper, kind: Kind::Gun, tint: (0.35, 0.3, 0.25), scale: 0.85, price: 3600, radius: 0.55, reach: 130.0, cooldown: 0.5, mag: 10, reload: 1.9, ammo_price: 70, ammo_pack: 20, starting_reserve: 30, damage: 45.0, heavy: true, ..BASE },
    Spec { key: "sawedoff", name: "Canon scié", short: "Scié", cat: Cat::Rifle, look: Look::Shotgun, kind: Kind::Gun, tint: (0.4, 0.25, 0.12), scale: 0.7, price: 900, radius: 0.3, reach: 18.0, cooldown: 0.5, mag: 2, pellets: 12, spread: 0.15, reload: 1.4, ammo_price: 30, ammo_pack: 8, starting_reserve: 12, damage: 9.0, heavy: true, ..BASE },
    Spec { key: "autoshotgun", name: "Fusil automatique", short: "Auto-pompe", cat: Cat::Rifle, look: Look::Shotgun, kind: Kind::Gun, tint: (0.15, 0.15, 0.17), scale: 1.1, price: 4200, radius: 0.3, reach: 28.0, cooldown: 0.25, auto: true, mag: 8, pellets: 8, spread: 0.08, reload: 2.0, ammo_price: 60, ammo_pack: 16, starting_reserve: 24, damage: 9.0, heavy: true, ..BASE },
    Spec { key: "slug", name: "Fusil à chevrotine lourde", short: "Slug", cat: Cat::Rifle, look: Look::Shotgun, kind: Kind::Gun, tint: (0.5, 0.3, 0.1), scale: 1.1, price: 1600, radius: 0.7, reach: 60.0, cooldown: 0.8, mag: 6, reload: 1.8, ammo_price: 40, ammo_pack: 12, starting_reserve: 18, damage: 60.0, heavy: true, ..BASE },
    Spec { key: "lmg", name: "Mitrailleuse légère", short: "MitrLég.", cat: Cat::Rifle, look: Look::Rifle, kind: Kind::Gun, tint: (0.25, 0.3, 0.25), scale: 1.35, price: 5200, radius: 0.4, reach: 100.0, cooldown: 0.06, auto: true, mag: 100, spread: 0.035, reload: 4.0, ammo_price: 100, ammo_pack: 100, starting_reserve: 200, damage: 12.0, ..BASE },
    Spec { key: "crossbow", name: "Arbalète", short: "Arbalète", cat: Cat::Rifle, look: Look::Crossbow, kind: Kind::Gun, tint: (0.4, 0.28, 0.15), scale: 1.0, price: 700, radius: 0.45, reach: 90.0, cooldown: 1.1, pierce: 4, mag: 1, reload: 1.4, ammo_price: 25, ammo_pack: 5, starting_reserve: 10, damage: 55.0, ..BASE },
    Spec { key: "bow", name: "Arc", short: "Arc", cat: Cat::Rifle, look: Look::Crossbow, kind: Kind::Gun, tint: (0.55, 0.4, 0.2), scale: 1.2, price: 450, radius: 0.4, reach: 80.0, cooldown: 0.9, pierce: 2, mag: 1, reload: 0.9, ammo_price: 15, ammo_pack: 5, starting_reserve: 15, damage: 40.0, ..BASE },
    Spec { key: "flamethrower", name: "Lance-flammes", short: "Flammes", cat: Cat::Rifle, look: Look::Flamer, kind: Kind::Gun, tint: (0.8, 0.2, 0.1), scale: 1.0, price: 6500, radius: 0.5, reach: 12.0, cooldown: 0.05, auto: true, mag: 120, spread: 0.06, reload: 3.5, ammo_price: 100, ammo_pack: 120, starting_reserve: 240, damage: 4.0, incendiary: 2.0, ..BASE },
    Spec { key: "amr", name: "Fusil antimatériel", short: "AMR", cat: Cat::Heavy, look: Look::Sniper, kind: Kind::Gun, tint: (0.2, 0.25, 0.2), scale: 1.3, price: 7500, radius: 0.9, reach: 220.0, cooldown: 1.6, pierce: 12, mag: 3, reload: 2.8, ammo_price: 150, ammo_pack: 6, starting_reserve: 9, damage: 140.0, heavy: true, ..BASE },
    Spec { key: "gauss", name: "Canon Gauss", short: "Gauss", cat: Cat::Heavy, look: Look::Rail, kind: Kind::Gun, tint: (0.4, 0.5, 0.9), scale: 1.2, price: 11000, radius: 0.8, reach: 250.0, cooldown: 1.2, pierce: 20, mag: 6, reload: 2.4, ammo_price: 180, ammo_pack: 6, starting_reserve: 12, damage: 110.0, heavy: true, ..BASE },
    Spec { key: "plasma", name: "Fusil à plasma", short: "Plasma", cat: Cat::Heavy, look: Look::Rail, kind: Kind::Gun, tint: (0.2, 1.0, 0.5), scale: 1.0, price: 9000, radius: 1.4, reach: 120.0, cooldown: 0.35, auto: true, mag: 30, reload: 2.2, ammo_price: 120, ammo_pack: 30, starting_reserve: 60, damage: 20.0, incendiary: 1.5, heavy: true, ..BASE },
    Spec { key: "bazooka", name: "Bazooka", short: "Bazooka", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.35, 0.4, 0.25), scale: 1.2, price: 12000, radius: 3.6, reach: 160.0, cooldown: 0.8, mag: 1, reload: 2.2, ammo_price: 220, ammo_pack: 3, starting_reserve: 3, projectile: true, speed: 40.0, heavy: true, ..BASE },
    Spec { key: "minimissile", name: "Mini-missiles", short: "Missiles", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.5, 0.5, 0.55), scale: 0.9, price: 10000, radius: 1.6, reach: 150.0, cooldown: 0.25, auto: true, mag: 12, reload: 3.0, ammo_price: 200, ammo_pack: 12, starting_reserve: 24, projectile: true, speed: 50.0, heavy: true, ..BASE },
    Spec { key: "grenadelauncher", name: "Lance-grenades", short: "Lance-gren.", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.25, 0.3, 0.25), scale: 0.9, price: 6000, radius: 2.4, reach: 60.0, cooldown: 0.7, mag: 6, reload: 2.4, ammo_price: 120, ammo_pack: 6, starting_reserve: 12, fuse: 2.0, speed: 26.0, heavy: true, ..BASE },
    Spec { key: "cannon", name: "Canon", short: "Canon", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.15, 0.15, 0.17), scale: 1.4, price: 18000, radius: 5.0, reach: 180.0, cooldown: 1.5, mag: 1, reload: 3.0, ammo_price: 400, ammo_pack: 2, starting_reserve: 2, projectile: true, speed: 55.0, heavy: true, ..BASE },
    Spec { key: "tsar", name: "Tsar", short: "Tsar", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.7, 0.1, 0.1), scale: 1.5, price: 40000, radius: 14.0, reach: 200.0, cooldown: 3.0, mag: 1, reload: 4.0, ammo_price: 2000, ammo_pack: 1, starting_reserve: 1, projectile: true, speed: 35.0, heavy: true, ..BASE },
    Spec { key: "thermite", name: "Lance-thermite", short: "Thermite", cat: Cat::Heavy, look: Look::Launcher, kind: Kind::Gun, tint: (0.9, 0.4, 0.1), scale: 1.0, price: 5000, radius: 1.2, reach: 120.0, cooldown: 0.6, mag: 4, reload: 2.5, ammo_price: 120, ammo_pack: 4, starting_reserve: 8, projectile: true, speed: 42.0, incendiary: 5.0, heavy: true, ..BASE },
    Spec { key: "laserrifle", name: "Fusil laser", short: "Laser", cat: Cat::Heavy, look: Look::Rail, kind: Kind::Gun, tint: (1.0, 0.2, 0.2), scale: 1.3, price: 14000, radius: 0.3, reach: 200.0, cooldown: 0.08, auto: true, pierce: 10, mag: 60, reload: 2.5, ammo_price: 200, ammo_pack: 60, starting_reserve: 120, damage: 12.0, ..BASE },
    Spec { key: "frag", name: "Grenade à fragmentation", short: "Frag", cat: Cat::Explosive, look: Look::Ball, kind: Kind::Thrown, tint: (0.25, 0.3, 0.2), scale: 1.0, price: 80, radius: 2.4, reach: 16.0, cooldown: 0.6, fuse: 2.0, speed: 18.0, ..BASE },
    Spec { key: "impact", name: "Grenade à impact", short: "Impact", cat: Cat::Explosive, look: Look::Ball, kind: Kind::Thrown, tint: (0.7, 0.2, 0.15), scale: 1.0, price: 100, radius: 2.2, reach: 16.0, cooldown: 0.6, fuse: 6.0, contact: true, speed: 19.0, ..BASE },
    Spec { key: "molotov", name: "Cocktail Molotov", short: "Molotov", cat: Cat::Explosive, look: Look::Bottle, kind: Kind::Thrown, tint: (0.5, 0.8, 0.3), scale: 1.0, price: 120, radius: 1.4, reach: 16.0, cooldown: 0.7, fuse: 6.0, contact: true, incendiary: 5.0, speed: 15.0, ..BASE },
    Spec { key: "napalm", name: "Napalm", short: "Napalm", cat: Cat::Explosive, look: Look::Ball, kind: Kind::Thrown, tint: (0.9, 0.5, 0.1), scale: 1.2, price: 600, radius: 2.0, reach: 14.0, cooldown: 0.8, fuse: 2.5, incendiary: 8.0, speed: 15.0, ..BASE },
    Spec { key: "sticky", name: "Bombe collante", short: "Collante", cat: Cat::Explosive, look: Look::Ball, kind: Kind::Thrown, tint: (0.3, 0.8, 0.3), scale: 1.0, price: 300, radius: 3.0, reach: 14.0, cooldown: 0.7, fuse: 2.5, sticky: true, speed: 15.0, ..BASE },
    Spec { key: "firework", name: "Feu d'artifice", short: "Artifice", cat: Cat::Explosive, look: Look::Stick, kind: Kind::Thrown, tint: (0.9, 0.2, 0.6), scale: 0.8, price: 50, radius: 1.2, reach: 14.0, cooldown: 0.5, fuse: 1.5, incendiary: 2.0, speed: 14.0, ..BASE },
    Spec { key: "mine", name: "Mine", short: "Mine", cat: Cat::Explosive, look: Look::Brick, kind: Kind::Placed, tint: (0.3, 0.35, 0.3), scale: 0.8, price: 250, radius: 3.2, reach: 6.0, cooldown: 0.6, trigger: 1.8, ..BASE },
    Spec { key: "claymore", name: "Claymore", short: "Claymore", cat: Cat::Explosive, look: Look::Brick, kind: Kind::Placed, tint: (0.2, 0.3, 0.2), scale: 1.1, price: 450, radius: 4.0, reach: 6.0, cooldown: 0.6, trigger: 4.5, ..BASE },
    Spec { key: "shaped", name: "Charge creuse", short: "Creuse", cat: Cat::Explosive, look: Look::Brick, kind: Kind::Placed, tint: (0.5, 0.5, 0.55), scale: 1.0, price: 900, radius: 5.0, reach: 6.0, cooldown: 0.8, fuse: 3.0, ..BASE },
    Spec { key: "timebomb", name: "Bombe à retardement", short: "Retard", cat: Cat::Explosive, look: Look::Mega, kind: Kind::Placed, tint: (0.5, 0.1, 0.1), scale: 0.8, price: 2000, radius: 8.0, reach: 6.0, cooldown: 1.0, fuse: 15.0, ..BASE },
    Spec { key: "nuke", name: "Mini-nuke", short: "Nuke", cat: Cat::Explosive, look: Look::Mega, kind: Kind::Placed, tint: (0.9, 0.8, 0.1), scale: 1.2, price: 25000, radius: 12.0, reach: 6.0, cooldown: 2.0, fuse: 12.0, ..BASE },
    Spec { key: "dynamite_pack", name: "Pack de dynamite", short: "Pack", cat: Cat::Explosive, look: Look::Stick, kind: Kind::Thrown, tint: (0.8, 0.15, 0.1), scale: 1.6, price: 500, radius: 5.0, reach: 12.0, cooldown: 0.8, fuse: 4.0, sticky: true, speed: 12.0, ..BASE },
    Spec { key: "extinguisher", name: "Extincteur", short: "Extincteur", cat: Cat::Tool, look: Look::Extinguisher, kind: Kind::Gun, tint: (0.75, 0.1, 0.1), scale: 1.0, price: 300, radius: 0.05, reach: 9.0, cooldown: 0.04, auto: true, mag: 400, reload: 2.0, ammo_price: 40, ammo_pack: 400, starting_reserve: 800, extinguish: true, ..BASE },
    Spec { key: "torch", name: "Chalumeau", short: "Chalumeau", cat: Cat::Tool, look: Look::Flamer, kind: Kind::Gun, tint: (0.3, 0.5, 0.9), scale: 0.7, price: 900, radius: 0.3, reach: 4.5, cooldown: 0.05, auto: true, mag: 500, reload: 2.0, ammo_price: 30, ammo_pack: 500, starting_reserve: 1000, damage: 2.0, incendiary: 1.0, ..BASE },
];

impl WeaponId {
    pub fn spec(self) -> &'static Spec {
        &WEAPONS[(self.0 as usize).min(COUNT - 1)]
    }

    pub fn kind(self) -> Kind {
        self.spec().kind
    }

    pub fn cat(self) -> Cat {
        self.spec().cat
    }

    pub fn is_bomb(self) -> bool {
        matches!(self.kind(), Kind::Thrown | Kind::Placed)
    }

    /// Nom court pour `server.conf` (`railgun`, `c4`…).
    pub fn key(self) -> &'static str {
        self.spec().key
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let k = key.trim().to_lowercase();
        ALL_WEAPONS.iter().copied().find(|w| w.key() == k)
    }

    /// Identifiant réseau.
    pub fn tag(self) -> u8 {
        self.0
    }

    pub fn from_tag(tag: u8) -> Self {
        if (tag as usize) < COUNT {
            WeaponId(tag)
        } else {
            WeaponId(0)
        }
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
    /// Partie multijoueur en cours (le monde continue pendant la pause).
    pub multi: bool,
    /// Le serveur autorise le vol libre.
    pub fly_allowed: bool,
    /// Rouge à l'écran après un coup (0..1), pour l'effet de dégâts.
    pub hurt_flash: f32,
    /// Dernière arme utilisée par catégorie (indice dans `Cat::ALL`).
    pub last_in_cat: [u8; 6],
    /// Le joueur écrit dans le chat : les touches de jeu sont suspendues.
    pub typing: bool,
    /// Corps rigide du véhicule conduit (0 = à pied).
    pub driving: u32,
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
            multi: false,
            fly_allowed: true,
            hurt_flash: 0.0,
            last_in_cat: [0, 5, 7, 10, 14, 18],
            typing: false,
            driving: 0,
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

    /// La simulation locale tourne : en jeu, ou en multijoueur même avec le
    /// menu pause ouvert (le monde ne s'arrête pas pour les autres).
    pub fn simulating(&self) -> bool {
        self.phase == Phase::Playing || (self.started && self.multi && self.phase == Phase::Menu)
    }

    /// En jeu et vivant : peut agir.
    pub fn alive(&self) -> bool {
        self.phase == Phase::Playing && !self.dead && !self.loading && !self.typing && self.driving == 0
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
    /// Anticrénelage : 0 aucun, 4 échantillons. Le format 2× est.proposé par
    /// aucun matériel current et casse WebGPU sur les pilotes logiciels
    /// (llvmpipe), on ne le propose donc plus.
    pub msaa: u8,
    /// Halo lumineux.
    pub bloom: bool,
    /// Poussière et fumée : 0 aucune, 1 réduite, 2 pleine.
    pub particles: u8,
    /// Musique (0 = coupée).
    pub music: f32,
    /// Voix de proximité : 0 désactivée, 1 activation vocale, 2 touche.
    pub voice_mode: u8,
    /// Volume des voix des autres joueurs.
    pub voice_volume: f32,
    /// Côté du joystick tactile : 0 à gauche, 1 à droite.
    pub joystick_side: u8,
    /// Sensibilité du regard au doigt (radian par pixel d'écran).
    pub touch_sensitivity: f32,
}

/// Noms des préréglages graphiques.
pub const QUALITY_NAMES: [&str; 5] = ["Bas", "Moyen", "Élevé", "Ultra", "Personnalisé"];

impl Settings {
    /// Applique un préréglage (0..=3) aux réglages graphiques.
    pub fn apply_quality(&mut self, q: u8) {
        let (view, shadows, msaa, bloom, particles) = match q {
            0 => (150.0, 0, 0, false, 1),
            1 => (250.0, 1, 4, false, 1),
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
            msaa: 4,
            bloom: false,
            particles: 1,
            music: 0.4,
            voice_mode: 1,
            voice_volume: 0.8,
            joystick_side: 0,
            touch_sensitivity: 0.005,
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
        self.voice_mode = self.voice_mode.min(2);
        self.voice_volume = self.voice_volume.clamp(0.0, 1.0);
        self.joystick_side = self.joystick_side.min(1);
        self.touch_sensitivity = self.touch_sensitivity.clamp(0.001, 0.02);
        self.quality = self.quality.min(4);
        self.particles = self.particles.min(2);
        // Un ancien réglage « 2× » est ramené à 4× : les pilotes
        // logiciels (llvmpipe/Mesa) ne gèrent que 1 et 4 échantillons et
        // plantent la fenêtre avec une erreur de validation WebGPU.
        if !matches!(self.msaa, 0 | 4) {
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
    // Android : dossier privé de l'application (pas de HOME).
    #[cfg(target_os = "android")]
    {
        return bevy::android::ANDROID_APP.get().and_then(|app| app.internal_data_path());
    }
    #[allow(unreachable_code)]
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
voice_mode={}
voice_volume={}
joystick_side={}
touch_sensitivity={}
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
        s.music,
        s.voice_mode,
        s.voice_volume,
        s.joystick_side,
        s.touch_sensitivity
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
            "voice_mode" => s.voice_mode = v.parse().unwrap_or(s.voice_mode),
            "voice_volume" => s.voice_volume = v.parse().unwrap_or(s.voice_volume),
            "joystick_side" => s.joystick_side = v.parse().unwrap_or(s.joystick_side),
            "touch_sensitivity" => s.touch_sensitivity = v.parse().unwrap_or(s.touch_sensitivity),
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

/// Condition de système : la simulation continue (y compris menu pause en
/// multijoueur).
pub fn simulating(state: Res<GameState>) -> bool {
    state.simulating()
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
        assert!(g.has_bomb(WeaponId::Megabomb));
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
