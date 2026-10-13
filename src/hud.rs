//! Interface en jeu : viseur, argent, barre d'armes, munitions, joueurs en
//! ligne (Tab), FPS, aide.

use bevy::prelude::*;

use crate::game::{GameState, Kind, Mode, WeaponId, ALL_WEAPONS};
use crate::net::client::Lobby;
use crate::ui::{self, UiFont};
use crate::weapon::{Bomb, Hands};
use crate::world::VoxelWorld;

/// Marque les éléments d'interface (masqués hors du jeu).
#[derive(Component)]
pub struct Hud;

/// Texte du HUD mis à jour chaque image.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HudLabel {
    Money,
    Info,
    Fps,
    WeaponName,
    Ammo,
    Hint,
    Gain,
    Players,
    /// Message de mort, au centre de l'écran.
    Death,
}
#[derive(Component)]
pub struct ReloadBar;
#[derive(Component)]
pub struct PlayersPanel;
#[derive(Component)]
pub struct CatChip(pub crate::game::Cat);
#[derive(Component)]
pub struct HpBar;
#[derive(Component)]
pub struct HpPanel;
#[derive(Component)]
pub struct CrosshairRoot;
#[derive(Component)]
pub struct LoadingRoot;
#[derive(Component)]
pub struct DamageOverlay;
#[derive(Component)]
pub struct LoadingBar;
#[derive(Component)]
pub struct LoadingText;

/// Case de la barre d'armes.
#[derive(Component)]
pub struct HotbarSlot(WeaponId);

/// Texte d'une case : touche, nom ou stock.
#[derive(Component, Clone, Copy)]
pub enum HotbarText {
    Key(WeaponId),
    Name(WeaponId),
    Count(WeaponId),
}

/// Compteur d'images : moyenne glissante sur une fenêtre fixe.
#[derive(Resource, Default)]
pub struct FpsMeter {
    elapsed: f32,
    frames: u32,
}

fn chip() -> (Node, BackgroundColor) {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(14.0), px(10.0)),
            border_radius: BorderRadius::all(px(10.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.62)),
    )
}

pub fn setup(mut commands: Commands, font: Res<UiFont>) {
    let font = font.into_inner().clone();

    // Viseur : quatre traits et un point, contour sombre pour rester
    // lisible sur fond clair.
    commands
        .spawn((
            Hud,
            CrosshairRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: px(0.0),
                height: px(0.0),
                ..default()
            },
        ))
        .with_children(|c| {
            for (x, y, w, h) in [
                (-1.0, -12.0, 2.0, 7.0),
                (-1.0, 5.0, 2.0, 7.0),
                (-12.0, -1.0, 7.0, 2.0),
                (5.0, -1.0, 7.0, 2.0),
                (-1.5, -1.5, 3.0, 3.0),
            ] {
                c.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(x),
                        top: px(y),
                        width: px(w),
                        height: px(h),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.92)),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0.0), px(0.0), px(1.0), px(1.0)),
                ));
            }
            // Gain d'argent, à droite du viseur.
            c.spawn((
                HudLabel::Gain,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(22.0),
                    top: px(-12.0),
                    ..default()
                },
                ui::text(&font, "", 18.0, ui::GREEN),
            ));
        });

    // Argent / mode (haut gauche).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                left: px(16),
                top: px(16),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(chip()).with_children(|c| {
                c.spawn((HudLabel::Money, ui::text(&font, "", 26.0, ui::TEXT)));
                c.spawn((HudLabel::Info, ui::text(&font, "", 13.0, ui::MUTED)));
            });
        });

    // FPS (haut droite) + joueurs en ligne (Tab).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                top: px(16),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(8.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((HudLabel::Fps, ui::text(&font, "", 13.0, Color::srgba(1.0, 1.0, 1.0, 0.6))));
            p.spawn((PlayersPanel, chip()))
                .insert(Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(16.0), px(12.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    display: Display::None,
                    ..default()
                })
                .with_child((HudLabel::Players, ui::text(&font, "", 15.0, ui::TEXT)));
        });

    // Voile rouge des dégâts (sous l'interface).
    commands.spawn((
        Hud,
        DamageOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.8, 0.0, 0.0, 0.0)),
        GlobalZIndex(-1),
    ));

    // Écran de chargement (par-dessus tout).
    commands
        .spawn((
            LoadingRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18.0),
                // Visible dès la première image (démarrage du jeu).
                display: Display::Flex,
                ..default()
            },
            BackgroundColor(Color::srgb(0.03, 0.035, 0.05)),
            GlobalZIndex(300),
        ))
        .with_children(|p| {
            p.spawn(ui::text(&font, "BOOMGROUND", 54.0, ui::ACCENT));
            p.spawn((LoadingText, ui::text(&font, "Génération de la ville…", 22.0, ui::TEXT)));
            p.spawn((
                Node {
                    width: px(420.0),
                    height: px(8.0),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
            ))
            .with_child((
                LoadingBar,
                Node {
                    width: Val::Percent(0.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(ui::ACCENT),
            ));
        });

    // Points de vie (bas gauche).
    commands
        .spawn((
            Hud,
            HpPanel,
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                bottom: px(104),
                width: px(220.0),
                height: px(10.0),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        ))
        .with_child((
            HpBar,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.85, 0.2, 0.2)),
        ));

    // Message de mort (centre).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(120),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((HudLabel::Death, ui::text(&font, "", 30.0, Color::srgb(1.0, 0.45, 0.4))));

    // Arme en main + munitions (bas droite).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                right: px(20),
                bottom: px(104),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(chip())
                .insert(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    padding: UiRect::axes(px(18.0), px(12.0)),
                    border_radius: BorderRadius::all(px(12.0)),
                    min_width: px(200.0),
                    ..default()
                })
                .with_children(|c| {
                    c.spawn((HudLabel::WeaponName, ui::text(&font, "", 17.0, ui::MUTED)));
                    c.spawn((HudLabel::Ammo, ui::text(&font, "", 38.0, ui::TEXT)));
                    c.spawn((HudLabel::Hint, ui::text(&font, "", 13.0, ui::MUTED)));
                    c.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: px(4.0),
                            margin: UiRect::top(px(6.0)),
                            border_radius: BorderRadius::all(px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1)),
                    ))
                    .with_child((
                        ReloadBar,
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(px(2.0)),
                            ..default()
                        },
                        BackgroundColor(ui::ACCENT),
                    ));
                });
        });

    // Catégories d'armes (au-dessus de la barre).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(96),
                width: Val::Percent(100.0),
                // Sur mobile, les commandes tactiles remplacent ces barres.
                display: if crate::touch::is_touch_platform() { Display::None } else { Display::Flex },
                justify_content: JustifyContent::Center,
                column_gap: px(6.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for (i, cat) in crate::game::Cat::ALL.iter().enumerate() {
                row.spawn((
                    CatChip(*cat),
                    Node {
                        padding: UiRect::axes(px(10.0), px(3.0)),
                        border_radius: BorderRadius::all(px(7.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.6)),
                    ui::text(&font, format!("{} {}", i + 1, cat.name()), 13.0, ui::MUTED),
                ));
            }
        });

    // Barre d'armes (bas centre).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(18),
                width: Val::Percent(100.0),
                // Sur mobile, les commandes tactiles remplacent ces barres.
                display: if crate::touch::is_touch_platform() { Display::None } else { Display::Flex },
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                column_gap: px(4.0),
                ..default()
            },
        ))
        .with_children(|bar| {
            for &id in &ALL_WEAPONS {
                bar.spawn((
                    HotbarSlot(id),
                    Node {
                        width: px(58.0),
                        height: px(62.0),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(2.0)),
                        border_radius: BorderRadius::all(px(10.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.6)),
                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                ))
                .with_children(|s| {
                    s.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|top| {
                        top.spawn((HotbarText::Key(id), ui::text(&font, "", 12.0, ui::MUTED)));
                        top.spawn((HotbarText::Count(id), ui::text(&font, "", 12.0, ui::ACCENT)));
                    });
                    s.spawn((HotbarText::Name(id), ui::text(&font, "", 11.0, ui::TEXT)));
                });
            }
        });
}

fn set(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

/// Sépare les milliers : 12500 → « 12 500 ».
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Met à jour l'interface (le texte n'est réécrit que s'il change, pour ne
/// pas refaire la mise en page à chaque image).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut gain: Local<(u32, u32, f32)>,
    world: Res<VoxelWorld>,
    game: Res<GameState>,
    hands: Res<Hands>,
    settings: Res<crate::game::Settings>,
    lobby: Res<Lobby>,
    net: Res<crate::net::client::NetClient>,
    mut meter: ResMut<FpsMeter>,
    bombs: Query<&Bomb>,
    mut labels: Query<(&HudLabel, &mut Text), (Without<HotbarText>, Without<CatChip>)>,
    mut chips: Query<(&CatChip, &mut Text, &mut TextColor, &mut BackgroundColor), (Without<HudLabel>, Without<HotbarSlot>)>,
    mut slot_texts: Query<(&HotbarText, &mut Text), (Without<HudLabel>, Without<CatChip>)>,
    mut nodes: ParamSet<(
        Query<(&HotbarSlot, &mut Node, &mut BorderColor, &mut BackgroundColor)>,
        Query<&mut Node, With<ReloadBar>>,
        Query<&mut Node, With<PlayersPanel>>,
        Query<&mut Node, (With<HpBar>, Without<HotbarSlot>, Without<ReloadBar>, Without<PlayersPanel>)>,
        Query<&mut Node, (With<CrosshairRoot>, Without<HotbarSlot>, Without<ReloadBar>, Without<PlayersPanel>, Without<HpBar>)>,
    )>,
) {
    let mut values: Vec<(HudLabel, String)> = Vec::new();
    // --- Argent / mode ---
    let money = match game.mode {
        Mode::Sandbox => "BAC À SABLE".to_string(),
        Mode::Normal => format!("{} $", thousands(game.money)),
    };
    values.push((HudLabel::Money, money));
    let destroyed = world.total.saturating_sub(world.alive);
    let mut info = format!("Seed {} · {} voxels détruits", world.seed, thousands(destroyed));
    if lobby.active && !lobby.solo {
        info.push_str(&format!(" · {} en ligne", lobby.players.len()));
    }
    values.push((HudLabel::Info, info));

    // --- Gain d'argent (« +12 $ ») ---
    let (last, shown, timer) = &mut *gain;
    if game.money > *last && game.mode == Mode::Normal {
        *shown += game.money - *last;
        *timer = 1.2;
    }
    *last = game.money;
    *timer -= time.delta_secs();
    if *timer <= 0.0 {
        *shown = 0;
    }
    let gain_text = if *shown > 0 { format!("+{} $", thousands(*shown)) } else { String::new() };
    values.push((HudLabel::Gain, gain_text));

    // --- FPS : moyenne sur 0,5 s pour rester lisible ---
    meter.elapsed += time.delta_secs();
    meter.frames += 1;
    if meter.elapsed >= 0.5 {
        let average = meter.frames as f32 / meter.elapsed;
        if std::env::var_os("BOOM_FPSLOG").is_some() {
            eprintln!("FPS {average:.0}");
        }
        let fps = if settings.show_fps { format!("{average:.0} FPS") } else { String::new() };
        values.push((HudLabel::Fps, fps));
        meter.elapsed = 0.0;
        meter.frames = 0;
    }

    // --- Arme en main ---
    let id = game.weapon;
    let spec = id.spec();
    let slot = game.slot(id);
    let infinite = game.sandbox();
    let (ammo, hint) = match spec.kind {
        Kind::Gun => {
            let reserve = if infinite { "illimitée".to_string() } else { slot.reserve.to_string() };
            let hint = if hands.reload > 0.0 {
                "Rechargement…".to_string()
            } else if slot.mag == 0 {
                "R pour recharger".to_string()
            } else {
                format!("réserve : {reserve}")
            };
            (format!("{} / {}", slot.mag, spec.mag), hint)
        }
        Kind::Thrown | Kind::Placed => {
            let stock = if infinite { "illimité".to_string() } else { format!("× {}", slot.reserve) };
            let hint = if spec.remote {
                let placed = bombs.iter().filter(|b| b.local && b.id == id).count();
                format!("{placed} posée(s) · {} : BOUM", if crate::touch::is_touch_platform() { "ALT" } else { "clic droit" })
            } else if spec.trigger > 0.0 {
                "Mine : se déclenche à l'approche".to_string()
            } else if spec.kind == Kind::Placed && spec.fuse > 0.0 {
                format!("Minuterie de {:.0} s : cours !", spec.fuse)
            } else {
                if crate::touch::is_touch_platform() { "TIR : lancer" } else { "Clic gauche : lancer" }.to_string()
            };
            (stock, hint)
        }
        Kind::Build => (
            crate::world::material_name(game.build_material()).to_string(),
            if crate::touch::is_touch_platform() { "TIR : poser · ALT : matériau" } else { "Clic : poser · clic droit : matériau" }.to_string(),
        ),
        Kind::Melee => (
            format!("{:.1} m", spec.radius),
            if spec.auto {
                "Maintiens le clic".to_string()
            } else {
                "Rayon de destruction".to_string()
            },
        ),
    };
    values.push((HudLabel::WeaponName, spec.name.to_string()));
    values.push((HudLabel::Ammo, ammo));
    values.push((HudLabel::Hint, hint));
    if let Ok(mut bar) = nodes.p1().single_mut() {
        let p = if hands.reload > 0.0 && hands.reload_total > 0.0 {
            (1.0 - hands.reload / hands.reload_total) * 100.0
        } else {
            0.0
        };
        let w = Val::Percent(p);
        if bar.width != w {
            bar.width = w;
        }
    }

    // --- Points de vie, viseur, message de mort ---
    if let Ok(mut bar) = nodes.p3().single_mut() {
        let w = Val::Percent((game.hp / crate::game::MAX_HP * 100.0).clamp(0.0, 100.0));
        if bar.width != w {
            bar.width = w;
        }
    }
    if let Ok(mut node) = nodes.p4().single_mut() {
        let d = if settings.crosshair && !game.dead { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    }
    let death = if game.dead {
        format!("Tu es mort · réapparition dans {:.0} s", game.respawn_in.ceil().max(0.0))
    } else {
        String::new()
    };
    values.push((HudLabel::Death, death));

    // --- Joueurs en ligne (Tab maintenu) ---
    let show_players = lobby.active && !lobby.solo && keys.pressed(KeyCode::Tab);
    if let Ok(mut node) = nodes.p2().single_mut() {
        let d = if show_players { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    }
    if show_players {
        let mut list = format!("Code : {}\n", crate::menu::lobby_code(&lobby));
        for (pid, name) in &lobby.players {
            let host = if *pid == lobby.host_id { "  (hôte)" } else { "" };
            let me = if Some(*pid) == net.id { "  ← toi" } else { "" };
            list.push_str(&format!("\n•  {name}{host}{me}"));
        }
        values.push((HudLabel::Players, list));
    }

    // --- Barre d'armes : armes possédées, numérotées dans l'ordre ---
    let all_owned = game.owned();
    let current_cat = game.weapon.cat();
    // Seules les armes de la catégorie en cours s'affichent.
    let owned: Vec<WeaponId> = all_owned.iter().copied().filter(|w| w.cat() == current_cat).collect();
    for (slot_id, mut node, mut border, mut background) in &mut nodes.p0() {
        let index = owned.iter().position(|&w| w == slot_id.0);
        let display = if index.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let selected = slot_id.0 == game.weapon;
        let b = if selected { ui::ACCENT } else { Color::srgba(1.0, 1.0, 1.0, 0.08) };
        if border.top != b {
            *border = BorderColor::all(b);
        }
        let bg = if selected {
            Color::srgba(0.18, 0.13, 0.03, 0.85)
        } else {
            Color::srgba(0.02, 0.025, 0.04, 0.6)
        };
        if background.0 != bg {
            background.0 = bg;
        }
        let h = px(if selected { 72.0 } else { 62.0 });
        if node.height != h {
            node.height = h;
        }
    }
    for (chip, mut text, mut color, mut bg) in &mut chips {
        let n = all_owned.iter().filter(|w| w.cat() == chip.0).count();
        let active = chip.0 == current_cat;
        let c = if active { ui::ACCENT } else if n == 0 { Color::srgba(1.0, 1.0, 1.0, 0.25) } else { ui::TEXT };
        let value = format!("{} {}", chip.0.index() + 1, chip.0.name());
        if text.0 != value {
            text.0 = value;
        }
        if color.0 != c {
            color.0 = c;
        }
        let b = if active { Color::srgba(0.18, 0.13, 0.03, 0.85) } else { Color::srgba(0.02, 0.025, 0.04, 0.6) };
        if bg.0 != b {
            bg.0 = b;
        }
    }
    for (part, mut t) in &mut slot_texts {
        let value = match *part {
            HotbarText::Key(id) => match owned.iter().position(|&w| w == id) {
                Some(index) => format!("{}", index + 1),
                _ => String::new(),
            },
            HotbarText::Name(id) => id.spec().short.to_string(),
            HotbarText::Count(id) => {
                let s = game.slot(id);
                match id.kind() {
                    Kind::Gun if !infinite => format!("{}", s.mag + s.reserve),
                    Kind::Thrown | Kind::Placed if !infinite => format!("×{}", s.reserve),
                    _ => String::new(),
                }
            }
        };
        set(&mut t, value);
    }

    for (label, mut t) in &mut labels {
        if let Some((_, v)) = values.iter().find(|(l, _)| l == label) {
            set(&mut t, v.clone());
        }
    }
}

/// Écran de chargement : ville générée en arrière-plan, puis maillage des
/// morceaux proches. Le jeu ne démarre que quand le terrain autour du
/// joueur est prêt (plus de saccades à l'arrivée).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn loading_screen(
    time: Res<Time>,
    mut game: ResMut<GameState>,
    loading: Res<crate::net::client::WorldLoading>,
    chunks: Res<crate::vmesh::ChunkEntities>,
    mut root: Query<&mut Node, (With<LoadingRoot>, Without<LoadingBar>)>,
    mut bar: Query<&mut Node, (With<LoadingBar>, Without<LoadingRoot>)>,
    mut text: Query<&mut Text, With<LoadingText>>,
    mut initial: Local<usize>,
    mut calm: Local<f32>,
    mut boot: Local<(f32, usize)>,
) {
    if game.booting {
        // Démarrage : maillage du décor du menu et compilation des shaders.
        boot.0 += time.delta_secs();
        let pending = chunks.pending_all + if chunks.scanned { 0 } else { 10_000 };
        boot.1 = boot.1.max(pending);
        let done = if boot.1 == 0 { 0.0 } else { 1.0 - pending as f32 / boot.1 as f32 };
        game.loading_progress = done.clamp(0.0, 1.0);
        if (pending == 0 && boot.0 > 0.6) || boot.0 > 20.0 {
            game.booting = false;
            game.loading_progress = 0.0;
        }
    } else if game.loading {
        // Tout ce qui est dans la distance d'affichage doit être maillé.
        let pending = chunks.pending_all + if chunks.scanned { 0 } else { 10_000 };
        let stage_gen = loading.installed_at.is_none();
        if stage_gen {
            *initial = 0;
            *calm = 0.0;
            // Barre indéterminée pendant la génération.
            game.loading_progress = (game.loading_progress + time.delta_secs() * 0.15).min(0.45);
        } else {
            if pending > *initial {
                *initial = pending;
            }
            let meshed = if *initial == 0 {
                0.0
            } else {
                1.0 - pending as f32 / *initial as f32
            };
            game.loading_progress = 0.45 + 0.55 * meshed.clamp(0.0, 1.0);
            let waited = loading.installed_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0);
            // Prêt : le terrain proche est maillé (et on a attendu un peu).
            // En ligne, le serveur envoie des modifications en continu : un
            // petit reste à mailler ne doit pas bloquer le chargement (il
            // remettait le compteur à zéro indéfiniment).
            if pending <= 64 && waited > 0.8 {
                // Quelques instants de plus : envoi des maillages au GPU,
                // compilation des shaders.
                *calm += time.delta_secs();
            }
            if *calm > 0.9 || waited > 25.0 {
                game.loading = false;
                game.loading_progress = 1.0;
            }
        }
    }
    for mut n in &mut root {
        let d = if game.loading || game.booting { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    for mut n in &mut bar {
        let w = Val::Percent(game.loading_progress * 100.0);
        if n.width != w {
            n.width = w;
        }
    }
    for mut t in &mut text {
        let s = if game.booting {
            "Chargement…"
        } else if loading.installed_at.is_none() {
            "Génération de la ville…"
        } else {
            "Chargement du terrain…"
        };
        if t.0 != s {
            t.0 = s.to_string();
        }
    }
}

/// Voile rouge : coup reçu (qui s'estompe) + pulsation quand la vie est basse.
pub fn damage_overlay(
    time: Res<Time>,
    game: Res<GameState>,
    mut overlay: Query<&mut BackgroundColor, With<DamageOverlay>>,
) {
    let low = if game.dead {
        0.55
    } else if game.hp < 40.0 {
        let danger = 1.0 - game.hp / 40.0;
        danger * (0.16 + 0.1 * (time.elapsed_secs() * (3.0 + danger * 5.0)).sin().abs())
    } else {
        0.0
    };
    let alpha = (game.hurt_flash * 0.5 + low).min(0.7);
    for mut bg in &mut overlay {
        let c = Color::srgba(0.75, 0.02, 0.02, alpha);
        if bg.0 != c {
            bg.0 = c;
        }
    }
}
