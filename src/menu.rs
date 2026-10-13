//! Interface hors jeu : installation, menu principal (Jouer, Multijoueur,
//! Paramètres) et lobby multijoueur.
//!
//! Derrière le menu, la caméra survole lentement la ville.

use std::net::{SocketAddr, SocketAddrV4};

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::window::{CursorOptions, MonitorSelection, PrimaryWindow, WindowMode};

use crate::game::{self, GameState, Mode, Phase, Settings};
use crate::install;
use crate::net::client::{player_color, Lobby, NetClient};
use crate::net::lan::LanBrowser;
use crate::net::protocol::{encode_code, resolve_target};
use crate::net::server::{self, Internet};
use crate::ui::{self, Switch, TextField, Toasts, UiFont};
use crate::world::VoxelWorld;

/// Onglet du menu principal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Play,
    Multi,
    Settings,
}

/// État du menu.
#[derive(Resource)]
pub struct MenuState {
    pub tab: Tab,
    /// Mode choisi pour une partie solo.
    pub mode: Mode,
    /// Mode choisi pour un lobby.
    pub lobby_mode: Mode,
    pub status: String,
    /// Mondes sauvegardés (nom, chemin).
    pub worlds: Vec<(String, std::path::PathBuf)>,
    /// Taille de la carte pour une nouvelle partie (pâtés de maisons).
    pub map_blocks: u16,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            tab: Tab::Play,
            mode: Mode::Sandbox,
            lobby_mode: Mode::Sandbox,
            status: String::new(),
            worlds: Vec::new(),
            map_blocks: 17,
        }
    }
}

/// Action d'un bouton.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum MenuAction {
    Tab(Tab),
    Resume,
    SaveWorld,
    LoadWorld(usize),
    JoinServer(usize),
    RemoveServer(usize),
    AddServer,
    RefreshServers,
    MapSize(u16),
    Leave,
    Quit,
    SelectMode(Mode),
    RandomSeed,
    Launch,
    LobbyMode(Mode),
    CreateLobby,
    Join,
    JoinLan(SocketAddr),
    LobbyStart,
    LobbyLeave,
    CopyCode,
    Install,
    SkipInstall,
    Uninstall,
    Setting(SettingKind, i8),
}

/// Réglage modifiable par −/+.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingKind {
    VoiceMode,
    VoiceVolume,
    Quality,
    Msaa,
    Particles,
    Music,
    Sensitivity,
    Volume,
    Sfx,
    Fov,
    Fps,
    ViewDistance,
    Shadows,
    Shake,
    /// Côté du joystick tactile (mobile).
    JoystickSide,
    /// Sensibilité du regard au doigt (mobile).
    TouchSensitivity,
}

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct LobbyRoot;
#[derive(Component)]
pub struct InstallRoot;
#[derive(Component)]
pub struct ContentPanel(Tab);
#[derive(Component)]
pub struct NavButton(Tab);
#[derive(Component)]
pub struct ModeCard(Mode);
#[derive(Component)]
pub struct LobbyModeButton(Mode);
#[derive(Component)]
pub struct OnlyWhenStarted;
#[derive(Component)]
pub struct SeedField;
#[derive(Component)]
pub struct LobbySeedField;
#[derive(Component)]
pub struct NameField;
#[derive(Component)]
pub struct JoinField;
#[derive(Component)]
pub struct StatusText;
#[derive(Component)]
pub struct LanList;
#[derive(Component)]
pub struct WorldList;
#[derive(Component)]
pub struct ServerListUi;
#[derive(Component)]
pub struct AddNameField;
#[derive(Component)]
pub struct AddAddressField;
#[derive(Component)]
pub struct MapSizeButton(u16);
#[derive(Component)]
pub struct SettingValue(SettingKind);
#[derive(Component)]
pub struct InvertSwitch;
#[derive(Component)]
pub struct FullscreenSwitch;
#[derive(Component)]
pub struct ShowFpsSwitch;
#[derive(Component)]
pub struct CrosshairSwitch;
#[derive(Component)]
pub struct BobSwitch;
#[derive(Component)]
pub struct BloomSwitch;
#[derive(Component)]
pub struct DesktopSwitch;
#[derive(Component)]
pub struct FirewallSwitch;
#[derive(Component)]
pub struct InstallStatus;
#[derive(Component)]
pub struct UninstallRow;
#[derive(Component)]
pub struct LobbyTitle;
#[derive(Component)]
pub struct LobbyCode;
#[derive(Component)]
pub struct LobbyNet;
#[derive(Component)]
pub struct LobbyPlayers;
#[derive(Component)]
pub struct LobbyHint;
#[derive(Component)]
pub struct HostOnly;
#[derive(Component)]
pub struct GuestOnly;

fn overlay_gradient() -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_right(vec![
        ColorStop::new(Color::srgba(0.02, 0.025, 0.04, 0.97), Val::Percent(0.0)),
        ColorStop::new(Color::srgba(0.02, 0.025, 0.04, 0.88), Val::Percent(35.0)),
        ColorStop::new(Color::srgba(0.02, 0.025, 0.04, 0.45), Val::Percent(100.0)),
    ]))
}

fn shadow() -> BoxShadow {
    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0.0), px(8.0), px(0.0), px(24.0))
}

/// Logo « BOOM GROUND ».
fn logo(parent: &mut ChildSpawnerCommands, font: &UiFont, size: f32) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            ..default()
        })
        .with_children(|row| {
            row.spawn(ui::text(font, "BOOM", size, ui::ACCENT));
            row.spawn(ui::text(font, "GROUND", size, ui::TEXT));
        });
}

fn nav_button(parent: &mut ChildSpawnerCommands, font: &UiFont, tab: Tab, label: &str) {
    parent
        .spawn((
            MenuAction::Tab(tab),
            NavButton(tab),
            ui::button_with(
                Node {
                    width: Val::Percent(100.0),
                    height: px(52.0),
                    ..default()
                },
                Color::NONE,
            ),
        ))
        .insert(Node {
            width: Val::Percent(100.0),
            height: px(52.0),
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            padding: UiRect::left(px(18.0)),
            border: UiRect::left(px(4.0)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        })
        .with_child(ui::text(font, label, 22.0, ui::TEXT));
}

/// Ligne de réglage « libellé  [-] valeur [+] ».
fn setting_row(parent: &mut ChildSpawnerCommands, font: &UiFont, name: &str, kind: SettingKind) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: px(48.0),
            align_items: AlignItems::Center,
            column_gap: px(10.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                ui::text(font, name, 19.0, ui::TEXT),
            ));
            row.spawn((MenuAction::Setting(kind, -1), ui::button(42.0, 36.0, ui::BUTTON)))
                .with_child(ui::label(font, "-", 22.0, ui::TEXT));
            row.spawn((
                SettingValue(kind),
                Node {
                    width: px(90.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_child(ui::label(font, "", 19.0, ui::BLUE));
            row.spawn((MenuAction::Setting(kind, 1), ui::button(42.0, 36.0, ui::BUTTON)))
                .with_child(ui::label(font, "+", 22.0, ui::TEXT));
        });
}

/// Ligne « libellé ……… [interrupteur] ».
fn switch_row(parent: &mut ChildSpawnerCommands, font: &UiFont, name: &str, on: bool, marker: impl Component) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: px(44.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                ui::text(font, name, 18.0, ui::TEXT),
            ));
            let (s, knob) = ui::switch(on);
            row.spawn((marker, s)).with_child(knob);
        });
}

fn card_node(width: f32) -> Node {
    Node {
        width: px(width),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(20.0)),
        row_gap: px(10.0),
        border: UiRect::all(px(1.0)),
        border_radius: BorderRadius::all(px(12.0)),
        ..default()
    }
}

/// Carte de mode de jeu (cliquable).
fn mode_card(parent: &mut ChildSpawnerCommands, font: &UiFont, mode: Mode) {
    let (title, color, lines): (&str, Color, [&str; 3]) = match mode {
        Mode::Sandbox => (
            "BAC À SABLE",
            ui::BLUE,
            [
                "Les 13 armes d'office",
                "Munitions et bombes illimitées",
                "Vol libre avec V",
            ],
        ),
        Mode::Normal => (
            "MODE NORMAL",
            ui::ACCENT,
            [
                "Un marteau pour commencer",
                "Chaque voxel cassé rapporte de l'argent",
                "Boutique (B) : armes, marteaux, bombes",
            ],
        ),
    };
    let node = Node {
        width: px(330.0),
        height: px(190.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::FlexStart,
        justify_content: JustifyContent::FlexStart,
        padding: UiRect::all(px(20.0)),
        row_gap: px(8.0),
        border: UiRect::all(px(2.0)),
        border_radius: BorderRadius::all(px(12.0)),
        ..default()
    };
    parent
        .spawn((MenuAction::SelectMode(mode), ModeCard(mode), ui::button_with(node.clone(), ui::CARD), shadow()))
        .insert(node)
        .with_children(|card| {
            card.spawn((
                Node {
                    width: px(46.0),
                    height: px(5.0),
                    border_radius: BorderRadius::all(px(3.0)),
                    margin: UiRect::bottom(px(6.0)),
                    ..default()
                },
                BackgroundColor(color),
            ));
            card.spawn(ui::text(font, title, 26.0, color));
            for line in lines {
                card.spawn(ui::text(font, format!("•  {line}"), 15.0, ui::MUTED));
            }
        });
}

pub fn setup(mut commands: Commands, font: Res<UiFont>, settings: Res<Settings>) {
    let font = font.into_inner().clone();
    commands.insert_resource(MenuState::default());

    // ------------------------------------------------------------------
    // Menu principal
    // ------------------------------------------------------------------
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                ..default()
            },
            overlay_gradient(),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            // --- Colonne de gauche : logo et navigation ---
            root.spawn(Node {
                // Assez large pour que le logo tienne dans la colonne :
                // il chevauchait sinon le titre du panneau de droite.
                width: px(400.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(px(40.0), px(30.0), px(56.0), px(32.0)),
                row_gap: px(8.0),
                overflow: Overflow::clip(),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|side| {
                logo(side, &font, 46.0);
                side.spawn((
                    ui::text(&font, "Tout est destructible.", 16.0, ui::MUTED),
                    Node {
                        margin: UiRect::bottom(px(40.0)),
                        ..default()
                    },
                ));
                side.spawn((
                    OnlyWhenStarted,
                    MenuAction::Resume,
                    ui::button_with(
                        Node {
                            width: Val::Percent(100.0),
                            height: px(54.0),
                            margin: UiRect::bottom(px(14.0)),
                            ..default()
                        },
                        ui::BUTTON_PRIMARY,
                    ),
                ))
                .with_child(ui::label(&font, "REPRENDRE", 22.0, ui::ACCENT_DARK));
                side.spawn((
                    OnlyWhenStarted,
                    MenuAction::SaveWorld,
                    ui::button_with(
                        Node {
                            width: Val::Percent(100.0),
                            height: px(44.0),
                            margin: UiRect::bottom(px(14.0)),
                            ..default()
                        },
                        ui::BUTTON,
                    ),
                ))
                .with_child(ui::label(&font, "Sauvegarder le monde", 17.0, ui::TEXT));
                nav_button(side, &font, Tab::Play, "Jouer");
                nav_button(side, &font, Tab::Multi, "Multijoueur");
                nav_button(side, &font, Tab::Settings, "Paramètres");
                side.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                side.spawn((
                    OnlyWhenStarted,
                    MenuAction::Leave,
                    ui::button_with(
                        Node {
                            width: Val::Percent(100.0),
                            height: px(44.0),
                            margin: UiRect::bottom(px(8.0)),
                            ..default()
                        },
                        ui::BUTTON,
                    ),
                ))
                .with_child(ui::label(&font, "Quitter la partie", 17.0, ui::TEXT));
                side.spawn((
                    MenuAction::Quit,
                    ui::wide_button(44.0, Color::srgba(0.35, 0.08, 0.08, 0.85)),
                ))
                .with_child(ui::label(&font, "Quitter le jeu", 17.0, ui::TEXT));
                side.spawn((
                    ui::text(&font, format!("v{}", install::VERSION), 13.0, ui::MUTED),
                    Node {
                        margin: UiRect::top(px(12.0)),
                        ..default()
                    },
                ));
            });

            // --- Contenu ---
            root.spawn(Node {
                flex_grow: 1.0,
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::new(px(30.0), px(48.0), px(40.0), px(40.0)),
                ..default()
            })
            .with_children(|content| {
                // ===== Jouer =====
                content
                    .spawn((
                        ContentPanel(Tab::Play),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(16.0),
                            ..default()
                        },
                    ))
                    .with_children(|p| {
                        p.spawn(ui::text(&font, "Nouvelle partie", 38.0, ui::TEXT));
                        p.spawn(Node {
                            column_gap: px(18.0),
                            ..default()
                        })
                        .with_children(|row| {
                            mode_card(row, &font, Mode::Sandbox);
                            mode_card(row, &font, Mode::Normal);
                        });
                        p.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            margin: UiRect::top(px(6.0)),
                            ..default()
                        })
                        .with_children(|col| {
                            col.spawn(ui::caption(&font, "Seed de la ville (un nombre ou un mot)"));
                            col.spawn(Node {
                                column_gap: px(10.0),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_children(|row| {
                                let (f, t) = ui::field(&font, px(330.0), "", "Au hasard", 24);
                                row.spawn((SeedField, f)).with_child(t);
                                row.spawn((MenuAction::RandomSeed, ui::button(140.0, 44.0, ui::BUTTON)))
                                    .with_child(ui::label(&font, "Hasard", 17.0, ui::TEXT));
                            });
                        });
                        p.spawn(ui::caption(&font, "Taille de la ville (plus petite = moins de mémoire)"));
                        p.spawn(Node {
                            column_gap: px(8.0),
                            ..default()
                        })
                        .with_children(|r| {
                            for (blocks, label) in [(9u16, "Petite"), (17, "Moyenne"), (33, "Grande")] {
                                r.spawn((
                                    MenuAction::MapSize(blocks),
                                    MapSizeButton(blocks),
                                    ui::button(150.0, 40.0, ui::BUTTON),
                                ))
                                .with_child(ui::label(&font, label, 16.0, ui::TEXT));
                            }
                        });
                        p.spawn((
                            MenuAction::Launch,
                            ui::button_with(
                                Node {
                                    width: px(678.0),
                                    height: px(62.0),
                                    margin: UiRect::top(px(8.0)),
                                    ..default()
                                },
                                ui::BUTTON_PRIMARY,
                            ),
                            shadow(),
                        ))
                        .with_child(ui::label(&font, "LANCER LA PARTIE", 26.0, ui::ACCENT_DARK));
                        p.spawn(ui::caption(&font, "Mondes sauvegardés"));
                        p.spawn((
                            WorldList,
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(6.0),
                                width: px(678.0),
                                ..default()
                            },
                        ));
                    });

                // ===== Multijoueur =====
                content
                    .spawn((
                        ContentPanel(Tab::Multi),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(16.0),
                            display: Display::None,
                            ..default()
                        },
                    ))
                    .with_children(|p| {
                        p.spawn(ui::text(&font, "Multijoueur", 38.0, ui::TEXT));
                        p.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: px(12.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(ui::text(&font, "Ton pseudo", 17.0, ui::MUTED));
                            let (f, t) = ui::field(&font, px(260.0), &settings.name, "Pseudo", 16);
                            row.spawn((NameField, f)).with_child(t);
                        });
                        p.spawn(Node {
                            column_gap: px(18.0),
                            align_items: AlignItems::Stretch,
                            ..default()
                        })
                        .with_children(|row| {
                            // --- Créer ---
                            row.spawn((ui::card(), shadow()))
                                .insert(card_node(360.0))
                                .with_children(|c| {
                                    c.spawn(ui::text(&font, "Créer un lobby", 24.0, ui::ACCENT));
                                    c.spawn(ui::text(
                                        &font,
                                        "La partie tourne en fond sur ton PC. Tes amis entrent avec le code, \
                                         ou la voient toute seule s'ils sont sur le même Wi-Fi.",
                                        15.0,
                                        ui::MUTED,
                                    ));
                                    c.spawn(ui::caption(&font, "Mode"));
                                    c.spawn(Node {
                                        column_gap: px(8.0),
                                        ..default()
                                    })
                                    .with_children(|r| {
                                        for (mode, name) in [(Mode::Sandbox, "Bac à sable"), (Mode::Normal, "Normal")] {
                                            r.spawn((
                                                MenuAction::LobbyMode(mode),
                                                LobbyModeButton(mode),
                                                ui::button(155.0, 40.0, ui::BUTTON),
                                            ))
                                            .with_child(ui::label(&font, name, 16.0, ui::TEXT));
                                        }
                                    });
                                    c.spawn(ui::caption(&font, "Seed"));
                                    let (f, t) = ui::field(&font, Val::Percent(100.0), "", "Au hasard", 24);
                                    c.spawn((LobbySeedField, f)).with_child(t);
                                    c.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    c.spawn((MenuAction::CreateLobby, ui::wide_button(52.0, ui::BUTTON_PRIMARY)))
                                        .with_child(ui::label(&font, "CRÉER LE LOBBY", 20.0, ui::ACCENT_DARK));
                                });

                            // --- Rejoindre ---
                            let mut join = card_node(400.0);
                            join.min_height = px(360.0);
                            row.spawn((ui::card(), shadow()))
                                .insert(join)
                                .with_children(|c| {
                                    c.spawn(ui::text(&font, "Rejoindre", 24.0, ui::BLUE));
                                    c.spawn(ui::caption(&font, "Mes serveurs (clique pour rejoindre)"));
                                    c.spawn((
                                        ServerListUi,
                                        Node {
                                            flex_direction: FlexDirection::Column,
                                            row_gap: px(6.0),
                                            min_height: px(56.0),
                                            ..default()
                                        },
                                    ));
                                    c.spawn(Node {
                                        column_gap: px(6.0),
                                        ..default()
                                    })
                                    .with_children(|r| {
                                        let (f, t) = ui::field(&font, px(120.0), "", "Nom", 24);
                                        r.spawn((AddNameField, f)).with_child(t);
                                        let (f, t) = ui::field(&font, px(150.0), "", "Adresse ou code", 60);
                                        r.spawn((AddAddressField, f)).with_child(t);
                                        r.spawn((MenuAction::AddServer, ui::button(44.0, 44.0, ui::BUTTON_PRIMARY)))
                                            .with_child(ui::label(&font, "+", 24.0, ui::ACCENT_DARK));
                                    });
                                    c.spawn(ui::caption(&font, "Parties sur ton réseau"));
                                    c.spawn((
                                        LanList,
                                        Node {
                                            flex_direction: FlexDirection::Column,
                                            row_gap: px(6.0),
                                            // Juste la hauteur d'une ligne :
                                            // le vide parqué ici repoussait
                                            // le champ de code vers le bas.
                                            min_height: px(24.0),
                                            ..default()
                                        },
                                    ));
                                    c.spawn(ui::caption(&font, "Code de partie ou adresse"));
                                    let (f, t) = ui::field(&font, Val::Percent(100.0), "", "ex. K7M2Q-9XH4C", 40);
                                    c.spawn((JoinField, f)).with_child(t);
                                    c.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    c.spawn((
                                        MenuAction::Join,
                                        ui::wide_button(52.0, Color::srgba(0.12, 0.30, 0.52, 1.0)),
                                    ))
                                    .with_child(ui::label(&font, "REJOINDRE", 20.0, ui::TEXT));
                                });
                        });
                    });

                // ===== Paramètres =====
                content
                    .spawn((
                        ContentPanel(Tab::Settings),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(16.0),
                            display: Display::None,
                            ..default()
                        },
                    ))
                    .with_children(|p| {
                        p.spawn(ui::text(&font, "Paramètres", 38.0, ui::TEXT));
                        p.spawn(Node {
                            column_gap: px(18.0),
                            align_items: AlignItems::FlexStart,
                            ..default()
                        })
                        .with_children(|cols| {
                            let mut node = card_node(430.0);
                            node.row_gap = px(2.0);
                            cols.spawn((ui::card(), shadow())).insert(node.clone()).with_children(|c| {
                                c.spawn(ui::text(&font, "JEU ET CONTRÔLES", 13.0, ui::MUTED));
                                let touch = crate::touch::is_touch_platform();
                                if !touch {
                                    setting_row(c, &font, "Sensibilité de la souris", SettingKind::Sensitivity);
                                }
                                setting_row(c, &font, "Champ de vision", SettingKind::Fov);
                                setting_row(c, &font, "Secousses de caméra", SettingKind::Shake);
                                switch_row(c, &font, "Inverser l'axe vertical", settings.invert_y, InvertSwitch);
                                switch_row(c, &font, "Balancement à la marche", settings.head_bob, BobSwitch);
                                if crate::touch::is_touch_platform() {
                                    c.spawn(ui::text(&font, "ÉCRAN TACTILE", 13.0, ui::MUTED));
                                    setting_row(c, &font, "Côté du joystick", SettingKind::JoystickSide);
                                    setting_row(c, &font, "Sensibilité du regard", SettingKind::TouchSensitivity);
                                }
                                c.spawn(ui::text(&font, "SON", 13.0, ui::MUTED));
                                setting_row(c, &font, "Volume général", SettingKind::Volume);
                                setting_row(c, &font, "Bruitages", SettingKind::Sfx);
                                setting_row(c, &font, "Musique", SettingKind::Music);
                                // Voix : micro et touche G, ordinateur seulement.
                                if !touch {
                                    setting_row(c, &font, "Voix de proximité", SettingKind::VoiceMode);
                                    setting_row(c, &font, "Volume des voix", SettingKind::VoiceVolume);
                                }
                            });
                            cols.spawn((ui::card(), shadow())).insert(node).with_children(|c| {
                                c.spawn(ui::text(&font, "AFFICHAGE", 13.0, ui::MUTED));
                                setting_row(c, &font, "Préréglage graphique", SettingKind::Quality);
                                setting_row(c, &font, "Distance d'affichage", SettingKind::ViewDistance);
                                setting_row(c, &font, "Qualité des ombres", SettingKind::Shadows);
                                setting_row(c, &font, "Anticrénelage", SettingKind::Msaa);
                                setting_row(c, &font, "Poussière et fumée", SettingKind::Particles);
                                switch_row(c, &font, "Halo lumineux (bloom)", settings.bloom, BloomSwitch);
                                setting_row(c, &font, "Limite d'images", SettingKind::Fps);
                                if !crate::touch::is_touch_platform() {
                                    switch_row(c, &font, "Plein écran", settings.fullscreen, FullscreenSwitch);
                                }
                                c.spawn(ui::text(&font, "INTERFACE", 13.0, ui::MUTED));
                                switch_row(c, &font, "Afficher les FPS", settings.show_fps, ShowFpsSwitch);
                                switch_row(c, &font, "Afficher le viseur", settings.crosshair, CrosshairSwitch);
                            });
                        });
                        p.spawn((
                            UninstallRow,
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: px(12.0),
                                ..default()
                            },
                        ))
                        .with_children(|r| {
                            r.spawn((
                                MenuAction::Uninstall,
                                ui::button(200.0, 38.0, Color::srgba(0.35, 0.08, 0.08, 0.85)),
                            ))
                            .with_child(ui::label(&font, "Désinstaller", 16.0, ui::TEXT));
                            r.spawn(ui::text(&font, install::install_dir(), 13.0, ui::MUTED));
                        });
                    });

                content.spawn((
                    StatusText,
                    Node {
                        margin: UiRect::top(px(18.0)),
                        max_width: px(760.0),
                        ..default()
                    },
                    ui::text(&font, "", 17.0, ui::HOT),
                ));
            });
        });

    // ------------------------------------------------------------------
    // Lobby
    // ------------------------------------------------------------------
    commands
        .spawn((
            LobbyRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.72)),
            GlobalZIndex(110),
        ))
        .with_children(|root| {
            let mut node = card_node(780.0);
            node.padding = UiRect::all(px(28.0));
            node.row_gap = px(14.0);
            node.border_radius = BorderRadius::all(px(16.0));
            root.spawn((ui::card(), shadow()))
                .insert((node, BackgroundColor(ui::PANEL)))
                .with_children(|c| {
                    c.spawn((LobbyTitle, ui::text(&font, "Lobby", 32.0, ui::TEXT)));
                    // Code de la partie.
                    c.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(18.0)),
                            row_gap: px(6.0),
                            border_radius: BorderRadius::all(px(12.0)),
                            ..default()
                        },
                        BackgroundColor(ui::FIELD),
                    ))
                    .with_children(|b| {
                        b.spawn(ui::caption(&font, "Code de la partie"));
                        b.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: px(16.0),
                            ..default()
                        })
                        .with_children(|r| {
                            r.spawn((LobbyCode, ui::text(&font, "…", 46.0, ui::ACCENT)));
                            r.spawn((MenuAction::CopyCode, ui::button(110.0, 40.0, ui::BUTTON)))
                                .with_child(ui::label(&font, "Copier", 16.0, ui::TEXT));
                        });
                        b.spawn((LobbyNet, ui::text(&font, "", 15.0, ui::MUTED)));
                    });
                    c.spawn(ui::caption(&font, "Joueurs"));
                    c.spawn((
                        LobbyPlayers,
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6.0),
                            min_height: px(120.0),
                            ..default()
                        },
                    ));
                    c.spawn(Node {
                        column_gap: px(12.0),
                        align_items: AlignItems::Center,
                        margin: UiRect::top(px(8.0)),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((HostOnly, MenuAction::LobbyStart, ui::button(330.0, 56.0, ui::BUTTON_PRIMARY)))
                            .with_child(ui::label(&font, "LANCER LA PARTIE", 22.0, ui::ACCENT_DARK));
                        r.spawn((GuestOnly, LobbyHint, ui::text(&font, "En attente de l'hôte…", 19.0, ui::MUTED)));
                        r.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        r.spawn((MenuAction::LobbyLeave, ui::button(200.0, 48.0, ui::BUTTON)))
                            .with_child(ui::label(&font, "Quitter le lobby", 17.0, ui::TEXT));
                    });
                });
        });

    // ------------------------------------------------------------------
    // Installation (premier lancement)
    // ------------------------------------------------------------------
    let update = install::already_installed();
    commands
        .spawn((
            InstallRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.8)),
            GlobalZIndex(120),
        ))
        .with_children(|root| {
            let mut node = card_node(640.0);
            node.padding = UiRect::all(px(36.0));
            node.row_gap = px(14.0);
            node.border_radius = BorderRadius::all(px(16.0));
            root.spawn((ui::card(), shadow()))
                .insert((node, BackgroundColor(ui::PANEL)))
                .with_children(|c| {
                    logo(c, &font, 48.0);
                    c.spawn(ui::text(
                        &font,
                        if update { "Une version est déjà installée." } else { "Bienvenue !" },
                        26.0,
                        ui::TEXT,
                    ));
                    c.spawn(ui::text(
                        &font,
                        "Installe BoomGround pour le retrouver sur ton Bureau et dans tes applications. \
                         Rien d'autre à télécharger, et pas besoin d'être administrateur.",
                        16.0,
                        ui::MUTED,
                    ));
                    c.spawn(ui::text(&font, format!("Dossier : {}", install::install_dir()), 13.0, ui::MUTED));
                    switch_row(c, &font, "Raccourci sur le Bureau", true, DesktopSwitch);
                    if cfg!(windows) {
                        switch_row(c, &font, "Autoriser le multijoueur dans le pare-feu", true, FirewallSwitch);
                    }
                    c.spawn((
                        MenuAction::Install,
                        ui::button_with(
                            Node {
                                width: Val::Percent(100.0),
                                height: px(58.0),
                                margin: UiRect::top(px(8.0)),
                                ..default()
                            },
                            ui::BUTTON_PRIMARY,
                        ),
                    ))
                    .with_child(ui::label(
                        &font,
                        if update { "METTRE À JOUR" } else { "INSTALLER" },
                        24.0,
                        ui::ACCENT_DARK,
                    ));
                    c.spawn((MenuAction::SkipInstall, ui::wide_button(44.0, ui::BUTTON)))
                        .with_child(ui::label(&font, "Jouer sans installer", 17.0, ui::TEXT));
                    c.spawn((InstallStatus, ui::text(&font, "", 15.0, ui::HOT)));
                });
        });
}

/// Phase de départ : écran d'installation si le jeu n'est pas installé.
pub fn initial_phase(mut game: ResMut<GameState>) {
    if install::should_offer() {
        game.phase = Phase::Install;
    }
}

/// Affiche les bons écrans, panneaux et sélections.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn visibility(
    game: Res<GameState>,
    state: Res<MenuState>,
    lobby: Res<Lobby>,
    mut roots: ParamSet<(
        Query<&mut Node, With<MenuRoot>>,
        Query<&mut Node, With<LobbyRoot>>,
        Query<&mut Node, With<InstallRoot>>,
        Query<(&ContentPanel, &mut Node)>,
        Query<(&mut Node, Has<OnlyWhenStarted>), Or<(With<OnlyWhenStarted>, With<UninstallRow>)>>,
        Query<(&mut Node, Has<HostOnly>), Or<(With<HostOnly>, With<GuestOnly>)>>,
    )>,
    mut hud: Query<&mut Visibility, With<crate::hud::Hud>>,
    mut nav: Query<(&NavButton, &mut BorderColor), (Without<ModeCard>, Without<LobbyModeButton>)>,
    mut cards: Query<(&ModeCard, &mut BorderColor), (Without<NavButton>, Without<LobbyModeButton>)>,
    mut lobby_modes: Query<(&LobbyModeButton, &mut BorderColor), (Without<NavButton>, Without<ModeCard>)>,
) {
    let show = |node: &mut Node, on: bool| {
        let d = if on { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    };
    for mut n in &mut roots.p0() {
        show(&mut n, game.phase == Phase::Menu);
    }
    for mut n in &mut roots.p1() {
        show(&mut n, game.phase == Phase::Lobby);
    }
    for mut n in &mut roots.p2() {
        show(&mut n, game.phase == Phase::Install);
    }
    for (panel, mut n) in &mut roots.p3() {
        show(&mut n, panel.0 == state.tab);
    }
    let installed = install::is_installed_copy();
    for (mut n, started_only) in &mut roots.p4() {
        show(&mut n, if started_only { game.started } else { installed && !crate::touch::is_touch_platform() });
    }
    let host = lobby.is_host();
    for (mut n, host_only) in &mut roots.p5() {
        show(&mut n, host_only == host);
    }
    for mut v in &mut hud {
        v.set_if_neq(if game.phase == Phase::Playing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (b, mut border) in &mut nav {
        let c = if b.0 == state.tab { ui::ACCENT } else { Color::NONE };
        if border.left != c {
            *border = BorderColor::all(c);
        }
    }
    for (card, mut border) in &mut cards {
        let c = if card.0 == state.mode { ui::ACCENT } else { ui::LINE };
        if border.top != c {
            *border = BorderColor::all(c);
        }
    }
    for (b, mut border) in &mut lobby_modes {
        let c = if b.0 == state.lobby_mode { ui::ACCENT } else { ui::LINE };
        if border.top != c {
            *border = BorderColor::all(c);
        }
    }
}

/// Lance une partie solo : un serveur privé local, auquel le jeu se
/// connecte comme n'importe quel client (une seule logique de jeu).
pub fn launch_solo(
    net: &mut NetClient,
    lobby: &mut Lobby,
    game: &mut GameState,
    name: &str,
    seed_text: &str,
    mode: Mode,
    load: Option<(String, std::path::PathBuf)>,
    map_blocks: u16,
) -> Result<u64, String> {
    leave_online(net, lobby, game);
    let seed = game::parse_seed(seed_text).unwrap_or_else(game::random_seed);
    let mode_tag = if mode == Mode::Normal { 1 } else { 0 };
    let world_name = match &load {
        Some((n, _)) => n.clone(),
        None => format!("monde-{seed}"),
    };
    let handle = server::host_in_background(server::BackgroundOptions {
        seed,
        mode: mode_tag,
        name: "Solo".into(),
        solo: true,
        load: load.map(|(_, p)| p),
        world_name: world_name.clone(),
        map_blocks,
    })?;
    let address = format!("127.0.0.1:{}", handle.port);
    let token = handle.token;
    lobby.hosting = Some(handle);
    lobby.solo = true;
    lobby.mode = mode;
    net.connect(&address, name, token)?;
    lobby.joined_with = address;
    game.world_name = world_name;
    game.started = false;
    game.phase = Phase::Lobby;
    Ok(seed)
}

/// Quitte la partie en ligne (ferme le lobby si on l'héberge).
pub fn leave_online(net: &mut NetClient, lobby: &mut Lobby, game: &mut GameState) {
    let was_online = lobby.active || net.is_online() || lobby.hosting.is_some();
    net.disconnect();
    net.lost = None;
    lobby.reset();
    if was_online {
        game.started = false;
        game.phase = Phase::Menu;
    }
}

/// Crée un lobby : serveur en fond, puis connexion comme hôte.
pub fn create_lobby(
    net: &mut NetClient,
    lobby: &mut Lobby,
    game: &mut GameState,
    name: &str,
    seed_text: &str,
    mode: Mode,
) -> Result<(), String> {
    leave_online(net, lobby, game);
    let seed = game::parse_seed(seed_text).unwrap_or_else(game::random_seed);
    let mode_tag = if mode == Mode::Normal { 1 } else { 0 };
    let handle = server::host_in_background(server::BackgroundOptions {
        seed,
        mode: mode_tag,
        name: format!("Partie de {name}"),
        solo: false,
        load: None,
        world_name: "lobby".into(),
        map_blocks: 17,
    })?;
    let address = format!("127.0.0.1:{}", handle.port);
    let token = handle.token;
    lobby.hosting = Some(handle);
    lobby.mode = mode;
    net.connect(&address, name, token)?;
    lobby.joined_with = address;
    game.started = false;
    game.phase = Phase::Lobby;
    Ok(())
}

/// Rejoint une partie (code, adresse ou partie du réseau local).
pub fn join_lobby(net: &mut NetClient, lobby: &mut Lobby, game: &mut GameState, name: &str, target: &str) -> Result<(), String> {
    leave_online(net, lobby, game);
    let address = resolve_target(target);
    if address.is_empty() {
        return Err("saisis le code de la partie (ex. K7M2Q-9XH4C) ou une adresse".into());
    }
    net.connect(&address, name, 0)?;
    lobby.joined_with = target.trim().to_uppercase();
    game.started = false;
    game.phase = Phase::Lobby;
    Ok(())
}

/// Code affiché dans le lobby.
pub fn lobby_code(lobby: &Lobby) -> String {
    if let Some(host) = &lobby.hosting {
        let ip = match host.internet() {
            Internet::Open(ip) => Some(ip),
            _ => host.lan_ip(),
        };
        return match ip {
            Some(ip) => encode_code(SocketAddrV4::new(ip, host.port)),
            None => "…".into(),
        };
    }
    lobby.joined_with.clone()
}

/// Copie un texte dans le presse-papiers du système.
fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidates: &[(&str, &[&str])] = if cfg!(windows) {
        &[("clip", &[])]
    } else if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (program, args) in candidates {
        let mut cmd = Command::new(program);
        cmd.args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        if let Ok(mut child) = cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }
    }
    false
}

fn field_value<F: bevy::ecs::query::QueryFilter>(q: &Query<&mut TextField, F>) -> String {
    q.iter().next().map(|f| f.value.clone()).unwrap_or_default()
}

/// Boutons du menu, du lobby et de l'installation.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn actions(
    actions: Query<&MenuAction, Added<Pressed>>,
    mut state: ResMut<MenuState>,
    mut game: ResMut<GameState>,
    mut net: ResMut<NetClient>,
    mut lobby: ResMut<Lobby>,
    mut settings: ResMut<Settings>,
    mut toasts: ResMut<Toasts>,
    mut cursor: Single<&mut CursorOptions>,
    mut fields: ParamSet<(
        Query<&mut TextField, With<SeedField>>,
        Query<&mut TextField, With<LobbySeedField>>,
        Query<&mut TextField, With<NameField>>,
        Query<&mut TextField, With<JoinField>>,
        Query<&mut TextField, With<AddNameField>>,
        Query<&mut TextField, With<AddAddressField>>,
    )>,
    mut servers: ResMut<crate::servers::ServerList>,
    switches: Query<(&Switch, Has<DesktopSwitch>, Has<FirewallSwitch>)>,
    mut install_status: Query<&mut Text, With<InstallStatus>>,
    mut exit: MessageWriter<AppExit>,
) {
    for action in &actions {
        state.status.clear();
        match *action {
            MenuAction::Tab(tab) => state.tab = tab,
            MenuAction::Resume => {
                if game.started {
                    game.phase = Phase::Playing;
                    crate::player::grab(&mut cursor, true);
                }
            }
            MenuAction::Leave => {
                leave_online(&mut net, &mut lobby, &mut game);
                game.started = false;
                game.phase = Phase::Menu;
            }
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            MenuAction::SelectMode(mode) => state.mode = mode,
            MenuAction::RandomSeed => {
                let seed = game::random_seed().to_string();
                for mut f in &mut fields.p0() {
                    f.value = seed.clone();
                }
            }
            MenuAction::Launch => {
                let seed_text = field_value(&fields.p0());
                let name = field_value(&fields.p2());
                match launch_solo(&mut net, &mut lobby, &mut game, &name, &seed_text, state.mode, None, state.map_blocks) {
                    Ok(_) => crate::player::grab(&mut cursor, true),
                    Err(e) => state.status = e,
                }
            }
            MenuAction::LoadWorld(i) => {
                let name = field_value(&fields.p2());
                if let Some((stem, path)) = state.worlds.get(i).cloned() {
                    match launch_solo(&mut net, &mut lobby, &mut game, &name, "", state.mode, Some((stem, path)), state.map_blocks) {
                        Ok(_) => crate::player::grab(&mut cursor, true),
                        Err(e) => state.status = e,
                    }
                }
            }
            MenuAction::MapSize(blocks) => state.map_blocks = blocks,
            MenuAction::RefreshServers => servers.refresh(),
            MenuAction::AddServer => {
                let name = field_value(&fields.p4());
                let address = field_value(&fields.p5());
                if servers.add(&name, &address) {
                    for mut f in &mut fields.p4() {
                        f.value.clear();
                    }
                    for mut f in &mut fields.p5() {
                        f.value.clear();
                    }
                } else {
                    state.status = "Adresse vide ou déjà enregistrée.".into();
                }
            }
            MenuAction::RemoveServer(i) => servers.remove(i),
            MenuAction::JoinServer(i) => {
                let name = field_value(&fields.p2());
                if let Some(entry) = servers.entries.get(i).cloned() {
                    match join_lobby(&mut net, &mut lobby, &mut game, &name, &entry.address) {
                        Ok(()) => state.status = format!("Connexion à {}…", entry.name),
                        Err(e) => state.status = e,
                    }
                }
            }
            MenuAction::SaveWorld => {
                if net.is_online() {
                    let text = format!("save {}", game.world_name);
                    net.send_raw(crate::net::protocol::Packet::Command { text });
                    toasts.push("Sauvegarde du monde…");
                } else {
                    state.status = "Aucune partie à sauvegarder.".into();
                }
            }
            MenuAction::LobbyMode(mode) => state.lobby_mode = mode,
            MenuAction::CreateLobby | MenuAction::Join | MenuAction::JoinLan(_) => {
                let name = field_value(&fields.p2());
                if !name.trim().is_empty() && name != settings.name {
                    settings.name = name;
                    settings.clamp();
                }
                let name = settings.name.clone();
                let result = match *action {
                    MenuAction::CreateLobby => {
                        let seed_text = field_value(&fields.p1());
                        create_lobby(&mut net, &mut lobby, &mut game, &name, &seed_text, state.lobby_mode)
                    }
                    MenuAction::JoinLan(addr) => join_lobby(&mut net, &mut lobby, &mut game, &name, &addr.to_string()),
                    _ => {
                        let target = field_value(&fields.p3());
                        join_lobby(&mut net, &mut lobby, &mut game, &name, &target)
                    }
                };
                if let Err(e) = result {
                    leave_online(&mut net, &mut lobby, &mut game);
                    game.phase = Phase::Menu;
                    state.status = e;
                    state.tab = Tab::Multi;
                }
            }
            MenuAction::LobbyStart => {
                if lobby.is_host() {
                    net.send_raw(crate::net::protocol::Packet::Start);
                }
            }
            MenuAction::LobbyLeave => {
                leave_online(&mut net, &mut lobby, &mut game);
                game.phase = Phase::Menu;
                state.tab = Tab::Multi;
            }
            MenuAction::CopyCode => {
                let code = lobby_code(&lobby);
                if copy_to_clipboard(&code) {
                    toasts.push(format!("Code copié : {code}"));
                } else {
                    toasts.push("Copie impossible : note le code à la main");
                }
            }
            MenuAction::Install => {
                let mut options = install::Options::default();
                for (s, desktop, firewall) in &switches {
                    if desktop {
                        options.desktop_shortcut = s.on;
                    }
                    if firewall {
                        options.firewall = s.on;
                    }
                }
                let message = match install::install(options) {
                    Ok((exe, warnings)) => match install::launch(&exe) {
                        Ok(()) => {
                            exit.write(AppExit::Success);
                            format!("Installé ! Lancement… {}", warnings.join(" "))
                        }
                        Err(e) => {
                            game.phase = Phase::Menu;
                            format!("Installé, mais relance impossible ({e}) : lance BoomGround depuis le Bureau.")
                        }
                    },
                    Err(e) => e,
                };
                for mut t in &mut install_status {
                    t.0 = message.clone();
                }
            }
            MenuAction::SkipInstall => game.phase = Phase::Menu,
            MenuAction::Uninstall => {
                if install::uninstall().is_ok() {
                    exit.write(AppExit::Success);
                } else {
                    state.status = "Désinstallation impossible.".into();
                }
            }
            MenuAction::Setting(kind, dir) => {
                let d = dir as f32;
                // Un réglage graphique individuel passe en « Personnalisé ».
                if matches!(
                    kind,
                    SettingKind::ViewDistance | SettingKind::Shadows | SettingKind::Msaa | SettingKind::Particles
                ) {
                    settings.quality = 4;
                }
                match kind {
                    SettingKind::Quality => {
                        let q = (settings.quality as i32 + dir as i32).rem_euclid(5) as u8;
                        if q < 4 {
                            settings.apply_quality(q);
                        } else {
                            settings.quality = 4;
                        }
                    }
                    SettingKind::Msaa => {
                        // Seulement « aucun » et « 4× » : le 2× n'est
                        // garanti par aucun pilote et fait planter WebGPU
                        // sur les adaptateurs logiciels (llvmpipe).
                        settings.msaa = if settings.msaa == 0 {
                            if dir > 0 { 4 } else { 0 }
                        } else if dir > 0 {
                            0
                        } else {
                            4
                        }
                    }
                    SettingKind::Particles => {
                        settings.particles = (settings.particles as i32 + dir as i32).rem_euclid(3) as u8
                    }
                    SettingKind::Music => settings.music += 0.1 * d,
                    SettingKind::VoiceVolume => settings.voice_volume += 0.1 * d,
                    SettingKind::VoiceMode => {
                        settings.voice_mode = (settings.voice_mode as i32 + dir as i32).rem_euclid(3) as u8
                    }
                    SettingKind::Sensitivity => settings.sensitivity *= if dir > 0 { 1.15 } else { 1.0 / 1.15 },
                    SettingKind::Volume => settings.volume += 0.1 * d,
                    SettingKind::Sfx => settings.sfx += 0.1 * d,
                    SettingKind::Fov => settings.fov += 5.0 * d,
                    SettingKind::ViewDistance => settings.view_distance += 40.0 * d,
                    SettingKind::Shadows => {
                        settings.shadows = (settings.shadows as i32 + dir as i32).clamp(0, 2) as u8
                    }
                    SettingKind::Shake => settings.shake += 0.25 * d,
                    SettingKind::JoystickSide => {
                        settings.joystick_side = (settings.joystick_side as i32 + dir as i32).rem_euclid(2) as u8
                    }
                    SettingKind::TouchSensitivity => settings.touch_sensitivity *= if dir > 0 { 1.15 } else { 1.0 / 1.15 },
                    SettingKind::Fps => {
                        settings.fps_limit = match (settings.fps_limit <= 0.0, dir > 0) {
                            (true, true) => 0.0,
                            (true, false) => 240.0,
                            (false, true) if settings.fps_limit >= 240.0 => 0.0,
                            (false, _) => (settings.fps_limit + 30.0 * d).max(30.0),
                        }
                    }
                }
                settings.clamp();
            }
        }
    }
}

/// Transitions automatiques : connexion perdue, partie lancée par l'hôte,
/// pause avec Échap.
#[allow(clippy::too_many_arguments)]
pub fn flow(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<GameState>,
    mut state: ResMut<MenuState>,
    mut net: ResMut<NetClient>,
    mut lobby: ResMut<Lobby>,
    mut toasts: ResMut<Toasts>,
    chat: Res<crate::chat::Chat>,
    shop: Res<crate::shop::ShopState>,
    fields: Query<&TextField>,
    mut cursor: Single<&mut CursorOptions>,
) {
    if let Some(reason) = net.lost.take() {
        let was_in_game = game.phase == Phase::Playing;
        leave_online(&mut net, &mut lobby, &mut game);
        game.phase = Phase::Menu;
        state.tab = Tab::Multi;
        state.status = reason.clone();
        if was_in_game {
            toasts.push(reason);
        }
    }

    if game.phase == Phase::Lobby && lobby.started {
        game.phase = Phase::Playing;
        game.started = true;
        crate::player::grab(&mut cursor, true);
        toasts.push("La partie commence !");
    }

    let escape = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::F1);
    if escape && !ui::typing(&fields) && !game.typing && !chat.closed_now {
        match game.phase {
            Phase::Playing if !shop.open => game.phase = Phase::Menu,
            Phase::Menu if game.started => {
                game.phase = Phase::Playing;
                crate::player::grab(&mut cursor, true);
            }
            _ => {}
        }
    }
    if game.phase != Phase::Playing && cursor.grab_mode != bevy::window::CursorGrabMode::None {
        crate::player::grab(&mut cursor, false);
    }
}

/// Liste des parties du réseau local (onglet Multijoueur).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn lan_list(
    game: Res<GameState>,
    state: Res<MenuState>,
    font: Res<UiFont>,
    mut browser: ResMut<LanBrowser>,
    mut shown: Local<Option<Vec<(SocketAddr, String, u8, bool, u8)>>>,
    list: Query<Entity, With<LanList>>,
    mut commands: Commands,
) {
    if game.phase != Phase::Menu || state.tab != Tab::Multi {
        return;
    }
    browser.refresh();
    let current: Vec<_> = browser
        .games
        .iter()
        .map(|g| (g.addr, g.info.name.clone(), g.info.players, g.info.started, g.info.mode))
        .collect();
    if shown.as_ref() == Some(&current) {
        return;
    }
    *shown = Some(current.clone());
    let Ok(list) = list.single() else { return };
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|l| {
        if current.is_empty() {
            l.spawn(ui::text(&font, "Recherche de parties sur ton Wi-Fi…", 15.0, ui::MUTED));
            return;
        }
        for (addr, name, players, started, mode) in current {
            let node = Node {
                width: Val::Percent(100.0),
                height: px(48.0),
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(14.0)),
                ..default()
            };
            l.spawn((MenuAction::JoinLan(addr), ui::button_with(node, ui::CARD_HOVER)))
                .with_children(|row| {
                    row.spawn(ui::text(&font, name, 17.0, ui::TEXT));
                    let mode = if mode == 1 { "Normal" } else { "Bac à sable" };
                    let phase = if started { "en cours" } else { "lobby" };
                    row.spawn(ui::text(&font, format!("{players} joueur(s) · {mode} · {phase}"), 13.0, ui::MUTED));
                });
        }
    });
}

/// Liste des mondes sauvegardés (onglet Jouer).
pub fn world_list(
    game: Res<GameState>,
    mut state: ResMut<MenuState>,
    font: Res<UiFont>,
    mut shown: Local<Option<Vec<(String, u64)>>>,
    list: Query<Entity, With<WorldList>>,
    mut commands: Commands,
) {
    if game.phase != Phase::Menu || state.tab != Tab::Play {
        *shown = None;
        return;
    }
    let dir = game::config_dir().map(|d| d.join("saves")).unwrap_or_default();
    let found = crate::net::save::list(&dir);
    let current: Vec<(String, u64)> = found.iter().map(|(n, _, t)| (n.clone(), *t)).collect();
    if shown.as_ref() == Some(&current) {
        return;
    }
    *shown = Some(current.clone());
    state.worlds = found.into_iter().map(|(n, p, _)| (n, p)).collect();
    let Ok(list) = list.single() else { return };
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|l| {
        if state.worlds.is_empty() {
            l.spawn(ui::text(&font, "Aucun monde sauvegardé pour l'instant.", 15.0, ui::MUTED));
            return;
        }
        for (i, (name, _)) in state.worlds.iter().enumerate().take(6) {
            let node = Node {
                width: Val::Percent(100.0),
                height: px(44.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(14.0)),
                ..default()
            };
            l.spawn((MenuAction::LoadWorld(i), ui::button_with(node, ui::CARD_HOVER)))
                .with_children(|row| {
                    row.spawn(ui::text(&font, name.clone(), 17.0, ui::TEXT));
                    row.spawn(ui::text(&font, "Charger", 14.0, ui::ACCENT));
                });
        }
    });
}

/// Liste des serveurs enregistrés (onglet Multijoueur).
pub fn server_list_ui(
    game: Res<GameState>,
    state: Res<MenuState>,
    font: Res<UiFont>,
    mut servers: ResMut<crate::servers::ServerList>,
    list: Query<Entity, With<ServerListUi>>,
    mut shown: Local<u32>,
    mut commands: Commands,
) {
    let active = game.phase == Phase::Menu && state.tab == Tab::Multi;
    servers.poll(active);
    if !active {
        *shown = u32::MAX;
        return;
    }
    if *shown == servers.revision {
        return;
    }
    *shown = servers.revision;
    let Ok(list) = list.single() else { return };
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|l| {
        if servers.entries.is_empty() {
            l.spawn(ui::text(&font, "Aucun serveur : ajoute-en un ci-dessous.", 15.0, ui::MUTED));
            return;
        }
        for (i, e) in servers.entries.iter().enumerate() {
            let (detail, color) = match &e.state {
                crate::servers::State::Online(info) => {
                    let mode = if info.mode == 1 { "Normal" } else { "Bac à sable" };
                    let old = info.version != crate::net::protocol::PROTOCOL_VERSION;
                    (
                        format!(
                            "{}/{} · {} · {} ms{}",
                            info.players,
                            info.max,
                            mode,
                            info.ping_ms,
                            if old { " · version différente" } else { "" }
                        ),
                        if old { ui::HOT } else { ui::GREEN },
                    )
                }
                crate::servers::State::Offline => ("hors ligne".to_string(), ui::RED),
                _ => ("…".to_string(), ui::MUTED),
            };
            l.spawn(Node {
                column_gap: px(6.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    MenuAction::JoinServer(i),
                    ui::button_with(
                        Node {
                            flex_grow: 1.0,
                            min_height: px(52.0),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::horizontal(px(12.0)),
                            ..default()
                        },
                        ui::CARD_HOVER,
                    ),
                ))
                .with_children(|b| {
                    b.spawn(ui::text(&font, e.name.clone(), 17.0, ui::TEXT));
                    let motd = match &e.state {
                        crate::servers::State::Online(info) if !info.motd.is_empty() => info.motd.clone(),
                        _ => e.address.clone(),
                    };
                    b.spawn(ui::text(&font, motd, 12.0, ui::MUTED));
                    b.spawn(ui::text(&font, detail, 12.0, color));
                });
                row.spawn((MenuAction::RemoveServer(i), ui::button(38.0, 52.0, Color::srgba(0.35, 0.08, 0.08, 0.85))))
                    // « × » (U+00D7) et non « ✕ » (U+2715) : ce dernier
                    // n'existe pas dans la police embarquée et s'affichait
                    // en carré vide.
                    .with_child(ui::label(&font, "×", 26.0, ui::TEXT));
            });
        }
    });
}

/// Contour de la taille de carte choisie.
pub fn map_size_highlight(state: Res<MenuState>, mut buttons: Query<(&MapSizeButton, &mut BorderColor)>) {
    for (b, mut border) in &mut buttons {
        let c = if b.0 == state.map_blocks { ui::ACCENT } else { ui::LINE };
        if border.top != c {
            *border = BorderColor::all(c);
        }
    }
}

/// Solo : le serveur privé se met en pause quand le menu est ouvert.
pub fn solo_pause(game: Res<GameState>, lobby: Res<Lobby>, mut net: ResMut<NetClient>, mut was: Local<bool>) {
    let paused = lobby.solo && game.started && game.phase != Phase::Playing;
    if paused != *was && net.is_online() {
        *was = paused;
        let text = if paused { "pause on" } else { "pause off" };
        net.send_raw(crate::net::protocol::Packet::Command { text: text.into() });
    }
}

/// Textes du lobby : titre, code, réseau, joueurs.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn lobby_screen(
    game: Res<GameState>,
    lobby: Res<Lobby>,
    net: Res<NetClient>,
    font: Res<UiFont>,
    mut texts: ParamSet<(
        Query<&mut Text, With<LobbyTitle>>,
        Query<&mut Text, With<LobbyCode>>,
        Query<&mut Text, With<LobbyNet>>,
        Query<&mut Text, With<LobbyHint>>,
    )>,
    players: Query<Entity, With<LobbyPlayers>>,
    mut shown: Local<Option<Vec<(u16, String)>>>,
    mut commands: Commands,
) {
    if game.phase != Phase::Lobby {
        *shown = None;
        return;
    }
    let set = |t: &mut Text, v: String| {
        if t.0 != v {
            t.0 = v;
        }
    };
    let mode = match lobby.mode {
        Mode::Sandbox => "Bac à sable",
        Mode::Normal => "Mode normal",
    };
    let host_name = lobby
        .players
        .iter()
        .find(|(id, _)| *id == lobby.host_id)
        .map(|(_, n)| n.clone());
    let title = match (&host_name, lobby.active) {
        (_, false) => "Connexion…".to_string(),
        (Some(name), true) => format!("Partie de {name} · {mode}"),
        (None, true) => format!("Serveur · {mode}"),
    };
    for mut t in &mut texts.p0() {
        set(&mut t, title.clone());
    }
    for mut t in &mut texts.p1() {
        set(&mut t, lobby_code(&lobby));
    }
    let network = match &lobby.hosting {
        Some(host) => {
            let lan = host
                .lan_ip()
                .map(|ip| format!("Wi-Fi : {ip}:{}", host.port))
                .unwrap_or_default();
            match host.internet() {
                Internet::Checking => format!("Ouverture du port sur ta box…   {lan}"),
                Internet::Open(_) => format!("Accessible depuis Internet et sur ton Wi-Fi.   {lan}"),
                Internet::Shared(_) => format!(
                    "Ta box est derrière le réseau de ton opérateur : seuls les joueurs de ton Wi-Fi peuvent entrer.   {lan}"
                ),
                Internet::Closed(reason) => format!(
                    "Les joueurs de ton Wi-Fi peuvent entrer. Internet : {reason}. Active l'UPnP dans \
                     l'interface de ta box, ou redirige le port {} (TCP) vers ce PC ({}).",
                    host.port,
                    host.lan_ip().map(|ip| ip.to_string()).unwrap_or_default()
                ),
            }
        }
        None => "Ce code permet à d'autres amis de rejoindre aussi.".into(),
    };
    for mut t in &mut texts.p2() {
        set(&mut t, network.clone());
    }
    let hint = if lobby.active { "En attente de l'hôte…" } else { "Connexion au lobby…" };
    for mut t in &mut texts.p3() {
        set(&mut t, hint.to_string());
    }

    if shown.as_ref() == Some(&lobby.players) {
        return;
    }
    *shown = Some(lobby.players.clone());
    let Ok(list) = players.single() else { return };
    commands.entity(list).despawn_children();
    let me = net.id;
    commands.entity(list).with_children(|l| {
        for (id, name) in &lobby.players {
            l.spawn((
                Node {
                    height: px(42.0),
                    align_items: AlignItems::Center,
                    column_gap: px(12.0),
                    padding: UiRect::horizontal(px(14.0)),
                    border_radius: BorderRadius::all(px(8.0)),
                    ..default()
                },
                BackgroundColor(ui::CARD_HOVER),
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: px(14.0),
                        height: px(14.0),
                        border_radius: BorderRadius::all(px(7.0)),
                        ..default()
                    },
                    BackgroundColor(player_color(*id)),
                ));
                row.spawn(ui::text(&font, name.clone(), 18.0, ui::TEXT));
                if *id == lobby.host_id {
                    row.spawn(ui::text(&font, "HÔTE", 12.0, ui::ACCENT));
                }
                if Some(*id) == me {
                    row.spawn(ui::text(&font, "(toi)", 14.0, ui::MUTED));
                }
            });
        }
    });
}

/// Valeurs des réglages et application (plein écran, champ de vision…).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn settings_panel(
    mut settings: ResMut<Settings>,
    mut volume: ResMut<GlobalVolume>,
    mut limit: ResMut<crate::perf::FpsLimit>,
    values: Query<(&SettingValue, &Children)>,
    mut texts: Query<&mut Text>,
    switches: Query<
        (
            &Switch,
            Has<InvertSwitch>,
            Has<FullscreenSwitch>,
            Has<ShowFpsSwitch>,
            Has<CrosshairSwitch>,
            Has<BobSwitch>,
            Has<BloomSwitch>,
        ),
        Changed<Switch>,
    >,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut projection: Query<&mut Projection, With<crate::player::WorldCamera>>,
    mut fog: Query<&mut DistanceFog, With<crate::player::WorldCamera>>,
    mut suns: Query<&mut DirectionalLight>,
    mut shadow_map: ResMut<bevy::light::DirectionalLightShadowMap>,
    mut density: ResMut<crate::fx::ParticleDensity>,
    mut cameras: Query<(Entity, &mut Msaa), With<Camera3d>>,
    world_cam: Query<Entity, With<crate::player::WorldCamera>>,
    mut commands: Commands,
    mut first: Local<bool>,
) {
    for (s, invert, fullscreen, show_fps, crosshair, bob, bloom) in &switches {
        if bloom && settings.bloom != s.on {
            settings.bloom = s.on;
            settings.quality = 4;
        }
        if show_fps && settings.show_fps != s.on {
            settings.show_fps = s.on;
        }
        if crosshair && settings.crosshair != s.on {
            settings.crosshair = s.on;
        }
        if bob && settings.head_bob != s.on {
            settings.head_bob = s.on;
        }
        if invert && settings.invert_y != s.on {
            settings.invert_y = s.on;
        }
        if fullscreen && settings.fullscreen != s.on {
            settings.fullscreen = s.on;
        }
    }
    if !settings.is_changed() && *first {
        return;
    }
    if *first {
        game::save_settings(&settings);
    }
    *first = true;
    volume.volume = bevy::audio::Volume::Linear(settings.volume);
    limit.fps = settings.fps_limit;
    if let Ok(mut w) = window.single_mut() {
        let mode = if settings.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
        if w.mode != mode {
            w.mode = mode;
        }
    }
    crate::fx::set_sfx_scale(settings.sfx);
    density.0 = settings.particles;
    let msaa = if settings.msaa == 0 { Msaa::Off } else { Msaa::Sample4 };
    for (_, mut m) in &mut cameras {
        if *m != msaa {
            *m = msaa;
        }
    }
    if let Ok(e) = world_cam.single() {
        if settings.bloom {
            commands.entity(e).insert(bevy::post_process::bloom::Bloom {
                intensity: 0.12,
                ..bevy::post_process::bloom::Bloom::NATURAL
            });
        } else {
            commands.entity(e).remove::<bevy::post_process::bloom::Bloom>();
        }
    }
    if let Ok(mut p) = projection.single_mut() {
        if let Projection::Perspective(persp) = p.as_mut() {
            persp.fov = settings.fov.to_radians();
            persp.far = settings.view_distance + 60.0;
        }
    }
    if let Ok(mut f) = fog.single_mut() {
        f.falloff = FogFalloff::Linear {
            start: settings.view_distance * 0.35,
            end: settings.view_distance,
        };
    }
    for mut sun in &mut suns {
        if sun.illuminance > 5000.0 {
            sun.shadow_maps_enabled = settings.shadows > 0;
        }
    }
    let size = if settings.shadows >= 2 { 4096 } else { 2048 };
    if shadow_map.size != size {
        shadow_map.size = size;
    }
    for (value, children) in &values {
        let v = match value.0 {
            SettingKind::Sensitivity => format!("{:.0} %", settings.sensitivity / 0.0022 * 100.0),
            SettingKind::Volume => format!("{:.0} %", settings.volume * 100.0),
            SettingKind::Fov => format!("{:.0}°", settings.fov),
            SettingKind::Quality => crate::game::QUALITY_NAMES[settings.quality.min(4) as usize].to_string(),
            SettingKind::Msaa => match settings.msaa {
                0 => "aucun".to_string(),
                n => format!("{n}×"),
            },
            SettingKind::Particles => ["aucune", "réduites", "pleines"][settings.particles.min(2) as usize].to_string(),
            SettingKind::Music => format!("{:.0} %", settings.music * 100.0),
            SettingKind::VoiceVolume => format!("{:.0} %", settings.voice_volume * 100.0),
            SettingKind::VoiceMode => ["désactivée", "activation vocale", "touche G"][settings.voice_mode.min(2) as usize].to_string(),
            SettingKind::Sfx => format!("{:.0} %", settings.sfx * 100.0),
            SettingKind::ViewDistance => format!("{:.0} m", settings.view_distance),
            SettingKind::Shadows => ["aucune", "moyenne", "haute"][settings.shadows.min(2) as usize].to_string(),
            SettingKind::Shake => format!("{:.0} %", settings.shake * 100.0),
            SettingKind::JoystickSide => ["gauche", "droite"][settings.joystick_side.min(1) as usize].to_string(),
            SettingKind::TouchSensitivity => format!("{:.0} %", settings.touch_sensitivity / 0.005 * 100.0),
            SettingKind::Fps => {
                if settings.fps_limit <= 0.0 {
                    "écran".to_string()
                } else {
                    format!("{:.0}", settings.fps_limit)
                }
            }
        };
        for child in children.iter() {
            if let Ok(mut t) = texts.get_mut(child) {
                if t.0 != v {
                    t.0 = v.clone();
                }
            }
        }
    }
}

/// Ligne d'état sous le menu.
pub fn status_text(state: Res<MenuState>, mut text: Query<&mut Text, With<StatusText>>) {
    if !state.is_changed() {
        return;
    }
    for mut t in &mut text {
        if t.0 != state.status {
            t.0 = state.status.clone();
        }
    }
}

/// Derrière le menu, la caméra tourne lentement au-dessus de la ville ;
/// au lancement, le joueur est remis au point d'apparition.
pub fn cinematic_camera(
    time: Res<Time>,
    game: Res<GameState>,
    world: Res<VoxelWorld>,
    net: Res<NetClient>,
    mut was_cinematic: Local<bool>,
    mut player: Query<(&mut Transform, &mut crate::player::Body), With<crate::player::Player>>,
    mut head: Query<&mut Transform, (With<crate::player::Head>, Without<crate::player::Player>)>,
) {
    let cinematic = matches!(game.phase, Phase::Install | Phase::Lobby) || (game.phase == Phase::Menu && !game.started);
    let Ok((mut t, mut body)) = player.single_mut() else { return };
    let Ok(mut h) = head.single_mut() else { return };
    if cinematic {
        let a = time.elapsed_secs() * 0.04;
        let center = world.spawn;
        t.translation = center + Vec3::new(a.cos() * 46.0, 30.0, a.sin() * 46.0);
        let to = center - t.translation;
        body.yaw = (-to.x).atan2(-to.z);
        body.pitch = -0.5;
        body.vel = Vec3::ZERO;
        t.rotation = Quat::from_rotation_y(body.yaw);
        h.rotation = Quat::from_rotation_x(body.pitch);
    } else if *was_cinematic {
        t.translation = crate::player::spawn_point(&world, net.id);
        body.pitch = 0.0;
        body.fly = false;
        body.vel = Vec3::ZERO;
        h.rotation = Quat::IDENTITY;
    }
    *was_cinematic = cinematic;
}
