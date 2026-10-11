//! Menu principal : bac à sable, mode normal, seed, multijoueur, réglages.
//!
//! La seed peut être un nombre ou un mot (« paris » donne toujours la même
//! ville). Laissée vide, elle est tirée au hasard.

use bevy::app::AppExit;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::window::CursorOptions;

use crate::game::{self, GameState, Mode, Phase};
use crate::net;
use crate::net::client::NetClient;
use crate::ui::{self, UiFont};
use crate::world::VoxelWorld;

/// Champ de saisie actif.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    None,
    Seed,
    Address,
}

/// Ce que le menu affiche.
#[derive(Resource, Default)]
pub struct MenuView {
    pub show_multi: bool,
    /// Le panneau des paramètres est ouvert (on masque le reste du menu).
    pub show_settings: bool,
    pub focus: Focus,
    pub seed_text: String,
    pub status: String,
}

/// Action d'un bouton du menu.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Sandbox,
    Normal,
    Resume,
    Multi,
    Join,
    Host,
    Settings,
    SettingsBack,
    Quit,
    RandomSeed,
    FocusSeed,
    FocusAddress,
}

/// Parties du menu qu'on montre ou cache.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum MenuPanel {
    Main,
    Settings,
    Multi,
    Resume,
}

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct SeedText;
#[derive(Component)]
pub struct AddressText;
#[derive(Component)]
pub struct StatusText;

#[derive(Component)]
pub struct SensitivityUp;
#[derive(Component)]
pub struct SensitivityDown;
#[derive(Component)]
pub struct VolumeUp;
#[derive(Component)]
pub struct VolumeDown;
#[derive(Component)]
pub struct FpsUp;
#[derive(Component)]
pub struct FpsDown;
#[derive(Component)]
pub struct InvertY;
#[derive(Component)]
pub struct SensitivityText;
#[derive(Component)]
pub struct VolumeText;
#[derive(Component)]
pub struct FpsText;
#[derive(Component)]
pub struct InvertText;

/// Ligne « libellé  [-] valeur [+] ».
fn setting_row(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    name: &str,
    minus: impl Component,
    plus: impl Component,
    value: impl Component,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: px(46.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(200.0),
                    ..default()
                },
                ui::text(font, name, 20.0, ui::TEXT),
            ));
            row.spawn((minus, ui::button(48.0, 38.0, ui::BUTTON)))
                .with_child(ui::label(font, "-", 22.0, ui::TEXT));
            row.spawn((
                value,
                Node {
                    width: px(110.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                ui::label(font, "", 20.0, Color::srgb(0.6, 0.85, 1.0)),
            ));
            row.spawn((plus, ui::button(48.0, 38.0, ui::BUTTON)))
                .with_child(ui::label(font, "+", 22.0, ui::TEXT));
        });
}

pub fn setup(mut commands: Commands, font: Res<UiFont>, net: Res<NetClient>) {
    let font = font.into_inner().clone();
    commands.insert_resource(MenuView::default());

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.82)),
            GlobalZIndex(100),
        ))
        .with_children(|menu| {
            menu.spawn((
                ui::text(&font, "BOOMGROUND", 72.0, ui::ACCENT),
                Node {
                    margin: UiRect::bottom(px(4.0)),
                    ..default()
                },
            ));
            menu.spawn((
                ui::text(&font, "Tout est destructible.", 18.0, ui::MUTED),
                Node {
                    margin: UiRect::bottom(px(26.0)),
                    ..default()
                },
            ));

            // --- Colonne principale ---
            menu.spawn((
                MenuPanel::Main,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(10.0),
                    ..default()
                },
            ))
            .with_children(|col| {
                col.spawn((MenuPanel::Resume, MenuAction::Resume, ui::button(380.0, 52.0, Color::srgba(0.12, 0.30, 0.16, 0.96))))
                    .with_child(ui::label(&font, "REPRENDRE", 24.0, ui::TEXT));

                // Seed.
                col.spawn(Node {
                    column_gap: px(8.0),
                    align_items: AlignItems::Center,
                    margin: UiRect::vertical(px(6.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(ui::text(&font, "Seed", 20.0, ui::MUTED));
                    row.spawn((MenuAction::FocusSeed, ui::button(210.0, 44.0, Color::srgba(0.09, 0.10, 0.13, 0.98))))
                        .with_child((SeedText, ui::label(&font, "", 20.0, ui::TEXT)));
                    row.spawn((MenuAction::RandomSeed, ui::button(110.0, 44.0, ui::BUTTON)))
                        .with_child(ui::label(&font, "Hasard", 17.0, ui::TEXT));
                });

                col.spawn((MenuAction::Sandbox, ui::button(380.0, 64.0, Color::srgba(0.13, 0.17, 0.26, 0.96))))
                    .with_children(|b| {
                        b.spawn(ui::label(&font, "BAC À SABLE", 28.0, ui::TEXT));
                    });
                col.spawn(ui::text(&font, "Toutes les armes, munitions et bombes illimitées", 14.0, ui::MUTED));

                col.spawn((MenuAction::Normal, ui::button(380.0, 64.0, Color::srgba(0.26, 0.17, 0.08, 0.96))))
                    .insert(Node {
                        width: px(380.0),
                        height: px(64.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::top(px(8.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(6.0)),
                        ..default()
                    })
                    .with_child(ui::label(&font, "MODE NORMAL", 28.0, ui::ACCENT));
                col.spawn(ui::text(&font, "Un marteau pour commencer : cassez, gagnez, achetez (B)", 14.0, ui::MUTED));

                col.spawn(Node {
                    column_gap: px(10.0),
                    margin: UiRect::top(px(14.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((MenuAction::Multi, ui::button(185.0, 46.0, ui::BUTTON)))
                        .with_child(ui::label(&font, "MULTIJOUEUR", 18.0, Color::srgb(0.55, 0.80, 1.0)));
                    row.spawn((MenuAction::Settings, ui::button(185.0, 46.0, ui::BUTTON)))
                        .with_child(ui::label(&font, "PARAMÈTRES", 18.0, ui::TEXT));
                });

                // Multijoueur : adresse + rejoindre.
                col.spawn((
                    MenuPanel::Multi,
                    Node {
                        column_gap: px(8.0),
                        align_items: AlignItems::Center,
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|row| {
                    row.spawn((MenuAction::FocusAddress, ui::button(270.0, 44.0, Color::srgba(0.09, 0.10, 0.13, 0.98))))
                        .with_child((AddressText, ui::label(&font, net.address.clone(), 18.0, ui::TEXT)));
                    row.spawn((MenuAction::Join, ui::button(140.0, 44.0, Color::srgba(0.10, 0.20, 0.32, 0.96))))
                        .with_child(ui::label(&font, "REJOINDRE", 17.0, Color::srgb(0.55, 0.80, 1.0)));
                    row.spawn((MenuAction::Host, ui::button(140.0, 44.0, Color::srgba(0.10, 0.26, 0.20, 0.96))))
                        .with_child(ui::label(&font, "HÉBERGER", 17.0, Color::srgb(0.55, 1.0, 0.75)));
                });

                col.spawn((MenuAction::Quit, ui::button(180.0, 40.0, Color::srgba(0.25, 0.08, 0.08, 0.9))))
                    .insert(Node {
                        width: px(180.0),
                        height: px(40.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::top(px(12.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(6.0)),
                        ..default()
                    })
                    .with_child(ui::label(&font, "QUITTER", 17.0, ui::TEXT));
            });

            // --- Paramètres ---
            menu.spawn((
                MenuPanel::Settings,
                Node {
                    width: px(520.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10.0),
                    padding: UiRect::all(px(20.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.07, 0.08, 0.11, 0.97)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    ui::text(&font, "PARAMÈTRES", 28.0, ui::ACCENT),
                    Node {
                        margin: UiRect::bottom(px(10.0)),
                        ..default()
                    },
                ));
                setting_row(panel, &font, "Sensibilité", SensitivityDown, SensitivityUp, SensitivityText);
                setting_row(panel, &font, "Volume", VolumeDown, VolumeUp, VolumeText);
                setting_row(panel, &font, "Limite d'images", FpsDown, FpsUp, FpsText);
                panel
                    .spawn((InvertY, ui::button(480.0, 44.0, ui::BUTTON)))
                    .with_child((InvertText, ui::label(&font, "", 18.0, ui::TEXT)));
                panel
                    .spawn((MenuAction::SettingsBack, ui::button(480.0, 44.0, Color::srgba(0.12, 0.30, 0.16, 0.96))))
                    .with_child(ui::label(&font, "RETOUR", 18.0, ui::TEXT));
            });

            menu.spawn((
                StatusText,
                Node {
                    margin: UiRect::top(px(16.0)),
                    max_width: px(700.0),
                    ..default()
                },
                ui::label(&font, "", 16.0, ui::MUTED),
            ));
        });
}

/// Affiche ou masque le menu et le HUD selon la phase de jeu.
pub fn toggle_visibility(
    game: Res<GameState>,
    view: Res<MenuView>,
    mut root: Query<&mut Visibility, (With<MenuRoot>, Without<crate::hud::Hud>)>,
    mut panels: Query<(&MenuPanel, &mut Node)>,
    mut hud: Query<&mut Visibility, (With<crate::hud::Hud>, Without<MenuRoot>)>,
) {
    let vis = |shown: bool| if shown { Visibility::Inherited } else { Visibility::Hidden };
    let in_menu = game.phase == Phase::Menu;
    for mut v in &mut root {
        v.set_if_neq(vis(in_menu));
    }
    for mut v in &mut hud {
        v.set_if_neq(vis(!in_menu));
    }
    // `Display::None` et non `Visibility::Hidden` : un panneau caché ne
    // doit pas occuper de place (sinon le titre sortait de l'écran).
    for (panel, mut node) in &mut panels {
        let shown = match panel {
            MenuPanel::Main => !view.show_settings,
            MenuPanel::Settings => view.show_settings,
            MenuPanel::Multi => view.show_multi,
            MenuPanel::Resume => game.started,
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Panneau de réglages : sensibilité, volume, limite d'images, axe Y.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn settings_input(
    mut settings: ResMut<crate::game::Settings>,
    mut volume: ResMut<GlobalVolume>,
    mut limit: ResMut<crate::perf::FpsLimit>,
    mut texts: ParamSet<(
        Query<&mut Text, With<SensitivityText>>,
        Query<&mut Text, With<VolumeText>>,
        Query<&mut Text, With<FpsText>>,
        Query<&mut Text, With<InvertText>>,
    )>,
    sens_down: Query<(), (Added<Pressed>, With<SensitivityDown>)>,
    sens_up: Query<(), (Added<Pressed>, With<SensitivityUp>)>,
    vol_down: Query<(), (Added<Pressed>, With<VolumeDown>)>,
    vol_up: Query<(), (Added<Pressed>, With<VolumeUp>)>,
    fps_down: Query<(), (Added<Pressed>, With<FpsDown>)>,
    fps_up: Query<(), (Added<Pressed>, With<FpsUp>)>,
    invert: Query<(), (Added<Pressed>, With<InvertY>)>,
) {
    let before = (settings.sensitivity, settings.volume, settings.fps_limit, settings.invert_y);
    if !sens_down.is_empty() {
        settings.sensitivity *= 0.8;
    }
    if !sens_up.is_empty() {
        settings.sensitivity *= 1.25;
    }
    if !vol_down.is_empty() {
        settings.volume -= 0.1;
    }
    if !vol_up.is_empty() {
        settings.volume += 0.1;
    }
    if !fps_down.is_empty() {
        settings.fps_limit = if settings.fps_limit <= 0.0 {
            144.0
        } else {
            (settings.fps_limit - 15.0).max(30.0)
        };
    }
    if !fps_up.is_empty() {
        settings.fps_limit = if settings.fps_limit <= 0.0 {
            0.0
        } else if settings.fps_limit >= 360.0 {
            0.0
        } else {
            settings.fps_limit + 15.0
        };
    }
    if !invert.is_empty() {
        settings.invert_y = !settings.invert_y;
    }
    settings.clamp();
    let after = (settings.sensitivity, settings.volume, settings.fps_limit, settings.invert_y);
    if before != after {
        volume.volume = bevy::audio::Volume::Linear(settings.volume);
        if settings.fps_limit > 0.0 {
            limit.fps = settings.fps_limit;
        }
        crate::game::save_settings(&settings);
    }

    let set = |text: &mut Text, value: String| {
        if text.0 != value {
            text.0 = value;
        }
    };
    if let Ok(mut text) = texts.p0().single_mut() {
        set(&mut text, format!("{:.0} %", settings.sensitivity / 0.0022 * 100.0));
    }
    if let Ok(mut text) = texts.p1().single_mut() {
        set(&mut text, format!("{:.0} %", settings.volume * 100.0));
    }
    if let Ok(mut text) = texts.p2().single_mut() {
        set(
            &mut text,
            if settings.fps_limit <= 0.0 {
                "écran".to_string()
            } else {
                format!("{:.0}", settings.fps_limit)
            },
        );
    }
    if let Ok(mut text) = texts.p3().single_mut() {
        set(
            &mut text,
            format!("Inverser l'axe Y : {}", if settings.invert_y { "oui" } else { "non" }),
        );
    }
}

/// Lance une nouvelle partie dans un mode.
fn start(game: &mut GameState, world: &mut VoxelWorld, view: &mut MenuView, mode: Mode) {
    let seed = game::parse_seed(&view.seed_text).unwrap_or_else(game::random_seed);
    view.seed_text = seed.to_string();
    game.seed = seed;
    game.mode = mode;
    game.reset_inventory();
    game.started = true;
    game.phase = Phase::Playing;
    world.rebuild(seed);
}

/// Entrée dans une partie en ligne (toujours en bac à sable).
fn join(game: &mut GameState) {
    game.mode = Mode::Sandbox;
    game.reset_inventory();
    game.started = true;
    game.phase = Phase::Playing;
}

/// Interagit avec le menu.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Single<&mut CursorOptions>,
    mut game: ResMut<GameState>,
    mut view: ResMut<MenuView>,
    mut net: ResMut<NetClient>,
    mut world: ResMut<VoxelWorld>,
    shop: Res<crate::shop::ShopState>,
    actions: Query<&MenuAction, Added<Pressed>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<SeedText>>,
        Query<&mut Text, With<AddressText>>,
        Query<&mut Text, With<StatusText>>,
    )>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut exit: MessageWriter<AppExit>,
) {
    if view.seed_text.is_empty() && game.seed != 0 && !game.started {
        view.seed_text = game.seed.to_string();
    }

    // En jeu : Échap / F1 ouvre le menu (pause).
    if game.in_game() {
        keyboard.clear();
        if !shop.open && (keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::F1)) {
            game.phase = Phase::Menu;
            view.focus = Focus::None;
            crate::player::grab(&mut cursor, false);
        }
        return;
    }

    for action in &actions {
        view.focus = Focus::None;
        match action {
            MenuAction::Sandbox | MenuAction::Normal => {
                let mode = if *action == MenuAction::Sandbox {
                    Mode::Sandbox
                } else {
                    Mode::Normal
                };
                net.disconnect();
                start(&mut game, &mut world, &mut view, mode);
                crate::player::grab(&mut cursor, true);
                view.status.clear();
            }
            MenuAction::Resume => {
                if game.started {
                    game.phase = Phase::Playing;
                    crate::player::grab(&mut cursor, true);
                }
            }
            MenuAction::Multi => {
                view.show_multi = !view.show_multi;
                view.focus = if view.show_multi { Focus::Address } else { Focus::None };
                view.status = format!(
                    "Saisissez domaine.tld, une IP ou ip:port (port par défaut {}).",
                    net::DEFAULT_PORT
                );
            }
            MenuAction::Join => match net.connect() {
                Ok(addr) => {
                    view.status = format!("Connecté à {addr}");
                    join(&mut game);
                    crate::player::grab(&mut cursor, true);
                }
                Err(e) => view.status = format!("Échec de connexion : {e}"),
            },
            MenuAction::Host => {
                let seed = game::parse_seed(&view.seed_text).unwrap_or_else(game::random_seed);
                view.seed_text = seed.to_string();
                match net::server::host_in_background(seed) {
                    Ok(port) => {
                        net.address = format!("127.0.0.1:{port}");
                        match net.connect() {
                            Ok(_) => {
                                view.status = format!(
                                    "Partie hébergée (port {port}) : les autres joueurs saisissent votre IP."
                                );
                                join(&mut game);
                                crate::player::grab(&mut cursor, true);
                            }
                            Err(e) => view.status = format!("Serveur lancé mais connexion impossible : {e}"),
                        }
                    }
                    Err(e) => view.status = format!("Impossible d'héberger : {e}"),
                }
            }
            MenuAction::Settings => view.show_settings = true,
            MenuAction::SettingsBack => view.show_settings = false,
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            MenuAction::RandomSeed => {
                view.seed_text = game::random_seed().to_string();
            }
            MenuAction::FocusSeed => view.focus = Focus::Seed,
            MenuAction::FocusAddress => view.focus = Focus::Address,
        }
    }

    // Saisie au clavier dans le champ actif.
    for ev in keyboard.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        let field = match view.focus {
            Focus::Seed => &mut view.seed_text,
            Focus::Address => &mut net.address,
            Focus::None => continue,
        };
        match &ev.logical_key {
            Key::Backspace => {
                field.pop();
            }
            Key::Enter | Key::Escape | Key::Tab => {}
            _ => {
                if let Some(text) = ev.text.as_ref() {
                    for c in text.chars() {
                        if !c.is_control() && field.chars().count() < 40 {
                            field.push(c);
                        }
                    }
                }
            }
        }
    }

    if keys.just_pressed(KeyCode::Escape) {
        if view.focus != Focus::None {
            view.focus = Focus::None;
        } else if view.show_settings {
            view.show_settings = false;
        } else if game.started {
            game.phase = Phase::Playing;
            crate::player::grab(&mut cursor, true);
        }
    }

    // Textes.
    let caret = |focused: bool| if focused { "_" } else { "" };
    let seed = if view.seed_text.is_empty() && view.focus != Focus::Seed {
        "(au hasard)".to_string()
    } else {
        format!("{}{}", view.seed_text, caret(view.focus == Focus::Seed))
    };
    if let Ok(mut text) = texts.p0().single_mut() {
        text.set_if_neq(Text(seed));
    }
    let address = format!("{}{}", net.address, caret(view.focus == Focus::Address));
    if let Ok(mut text) = texts.p1().single_mut() {
        text.set_if_neq(Text(address));
    }
    let status = if view.status.is_empty() && game.started {
        format!("Partie en cours — seed {}", world.seed)
    } else {
        view.status.clone()
    };
    if let Ok(mut text) = texts.p2().single_mut() {
        text.set_if_neq(Text(status));
    }
}
