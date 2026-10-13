//! Langues de l'interface : français (texte source), anglais, espagnol,
//! allemand.
//!
//! Le code écrit les textes en français ; [`t`] les traduit au moment de
//! l'affichage. Les textes construits avec des valeurs passent par [`tf`],
//! dont le modèle contient des `{}`. Un texte absent de la table reste en
//! français.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Fr = 0,
    En = 1,
    Es = 2,
    De = 3,
}

pub const NAMES: [&str; 4] = ["Français", "English", "Español", "Deutsch"];

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: u8) {
    LANG.store(lang.min(3), Ordering::Relaxed);
}

pub fn current() -> u8 {
    LANG.load(Ordering::Relaxed)
}

/// Langue du système au premier lancement (`LANG`, `LC_ALL`, Android).
pub fn system_default() -> u8 {
    let env = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
        .to_lowercase();
    match env.get(..2) {
        Some("en") => 1,
        Some("es") => 2,
        Some("de") => 3,
        _ => 0,
    }
}

fn table() -> &'static HashMap<&'static str, [&'static str; 3]> {
    static T: OnceLock<HashMap<&'static str, [&'static str; 3]>> = OnceLock::new();
    T.get_or_init(|| STRINGS.iter().map(|(fr, en, es, de)| (*fr, [*en, *es, *de])).collect())
}

/// Traduit un texte français (inchangé en français ou s'il est inconnu).
pub fn t(fr: &str) -> String {
    t_in(current(), fr)
}

/// Traduit dans une langue donnée.
pub fn t_in(lang: u8, fr: &str) -> String {
    if lang == 0 {
        return fr.to_string();
    }
    match table().get(fr) {
        Some(tr) => tr[(lang.min(3) - 1) as usize].to_string(),
        None => fr.to_string(),
    }
}

/// Traduit un modèle à `{}` puis y place les valeurs, dans l'ordre.
pub fn tf(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let model = t(template);
    let mut out = String::with_capacity(model.len() + 16);
    let mut it = args.iter();
    let mut rest = model.as_str();
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = it.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// Texte d'interface à retraduire quand la langue change (texte source).
#[derive(bevy::prelude::Component, Clone)]
pub struct Tr(pub String, pub bool);

/// Retraduit les textes marqués quand la langue change.
pub fn retranslate(
    settings: bevy::prelude::Res<crate::game::Settings>,
    mut last: bevy::prelude::Local<Option<u8>>,
    mut texts: bevy::prelude::Query<(&Tr, &mut bevy::prelude::Text)>) {
    set(settings.lang);
    let lang = current();
    if *last == Some(lang) {
        return;
    }
    let before = last.unwrap_or(0);
    *last = Some(lang);
    let shown = |l: u8, tr: &Tr| if tr.1 { t_in(l, &tr.0).to_uppercase() } else { t_in(l, &tr.0) };
    for (tr, mut text) in &mut texts {
        // Seuls les textes qui affichent encore leur traduction d'origine :
        // une valeur écrite par le jeu entre-temps (réglage, nom…) reste.
        if tr.0.is_empty() || text.0 != shown(before, tr) {
            continue;
        }
        let v = shown(lang, tr);
        if text.0 != v {
            text.0 = v;
        }
    }
}

/// (français, anglais, espagnol, allemand)
const STRINGS: &[(&str, &str, &str, &str)] = &[
    // --- Menu principal ---
    ("BAC À SABLE", "SANDBOX", "SANDBOX", "SANDKASTEN"),
    ("Les 13 armes d'office", "All 13 weapons from the start", "Las 13 armas desde el inicio", "Alle 13 Waffen von Anfang an"),
    ("Munitions et bombes illimitées", "Unlimited ammo and bombs", "Munición y bombas ilimitadas", "Unbegrenzte Munition und Bomben"),
    ("Vol libre avec V", "Free flight with V", "Vuelo libre con V", "Freies Fliegen mit V"),
    ("MODE NORMAL", "NORMAL MODE", "MODO NORMAL", "NORMALER MODUS"),
    ("Un marteau pour commencer", "A hammer to start with", "Un martillo para empezar", "Ein Hammer zum Start"),
    ("Chaque voxel cassé rapporte de l'argent", "Every broken voxel earns money", "Cada vóxel roto da dinero", "Jeder zerstörte Voxel bringt Geld"),
    ("Boutique (B) : armes, marteaux, bombes", "Shop (B): weapons, hammers, bombs", "Tienda (B): armas, martillos, bombas", "Laden (B): Waffen, Hämmer, Bomben"),
    ("Tout est destructible.", "Everything is destructible.", "Todo es destructible.", "Alles ist zerstörbar."),
    ("Sauvegarder le monde", "Save world", "Guardar mundo", "Welt speichern"),
    ("REPRENDRE", "RESUME", "CONTINUAR", "FORTSETZEN"),
    ("Jouer", "Play", "Jugar", "Spielen"),
    ("Multijoueur", "Multiplayer", "Multijugador", "Mehrspieler"),
    ("Paramètres", "Settings", "Ajustes", "Einstellungen"),
    ("Quitter la partie", "Leave game", "Salir de la partida", "Spiel verlassen"),
    ("Quitter le jeu", "Quit game", "Salir del juego", "Spiel beenden"),
    ("Nouvelle partie", "New game", "Nueva partida", "Neues Spiel"),
    ("Seed de la ville (un nombre ou un mot)", "City seed (a number or a word)", "Semilla de la ciudad (número o palabra)", "Stadt-Seed (Zahl oder Wort)"),
    ("Au hasard", "Random", "Aleatorio", "Zufällig"),
    ("Hasard", "Random", "Azar", "Zufall"),
    ("Taille de la ville (plus petite = moins de mémoire)", "City size (smaller = less memory)", "Tamaño de la ciudad (más pequeña = menos memoria)", "Stadtgröße (kleiner = weniger Speicher)"),
    ("Petite", "Small", "Pequeña", "Klein"),
    ("Moyenne", "Medium", "Mediana", "Mittel"),
    ("Grande", "Large", "Grande", "Groß"),
    ("LANCER LA PARTIE", "START GAME", "EMPEZAR PARTIDA", "SPIEL STARTEN"),
    ("Mondes sauvegardés", "Saved worlds", "Mundos guardados", "Gespeicherte Welten"),
    ("Aucun monde sauvegardé pour l'instant.", "No saved world yet.", "Aún no hay mundos guardados.", "Noch keine gespeicherte Welt."),
    ("Charger", "Load", "Cargar", "Laden"),
    ("Ton pseudo", "Your name", "Tu nombre", "Dein Name"),
    ("Pseudo", "Name", "Nombre", "Name"),
    ("Créer un lobby", "Create a lobby", "Crear una sala", "Lobby erstellen"),
    ("Mode", "Mode", "Modo", "Modus"),
    ("Bac à sable", "Sandbox", "Sandbox", "Sandkasten"),
    ("Normal", "Normal", "Normal", "Normal"),
    ("Mode normal", "Normal mode", "Modo normal", "Normaler Modus"),
    ("Seed", "Seed", "Semilla", "Seed"),
    ("CRÉER LE LOBBY", "CREATE LOBBY", "CREAR SALA", "LOBBY ERSTELLEN"),
    ("Rejoindre", "Join", "Unirse", "Beitreten"),
    ("Mes serveurs (clique pour rejoindre)", "My servers (click to join)", "Mis servidores (pulsa para unirte)", "Meine Server (zum Beitreten klicken)"),
    ("Nom", "Name", "Nombre", "Name"),
    ("Adresse ou code", "Address or code", "Dirección o código", "Adresse oder Code"),
    ("Parties sur ton réseau", "Games on your network", "Partidas en tu red", "Spiele in deinem Netzwerk"),
    ("Code de partie ou adresse", "Game code or address", "Código de partida o dirección", "Spielcode oder Adresse"),
    ("ex. K7M2Q-9XH4C", "e.g. K7M2Q-9XH4C", "p. ej. K7M2Q-9XH4C", "z. B. K7M2Q-9XH4C"),
    ("Aucun serveur : ajoute-en un ci-dessous.", "No servers: add one below.", "Ningún servidor: añade uno abajo.", "Keine Server: füge unten einen hinzu."),
    ("Recherche de parties sur ton Wi-Fi…", "Looking for games on your Wi-Fi…", "Buscando partidas en tu Wi-Fi…", "Suche Spiele in deinem WLAN…"),
    ("en cours", "in progress", "en curso", "läuft"),
    ("hors ligne", "offline", "sin conexión", "offline"),
    (" · version différente", " · different version", " · versión diferente", " · andere Version"),
    ("Connexion…", "Connecting…", "Conectando…", "Verbinde…"),
    ("Connexion à {}…", "Connecting to {}…", "Conectando a {}…", "Verbinde mit {}…"),
    ("Connexion au lobby…", "Connecting to lobby…", "Conectando a la sala…", "Verbinde mit Lobby…"),
    ("Adresse vide ou déjà enregistrée.", "Empty or already saved address.", "Dirección vacía o ya guardada.", "Adresse leer oder schon gespeichert."),
    ("saisis le code de la partie (ex. K7M2Q-9XH4C) ou une adresse", "enter the game code (e.g. K7M2Q-9XH4C) or an address", "escribe el código (p. ej. K7M2Q-9XH4C) o una dirección", "gib den Spielcode (z. B. K7M2Q-9XH4C) oder eine Adresse ein"),
    ("saisis un code de partie ou une adresse", "enter a game code or an address", "escribe un código o una dirección", "gib einen Spielcode oder eine Adresse ein"),
    ("Sauvegarde du monde…", "Saving world…", "Guardando mundo…", "Speichere Welt…"),
    ("Aucune partie à sauvegarder.", "No game to save.", "No hay partida que guardar.", "Kein Spiel zum Speichern."),
    ("Copie impossible : note le code à la main", "Can't copy: write the code down", "No se puede copiar: apunta el código", "Kopieren unmöglich: Code notieren"),
    ("La partie commence !", "The game is starting!", "¡Empieza la partida!", "Das Spiel beginnt!"),
    ("Ce code permet à d'autres amis de rejoindre aussi.", "Friends can join with this code too.", "Con este código pueden unirse más amigos.", "Mit diesem Code können weitere Freunde beitreten."),
    ("HÔTE", "HOST", "ANFITRIÓN", "HOST"),
    ("Lobby", "Lobby", "Sala", "Lobby"),
    ("Code de la partie", "Game code", "Código de la partida", "Spielcode"),
    ("Copier", "Copy", "Copiar", "Kopieren"),
    ("Joueurs", "Players", "Jugadores", "Spieler"),
    ("En attente de l'hôte…", "Waiting for the host…", "Esperando al anfitrión…", "Warte auf den Host…"),
    ("Quitter le lobby", "Leave lobby", "Salir de la sala", "Lobby verlassen"),
    ("Solo", "Solo", "Solo", "Solo"),
    // --- Paramètres ---
    ("JEU ET CONTRÔLES", "GAME AND CONTROLS", "JUEGO Y CONTROLES", "SPIEL UND STEUERUNG"),
    ("Langue", "Language", "Idioma", "Sprache"),
    ("Sensibilité de la souris", "Mouse sensitivity", "Sensibilidad del ratón", "Mausempfindlichkeit"),
    ("Champ de vision", "Field of view", "Campo de visión", "Sichtfeld"),
    ("Secousses de caméra", "Camera shake", "Temblor de cámara", "Kamerawackeln"),
    ("Inverser l'axe vertical", "Invert vertical axis", "Invertir eje vertical", "Vertikale Achse umkehren"),
    ("Balancement à la marche", "Head bob", "Balanceo al caminar", "Kopfwippen"),
    ("ÉCRAN TACTILE", "TOUCH SCREEN", "PANTALLA TÁCTIL", "TOUCHSCREEN"),
    ("Côté du joystick", "Joystick side", "Lado del joystick", "Joystick-Seite"),
    ("Sensibilité du regard", "Look sensitivity", "Sensibilidad de la vista", "Blickempfindlichkeit"),
    ("SON", "SOUND", "SONIDO", "TON"),
    ("Volume général", "Master volume", "Volumen general", "Gesamtlautstärke"),
    ("Bruitages", "Sound effects", "Efectos", "Effekte"),
    ("Musique", "Music", "Música", "Musik"),
    ("Voix de proximité", "Proximity voice", "Voz de proximidad", "Näherungs-Sprachchat"),
    ("Volume des voix", "Voice volume", "Volumen de voces", "Sprachlautstärke"),
    ("AFFICHAGE", "DISPLAY", "PANTALLA", "ANZEIGE"),
    ("Préréglage graphique", "Graphics preset", "Preajuste gráfico", "Grafik-Voreinstellung"),
    ("Distance d'affichage", "View distance", "Distancia de visión", "Sichtweite"),
    ("Qualité des ombres", "Shadow quality", "Calidad de sombras", "Schattenqualität"),
    ("Anticrénelage", "Anti-aliasing", "Antialiasing", "Kantenglättung"),
    ("Poussière et fumée", "Dust and smoke", "Polvo y humo", "Staub und Rauch"),
    ("Halo lumineux (bloom)", "Glow (bloom)", "Resplandor (bloom)", "Leuchten (Bloom)"),
    ("Limite d'images", "Frame limit", "Límite de FPS", "Bildratenlimit"),
    ("Plein écran", "Fullscreen", "Pantalla completa", "Vollbild"),
    ("INTERFACE", "INTERFACE", "INTERFAZ", "OBERFLÄCHE"),
    ("Afficher les FPS", "Show FPS", "Mostrar FPS", "FPS anzeigen"),
    ("Afficher le viseur", "Show crosshair", "Mostrar mira", "Fadenkreuz anzeigen"),
    ("Désinstaller", "Uninstall", "Desinstalar", "Deinstallieren"),
    ("aucun", "none", "ninguno", "aus"),
    ("aucune", "none", "ninguna", "keine"),
    ("réduites", "reduced", "reducidas", "reduziert"),
    ("pleines", "full", "completas", "voll"),
    ("moyenne", "medium", "media", "mittel"),
    ("haute", "high", "alta", "hoch"),
    ("gauche", "left", "izquierda", "links"),
    ("droite", "right", "derecha", "rechts"),
    ("désactivée", "off", "desactivada", "aus"),
    ("activation vocale", "voice activation", "activación por voz", "Sprachaktivierung"),
    ("touche G", "G key", "tecla G", "Taste G"),
    ("illimitée", "unlimited", "ilimitada", "unbegrenzt"),
    ("écran", "screen", "pantalla", "Bildschirm"),
    ("Bas", "Low", "Bajo", "Niedrig"),
    ("Moyen", "Medium", "Medio", "Mittel"),
    ("Élevé", "High", "Alto", "Hoch"),
    ("Ultra", "Ultra", "Ultra", "Ultra"),
    ("Personnalisé", "Custom", "Personalizado", "Benutzerdefiniert"),
    // --- Installation ---
    ("Une version est déjà installée.", "A version is already installed.", "Ya hay una versión instalada.", "Eine Version ist bereits installiert."),
    ("Bienvenue !", "Welcome!", "¡Bienvenido!", "Willkommen!"),
    ("Dossier : {}", "Folder: {}", "Carpeta: {}", "Ordner: {}"),
    ("Raccourci sur le Bureau", "Desktop shortcut", "Acceso directo en el escritorio", "Desktop-Verknüpfung"),
    ("Autoriser le multijoueur dans le pare-feu", "Allow multiplayer in the firewall", "Permitir multijugador en el cortafuegos", "Mehrspieler in der Firewall erlauben"),
    ("METTRE À JOUR", "UPDATE", "ACTUALIZAR", "AKTUALISIEREN"),
    ("INSTALLER", "INSTALL", "INSTALAR", "INSTALLIEREN"),
    ("Jouer sans installer", "Play without installing", "Jugar sin instalar", "Ohne Installation spielen"),
    ("Installé ! Lancement… {}", "Installed! Launching… {}", "¡Instalado! Iniciando… {}", "Installiert! Starte… {}"),
    ("Désinstallation impossible.", "Uninstall failed.", "No se pudo desinstalar.", "Deinstallation fehlgeschlagen."),
    // --- HUD ---
    ("Génération de la ville…", "Generating the city…", "Generando la ciudad…", "Stadt wird erzeugt…"),
    ("Chargement du terrain…", "Loading terrain…", "Cargando terreno…", "Lade Gelände…"),
    ("Chargement…", "Loading…", "Cargando…", "Lädt…"),
    ("Seed {} · {} voxels détruits", "Seed {} · {} voxels destroyed", "Semilla {} · {} vóxeles destruidos", "Seed {} · {} Voxel zerstört"),
    (" · {} en ligne", " · {} online", " · {} en línea", " · {} online"),
    ("illimité", "unlimited", "ilimitado", "unbegrenzt"),
    ("Rechargement…", "Reloading…", "Recargando…", "Lade nach…"),
    ("R pour recharger", "R to reload", "R para recargar", "R zum Nachladen"),
    ("Mine : se déclenche à l'approche", "Mine: triggers when approached", "Mina: se activa al acercarse", "Mine: löst bei Annäherung aus"),
    ("TIR : lancer", "FIRE: throw", "DISPARO: lanzar", "FEUER: werfen"),
    ("Clic gauche : lancer", "Left click: throw", "Clic izquierdo: lanzar", "Linksklick: werfen"),
    ("TIR : poser · ALT : matériau", "FIRE: place · ALT: material", "DISPARO: colocar · ALT: material", "FEUER: setzen · ALT: Material"),
    ("Clic : poser · clic droit : matériau", "Click: place · right click: material", "Clic: colocar · clic derecho: material", "Klick: setzen · Rechtsklick: Material"),
    ("{} posée(s) · {} : BOUM", "{} placed · {}: BOOM", "{} colocada(s) · {}: BUM", "{} gelegt · {}: BUMM"),
    ("clic droit", "right click", "clic derecho", "Rechtsklick"),
    ("Minuterie de {} s : cours !", "{} s timer: run!", "Temporizador de {} s: ¡corre!", "{} s Zünder: lauf!"),
    ("Maintiens le clic", "Hold the click", "Mantén el clic", "Klick halten"),
    ("Rayon de destruction", "Destruction radius", "Radio de destrucción", "Zerstörungsradius"),
    ("  (hôte)", "  (host)", "  (anfitrión)", "  (Host)"),
    ("  ← toi", "  ← you", "  ← tú", "  ← du"),
    ("Message…", "Message…", "Mensaje…", "Nachricht…"),
    ("réserve : {}", "reserve: {}", "reserva: {}", "Reserve: {}"),
    ("Tu es mort · réapparition dans {} s", "You died · respawn in {} s", "Has muerto · reapareces en {} s", "Du bist tot · Wiedereinstieg in {} s"),
    ("Code : {}", "Code: {}", "Código: {}", "Code: {}"),
    ("Code copié : {}", "Code copied: {}", "Código copiado: {}", "Code kopiert: {}"),
    ("{} a rejoint la partie", "{} joined the game", "{} se ha unido", "{} ist beigetreten"),
    ("{} est parti", "{} left", "{} se ha ido", "{} ist gegangen"),
    ("Impossible : {}", "Not possible: {}", "Imposible: {}", "Nicht möglich: {}"),
    ("Dernier plantage : {}", "Last crash: {}", "Último fallo: {}", "Letzter Absturz: {}"),
    // --- Boutique ---
    ("ARME À FEU", "FIREARM", "ARMA DE FUEGO", "SCHUSSWAFFE"),
    ("BOMBE LANCÉE", "THROWN BOMB", "BOMBA LANZADA", "WURFBOMBE"),
    ("BOMBE POSÉE", "PLACED BOMB", "BOMBA COLOCADA", "GELEGTE BOMBE"),
    ("Chaque voxel cassé rapporte de l'argent (le sol ne vaut rien).", "Every broken voxel earns money (the ground is worth nothing).", "Cada vóxel roto da dinero (el suelo no vale nada).", "Jeder zerstörte Voxel bringt Geld (der Boden ist nichts wert)."),
    ("Fermer (B)", "Close (B)", "Cerrar (B)", "Schließen (B)"),
    ("{} acheté !", "{} bought!", "¡{} comprado!", "{} gekauft!"),
    ("+{} munitions pour {}", "+{} ammo for {}", "+{} munición para {}", "+{} Munition für {}"),
    ("Possédé", "Owned", "Comprado", "Besessen"),
    ("Munitions : {} + {}", "Ammo: {} + {}", "Munición: {} + {}", "Munition: {} + {}"),
    ("En stock : {}", "In stock: {}", "En stock: {}", "Vorrat: {}"),
    ("Dans ton arsenal", "In your arsenal", "En tu arsenal", "In deinem Arsenal"),
    ("déjà possédé", "already owned", "ya comprado", "bereits besessen"),
    ("pas assez d'argent", "not enough money", "no tienes dinero suficiente", "nicht genug Geld"),
    ("pas de munitions", "no ammo", "sin munición", "keine Munition"),
    ("achetez d'abord l'arme", "buy the weapon first", "compra primero el arma", "kauf zuerst die Waffe"),
    // --- Catégories ---
    ("Mêlée", "Melee", "Cuerpo a cuerpo", "Nahkampf"),
    ("Pistolets", "Pistols", "Pistolas", "Pistolen"),
    ("Fusils", "Rifles", "Fusiles", "Gewehre"),
    ("Lourdes", "Heavy", "Pesadas", "Schwere"),
    ("Explosifs", "Explosives", "Explosivos", "Sprengstoff"),
    ("Outils", "Tools", "Herramientas", "Werkzeuge"),
    // --- Armes ---
    ("Marteau", "Hammer", "Martillo", "Hammer"),
    ("Masse", "Sledge", "Mazo", "Vorschlag"),
    ("Masse de guerre", "War hammer", "Martillo de guerra", "Kriegshammer"),
    ("Guerre", "War", "Guerra", "Krieg"),
    ("Pioche", "Pickaxe", "Pico", "Spitzhacke"),
    ("Marteau-piqueur", "Jackhammer", "Martillo neumático", "Presslufthammer"),
    ("Piqueur", "Jackham.", "Neumático", "Pressluft"),
    ("Pistolet-mitrailleur", "Submachine gun", "Subfusil", "Maschinenpistole"),
    ("Fusil à pompe", "Shotgun", "Escopeta", "Schrotflinte"),
    ("Pompe", "Shotgun", "Escopeta", "Schrot"),
    ("Fusil d'assaut", "Assault rifle", "Fusil de asalto", "Sturmgewehr"),
    ("Fusil", "Rifle", "Fusil", "Gewehr"),
    ("Lance-roquettes", "Rocket launcher", "Lanzacohetes", "Raketenwerfer"),
    ("Roquette", "Rocket", "Cohete", "Rakete"),
    ("Méga-bombe", "Mega bomb", "Megabomba", "Megabombe"),
    ("Méga", "Mega", "Mega", "Mega"),
    ("Blocs", "Blocks", "Bloques", "Blöcke"),
    ("Hache", "Axe", "Hacha", "Axt"),
    ("Pied-de-biche", "Crowbar", "Palanca", "Brecheisen"),
    ("Biche", "Crowbar", "Palanca", "Brecheisen"),
    ("Machette", "Machete", "Machete", "Machete"),
    ("Pelle", "Shovel", "Pala", "Schaufel"),
    ("Tronçonneuse", "Chainsaw", "Motosierra", "Kettensäge"),
    ("Tronçon.", "Chainsaw", "Motosierra", "Säge"),
    ("Maillet", "Mallet", "Mazo", "Holzhammer"),
    ("Boulet", "Wrecking ball", "Bola de demolición", "Abrissbirne"),
    ("Perceuse", "Drill", "Taladro", "Bohrer"),
    ("Poing américain", "Brass knuckles", "Puño americano", "Schlagring"),
    ("Poing", "Fist", "Puño", "Faust"),
    ("Pistolet 1911", "1911 pistol", "Pistola 1911", "Pistole 1911"),
    ("Pistolet silencieux", "Silenced pistol", "Pistola con silenciador", "Schallgedämpfte Pistole"),
    ("Silence", "Silenced", "Silenciador", "Schalldämpfer"),
    ("Pistolet automatique", "Machine pistol", "Pistola automática", "Automatikpistole"),
    ("Auto", "Auto", "Auto", "Auto"),
    ("Désert", "Desert", "Desierto", "Wüste"),
    ("Pistolet de détresse", "Flare gun", "Pistola de bengalas", "Leuchtpistole"),
    ("Fusée", "Flare", "Bengala", "Leuchtrakete"),
    ("Cloueur", "Nail gun", "Clavadora", "Nagler"),
    ("Pistolet laser", "Laser pistol", "Pistola láser", "Laserpistole"),
    ("Carabine", "Carbine", "Carabina", "Karabiner"),
    ("Fusil à rafales", "Burst rifle", "Fusil de ráfagas", "Feuerstoßgewehr"),
    ("Rafale", "Burst", "Ráfaga", "Feuerstoß"),
    ("Fusil de précision", "Marksman rifle", "Fusil de precisión", "Präzisionsgewehr"),
    ("Précision", "Marksman", "Precisión", "Präzision"),
    ("Canon scié", "Sawed-off", "Recortada", "Abgesägte"),
    ("Scié", "Sawed-off", "Recortada", "Abgesägt"),
    ("Fusil automatique", "Auto shotgun", "Escopeta automática", "Automatikschrotflinte"),
    ("Auto-pompe", "Auto shotgun", "Escopeta auto", "Auto-Schrot"),
    ("Fusil à chevrotine lourde", "Slug shotgun", "Escopeta de posta", "Flintenlaufgeschoss"),
    ("Mitrailleuse légère", "Light machine gun", "Ametralladora ligera", "Leichtes MG"),
    ("MitrLég.", "LMG", "Ametr.", "LMG"),
    ("Arbalète", "Crossbow", "Ballesta", "Armbrust"),
    ("Arc", "Bow", "Arco", "Bogen"),
    ("Lance-flammes", "Flamethrower", "Lanzallamas", "Flammenwerfer"),
    ("Flammes", "Flames", "Llamas", "Flammen"),
    ("Fusil antimatériel", "Anti-materiel rifle", "Fusil antimaterial", "Anti-Materiel-Gewehr"),
    ("Canon Gauss", "Gauss cannon", "Cañón Gauss", "Gaußkanone"),
    ("Fusil à plasma", "Plasma rifle", "Fusil de plasma", "Plasmagewehr"),
    ("Mini-missiles", "Mini missiles", "Minimisiles", "Mini-Raketen"),
    ("Missiles", "Missiles", "Misiles", "Raketen"),
    ("Lance-grenades", "Grenade launcher", "Lanzagranadas", "Granatwerfer"),
    ("Lance-gren.", "Gren. launcher", "Lanzagr.", "Granatw."),
    ("Canon", "Cannon", "Cañón", "Kanone"),
    ("Lance-thermite", "Thermite launcher", "Lanzatermita", "Thermitwerfer"),
    ("Fusil laser", "Laser rifle", "Fusil láser", "Lasergewehr"),
    ("Grenade à fragmentation", "Frag grenade", "Granada de fragmentación", "Splittergranate"),
    ("Grenade à impact", "Impact grenade", "Granada de impacto", "Aufschlaggranate"),
    ("Cocktail Molotov", "Molotov cocktail", "Cóctel molotov", "Molotowcocktail"),
    ("Bombe collante", "Sticky bomb", "Bomba adhesiva", "Haftbombe"),
    ("Collante", "Sticky", "Adhesiva", "Haft"),
    ("Feu d'artifice", "Fireworks", "Fuegos artificiales", "Feuerwerk"),
    ("Artifice", "Fireworks", "Fuegos", "Feuerwerk"),
    ("Mine", "Mine", "Mina", "Mine"),
    ("Charge creuse", "Shaped charge", "Carga hueca", "Hohlladung"),
    ("Creuse", "Shaped", "Hueca", "Hohl"),
    ("Bombe à retardement", "Time bomb", "Bomba de relojería", "Zeitbombe"),
    ("Retard", "Timer", "Retardo", "Zeitzünder"),
    ("Pack de dynamite", "Dynamite pack", "Paquete de dinamita", "Dynamitpaket"),
    ("Extincteur", "Extinguisher", "Extintor", "Feuerlöscher"),
    ("Chalumeau", "Blowtorch", "Soplete", "Lötlampe"),
    // --- Matériaux ---
    ("Bois", "Wood", "Madera", "Holz"),
    ("Brique", "Brick", "Ladrillo", "Ziegel"),
    ("Béton", "Concrete", "Hormigón", "Beton"),
    ("Métal", "Metal", "Metal", "Metall"),
    ("Verre", "Glass", "Vidrio", "Glas"),
    ("Plâtre", "Plaster", "Yeso", "Gips"),
    ("Tuile", "Tile", "Teja", "Ziegelstein"),
    ("Noir", "Black", "Negro", "Schwarz"),
    ("Lumière", "Light", "Luz", "Licht"),
    ("Bloc", "Block", "Bloque", "Block"),
    // --- Serveur (messages reçus en jeu) ---
    ("aucune partie à cette adresse (lobby fermé, ou box de l'hôte fermée)", "no game at this address (lobby closed, or host's router closed)", "no hay partida en esta dirección (sala cerrada o router cerrado)", "kein Spiel unter dieser Adresse (Lobby geschlossen oder Router zu)"),
    ("aucune adresse joignable", "no reachable address", "ninguna dirección accesible", "keine erreichbare Adresse"),
    ("l'hôte a fermé la partie", "the host closed the game", "el anfitrión cerró la partida", "der Host hat das Spiel beendet"),
    ("le serveur ne répond pas", "the server is not responding", "el servidor no responde", "der Server antwortet nicht"),
    ("connexion fermée", "connection closed", "conexión cerrada", "Verbindung geschlossen"),
    ("inactif trop longtemps", "idle for too long", "inactivo demasiado tiempo", "zu lange inaktiv"),
    ("Banni", "Banned", "Expulsado permanentemente", "Gebannt"),
    ("Expulsé par le serveur", "Kicked by the server", "Expulsado por el servidor", "Vom Server gekickt"),
    ("Serveur arrêté", "Server stopped", "Servidor detenido", "Server gestoppt"),
    ("La porte est cassée ou bloquée.", "The door is broken or blocked.", "La puerta está rota o bloqueada.", "Die Tür ist kaputt oder blockiert."),
    ("Cette arme est désactivée sur ce serveur.", "This weapon is disabled on this server.", "Esta arma está desactivada en este servidor.", "Diese Waffe ist auf diesem Server deaktiviert."),
    ("Ce véhicule est trop loin.", "This vehicle is too far away.", "Este vehículo está demasiado lejos.", "Dieses Fahrzeug ist zu weit weg."),
    ("Ce véhicule est hors d'usage.", "This vehicle is wrecked.", "Este vehículo está inservible.", "Dieses Fahrzeug ist kaputt."),
    ("Le véhicule est hors d'usage.", "The vehicle is wrecked.", "El vehículo está inservible.", "Das Fahrzeug ist kaputt."),
    ("Cette voiture est trop abîmée.", "This car is too damaged.", "Este coche está demasiado dañado.", "Dieses Auto ist zu beschädigt."),
    ("Impossible de démarrer ce véhicule.", "Can't start this vehicle.", "No se puede arrancar este vehículo.", "Fahrzeug lässt sich nicht starten."),
    ("La place est prise.", "The seat is taken.", "El asiento está ocupado.", "Der Platz ist besetzt."),
    ("Le véhicule est trop endommagé pour rouler.", "The vehicle is too damaged to drive.", "El vehículo está demasiado dañado para conducir.", "Das Fahrzeug ist zu beschädigt zum Fahren."),
    ("Véhicule détruit.", "Vehicle destroyed.", "Vehículo destruido.", "Fahrzeug zerstört."),
    ("Tu es réduit au silence.", "You are muted.", "Estás silenciado.", "Du bist stummgeschaltet."),
    ("Doucement ! Trop de messages.", "Slow down! Too many messages.", "¡Más despacio! Demasiados mensajes.", "Langsam! Zu viele Nachrichten."),
    ("version du jeu différente : mettez BoomGround à jour", "different game version: update BoomGround", "versión del juego diferente: actualiza BoomGround", "andere Spielversion: aktualisiere BoomGround"),
    ("la partie est pleine", "the game is full", "la partida está llena", "das Spiel ist voll"),
    ("tu es banni de ce serveur", "you are banned from this server", "estás vetado en este servidor", "du bist auf diesem Server gebannt"),
    ("Monde sauvegardé : {}", "World saved: {}", "Mundo guardado: {}", "Welt gespeichert: {}"),
    // --- Voix ---
    ("aucune sortie audio", "no audio output", "sin salida de audio", "keine Audioausgabe"),
    ("aucun microphone", "no microphone", "sin micrófono", "kein Mikrofon"),
    ("Tu parles", "You're talking", "Estás hablando", "Du sprichst"),
    ("{} parle", "{} is talking", "{} habla", "{} spricht"),
    // --- Tactile ---
    ("TIR", "FIRE", "DISPARO", "FEUER"),
    ("CHAT", "CHAT", "CHAT", "CHAT"),
];
