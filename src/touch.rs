//! Commandes tactiles (Android / tout écran tactile).
//!
//! Le jeu se joue entièrement au doigt :
//!
//! - **joystick** à gauche (droite si le réglage est inversé) : il apparaît
//!   là où le doigt se pose et suit le déplacement ;
//! - **glissement** dans la zone libre du reste de l'écran : regard ;
//! - **boutons** tir, saut, rechargement, arme précédente/suivante,
//!   interaction.
//!
//! Tout est fait avec des nœuds d'interface « pickables » et non avec les
//! contacts bruts : le picking ne renvoie qu'un seul destinataire par doigt,
//! donc un doigt posé sur le bouton *tir* ne fait pas pivoter la caméra en
//! même temps, et deux doigts fonctionnent (un pour marcher, un pour tirer).

use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerPress, PointerRelease};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::Pressed;

use crate::game::{GameState, Phase, Settings};
use crate::shop::ShopState;
use crate::ui::{self, UiFont};

/// Rayon du joystick virtuel, en pixels d'interface.
pub const STICK_RADIUS: f32 = 120.0;

/// Vrai sur les plateformes sans clavier ni souris.
pub fn is_touch_platform() -> bool {
    cfg!(any(target_os = "android", target_os = "ios"))
}

/// Condition de système : les commandes tactiles n'existent que sur
/// téléphone. Ailleurs elles ne tournent pas du tout.
pub fn on_touch() -> bool {
    is_touch_platform()
}

/// Ce que fait un bouton tactile.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Fire,
    Jump,
    Reload,
    Interact,
    /// Action secondaire (clic droit) : faire sauter la C4, changer de matériau.
    Alt,
    /// Arme précédente / suivante dans la barre.
    PrevWeapon,
    NextWeapon,
    /// Catégorie d'armes (0 = mêlée, 1 = pistolets…).
    Category(usize),
}

/// Ce que les doigts demandent au jeu, cette image.
#[derive(Resource, Default, Clone, Debug)]
pub struct TouchState {
    /// Déplacement demandé par le joystick : x = droite, y = avant.
    pub stick: Vec2,
    /// Rotation demandée par le glissement (radians).
    pub look: Vec2,
    /// Bouton *tir* maintenu.
    pub fire: bool,
    /// Boutons « une fois » : remis à zéro en fin d'image.
    pub jump: bool,
    pub reload: bool,
    pub interact: bool,
    pub alt: bool,
    pub cycle: i8,
    pub category: Option<usize>,
}

/// Racine des commandes tactiles (masquée hors jeu).
#[derive(Component)]
pub struct TouchRoot;

/// Zone du joystick.
#[derive(Component)]
pub struct MoveZone;

/// Zone de regard.
#[derive(Component)]
pub struct LookZone;

/// Socle et bouton du joystick.
#[derive(Component)]
pub struct StickBase;
/// Boutons d'action (tir, saut…) : du côté opposé au joystick.
#[derive(Component)]
pub struct ButtonPad;
/// Pastilles de catégorie d'armes : du même côté que le joystick, en haut.
#[derive(Component)]
pub struct CategoryColumn;
#[derive(Component)]
pub struct StickKnob;

/// Bouton d'action.
#[derive(Component)]
pub struct TouchButton(pub Action);

// ---------------------------------------------------------------------------
// Mise en place
// ---------------------------------------------------------------------------

/// Zone transparente qui capte les doigts. `Pickable` est indispensable pour
/// que le picking lui envoie les événements de pointeur.
fn zone(size: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(size),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        Pickable::default(),
    )
}

fn round_button(size: f32, action: Action) -> impl Bundle {
    (
        TouchButton(action),
        Node {
            width: px(size),
            height: px(size),
            border_radius: BorderRadius::all(px(size * 0.5)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.42)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.22)),
    )
}

pub fn setup(mut commands: Commands, font: Res<UiFont>) {
    // Sur ordinateur, rien n'est créé : ces nœuds plein écran « pickables »
    // s'afficheraient et avaleraient la souris.
    if !is_touch_platform() {
        return;
    }
    commands
        .spawn((
            TouchRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // Au-dessus du HUD, sous le menu (`MenuRoot` vaut 100).
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            // --- Joystick : zone de gauche ou de droite ---
            root.spawn((MoveZone, zone(45.0)));
            // --- Regard : tout le reste de l'écran ---
            root.spawn((LookZone, zone(55.0)));

            // Joystick : posé là où le doigt touche la zone de déplacement
            // (enfant de la racine plein écran, donc en coordonnées écran).
                root.spawn((
                    StickBase,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        top: px(0.0),
                        width: px(STICK_RADIUS * 2.0),
                        height: px(STICK_RADIUS * 2.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(3.0)),
                        border_radius: BorderRadius::all(px(STICK_RADIUS)),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)),
                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.25)),
                ))
                .with_children(|base| {
                    base.spawn((
                        StickKnob,
                        Node {
                            width: px(STICK_RADIUS),
                            height: px(STICK_RADIUS),
                            border_radius: BorderRadius::all(px(STICK_RADIUS * 0.5)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 0.71, 0.15, 0.55)),
                    ));
                });

            // --- Boutons, en bas à droite ---
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(24.0),
                    bottom: px(200.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12.0),
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
                ButtonPad,
                // Le conteneur n'est pas pickable : seuls les boutons le sont.
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                // Ligne du haut : recharger / arme précédente / suivante.
                row.spawn(Node {
                    column_gap: px(10.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|line| {
                    line.spawn(round_button(66.0, Action::PrevWeapon))
                        .with_child(ui::label(&font, "‹", 34.0, ui::TEXT));
                    line.spawn(round_button(66.0, Action::Reload))
                        .with_child(ui::label(&font, "R", 26.0, ui::TEXT));
                    line.spawn(round_button(66.0, Action::NextWeapon))
                        .with_child(ui::label(&font, "›", 34.0, ui::TEXT));
                });
                // Ligne du bas : interaction et tir.
                row.spawn(Node {
                    column_gap: px(14.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|line| {
                    line.spawn(round_button(66.0, Action::Alt))
                        .with_child(ui::label(&font, "ALT", 18.0, ui::TEXT));
                    line.spawn(round_button(66.0, Action::Interact))
                        .with_child(ui::label(&font, "E", 26.0, ui::MUTED));
                    line.spawn((
                        TouchButton(Action::Fire),
                        Node {
                            width: px(120.0),
                            height: px(120.0),
                            border_radius: BorderRadius::all(px(60.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(3.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 0.35, 0.21, 0.32)),
                        BorderColor::all(Color::srgba(1.0, 0.35, 0.21, 0.6)),
                    ))
                    .with_child(ui::label(&font, "TIR", 22.0, ui::TEXT));
                });
            });

            // --- Catégories d'armes, en haut de l'écran (vertical) ---
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(90.0),
                    left: px(12.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8.0),
                    ..default()
                },
                CategoryColumn,
                Pickable::IGNORE,
            ))
            .with_children(|col| {
                for (i, cat) in crate::game::Cat::ALL.iter().enumerate() {
                    col.spawn((
                        TouchButton(Action::Category(i)),
                        Node {
                            width: px(112.0),
                            height: px(46.0),
                            border_radius: BorderRadius::all(px(10.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.5)),
                        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
                    ))
                    .with_child(ui::label(&font, cat.name(), 17.0, ui::TEXT));
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Lecture des doigts
// ---------------------------------------------------------------------------

/// Le doigt qui tient le joystick, et l'endroit où il s'est posé.
#[derive(Default)]
pub struct Stick {
    pointer: Option<String>,
    origin: Vec2,
}

/// Identifiant du pointeur (souris ou doigt) sous forme de texte, pour les
/// comparer sans dépendre du type interne.
fn pointer_key(pointer: &bevy::picking::events::Pointer) -> String {
    format!("{:?}", pointer.id)
}

/// Traduit les événements de pointeur en [`TouchState`] et déplace le
/// joystick.
pub fn pointer(
    mut state: ResMut<TouchState>,
    settings: Res<Settings>,
    ui_scale: Res<UiScale>,
    mut stick: Local<Stick>,
    mut press: MessageReader<PointerPress>,
    mut drags: MessageReader<PointerDrag>,
    mut drag_end: MessageReader<PointerDragEnd>,
    mut release: MessageReader<PointerRelease>,
    zones: Query<(), Or<(With<MoveZone>, With<LookZone>)>>,
    buttons: Query<&TouchButton>,
    // `Without<…>` croisé : sans lui, Bevy refuse deux `&mut Node` sur des
    // filtres qui ne sont pas prouvés disjoints (erreur B0001).
    mut base: Query<&mut Node, (With<StickBase>, Without<StickKnob>)>,
    mut knob: Query<&mut Node, (With<StickKnob>, Without<StickBase>)>,
) {
    let sensitivity = settings.touch_sensitivity;
    let mut move_value = Vec2::ZERO;
    let mut look_delta = Vec2::ZERO;

    for event in press.read() {
        if event.button != PointerButton::Primary {
            continue;
        }
        if zones.get(event.entity).is_err() {
            // Un bouton d'action.
            if let Ok(button) = buttons.get(event.entity) {
                match button.0 {
                    Action::Jump => state.jump = true,
                    Action::Reload => state.reload = true,
                    Action::Interact => state.interact = true,
                    Action::Alt => state.alt = true,
                    Action::PrevWeapon => state.cycle = -1,
                    Action::NextWeapon => state.cycle = 1,
                    Action::Category(i) => state.category = Some(i),
                    Action::Fire => {}
                }
            }
            continue;
        }
        // Premier doigt de la zone de déplacement : il pilote le joystick.
        if stick.pointer.is_none() {
            stick.pointer = Some(pointer_key(&event.pointer));
            stick.origin = event.pointer.position / ui_scale.0.max(0.01);
        }
    }

    for event in drags.read() {
        if event.button != PointerButton::Primary {
            continue;
        }
        let key = pointer_key(&event.pointer);
        if stick.pointer.as_deref() == Some(key.as_str()) {
            // `PointerDrag` porte le déplacement depuis le début du glissé
            // (`distance`) et depuis la dernière image (`delta`) : on se
            // sert du premier, ramené au rayon du joystick.
            let delta = event.distance / STICK_RADIUS;
            move_value = if delta.length() > 1.0 {
                delta.normalize()
            } else {
                delta
            };
        } else if zones.get(event.entity).is_ok() {
            look_delta += event.delta;
        }
    }

    for event in drag_end.read() {
        if stick.pointer.as_deref() == Some(pointer_key(&event.pointer).as_str()) {
            stick.pointer = None;
        }
    }
    for event in release.read() {
        if stick.pointer.as_deref() == Some(pointer_key(&event.pointer).as_str()) {
            stick.pointer = None;
        }
    }

    // --- Joystick ---
    if stick.pointer.is_some() {
        state.stick = move_value;
        if let Ok(mut b) = base.single_mut() {
            b.display = Display::Flex;
            b.left = px(stick.origin.x - STICK_RADIUS);
            b.top = px(stick.origin.y - STICK_RADIUS);
        }
        if let Ok(mut k) = knob.single_mut() {
            k.left = px(move_value.x * STICK_RADIUS);
            k.top = px(move_value.y * STICK_RADIUS);
        }
    } else {
        state.stick = Vec2::ZERO;
        if let Ok(mut b) = base.single_mut() {
            b.display = Display::None;
        }
        if let Ok(mut k) = knob.single_mut() {
            k.left = px(0.0);
            k.top = px(0.0);
        }
    }

    // --- Regard ---
    state.look = look_delta * sensitivity;
}

/// Le bouton *tir* est le seul qui reste enfoncé : on lit son état à chaque
/// image plutôt que sur un appui.
pub fn fire_button(buttons: Query<(&TouchButton, Has<Pressed>)>, mut state: ResMut<TouchState>) {
    for (button, pressed) in &buttons {
        if button.0 == Action::Fire {
            state.fire = pressed;
        }
    }
}

/// Le côté du joystick (gauche ou droite) et la visibilité des commandes.
pub fn layout(
    settings: Res<Settings>,
    game: Res<GameState>,
    shop: Res<ShopState>,
    mut root: Query<&mut Visibility, With<TouchRoot>>,
    mut zones: Query<(&mut Node, Has<MoveZone>), Or<(With<MoveZone>, With<LookZone>)>>,
    mut pad: Query<&mut Node, (With<ButtonPad>, Without<MoveZone>, Without<LookZone>, Without<CategoryColumn>)>,
    mut column: Query<&mut Node, (With<CategoryColumn>, Without<MoveZone>, Without<LookZone>, Without<ButtonPad>)>,
) {
    let on = is_touch_platform()
        && game.phase == Phase::Playing
        && !shop.open
        && game.alive()
        && !game.typing;
    if let Ok(mut v) = root.single_mut() {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
    // 0 = joystick à gauche, 1 = à droite. Les boutons vont en face.
    let left = settings.joystick_side == 0;
    let (near, far) = if left { (px(0.0), Val::Auto) } else { (Val::Auto, px(0.0)) };
    for (mut node, is_move) in &mut zones {
        let (l, r) = if is_move { (near, far) } else { (far, near) };
        node.left = l;
        node.right = r;
    }
    if let Ok(mut pad) = pad.single_mut() {
        let (l, r) = if left { (Val::Auto, px(24.0)) } else { (px(24.0), Val::Auto) };
        pad.left = l;
        pad.right = r;
    }
    if let Ok(mut col) = column.single_mut() {
        let (l, r) = if left { (px(12.0), Val::Auto) } else { (Val::Auto, px(12.0)) };
        col.left = l;
        col.right = r;
    }
}

/// Remet à zéro les actions ponctuelles une fois la frame traitée.
pub fn clear(mut state: ResMut<TouchState>) {
    state.jump = false;
    state.reload = false;
    state.interact = false;
    state.alt = false;
    state.cycle = 0;
    state.category = None;
    state.look = Vec2::ZERO;
}
