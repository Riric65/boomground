//! Chat textuel façon Minecraft : `T` (ou Entrée) ouvre la saisie, Entrée
//! envoie, Échap annule. Les messages s'affichent en bas à gauche et
//! s'estompent. La modération (débit, anti-répétition, mots interdits, mute)
//! est faite par le serveur.

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::game::{GameState, Phase};
use crate::net::client::{Lobby, NetClient};
use crate::net::protocol::Packet;
use crate::ui::{self, TextField, UiFont};

const LINES: usize = 10;
/// Durée d'affichage d'un message quand le chat est fermé.
const SHOWN_FOR: f32 = 9.0;

pub struct ChatLine {
    pub text: String,
    pub born: f32,
    pub system: bool,
}

#[derive(Resource, Default)]
pub struct Chat {
    pub lines: VecDeque<ChatLine>,
    pub open: bool,
    /// Fermé pendant cette image (empêche Échap d'ouvrir aussi le menu).
    pub closed_now: bool,
}

#[derive(Component)]
pub struct ChatRoot;
#[derive(Component)]
pub struct ChatField;
#[derive(Component)]
pub struct ChatLineText(usize);

pub fn setup(mut commands: Commands, font: Res<UiFont>) {
    let font = font.into_inner().clone();
    commands
        .spawn((
            ChatRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                bottom: px(124),
                width: px(520.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(2.0),
                ..default()
            },
        ))
        .with_children(|p| {
            for i in 0..LINES {
                p.spawn((
                    ChatLineText(i),
                    Node {
                        padding: UiRect::axes(px(8.0), px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                    ui::text(&font, "", 16.0, Color::WHITE),
                ));
            }
            let (f, t) = ui::field(&font, Val::Percent(100.0), "", "Message…", 200);
            p.spawn((ChatField, f, Visibility::Hidden)).with_child(t);
        });
}

impl Chat {
    pub fn push(&mut self, text: impl Into<String>, now: f32, system: bool) {
        self.lines.push_back(ChatLine {
            text: text.into(),
            born: now,
            system,
        });
        while self.lines.len() > 60 {
            self.lines.pop_front();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut chat: ResMut<Chat>,
    mut game: ResMut<GameState>,
    lobby: Res<Lobby>,
    shop: Res<crate::shop::ShopState>,
    mut net: ResMut<NetClient>,
    touch: Res<crate::touch::TouchState>,
    mut field: Query<(&mut TextField, &mut Visibility), With<ChatField>>,
    mut lines: Query<(&ChatLineText, &mut Text, &mut TextColor, &mut BackgroundColor)>,
) {
    chat.closed_now = false;
    let now = time.elapsed_secs();
    let Ok((mut f, mut vis)) = field.single_mut() else { return };

    let can_chat = lobby.chat && net.is_online() && game.phase == Phase::Playing && !game.dead && !shop.open;
    if !chat.open && can_chat && !game.loading && (keys.just_pressed(KeyCode::KeyT) || keys.just_pressed(KeyCode::Enter) || touch.chat) {
        chat.open = true;
        f.value.clear();
        f.focused = true;
        *vis = Visibility::Inherited;
    } else if chat.open {
        // `text_fields` retire le focus sur Entrée / Échap / Tab.
        if !f.focused {
            let send = keys.just_pressed(KeyCode::Enter) && !f.value.trim().is_empty();
            if send {
                net.send_raw(Packet::Chat { text: f.value.trim().to_string() });
            }
            chat.open = false;
            chat.closed_now = true;
            f.value.clear();
            *vis = Visibility::Hidden;
        } else if game.phase != Phase::Playing {
            f.focused = false;
        }
    }
    game.typing = chat.open;

    // Affichage : les dernières lignes, estompées avec l'âge.
    let visible: Vec<&ChatLine> = chat.lines.iter().rev().take(LINES).collect();
    for (slot, mut text, mut color, mut bg) in &mut lines {
        // Ligne 0 en haut : la plus ancienne des visibles.
        let idx = LINES - 1 - slot.0;
        match visible.get(idx) {
            Some(l) => {
                let age = now - l.born;
                let alpha = if chat.open { 1.0 } else { (1.0 - (age - (SHOWN_FOR - 1.5)) / 1.5).clamp(0.0, 1.0) };
                let c = if l.system { Color::srgba(1.0, 0.82, 0.35, alpha) } else { Color::srgba(1.0, 1.0, 1.0, alpha) };
                if text.0 != l.text {
                    text.0 = l.text.clone();
                }
                if color.0 != c {
                    color.0 = c;
                }
                let b = Color::srgba(0.0, 0.0, 0.0, 0.35 * alpha);
                if bg.0 != b {
                    bg.0 = b;
                }
            }
            None => {
                if !text.0.is_empty() {
                    text.0.clear();
                }
                bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.0);
            }
        }
    }
}
