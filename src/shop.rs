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
}

#[derive(Component)]
pub struct ShopRoot;
#[derive(Component)]
pub struct ShopBuy(WeaponId);
#[derive(Component)]
pub struct ShopAmmo(WeaponId);
#[derive(Component)]
pub struct ShopClose;
#[derive(Component)]
pub struct ShopCard(WeaponId);

/// Textes de la boutique mis à jour chaque image.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum ShopLabel {
    Money,
    Buy(WeaponId),
    Ammo(WeaponId),
    State(WeaponId),
}

fn category(kind: Kind) -> (&'static str, Color) {
    match kind {
        Kind::Melee => ("MARTEAU", ui::ACCENT),
        Kind::Gun => ("ARME À FEU", ui::BLUE),
        Kind::Thrown => ("BOMBE LANCÉE", ui::HOT),
        Kind::Placed => ("BOMBE POSÉE", ui::RED),
        Kind::Build => ("OUTIL", ui::GREEN),
    }
}

/// Petite ligne de caractéristiques.
fn stats(id: WeaponId) -> String {
    let s = id.spec();
    match s.kind {
        Kind::Melee => format!("Rayon {:.1} m · {:.2} s par coup", s.radius, s.cooldown),
        Kind::Gun if id == WeaponId::Rocket => format!("Explosion {:.1} m · portée {:.0} m", s.radius, s.reach),
        Kind::Gun => {
            let mut t = format!("Chargeur {} · portée {:.0} m", s.mag, s.reach);
            if s.pellets > 1 {
                t.push_str(&format!(" · {} plombs", s.pellets));
            }
            if s.pierce > 0 {
                t.push_str(" · traverse");
            }
            if s.auto {
                t.push_str(" · auto");
            }
            t
        }
        Kind::Thrown | Kind::Placed => format!("Explosion {:.1} m", s.radius),
        Kind::Build => String::new(),
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
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.015, 0.03, 0.72)),
            GlobalZIndex(50),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(1120.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14.0),
                    padding: UiRect::all(px(24.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(16.0)),
                    ..default()
                },
                BackgroundColor(ui::PANEL),
                BorderColor::all(ui::LINE),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0.0), px(10.0), px(0.0), px(30.0)),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: px(16.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn(ui::text(&font, "BOUTIQUE", 34.0, ui::ACCENT));
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                            ui::text(&font, "Chaque voxel cassé rapporte de l'argent (le sol ne vaut rien).", 14.0, ui::MUTED),
                        ));
                        row.spawn((
                            Node {
                                padding: UiRect::axes(px(16.0), px(6.0)),
                                border_radius: BorderRadius::all(px(8.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.1, 0.3, 0.16, 0.6)),
                        ))
                        .with_child((ShopLabel::Money, ui::text(&font, "", 26.0, ui::GREEN)));
                        row.spawn((ShopClose, ui::button(130.0, 40.0, ui::BUTTON)))
                            .with_child(ui::label(&font, "Fermer (B)", 16.0, ui::TEXT));
                    });

                panel
                    .spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(12.0),
                        row_gap: px(12.0),
                        ..default()
                    })
                    .with_children(|grid| {
                        for &id in &ALL_WEAPONS {
                            let spec = id.spec();
                            if spec.kind == Kind::Build {
                                continue;
                            }
                            let (cat, color) = category(spec.kind);
                            grid.spawn((
                                ShopCard(id),
                                Node {
                                    width: px(200.0),
                                    height: px(150.0),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(12.0)),
                                    row_gap: px(4.0),
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(10.0)),
                                    ..default()
                                },
                                BackgroundColor(ui::CARD),
                                BorderColor::all(ui::LINE),
                            ))
                            .with_children(|c| {
                                c.spawn(Node {
                                    justify_content: JustifyContent::SpaceBetween,
                                    align_items: AlignItems::Center,
                                    ..default()
                                })
                                .with_children(|r| {
                                    r.spawn(ui::text(&font, spec.name, 18.0, ui::TEXT));
                                    r.spawn(ui::text(&font, cat, 10.0, color));
                                });
                                c.spawn(ui::text(&font, stats(id), 12.0, ui::MUTED));
                                c.spawn((ShopLabel::State(id), ui::text(&font, "", 13.0, ui::GREEN)));
                                c.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                c.spawn(Node {
                                    column_gap: px(6.0),
                                    ..default()
                                })
                                .with_children(|r| {
                                    let wide = spec.kind != Kind::Gun;
                                    r.spawn((
                                        ShopBuy(id),
                                        ui::button(if wide { 232.0 } else { 118.0 }, 34.0, Color::srgba(0.13, 0.36, 0.20, 1.0)),
                                    ))
                                    .with_child((ShopLabel::Buy(id), ui::label(&font, "", 14.0, ui::TEXT)));
                                    if !wide {
                                        r.spawn((ShopAmmo(id), ui::button(108.0, 34.0, Color::srgba(0.30, 0.25, 0.08, 1.0))))
                                            .with_child((ShopLabel::Ammo(id), ui::label(&font, "", 13.0, ui::TEXT)));
                                    }
                                });
                            });
                        }
                    });
            });
        });
}

/// Ouvre / ferme la boutique (`B`, `Échap` pour fermer).
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
    if keys.just_pressed(KeyCode::KeyB) {
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
    mut commands: Commands,
    bank: Res<SfxBank>,
    mut game: ResMut<GameState>,
    shop: Res<ShopState>,
    mut toasts: ResMut<ui::Toasts>,
    mut net: ResMut<crate::net::client::NetClient>,
    buy: Query<&ShopBuy, Added<Pressed>>,
    ammo: Query<&ShopAmmo, Added<Pressed>>,
) {
    if !shop.open {
        return;
    }
    let mut result: Option<Result<String, &'static str>> = None;
    for ShopBuy(id) in &buy {
        if net.is_online() {
            net.send_raw(crate::net::protocol::Packet::Buy { weapon: id.tag(), ammo: false });
        }
        let r = game.buy(*id).map(|_| format!("{} acheté !", id.spec().name));
        if r.is_ok() && !id.is_bomb() {
            game.weapon = *id;
        }
        result = Some(r);
    }
    for ShopAmmo(id) in &ammo {
        if net.is_online() {
            net.send_raw(crate::net::protocol::Packet::Buy { weapon: id.tag(), ammo: true });
        }
        result = Some(
            game.buy_ammo(*id)
                .map(|_| format!("+{} munitions pour {}", id.spec().ammo_pack, id.spec().name)),
        );
    }
    match result {
        Some(Ok(msg)) => {
            fx::play(&mut commands, &bank, SfxKind::Buy, 0.6);
            toasts.push(msg);
        }
        Some(Err(e)) => {
            fx::play(&mut commands, &bank, SfxKind::Empty, 0.6);
            toasts.push(format!("Impossible : {e}"));
        }
        None => {}
    }
}

/// Met à jour les libellés (prix, possédé, stock) et l'affichage.
pub fn refresh(
    game: Res<GameState>,
    shop: Res<ShopState>,
    mut root: Query<&mut Node, With<ShopRoot>>,
    mut labels: Query<(&ShopLabel, &mut Text, &mut TextColor)>,
    mut cards: Query<(&ShopCard, &mut BorderColor)>,
) {
    if let Ok(mut node) = root.single_mut() {
        let d = if shop.open { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    }
    if !shop.open {
        return;
    }
    for (card, mut border) in &mut cards {
        let owned = game.slot(card.0).owned && !card.0.is_bomb();
        let c = if card.0 == game.weapon && owned {
            ui::ACCENT
        } else if owned {
            Color::srgba(0.24, 0.86, 0.52, 0.35)
        } else {
            ui::LINE
        };
        if border.top != c {
            *border = BorderColor::all(c);
        }
    }
    for (label, mut text, mut color) in &mut labels {
        let (value, c) = match *label {
            ShopLabel::Money => (format!("{} $", crate::hud::thousands(game.money)), ui::GREEN),
            ShopLabel::Buy(id) => {
                let spec = id.spec();
                let affordable = game.money >= spec.price;
                let value = if id.is_bomb() {
                    format!("+1   {} $", spec.price)
                } else if game.slot(id).owned {
                    "Possédé".to_string()
                } else {
                    format!("{} $", crate::hud::thousands(spec.price))
                };
                (value, if affordable || game.slot(id).owned { ui::TEXT } else { ui::MUTED })
            }
            ShopLabel::Ammo(id) => {
                let spec = id.spec();
                (format!("+{}  {} $", spec.ammo_pack, spec.ammo_price), ui::TEXT)
            }
            ShopLabel::State(id) => {
                let slot = game.slot(id);
                let value = match id.kind() {
                    Kind::Gun if slot.owned => format!("Munitions : {} + {}", slot.mag, slot.reserve),
                    Kind::Thrown | Kind::Placed if slot.reserve > 0 => format!("En stock : {}", slot.reserve),
                    _ if slot.owned => "Dans ton arsenal".to_string(),
                    _ => String::new(),
                };
                (value, ui::GREEN)
            }
        };
        if text.0 != value {
            text.0 = value;
        }
        if color.0 != c {
            color.0 = c;
        }
    }
}
