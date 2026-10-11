//! Boutique (mode normal) : touche `B`. On y achète marteaux, armes à feu,
//! munitions et bombes avec l'argent gagné en détruisant la ville.

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::window::CursorOptions;

use crate::fx::{self, SfxBank, SfxKind};
use crate::game::{GameState, Kind, Mode, WeaponId, ALL_WEAPONS};
use crate::ui::{self, UiFont};

#[derive(Resource, Default)]
pub struct ShopState {
    pub open: bool,
    message: String,
    message_time: f32,
}

#[derive(Component)]
pub struct ShopRoot;
#[derive(Component)]
pub struct ShopBuy(WeaponId);
#[derive(Component)]
pub struct ShopAmmo(WeaponId);
#[derive(Component)]
pub struct BuyLabel(WeaponId);
#[derive(Component)]
pub struct AmmoLabel(WeaponId);
#[derive(Component)]
pub struct InfoLabel(WeaponId);
#[derive(Component)]
pub struct ShopMoney;
#[derive(Component)]
pub struct ShopMessage;
#[derive(Component)]
pub struct ShopClose;

fn category(kind: Kind) -> &'static str {
    match kind {
        Kind::Melee => "Marteau",
        Kind::Gun => "Arme à feu",
        Kind::Thrown => "Bombe lancée",
        Kind::Placed => "Bombe posée",
    }
}

pub fn setup(mut commands: Commands, font: Res<UiFont>) {
    let font = font.into_inner().clone();
    commands
        .spawn((
            ShopRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(50),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(860.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4.0),
                    padding: UiRect::all(px(18.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.07, 0.10, 0.97)),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        margin: UiRect::bottom(px(8.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(ui::text(&font, "BOUTIQUE", 30.0, ui::ACCENT));
                        row.spawn((ShopMoney, ui::text(&font, "", 26.0, Color::srgb(0.45, 0.95, 0.45))));
                        row.spawn((ShopClose, ui::button(130.0, 36.0, ui::BUTTON)))
                            .with_child(ui::label(&font, "Fermer (B)", 16.0, ui::TEXT));
                    });

                for &id in &ALL_WEAPONS {
                    let spec = id.spec();
                    panel
                        .spawn(Node {
                            height: px(42.0),
                            align_items: AlignItems::Center,
                            column_gap: px(10.0),
                            padding: UiRect::horizontal(px(8.0)),
                            ..default()
                        })
                        .insert(BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.03)))
                        .with_children(|row| {
                            row.spawn((
                                Node {
                                    width: px(190.0),
                                    ..default()
                                },
                                ui::text(&font, spec.name, 19.0, ui::TEXT),
                            ));
                            row.spawn((
                                InfoLabel(id),
                                Node {
                                    width: px(250.0),
                                    ..default()
                                },
                                ui::text(&font, category(spec.kind), 14.0, ui::MUTED),
                            ));
                            row.spawn((ShopBuy(id), ui::button(180.0, 34.0, Color::srgba(0.12, 0.30, 0.16, 0.96))))
                                .with_child((BuyLabel(id), ui::label(&font, "", 16.0, ui::TEXT)));
                            if spec.kind == Kind::Gun {
                                row.spawn((ShopAmmo(id), ui::button(170.0, 34.0, Color::srgba(0.24, 0.22, 0.10, 0.96))))
                                    .with_child((AmmoLabel(id), ui::label(&font, "", 15.0, ui::TEXT)));
                            } else {
                                // Garde les boutons « Acheter » alignés.
                                row.spawn(Node {
                                    width: px(170.0),
                                    ..default()
                                });
                            }
                        });
                }

                panel.spawn((
                    ShopMessage,
                    Node {
                        margin: UiRect::top(px(8.0)),
                        ..default()
                    },
                    ui::text(&font, "", 17.0, Color::srgb(1.0, 0.6, 0.5)),
                ));
            });
        });
}

/// Ouvre / ferme la boutique (`B`, `Tab`, `Échap` pour fermer).
pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    game: Res<GameState>,
    mut shop: ResMut<ShopState>,
    mut cursor: Single<&mut CursorOptions>,
    close: Query<(), (Added<Pressed>, With<ShopClose>)>,
) {
    if !game.in_game() || game.mode != Mode::Normal {
        if shop.open {
                shop.open = false;
        }
        return;
    }
    let mut wanted = shop.open;
    if keys.just_pressed(KeyCode::KeyB) || keys.just_pressed(KeyCode::Tab) {
        wanted = !wanted;
    }
    if shop.open && (keys.just_pressed(KeyCode::Escape) || !close.is_empty()) {
        wanted = false;
    }
    if wanted != shop.open {
        shop.open = wanted;
        crate::player::grab(&mut cursor, !wanted);
    }
}

/// Achats.
pub fn interact(
    time: Res<Time>,
    mut commands: Commands,
    bank: Res<SfxBank>,
    mut game: ResMut<GameState>,
    mut shop: ResMut<ShopState>,
    buy: Query<&ShopBuy, Added<Pressed>>,
    ammo: Query<&ShopAmmo, Added<Pressed>>,
) {
    shop.message_time = (shop.message_time - time.delta_secs()).max(0.0);
    if !shop.open {
        return;
    }
    let mut result: Option<Result<String, &'static str>> = None;
    for ShopBuy(id) in &buy {
        result = Some(game.buy(*id).map(|_| format!("{} acheté !", id.spec().name)));
        if result.as_ref().is_some_and(|r| r.is_ok()) && !id.is_bomb() {
            game.weapon = *id;
        }
    }
    for ShopAmmo(id) in &ammo {
        result = Some(
            game.buy_ammo(*id)
                .map(|_| format!("+{} munitions pour {}", id.spec().ammo_pack, id.spec().name)),
        );
    }
    match result {
        Some(Ok(msg)) => {
            fx::play(&mut commands, &bank, SfxKind::Buy, 0.6);
            shop.message = msg;
            shop.message_time = 2.5;
        }
        Some(Err(e)) => {
            fx::play(&mut commands, &bank, SfxKind::Empty, 0.6);
            shop.message = e.to_string();
            shop.message_time = 2.5;
        }
        None => {}
    }
}

/// Met à jour les libellés (prix, possédé, stock).
#[allow(clippy::type_complexity)]
pub fn refresh(
    game: Res<GameState>,
    shop: Res<ShopState>,
    mut root: Query<&mut Visibility, With<ShopRoot>>,
    mut texts: ParamSet<(
        Query<(&BuyLabel, &mut Text)>,
        Query<(&AmmoLabel, &mut Text)>,
        Query<(&InfoLabel, &mut Text)>,
        Query<&mut Text, With<ShopMoney>>,
        Query<&mut Text, With<ShopMessage>>,
    )>,
) {
    if let Ok(mut v) = root.single_mut() {
        let wanted = if shop.open { Visibility::Inherited } else { Visibility::Hidden };
        if *v != wanted {
            *v = wanted;
        }
    }
    if !shop.open {
        return;
    }

    let set = |text: &mut Text, value: String| {
        if text.0 != value {
            text.0 = value;
        }
    };

    for (BuyLabel(id), mut text) in &mut texts.p0() {
        let spec = id.spec();
        let slot = game.slot(*id);
        let value = if id.is_bomb() {
            format!("+1  —  {} $", spec.price)
        } else if slot.owned {
            "Possédé".to_string()
        } else {
            format!("Acheter  —  {} $", spec.price)
        };
        set(&mut text, value);
    }
    for (AmmoLabel(id), mut text) in &mut texts.p1() {
        let spec = id.spec();
        set(&mut text, format!("+{} balles — {} $", spec.ammo_pack, spec.ammo_price));
    }
    for (InfoLabel(id), mut text) in &mut texts.p2() {
        let spec = id.spec();
        let slot = game.slot(*id);
        let value = match spec.kind {
            Kind::Gun if slot.owned => format!("{} · {} / {}", category(spec.kind), slot.mag, slot.reserve),
            Kind::Thrown | Kind::Placed => format!("{} · stock {}", category(spec.kind), slot.reserve),
            _ => format!("{} · rayon {:.1} m", category(spec.kind), spec.radius),
        };
        set(&mut text, value);
    }
    if let Ok(mut text) = texts.p3().single_mut() {
        set(&mut text, format!("{} $", game.money));
    }
    if let Ok(mut text) = texts.p4().single_mut() {
        let value = if shop.message_time > 0.0 {
            shop.message.clone()
        } else {
            "Chaque voxel détruit rapporte de l'argent (le sol ne vaut rien).".to_string()
        };
        set(&mut text, value);
    }
}
