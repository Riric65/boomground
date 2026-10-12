//! Briques d'interface partagées : palette, police embarquée, textes,
//! boutons (avec survol), champs de saisie, interrupteurs, notifications.
//!
//! La police est **compilée dans l'exécutable** : le jeu n'a besoin d'aucun
//! dossier `assets` à côté de lui, ce qui permet de le distribuer en un
//! seul fichier.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const BG: Color = Color::srgba(0.035, 0.043, 0.065, 0.94);
pub const PANEL: Color = Color::srgba(0.075, 0.09, 0.125, 0.96);
pub const CARD: Color = Color::srgba(0.105, 0.125, 0.17, 0.98);
pub const CARD_HOVER: Color = Color::srgba(0.14, 0.165, 0.22, 1.0);
pub const FIELD: Color = Color::srgba(0.05, 0.06, 0.085, 1.0);
pub const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);
pub const ACCENT: Color = Color::srgb(1.0, 0.71, 0.15);
pub const ACCENT_DARK: Color = Color::srgb(0.10, 0.07, 0.02);
pub const HOT: Color = Color::srgb(1.0, 0.35, 0.21);
pub const BLUE: Color = Color::srgb(0.31, 0.66, 1.0);
pub const GREEN: Color = Color::srgb(0.24, 0.86, 0.52);
pub const RED: Color = Color::srgb(0.95, 0.30, 0.30);
pub const TEXT: Color = Color::srgb(0.93, 0.945, 0.97);
pub const MUTED: Color = Color::srgb(0.56, 0.60, 0.68);

/// Bouton plein couleur d'accent.
pub const BUTTON_PRIMARY: Color = ACCENT;
/// Bouton secondaire.
pub const BUTTON: Color = Color::srgba(0.15, 0.175, 0.235, 0.98);

/// Police de l'interface (avec accents).
#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

pub fn setup_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let bytes = include_bytes!("../assets/fonts/ui.ttf").to_vec();
    commands.insert_resource(UiFont(fonts.add(Font::from_bytes(bytes))));
}

/// Texte simple.
pub fn text(font: &UiFont, value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(value.into()),
        TextFont {
            font: font.0.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Texte centré, pour l'intérieur d'un bouton.
pub fn label(font: &UiFont, value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (text(font, value, size, color), TextLayout::justify(Justify::Center))
}

/// Petit titre de section en capitales espacées.
pub fn caption(font: &UiFont, value: &str) -> impl Bundle {
    (
        text(font, value.to_uppercase(), 13.0, MUTED),
        Node {
            margin: UiRect::bottom(px(6.0)),
            ..default()
        },
    )
}

/// Couleurs d'un bouton (repos / survol).
#[derive(Component, Clone, Copy)]
pub struct ButtonColors {
    pub normal: Color,
    pub hover: Color,
}

fn lighten(color: Color, amount: f32) -> Color {
    let c = color.to_srgba();
    Color::srgba(
        (c.red + amount).min(1.0),
        (c.green + amount).min(1.0),
        (c.blue + amount * 1.1).min(1.0),
        c.alpha,
    )
}

/// Bouton de taille fixe, avec survol.
pub fn button(width: f32, height: f32, color: Color) -> impl Bundle {
    button_with(
        Node {
            width: px(width),
            height: px(height),
            ..default()
        },
        color,
    )
}

/// Bouton qui prend toute la largeur disponible.
pub fn wide_button(height: f32, color: Color) -> impl Bundle {
    button_with(
        Node {
            width: Val::Percent(100.0),
            height: px(height),
            ..default()
        },
        color,
    )
}

/// Bouton à partir d'un `Node` (taille, marges…) : centrage, bordure et
/// arrondi sont ajoutés.
pub fn button_with(node: Node, color: Color) -> impl Bundle {
    (
        Button,
        Hovered::default(),
        // Centré, bordé et arrondi, sauf si l'appelant a choisi autre chose.
        Node {
            justify_content: if node.justify_content == JustifyContent::Default {
                JustifyContent::Center
            } else {
                node.justify_content
            },
            align_items: if node.align_items == AlignItems::Default {
                AlignItems::Center
            } else {
                node.align_items
            },
            border: if node.border == UiRect::DEFAULT {
                UiRect::all(px(1.0))
            } else {
                node.border
            },
            border_radius: if node.border_radius == BorderRadius::DEFAULT {
                BorderRadius::all(px(8.0))
            } else {
                node.border_radius
            },
            ..node
        },
        BorderColor::all(LINE),
        BackgroundColor(color),
        ButtonColors {
            normal: color,
            hover: lighten(color, 0.08),
        },
    )
}

/// Carte : panneau arrondi avec bordure discrète.
pub fn card() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(18.0)),
            row_gap: px(10.0),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(12.0)),
            ..default()
        },
        BackgroundColor(CARD),
        BorderColor::all(LINE),
    )
}

/// Éclaircit les boutons survolés.
pub fn hover_buttons(
    mut buttons: Query<(&Hovered, &ButtonColors, &mut BackgroundColor), Changed<Hovered>>,
) {
    for (hovered, colors, mut background) in &mut buttons {
        background.0 = if hovered.0 { colors.hover } else { colors.normal };
    }
}

// ---------------------------------------------------------------------------
// Champs de saisie
// ---------------------------------------------------------------------------

/// Champ de saisie : on clique dessus, on tape.
#[derive(Component)]
pub struct TextField {
    pub value: String,
    pub placeholder: String,
    pub max: usize,
    pub focused: bool,
}

/// Texte affiché dans un champ.
#[derive(Component)]
pub struct FieldText;

/// Champ de saisie complet (placer le marqueur de l'appelant à côté).
pub fn field(font: &UiFont, width: Val, value: &str, placeholder: &str, max: usize) -> (impl Bundle, impl Bundle) {
    (
        (
            Button,
            Hovered::default(),
            TextField {
                value: value.to_string(),
                placeholder: placeholder.to_string(),
                max,
                focused: false,
            },
            Node {
                width,
                height: px(44.0),
                padding: UiRect::horizontal(px(14.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(8.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.12)),
        ),
        (FieldText, text(font, "", 18.0, TEXT)),
    )
}

/// Focus au clic, saisie au clavier, rendu du texte et du curseur.
pub fn text_fields(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<KeyboardInput>,
    clicked: Query<Entity, (With<TextField>, Added<Pressed>)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut fields: Query<(Entity, &mut TextField, &mut BorderColor, &Children)>,
    mut texts: Query<(&mut Text, &mut TextColor), With<FieldText>>,
) {
    // Un clic sur un champ le sélectionne ; un clic ailleurs désélectionne.
    let target = clicked.iter().next();
    if target.is_some() || mouse.just_pressed(MouseButton::Left) {
        for (entity, mut field, _, _) in &mut fields {
            let focus = Some(entity) == target;
            if field.focused != focus {
                field.focused = focus;
            }
        }
    }

    let events: Vec<KeyboardInput> = keyboard.read().cloned().collect();
    let blink = (time.elapsed_secs() * 2.0) as u32 % 2 == 0;
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    for (_, mut field, mut border, children) in &mut fields {
        if field.focused {
            // Copier / coller / couper.
            if ctrl && keys.just_pressed(KeyCode::KeyV) {
                if let Some(text) = crate::clipboard::get() {
                    for c in text.chars() {
                        if !c.is_control() && field.value.chars().count() < field.max {
                            field.value.push(c);
                        }
                    }
                }
            }
            if ctrl && keys.just_pressed(KeyCode::KeyC) {
                crate::clipboard::set(&field.value);
            }
            if ctrl && keys.just_pressed(KeyCode::KeyX) {
                crate::clipboard::set(&field.value);
                field.value.clear();
            }
            if ctrl && keys.just_pressed(KeyCode::KeyA) {
                field.value.clear();
            }
            for ev in &events {
                if ev.state != ButtonState::Pressed {
                    continue;
                }
                match &ev.logical_key {
                    Key::Backspace => {
                        field.value.pop();
                    }
                    Key::Enter | Key::Escape | Key::Tab => field.focused = false,
                    _ => {
                        if ctrl {
                            continue;
                        }
                        if let Some(text) = ev.text.as_ref() {
                            for c in text.chars() {
                                if !c.is_control() && field.value.chars().count() < field.max {
                                    field.value.push(c);
                                }
                            }
                        }
                    }
                }
            }
        }
        let wanted = if field.focused { ACCENT } else { Color::srgba(1.0, 1.0, 1.0, 0.12) };
        if border.top != wanted {
            *border = BorderColor::all(wanted);
        }
        for child in children.iter() {
            let Ok((mut text, mut color)) = texts.get_mut(child) else { continue };
            let (value, c) = if field.value.is_empty() && !field.focused {
                (field.placeholder.clone(), MUTED)
            } else {
                let caret = if field.focused && blink { "|" } else { "" };
                (format!("{}{caret}", field.value), TEXT)
            };
            if text.0 != value {
                text.0 = value;
            }
            if color.0 != c {
                color.0 = c;
            }
        }
    }
}

/// Un champ a-t-il le clavier ? (les raccourcis du menu s'effacent alors.)
pub fn typing(fields: &Query<&TextField>) -> bool {
    fields.iter().any(|f| f.focused)
}

// ---------------------------------------------------------------------------
// Interrupteurs
// ---------------------------------------------------------------------------

/// Interrupteur oui/non.
#[derive(Component)]
pub struct Switch {
    pub on: bool,
}

#[derive(Component)]
pub struct SwitchKnob;

pub fn switch(on: bool) -> (impl Bundle, impl Bundle) {
    (
        (
            Button,
            Hovered::default(),
            Switch { on },
            Node {
                width: px(48.0),
                height: px(26.0),
                padding: UiRect::all(px(3.0)),
                border_radius: BorderRadius::all(px(13.0)),
                ..default()
            },
            BackgroundColor(if on { GREEN } else { Color::srgba(1.0, 1.0, 1.0, 0.15) }),
        ),
        (
            SwitchKnob,
            Node {
                width: px(20.0),
                height: px(20.0),
                border_radius: BorderRadius::all(px(10.0)),
                ..default()
            },
            BackgroundColor(Color::WHITE),
        ),
    )
}

/// Bascule au clic et met à jour l'apparence.
pub fn switches(
    mut switches: Query<(&mut Switch, &mut Node, &mut BackgroundColor), Without<SwitchKnob>>,
    clicked: Query<Entity, (With<Switch>, Added<Pressed>)>,
) {
    for entity in &clicked {
        if let Ok((mut s, _, _)) = switches.get_mut(entity) {
            s.on = !s.on;
        }
    }
    for (s, mut node, mut bg) in &mut switches {
        let justify = if s.on { JustifyContent::FlexEnd } else { JustifyContent::FlexStart };
        if node.justify_content != justify {
            node.justify_content = justify;
        }
        let color = if s.on { GREEN } else { Color::srgba(1.0, 1.0, 1.0, 0.15) };
        if bg.0 != color {
            bg.0 = color;
        }
    }
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/// Messages courts affichés en haut de l'écran quelques secondes.
#[derive(Resource, Default)]
pub struct Toasts {
    queue: Vec<String>,
}

impl Toasts {
    pub fn push(&mut self, message: impl Into<String>) {
        self.queue.push(message.into());
    }
}

#[derive(Component)]
pub struct ToastArea;

#[derive(Component)]
pub struct Toast {
    life: f32,
}

pub fn setup_toasts(mut commands: Commands) {
    commands.spawn((
        ToastArea,
        Node {
            position_type: PositionType::Absolute,
            top: px(70.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },
        GlobalZIndex(200),
        Pickable::IGNORE,
    ));
}

pub fn update_toasts(
    time: Res<Time>,
    font: Res<UiFont>,
    mut toasts: ResMut<Toasts>,
    mut commands: Commands,
    area: Query<Entity, With<ToastArea>>,
    mut live: Query<(Entity, &mut Toast, &mut BackgroundColor, &Children)>,
    mut colors: Query<&mut TextColor>,
) {
    let Ok(area) = area.single() else { return };
    for message in toasts.queue.drain(..) {
        commands.entity(area).with_children(|a| {
            a.spawn((
                Toast { life: 3.5 },
                Node {
                    padding: UiRect::axes(px(16.0), px(8.0)),
                    border: UiRect::left(px(3.0)),
                    border_radius: BorderRadius::all(px(6.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.9)),
                BorderColor::all(ACCENT),
                Pickable::IGNORE,
            ))
            .with_child((text(&font, message, 16.0, TEXT), Pickable::IGNORE));
        });
    }
    for (entity, mut toast, mut bg, children) in &mut live {
        toast.life -= time.delta_secs();
        let alpha = toast.life.clamp(0.0, 0.5) * 2.0;
        bg.0 = bg.0.with_alpha(0.9 * alpha);
        for child in children.iter() {
            if let Ok(mut c) = colors.get_mut(child) {
                c.0 = c.0.with_alpha(alpha);
            }
        }
        if toast.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
