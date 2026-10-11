//! Menu principal : mode solo ou multijoueur (serveur par défaut : 28431).
//!
//! En multijoueur on saisit `domaine.tld` ou `ip:port` ; sans port explicite
//! on utilise [`crate::net::DEFAULT_PORT`].

use bevy::input::ButtonState;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::game::{GameState, Phase};
use crate::net;
use crate::net::client::NetClient;

/// Ce que le menu affiche.
#[derive(Resource, Default)]
pub struct MenuView {
    pub show_multi: bool,
    /// Le panneau des paramètres est ouvert (on masque le reste du menu).
    pub show_settings: bool,
}

#[derive(Component)]
pub struct SoloButton;

#[derive(Component)]
pub struct MultiButton;

#[derive(Component)]
pub struct JoinButton;

#[derive(Component)]
pub struct AddressField;

#[derive(Component)]
pub struct StatusText;

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct SettingsRoot;

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
pub struct SettingsButton;
#[derive(Component)]
pub struct SensitivityText;
#[derive(Component)]
pub struct VolumeText;
#[derive(Component)]
pub struct FpsText;

/// Ligne « libellé  [-] valeur [+] ».
fn setting_row(
    font: &Handle<Font>,
    name: &str,
    minus: impl Component,
    plus: impl Component,
    value: impl Component,
) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: px(44.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        children![
            (
                Text::new(name.to_string()),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.92, 0.96)),
            ),
            (minus, Button, button_node(52.0, 38.0), children![label(font, "-", 22.0, Color::WHITE)]),
            (value, Text::new(""), TextFont { font: font.clone().into(), font_size: FontSize::Px(20.0), ..default() }, TextColor(Color::srgb(0.6, 0.85, 1.0))),
            (plus, Button, button_node(52.0, 38.0), children![label(font, "+", 22.0, Color::WHITE)]),
        ],
    )
}

pub fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    net: Option<Res<NetClient>>,
) {
    let font: Handle<Font> = asset_server.load("fonts/ui.ttf");

    commands.insert_resource(MenuView::default());

    let mut root = commands.spawn((
        MenuRoot,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: px(14.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.92)),
        ZIndex(100),
    ));

    root.with_children(|menu| {
        menu.spawn((
            Text::new("BOOMGROUND"),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(64.0),
                ..default()
            },
            TextColor(Color::srgb(0.95, 0.80, 0.25)),
            Node {
                margin: UiRect::bottom(px(30.0)),
                ..default()
            },
        ));

        menu.spawn((
            SoloButton,
            Button,
            button_node(320.0, 60.0),
            BackgroundColor(Color::srgba(0.13, 0.15, 0.20, 0.95)),
            children![label(&font, "SOLO", 28.0, Color::srgb(0.92, 0.92, 0.95))],
        ));

        menu.spawn((
            SettingsButton,
            Button,
            button_node(320.0, 46.0),
            children![label(&font, "PARAMÈTRES", 20.0, Color::srgb(0.8, 0.85, 0.9))],
        ));

        menu.spawn((
            MultiButton,
            Button,
            button_node(320.0, 60.0),
            BackgroundColor(Color::srgba(0.10, 0.16, 0.24, 0.95)),
            children![label(&font, "MULTIJOUEUR", 28.0, Color::srgb(0.55, 0.80, 1.0))],
        ));

        // Champ d'adresse (masqué tant qu'on n'a pas choisi multijoueur).
        menu.spawn((
            AddressField,
            Node {
                width: px(460.0),
                height: px(48.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.14, 0.18, 0.95)),
            Text::new(net_addr(&net)),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.95, 1.0)),
            Visibility::Hidden,
        ));

        menu.spawn((
            JoinButton,
            Button,
            button_node(320.0, 52.0),
            BackgroundColor(Color::srgba(0.10, 0.16, 0.24, 0.95)),
            children![label(&font, "REJOINDRE", 24.0, Color::srgb(0.55, 0.80, 1.0))],
        ))
        .insert(Visibility::Hidden);

        menu.spawn((
            SettingsRoot,
            Node {
                width: px(520.0),
                height: Val::Percent(70.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.0),
                padding: UiRect::all(px(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.08, 0.11, 0.97)),
            Visibility::Hidden,
            children![
                (
                    Text::new("PARAMÈTRES"),
                    TextFont {
                        font: font.clone().into(),
                        font_size: FontSize::Px(28.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.80, 0.25)),
                    Node {
                        margin: UiRect::bottom(px(10.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ),
                setting_row(&font, "Sensibilité", SensitivityDown, SensitivityUp, SensitivityText),
                setting_row(&font, "Volume", VolumeDown, VolumeUp, VolumeText),
                setting_row(&font, "Limite d'images", FpsDown, FpsUp, FpsText),
                (
                    Button,
                    InvertY,
                    button_node(200.0, 44.0),
                    children![label(&font, "Inverser l'axe Y", 18.0, Color::srgb(0.9, 0.9, 0.95))],
                ),
            ],
        ));

        menu.spawn((
            StatusText,
            Text::new(""),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.78, 0.85)),
            Node {
                margin: UiRect::top(px(10.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ));
    });
}

/// Le contenu d'un bouton.
fn label(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text.to_string()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::justify(Justify::Center),
        Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

fn net_addr(net: &Option<Res<NetClient>>) -> String {
    net.as_ref()
        .map(|n| n.address.clone())
        .unwrap_or_else(|| String::new())
}

fn button_node(width: f32, height: f32) -> Node {
    Node {
        width: px(width),
        height: px(height),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// Affiche ou masque le menu selon la phase de jeu.
pub fn toggle_visibility(
    game: Res<GameState>,
    mut root: Query<&mut Visibility, (With<MenuRoot>, Without<AddressField>, Without<JoinButton>)>,
    mut hud: Query<&mut Visibility, (With<crate::hud::Hud>, Without<MenuRoot>, Without<AddressField>, Without<JoinButton>)>,
) {
    let shown = game.phase == Phase::Menu;
    let target = |shown: bool| {
        if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for mut v in &mut root {
        *v = target(shown);
    }
    // Le HUD n'a rien à faire dans le menu.
    for mut v in &mut hud {
        *v = target(!shown);
    }
}

/// Panneau de réglages : sensibilité, volume, limite d'images, axe Y.
pub fn settings_input(
    mut settings: ResMut<crate::game::Settings>,
    mut volume: ResMut<GlobalVolume>,
    mut limit: ResMut<crate::perf::FpsLimit>,
    view: ResMut<MenuView>,
    mut sens_text: Query<&mut Text, (With<SensitivityText>, Without<VolumeText>, Without<FpsText>)>,
    mut vol_text: Query<&mut Text, (With<VolumeText>, Without<SensitivityText>, Without<FpsText>)>,
    mut fps_text: Query<&mut Text, (With<FpsText>, Without<SensitivityText>, Without<VolumeText>)>,
    sens_down: Query<&Pressed, (Changed<Pressed>, With<SensitivityDown>)>,
    sens_up: Query<&Pressed, (Changed<Pressed>, With<SensitivityUp>)>,
    vol_down: Query<&Pressed, (Changed<Pressed>, With<VolumeDown>)>,
    vol_up: Query<&Pressed, (Changed<Pressed>, With<VolumeUp>)>,
    fps_down: Query<&Pressed, (Changed<Pressed>, With<FpsDown>)>,
    fps_up: Query<&Pressed, (Changed<Pressed>, With<FpsUp>)>,
    invert: Query<&Pressed, (Changed<Pressed>, With<InvertY>)>,
) {
    for _ in sens_down.iter() {
        settings.sensitivity *= 0.8;
    }
    for _ in sens_up.iter() {
        settings.sensitivity *= 1.25;
    }
    for _ in vol_down.iter() {
        settings.volume *= 0.8;
    }
    for _ in vol_up.iter() {
        settings.volume *= 1.25;
    }
    for _ in fps_down.iter() {
        settings.fps_limit = if settings.fps_limit <= 0.0 {
            144.0
        } else {
            (settings.fps_limit - 15.0).max(30.0)
        };
    }
    for _ in fps_up.iter() {
        settings.fps_limit = (settings.fps_limit + 15.0).min(360.0);
    }
    for _ in invert.iter() {
        settings.invert_y = !settings.invert_y;
    }
    settings.clamp();
    volume.volume = bevy::audio::Volume::Linear(settings.volume);
    limit.fps = settings.fps_limit;

    // Textes de valeurs.
    if let Ok(mut text) = sens_text.single_mut() {
        text.0 = format!("{:.0} %", settings.sensitivity / 0.0022 * 100.0);
    }
    if let Ok(mut text) = vol_text.single_mut() {
        text.0 = format!("{:.0} %", settings.volume * 100.0);
    }
    if let Ok(mut text) = fps_text.single_mut() {
        text.0 = match settings.fps_limit {
            0.0 => "écran".to_string(),
            f => format!("{f:.0}"),
        };
    }
    let _ = view;
}

/// Interagit avec le menu : solo, multijoueur, saisie de l'adresse.
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Single<&mut CursorOptions>,
    mut game: ResMut<GameState>,
    mut view: ResMut<MenuView>,
    mut net: Option<ResMut<NetClient>>,
    mut settings_root: Query<&mut Visibility, (With<SettingsRoot>, Without<AddressField>, Without<JoinButton>)>,
    settings_btn: Query<(), (Changed<Pressed>, With<SettingsButton>)>,
    solo: Query<(), (Changed<Pressed>, With<SoloButton>)>,
    multi: Query<(), (Changed<Pressed>, With<MultiButton>)>,
    join: Query<(), (Changed<Pressed>, With<JoinButton>)>,
    mut field: Query<
        (&mut Visibility, &mut Text),
        (With<AddressField>, Without<StatusText>, Without<JoinButton>),
    >,
    mut join_btn: Query<
        &mut Visibility,
        (With<JoinButton>, Without<AddressField>, Without<StatusText>),
    >,
    mut status: Query<
        &mut Text,
        (With<StatusText>, Without<AddressField>, Without<JoinButton>),
    >,
    mut keyboard: MessageReader<KeyboardInput>,
) {
    // Retour au menu depuis le jeu (Échap ou F1).
    if game.in_game() {
        if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::F1) {
            game.phase = Phase::Menu;
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        return;
    }

    // Panneau des paramètres.
    for _ in settings_btn.iter() {
        view.show_settings = !view.show_settings;
    }
    if keys.just_pressed(KeyCode::Escape) {
        view.show_settings = false;
    }
    if let Ok(mut v) = settings_root.single_mut() {
        *v = if view.show_settings {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    for _ in solo.iter() {
        game.phase = Phase::Playing;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    for _ in multi.iter() {
        view.show_multi = true;
    }

    // Saisie de l'adresse.
    let Ok((mut field_vis, mut field_text)) = field.single_mut() else {
        return;
    };
    *field_vis = if view.show_multi {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if let Ok(mut v) = join_btn.single_mut() {
        *v = if view.show_multi {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    let Ok(mut status_text) = status.single_mut() else {
        return;
    };

    let mut edited = false;
    for ev in keyboard.read() {
        if ev.state != ButtonState::Pressed || !view.show_multi {
            continue;
        }
        match ev.key_code {
            KeyCode::Backspace => {
                if let Some(c) = net.as_mut() {
                    c.address.pop();
                    field_text.0 = c.address.clone();
                    edited = true;
                }
            }
            KeyCode::Escape => {
                view.show_multi = false;
            }
            _ => {}
        }
        if let Some(text) = ev.text.as_ref() {
            if let Some(c) = net.as_mut() {
                c.address.push_str(text);
                field_text.0 = c.address.clone();
                edited = true;
            }
        }
    }
    let _ = edited;

    for _ in join.iter() {
        let Some(client) = net.as_mut() else {
            status_text.0 = "Réseau indisponible.".into();
            return;
        };
        match client.connect() {
            Ok(addr) => {
                status_text.0 = format!("Connecté à {addr}…");
                game.phase = Phase::Playing;
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
            Err(e) => {
                status_text.0 = format!("Échec de connexion : {e}");
            }
        }
    }

    // Message par défaut.
    if status_text.0.is_empty() {
        status_text.0 = format!(
            "Serveur par défaut : port {} — ex. caca.com ou caca.com:9393",
            net::DEFAULT_PORT
        );
    }
}