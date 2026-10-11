//! Briques d'interface partagées : police embarquée, textes, boutons avec
//! survol.
//!
//! La police est **compilée dans l'exécutable** : le jeu n'a besoin d'aucun
//! dossier `assets` à côté de lui, ce qui permet de le distribuer en un
//! seul fichier (et évite les textes invisibles quand on le lance depuis un
//! autre dossier).

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

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
    (
        text(font, value, size, color),
        TextLayout::justify(Justify::Center),
    )
}

/// Couleurs d'un bouton (repos / survol).
#[derive(Component, Clone, Copy)]
pub struct ButtonColors {
    pub normal: Color,
    pub hover: Color,
}

pub const BUTTON: Color = Color::srgba(0.14, 0.16, 0.21, 0.96);
pub const BUTTON_HOVER: Color = Color::srgba(0.24, 0.28, 0.36, 0.98);
pub const ACCENT: Color = Color::srgb(0.98, 0.78, 0.22);
pub const TEXT: Color = Color::srgb(0.93, 0.94, 0.97);
pub const MUTED: Color = Color::srgb(0.66, 0.70, 0.78);

/// Bouton de taille fixe, avec survol.
pub fn button(width: f32, height: f32, color: Color) -> impl Bundle {
    let c = color.to_srgba();
    let hover = Color::srgba(
        (c.red + 0.10).min(1.0),
        (c.green + 0.10).min(1.0),
        (c.blue + 0.12).min(1.0),
        c.alpha,
    );
    (
        Button,
        Hovered::default(),
        Node {
            width: px(width),
            height: px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        },
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.08)),
        BackgroundColor(color),
        ButtonColors {
            normal: color,
            hover,
        },
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
