//! Interface : viseur, compteur de voxels, FPS, rappel des contrôles.

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;

use crate::game::{GameState, Weapon, GLOCK_MAG};
use crate::world::VoxelWorld;

/// Marque les éléments d'interface (masqués dans le menu).
#[derive(Component)]
pub struct Hud;

#[derive(Component)]
pub struct StatsText {
    shown: u32,
}

#[derive(Component)]
pub struct FpsText {
    shown: u32,
}

/// Compteur d'images maison : moyenne glissante sur une fenêtre fixe, bien
/// plus lisible que l'EMA de Bevy qui saute d'une frame à l'autre.
#[derive(Resource, Default)]
pub struct FpsMeter {
    elapsed: f32,
    frames: u32,
}

/// Arme en main + munitions.
#[derive(Component)]
pub struct WeaponText;

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Police avec accents (la police intégrée à Bevy n'en a pas).
    let font = asset_server.load::<Font>("fonts/ui.ttf");
    // Viseur : un petit carré centré.
    commands.spawn((
        Hud,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: px(6),
            height: px(6),
            ..default()
        },
        UiTransform::from_translation(Val2::px(-3.0, -3.0)),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
    ));

    // Compteur de voxels.
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        StatsText { shown: u32::MAX },
    ));

    // FPS (en haut à droite).
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            right: px(12),
            top: px(12),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        FpsText { shown: u32::MAX },
    ));

    // Arme en main (en bas à droite).
    commands.spawn((
        Hud,
        WeaponText,
        Text::new("Masse"),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(20.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            right: px(16),
            bottom: px(16),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));

    // Contrôles.
    commands.spawn((
        Hud,
        Text::new(concat!(
            "ZQSD / WASD déplacer · Souris regarder · Clic gauche agir\n",
            "1 masse · 2 Glock (R recharger) · 3 C4 · F5 reconstruire · Échap menu",
        )),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(12),
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));
}

/// Met à jour le compteur et les FPS (uniquement quand la valeur change,
/// pour ne pas relayout le texte à chaque frame).
pub fn update_stats(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    game: Res<GameState>,
    diagnostics: Res<DiagnosticsStore>,
    mut meter: ResMut<FpsMeter>,
    mut stats: Query<(&mut Text, &mut StatsText), (With<StatsText>, Without<FpsText>, Without<WeaponText>)>,
    mut fps: Query<(&mut Text, &mut FpsText), (With<FpsText>, Without<StatsText>, Without<WeaponText>)>,
    mut weapon_text: Query<&mut Text, With<WeaponText>>,
) {
    if let Ok((mut text, mut stats)) = stats.single_mut() {
        let alive = world.alive;
        if stats.shown != alive {
            stats.shown = alive;
            text.0 = format!(
                "Voxels : {} / {}\nDétruits : {}",
                alive,
                world.total,
                world.total - alive
            );
        }
    }

    // Arme en main + munitions (en bas à droite).
    let weapon = match game.weapon {
        Weapon::Sledge => "Masse".to_string(),
        Weapon::Glock => format!("Glock 17 — {} / {}", game.ammo, GLOCK_MAG),
        Weapon::C4 => format!("C4 — {} en réserve", game.c4),
    };
    if let Ok(mut text) = weapon_text.single_mut() {
        if text.0 != weapon {
            text.0 = weapon.clone();
        }
    }

    if let Ok((mut text, mut fps)) = fps.single_mut() {
        // Moyenne sur 0,5 s : le nombre affiché ne bouge que deux fois par
        // seconde, donc il reste lisible.
        meter.elapsed += time.delta_secs();
        meter.frames += 1;
        if meter.elapsed >= 0.5 {
            let average = meter.frames as f32 / meter.elapsed;
            let value = average.round() as u32;
            if fps.shown != value {
                fps.shown = value;
                let frame_ms = 1000.0 / average.max(1.0);
                text.0 = format!("{value} FPS · {:.1} ms", frame_ms);
            }
            meter.elapsed = 0.0;
            meter.frames = 0;
        }
    }
    let _ = diagnostics;
}
