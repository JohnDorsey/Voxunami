//! Showing a player an interface: a panel the server describes, which the player's client draws, and what the player does
//! with it, back.
//!
//! A plugin shows a form with [`crate::Context::show_ui`] and hears what the player presses as the event [`UiResponded`]. A
//! form stays until the plugin changes it ([`crate::Context::update_ui`]) or closes it ([`crate::Context::close_ui`]), the
//! player dismisses a modal one, or the player leaves. The engine keeps which forms are open for whom, so that only the
//! player a form was shown to can respond to it, and only with a button it has; anything else is ignored.
//!
//! A *modal* form takes the player's input (the mouse is freed, the world stops hearing keys) and has buttons; one that is not
//! modal is a panel of the screen, such as a bar the server keeps up to date, and has none.
//!
//! The three messages are the engine's own, so every game's clients can draw a form with no plugin of their own, and a
//! recording shows it.

use std::collections::BTreeMap;

use binary_codec::{ByteReader, ByteWriter, DecodeError};
use namespaced_id::NamespacedId;
use protocol::{ClientId, Message};
use ui::{Ui, UiError};

/// How many forms may be open for one player at once.
pub const MAXIMUM_FORMS_PER_PLAYER: usize = 8;

fn kind(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// Which form. Numbered from 1 in the order they are shown, and not used again while the server runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormId(u64);

impl FormId {
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for FormId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Why a form could not be shown or changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormError {
    #[error(transparent)]
    Refused(#[from] UiError),
    #[error("a form that is not modal has no buttons: the player cannot reach them")]
    ButtonsNeedAModal,
    #[error("a player has at most {MAXIMUM_FORMS_PER_PLAYER} forms open")]
    TooManyForms,
    #[error("there is no such player to show it to")]
    NotConnected,
    #[error("that form is not open for that player")]
    NotOpen,
}

/// Whether an interface can be shown as a form.
pub(crate) fn check(ui: &Ui) -> Result<(), FormError> {
    ui.check()?;
    if !ui.modal && !ui.button_ids().is_empty() {
        return Err(FormError::ButtonsNeedAModal);
    }
    Ok(())
}

/// What a player did with a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// Pressed the button with this identifier.
    Pressed(u32),
    /// Put the form away (Esc). Only a modal form can be.
    Dismissed,
}

/// Server to client: show this form, or replace the form with this number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiShow {
    pub id: FormId,
    pub ui: Ui,
}

impl Message for UiShow {
    fn kind() -> NamespacedId {
        kind("engine:ui_show")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.id.get());
        self.ui.encode(&mut writer);
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = FormId::new(reader.read_varint()?);
        let ui = Ui::decode(&mut reader)?;
        reader.finish()?;
        check(&ui).map_err(|_| DecodeError::Invalid("a form is not one that can be shown"))?;
        Ok(Self { id, ui })
    }

    fn samples() -> Vec<Self> {
        use ui::{Anchor, Color, Direction, Node, Panel};
        vec![
            Self {
                id: FormId::new(1),
                ui: Ui {
                    modal: true,
                    anchor: Anchor::Center,
                    root: Panel {
                        title: Some("Delete the level?".to_owned()),
                        color: Color([20, 20, 40]),
                        direction: Direction::Column,
                        children: vec![
                            Node::Button {
                                id: 1,
                                text: "Keep it".to_owned(),
                            },
                            Node::Button {
                                id: 2,
                                text: "Delete it".to_owned(),
                            },
                        ],
                    },
                },
            },
            Self {
                id: FormId::new(300),
                ui: Ui {
                    modal: false,
                    anchor: Anchor::TopRight,
                    root: Panel {
                        title: None,
                        color: Color([0, 0, 0]),
                        direction: Direction::Row,
                        children: vec![Node::Bar {
                            permille: 640,
                            cells: 10,
                            color: Color([200, 30, 30]),
                        }],
                    },
                },
            },
        ]
    }
}

/// Server to client: a form is over (closed by the plugin or dismissed), so the client stops showing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiClose(pub FormId);

impl Message for UiClose {
    fn kind() -> NamespacedId {
        kind("engine:ui_close")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.0.get());
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = FormId::new(reader.read_varint()?);
        reader.finish()?;
        Ok(Self(id))
    }

    fn samples() -> Vec<Self> {
        vec![Self(FormId::new(1)), Self(FormId::new(1 << 40))]
    }
}

/// Client to server: what the player did with a form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiResponse {
    pub id: FormId,
    pub action: UiAction,
}

impl Message for UiResponse {
    fn kind() -> NamespacedId {
        kind("engine:ui_response")
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = ByteWriter::new();
        writer.write_varint(self.id.get());
        match self.action {
            UiAction::Pressed(button) => {
                writer.write_u8(0);
                writer.write_varint(u64::from(button));
            }
            UiAction::Dismissed => writer.write_u8(1),
        }
        writer.into_bytes()
    }

    fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = ByteReader::new(payload);
        let id = FormId::new(reader.read_varint()?);
        let action = match reader.read_u8()? {
            0 => UiAction::Pressed(
                u32::try_from(reader.read_varint()?)
                    .map_err(|_| DecodeError::Invalid("a button identifier is too large"))?,
            ),
            1 => UiAction::Dismissed,
            _ => return Err(DecodeError::Invalid("an action is not one that is known")),
        };
        reader.finish()?;
        Ok(Self { id, action })
    }

    fn samples() -> Vec<Self> {
        vec![
            Self {
                id: FormId::new(1),
                action: UiAction::Pressed(2),
            },
            Self {
                id: FormId::new(70_000),
                action: UiAction::Pressed(1 << 20),
            },
            Self {
                id: FormId::new(3),
                action: UiAction::Dismissed,
            },
        ]
    }
}

/// What the engine remembers of a form that is open.
#[derive(Debug)]
struct Open {
    modal: bool,
    buttons: Vec<u32>,
}

/// The forms that are open, and the numbering of them.
#[derive(Debug, Default)]
pub(crate) struct Forms {
    last: u64,
    open: BTreeMap<(ClientId, FormId), Open>,
}

impl Forms {
    fn remember(ui: &Ui) -> Open {
        Open {
            modal: ui.modal,
            buttons: ui.button_ids(),
        }
    }

    /// How many forms a client has open.
    fn count(&self, client: ClientId) -> usize {
        self.open
            .range((client, FormId(0))..=(client, FormId(u64::MAX)))
            .count()
    }

    /// Records a form shown to `client` and numbers it, if the client may have another.
    pub(crate) fn show(&mut self, client: ClientId, ui: &Ui) -> Result<FormId, FormError> {
        check(ui)?;
        if self.count(client) >= MAXIMUM_FORMS_PER_PLAYER {
            return Err(FormError::TooManyForms);
        }
        self.last += 1;
        let id = FormId::new(self.last);
        self.open.insert((client, id), Self::remember(ui));
        Ok(id)
    }

    /// Replaces an open form's contents.
    pub(crate) fn update(
        &mut self,
        client: ClientId,
        id: FormId,
        ui: &Ui,
    ) -> Result<(), FormError> {
        check(ui)?;
        let open = self.open.get_mut(&(client, id)).ok_or(FormError::NotOpen)?;
        *open = Self::remember(ui);
        Ok(())
    }

    /// Whether a response is one the player may make: to a form that is open for them, pressing a button it has, or putting
    /// away a modal one. A form that is put away is no longer open; one with a button pressed stays.
    pub(crate) fn respond(&mut self, client: ClientId, id: FormId, action: UiAction) -> bool {
        let Some(open) = self.open.get(&(client, id)) else {
            return false;
        };
        match action {
            UiAction::Pressed(button) => open.buttons.contains(&button),
            UiAction::Dismissed => {
                let modal = open.modal;
                if modal {
                    self.open.remove(&(client, id));
                }
                modal
            }
        }
    }

    /// Closes a form: whether it was open.
    pub(crate) fn close(&mut self, client: ClientId, id: FormId) -> bool {
        self.open.remove(&(client, id)).is_some()
    }

    /// Closes every form of a client, who has gone.
    pub(crate) fn forget(&mut self, client: ClientId) {
        self.open.retain(|(owner, _), _| *owner != client);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.open.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::{Anchor, Color, Direction, Node, Panel};

    fn modal(buttons: &[u32]) -> Ui {
        Ui {
            modal: true,
            anchor: Anchor::Center,
            root: Panel {
                title: None,
                color: Color::BLACK,
                direction: Direction::Column,
                children: buttons
                    .iter()
                    .map(|id| Node::Button {
                        id: *id,
                        text: "b".to_owned(),
                    })
                    .collect(),
            },
        }
    }

    fn panel() -> Ui {
        Ui {
            modal: false,
            ..modal(&[])
        }
    }

    #[test]
    fn the_messages_round_trip_and_pass_the_generated_check() {
        assert_eq!(protocol::check_message::<UiShow>(), Ok(()));
        assert_eq!(protocol::check_message::<UiClose>(), Ok(()));
        assert_eq!(protocol::check_message::<UiResponse>(), Ok(()));
    }

    fn refuses_damaged<M: Message>() {
        for sample in M::samples() {
            let good = sample.encode();
            for length in 0..good.len() {
                assert!(
                    M::decode(&good[..length]).is_err(),
                    "{} cut to {length} bytes",
                    M::kind()
                );
            }
            let mut trailing = good;
            trailing.push(0);
            assert!(
                M::decode(&trailing).is_err(),
                "{} with a byte more",
                M::kind()
            );
        }
    }

    #[test]
    fn damaged_messages_are_errors_never_panics() {
        refuses_damaged::<UiShow>();
        refuses_damaged::<UiClose>();
        refuses_damaged::<UiResponse>();
        // An action number nobody has.
        let mut writer = ByteWriter::new();
        writer.write_varint(1);
        writer.write_u8(9);
        assert!(UiResponse::decode(&writer.into_bytes()).is_err());
    }

    #[test]
    fn a_form_that_could_not_be_shown_does_not_decode_either() {
        // A panel with a button the player cannot reach.
        let unreachable = Ui {
            modal: false,
            ..modal(&[1])
        };
        let message = UiShow {
            id: FormId::new(1),
            ui: unreachable,
        };
        assert!(UiShow::decode(&message.encode()).is_err());
        assert!(
            UiShow::decode(
                &UiShow {
                    id: FormId::new(1),
                    ui: modal(&[1, 2])
                }
                .encode()
            )
            .is_ok()
        );
    }

    #[test]
    fn forms_are_numbered_from_1_never_reused_and_at_most_eight_are_open_for_a_player() {
        let mut forms = Forms::default();
        let client = ClientId::new(1);
        let first = forms.show(client, &panel()).unwrap();
        assert_eq!(first, FormId::new(1));
        assert!(forms.close(client, first));
        assert_eq!(
            forms.show(client, &panel()).unwrap(),
            FormId::new(2),
            "not 1 again"
        );
        for _ in 0..MAXIMUM_FORMS_PER_PLAYER - 1 {
            forms.show(client, &panel()).unwrap();
        }
        assert_eq!(forms.show(client, &panel()), Err(FormError::TooManyForms));
        // Another player has their own eight.
        assert!(forms.show(ClientId::new(2), &panel()).is_ok());
        // Closing one makes room.
        assert!(forms.close(client, FormId::new(2)));
        assert!(forms.show(client, &panel()).is_ok());
    }

    #[test]
    fn only_a_button_the_form_has_can_be_pressed_by_the_player_it_was_shown_to_and_the_form_stays()
    {
        let mut forms = Forms::default();
        let (client, other) = (ClientId::new(1), ClientId::new(2));
        let id = forms.show(client, &modal(&[1, 2])).unwrap();
        assert!(forms.respond(client, id, UiAction::Pressed(2)));
        assert!(
            forms.respond(client, id, UiAction::Pressed(2)),
            "it stays open and can be pressed again"
        );
        assert!(
            !forms.respond(client, id, UiAction::Pressed(3)),
            "a button it has not"
        );
        assert!(
            !forms.respond(other, id, UiAction::Pressed(1)),
            "another player's"
        );
        assert!(
            !forms.respond(client, FormId::new(77), UiAction::Pressed(1)),
            "no such form"
        );
        assert_eq!(forms.len(), 1);
    }

    #[test]
    fn a_modal_form_can_be_dismissed_once_and_a_panel_cannot() {
        let mut forms = Forms::default();
        let client = ClientId::new(1);
        let modal_form = forms.show(client, &modal(&[1])).unwrap();
        let panel_form = forms.show(client, &panel()).unwrap();
        assert!(!forms.respond(client, panel_form, UiAction::Dismissed));
        assert!(forms.respond(client, modal_form, UiAction::Dismissed));
        assert!(
            !forms.respond(client, modal_form, UiAction::Dismissed),
            "it is gone"
        );
        assert!(!forms.respond(client, modal_form, UiAction::Pressed(1)));
        assert_eq!(forms.len(), 1);
    }

    #[test]
    fn updating_replaces_the_buttons_and_only_of_a_form_that_is_open() {
        let mut forms = Forms::default();
        let client = ClientId::new(1);
        let id = forms.show(client, &modal(&[1])).unwrap();
        forms.update(client, id, &modal(&[5])).unwrap();
        assert!(
            !forms.respond(client, id, UiAction::Pressed(1)),
            "the old button is gone"
        );
        assert!(forms.respond(client, id, UiAction::Pressed(5)));
        assert_eq!(
            forms.update(client, FormId::new(9), &modal(&[])),
            Err(FormError::NotOpen)
        );
        assert_eq!(
            forms.update(ClientId::new(2), id, &modal(&[])),
            Err(FormError::NotOpen)
        );
        // A form cannot be updated into one that is not allowed.
        assert_eq!(
            forms.update(
                client,
                id,
                &Ui {
                    modal: false,
                    ..modal(&[1])
                }
            ),
            Err(FormError::ButtonsNeedAModal)
        );
    }

    #[test]
    fn a_form_beyond_the_limits_of_an_interface_is_refused() {
        let mut forms = Forms::default();
        let mut too_long = modal(&[]);
        too_long.root.title = Some("x".repeat(ui::MAXIMUM_TEXT_CHARACTERS + 1));
        assert_eq!(
            forms.show(ClientId::new(1), &too_long),
            Err(FormError::Refused(UiError::TextTooLong))
        );
        assert_eq!(forms.len(), 0, "nothing was recorded");
    }

    #[test]
    fn forgetting_a_client_closes_its_forms_and_nobody_elses() {
        let mut forms = Forms::default();
        forms.show(ClientId::new(1), &panel()).unwrap();
        forms.show(ClientId::new(2), &panel()).unwrap();
        forms.forget(ClientId::new(1));
        assert_eq!(forms.len(), 1);
    }
}
