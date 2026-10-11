//! Interface en jeu : viseur, argent, seed, barre d'armes, munitions, FPS,
//! rappel des contrôles.

use bevy::prelude::*;

use crate::game::{GameState, Kind, Mode, WeaponId, ALL_WEAPONS};
use crate::ui::{self, UiFont};
use crate::weapon::{Bomb, Hands};
use crate::world::VoxelWorld;

/// Marque les éléments d'interface (masqués dans le menu).
#[derive(Component)]
pub struct Hud;

#[derive(Component)]
pub struct StatsText;

#[derive(Component)]
pub struct FpsText;

#[derive(Component)]
pub struct WeaponText;

#[derive(Component)]
pub struct HelpText;

/// Case de la barre d'armes.
#[derive(Component)]
pub struct HotbarSlot(WeaponId);

#[derive(Component)]
pub struct HotbarLabel(WeaponId);

/// Compteur d'images : moyenne glissante sur une fenêtre fixe.
#[derive(Resource, Default)]
pub struct FpsMeter {
    elapsed: f32,
    frames: u32,
}

fn panel() -> impl Bundle {
    (
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::all(px(10)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        },
    )
}

pub fn setup(mut commands: Commands, font: Res<UiFont>) {
    let font = font.into_inner().clone();

    // Viseur : un point cerclé.
    commands.spawn((
        Hud,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: px(6),
            height: px(6),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(3.0)),
            ..default()
        },
        UiTransform::from_translation(Val2::px(-3.0, -3.0)),
        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
    ));

    // Argent, seed, voxels (en haut à gauche).
    commands
        .spawn((Hud, panel()))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            padding: UiRect::all(px(10)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        })
        .with_child((StatsText, ui::text(&font, "", 17.0, ui::TEXT)));

    // FPS (en haut à droite).
    commands
        .spawn((Hud, panel()))
        .insert(Node {
            position_type: PositionType::Absolute,
            right: px(12),
            top: px(12),
            padding: UiRect::all(px(8)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        })
        .with_child((FpsText, ui::text(&font, "", 15.0, ui::TEXT)));

    // Arme en main + munitions (en bas à droite).
    commands
        .spawn((Hud, panel()))
        .insert(Node {
            position_type: PositionType::Absolute,
            right: px(16),
            bottom: px(84),
            padding: UiRect::all(px(12)),
            border_radius: BorderRadius::all(px(6.0)),
            ..default()
        })
        .with_child((WeaponText, ui::text(&font, "", 22.0, ui::TEXT), TextLayout::justify(Justify::Right)));

    // Contrôles (en haut au centre, discrets ; H pour masquer).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                top: px(10),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((
            HelpText,
            ui::label(&font, "", 13.0, Color::srgba(1.0, 1.0, 1.0, 0.8)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            Node {
                padding: UiRect::axes(px(10), px(5)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
        ));

    // Barre d'armes (en bas au centre).
    commands
        .spawn((
            Hud,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: px(4.0),
                ..default()
            },
        ))
        .with_children(|bar| {
            for &id in &ALL_WEAPONS {
                bar.spawn((
                    HotbarSlot(id),
                    Node {
                        width: px(74.0),
                        height: px(52.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2.0)),
                        border_radius: BorderRadius::all(px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.1)),
                ))
                .with_child((HotbarLabel(id), ui::label(&font, "", 12.0, ui::TEXT)));
            }
        });
}

/// Nom court pour la barre d'armes.
fn short(id: WeaponId) -> &'static str {
    use WeaponId::*;
    match id {
        Hammer => "Marteau",
        Sledge => "Masse",
        WarHammer => "Guerre",
        Jackhammer => "Piqueur",
        Glock => "Glock",
        Shotgun => "Pompe",
        Rifle => "Fusil",
        Sniper => "Sniper",
        Rocket => "Roquette",
        Grenade => "Grenade",
        Dynamite => "Dynamite",
        C4 => "C4",
        MegaBomb => "Méga",
    }
}

fn set(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

/// Met à jour l'interface (le texte n'est réécrit que s'il change, pour ne
/// pas refaire la mise en page à chaque image).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut hide_help: Local<bool>,
    world: Res<VoxelWorld>,
    game: Res<GameState>,
    hands: Res<Hands>,
    net: Res<crate::net::client::NetClient>,
    mut meter: ResMut<FpsMeter>,
    bombs: Query<&Bomb>,
    mut texts: ParamSet<(
        Query<&mut Text, With<StatsText>>,
        Query<&mut Text, With<FpsText>>,
        Query<&mut Text, With<WeaponText>>,
        Query<&mut Text, With<HelpText>>,
        Query<(&HotbarLabel, &mut Text)>,
    )>,
    mut slots: Query<(&HotbarSlot, &mut Node, &mut BorderColor, &mut BackgroundColor)>,
) {
    // Haut gauche.
    let destroyed = world.total.saturating_sub(world.alive);
    let mut stats = match game.mode {
        Mode::Sandbox => "BAC À SABLE".to_string(),
        Mode::Normal => format!("{} $   (gagné : {} $)", game.money, game.earned),
    };
    stats.push_str(&format!("\nSeed : {}\nVoxels détruits : {}", world.seed, destroyed));
    if net.is_online() {
        stats.push_str("\nEn ligne");
    }
    if let Ok(mut text) = texts.p0().single_mut() {
        set(&mut text, stats);
    }

    // FPS : moyenne sur 0,5 s pour rester lisible.
    meter.elapsed += time.delta_secs();
    meter.frames += 1;
    if meter.elapsed >= 0.5 {
        let average = meter.frames as f32 / meter.elapsed;
        if let Ok(mut text) = texts.p1().single_mut() {
            set(&mut text, format!("{:.0} FPS · {:.1} ms", average, 1000.0 / average.max(1.0)));
        }
        meter.elapsed = 0.0;
        meter.frames = 0;
    }

    // Arme en main.
    let id = game.weapon;
    let spec = id.spec();
    let slot = game.slot(id);
    let infinite = game.sandbox();
    let mut weapon = spec.name.to_string();
    match spec.kind {
        Kind::Gun => {
            if hands.reload > 0.0 {
                weapon.push_str("\nRECHARGEMENT…");
            } else if infinite {
                weapon.push_str(&format!("\n{} / {}  ·  illimité", slot.mag, spec.mag));
            } else {
                weapon.push_str(&format!("\n{} / {}  ·  {}", slot.mag, spec.mag, slot.reserve));
            }
        }
        Kind::Thrown | Kind::Placed => {
            if infinite {
                weapon.push_str("\nIllimité");
            } else {
                weapon.push_str(&format!("\nStock : {}", slot.reserve));
            }
            if id == WeaponId::C4 {
                let armed = bombs.iter().filter(|b| b.local && b.id == WeaponId::C4).count();
                weapon.push_str(&format!("\nPosées : {armed} · clic droit : BOUM"));
            }
        }
        Kind::Melee => {}
    }
    if let Ok(mut text) = texts.p2().single_mut() {
        set(&mut text, weapon);
    }

    // Aide (H la masque / l'affiche).
    if keys.just_pressed(KeyCode::KeyH) {
        *hide_help = !*hide_help;
    }
    let mut help = String::from(
        "ZQSD/WASD bouger · Maj courir · Espace sauter · Clic agir\n1-9, 0, molette, Q/E changer d'arme · R recharger",
    );
    match game.mode {
        Mode::Normal => help.push_str(" · B boutique"),
        Mode::Sandbox => help.push_str(" · V voler"),
    }
    if !net.is_online() {
        help.push_str("\nF5 rejouer la seed · F6 nouvelle ville");
    }
    help.push_str(" · Échap menu · H masquer");
    if *hide_help {
        help.clear();
    }
    if let Ok(mut text) = texts.p3().single_mut() {
        set(&mut text, help);
    }

    // Barre d'armes : seules les armes possédées, numérotées dans l'ordre.
    let owned = game.owned();
    for (slot_id, mut node, mut border, mut background) in &mut slots {
        let index = owned.iter().position(|&w| w == slot_id.0);
        let display = if index.is_some() { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let selected = slot_id.0 == game.weapon;
        let b = if selected { ui::ACCENT } else { Color::srgba(1.0, 1.0, 1.0, 0.1) };
        if border.top != b {
            *border = BorderColor::all(b);
        }
        let bg = if selected {
            Color::srgba(0.25, 0.20, 0.05, 0.7)
        } else {
            Color::srgba(0.0, 0.0, 0.0, 0.5)
        };
        if background.0 != bg {
            background.0 = bg;
        }
    }
    for (label, mut text) in &mut texts.p4() {
        let Some(index) = owned.iter().position(|&w| w == label.0) else {
            continue;
        };
        let key = if index < 9 {
            format!("{}", index + 1)
        } else if index == 9 {
            "0".to_string()
        } else {
            "·".to_string()
        };
        let s = game.slot(label.0);
        let count = match label.0.kind() {
            Kind::Gun if !infinite => format!("\n{}", s.mag + s.reserve),
            Kind::Thrown | Kind::Placed if !infinite => format!("\n×{}", s.reserve),
            _ => String::new(),
        };
        set(&mut text, format!("{key}  {}{count}", short(label.0)));
    }
}
