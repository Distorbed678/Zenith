//! Click and hover events (spec A.4, A.5) in their 26.3 formats.

use zenith_codec::codec::DataResult;
use zenith_nbt::Tag;
use zenith_registry::Identifier;

use super::Component;
use super::decode::decode_component;
use super::encode::encode_component;
use crate::dynval::{Dyn, MapBuilder, Num, Rec, read_identifier, read_int, read_string};
use crate::grammar::{format_uuid, parse_uri_scheme, parse_uuid_lenient};
use crate::item::ItemTemplate;

/// A dialog reference of `show_dialog`.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// A registered dialog.
    Id(Identifier),
    /// An inline definition (kept as written; the dialog codec belongs to the dialog spec).
    Inline(Dyn),
}

/// A click event.
#[derive(Debug, Clone, PartialEq)]
pub enum ClickEvent {
    /// Open an `http`/`https` URL.
    OpenUrl(String),
    /// Open a file (in-memory only; never serialized).
    OpenFile(String),
    /// Run a command.
    RunCommand(String),
    /// Put a command in the chat box.
    SuggestCommand(String),
    /// Show a dialog.
    ShowDialog(Dialog),
    /// Turn to a book page (≥ 1).
    ChangePage(i32),
    /// Copy text to the clipboard.
    CopyToClipboard(String),
    /// A custom event.
    Custom {
        /// Event id.
        id: Identifier,
        /// Optional payload.
        payload: Option<Tag>,
    },
}

/// A hover event.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverEvent {
    /// Show a component.
    ShowText(Box<Component>),
    /// Show an item tooltip.
    ShowItem(Box<ItemTemplate>),
    /// Show an entity tooltip.
    ShowEntity {
        /// Entity type id.
        id: Identifier,
        /// UUID as four ints.
        uuid: [i32; 4],
        /// Optional name.
        name: Option<Box<Component>>,
    },
}

/// A character is chat-safe if it is not `§`, below U+0020 or U+007F.
pub fn is_chat_safe(c: u16) -> bool {
    c != 0xA7 && c >= 0x20 && c != 0x7F
}

fn command(d: &Dyn) -> DataResult<String> {
    read_string(d).and_then(|s| {
        if s.encode_utf16().all(is_chat_safe) {
            DataResult::Success(s)
        } else {
            DataResult::error(format!("Command contains illegal characters: {s:?}"))
        }
    })
}

/// Reads a UUID: four ints (array or list) or a hyphenated string.
pub fn read_uuid(d: &Dyn) -> DataResult<[i32; 4]> {
    match d {
        Dyn::Str(s) => {
            let s = s.to_string_lossy();
            match parse_uuid_lenient(&s) {
                Some(u) => DataResult::Success(u),
                None => DataResult::error(format!("Invalid UUID string: {s}")),
            }
        }
        Dyn::IntArray(a) if a.len() == 4 => DataResult::Success([a[0], a[1], a[2], a[3]]),
        Dyn::List(l) if l.len() == 4 => {
            let mut out = [0; 4];
            for (i, e) in l.iter().enumerate() {
                match read_int(e) {
                    DataResult::Success(v) => out[i] = v,
                    other => return other.map(|_| [0; 4]),
                }
            }
            DataResult::Success(out)
        }
        Dyn::IntArray(_) | Dyn::List(_) => DataResult::error("Expected a uuid of 4 ints"),
        other => DataResult::error(format!("Not a UUID: {}", other.kind())),
    }
}

/// Decodes `click_event`.
pub fn decode_click_event(d: &Dyn) -> DataResult<ClickEvent> {
    let mut r = Rec::new(d);
    let action = r.required("action", read_string);
    let Some(action) = action else {
        return r.finish(|| None);
    };
    let ev = match action.as_str() {
        "open_url" => r
            .required("url", |v| {
                read_string(v).and_then(|u| match parse_uri_scheme(&u) {
                    Some(s)
                        if s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https") =>
                    {
                        DataResult::Success(u)
                    }
                    Some(s) => DataResult::error(format!("Unsupported protocol: {s}")),
                    None => DataResult::error(format!("Invalid URI: {u}")),
                })
            })
            .map(ClickEvent::OpenUrl),
        "open_file" => {
            r.push_error("Action not allowed: open_file");
            None
        }
        "run_command" => r.required("command", command).map(ClickEvent::RunCommand),
        "suggest_command" => r
            .required("command", command)
            .map(ClickEvent::SuggestCommand),
        "show_dialog" => r
            .required("dialog", |v| match v {
                Dyn::Str(_) => read_identifier(v).map(Dialog::Id),
                Dyn::Map(_) => DataResult::Success(Dialog::Inline(v.clone())),
                other => DataResult::error(format!("Not a dialog: {}", other.kind())),
            })
            .map(ClickEvent::ShowDialog),
        "change_page" => r
            .required("page", |v| {
                read_int(v).and_then(|p| {
                    if p >= 1 {
                        DataResult::Success(p)
                    } else {
                        DataResult::error(format!("Value must be at least 1: {p}"))
                    }
                })
            })
            .map(ClickEvent::ChangePage),
        "copy_to_clipboard" => r
            .required("value", read_string)
            .map(ClickEvent::CopyToClipboard),
        "custom" => {
            let id = r.required("id", read_identifier);
            let payload = r.optional_opt("payload", |v| match v.to_tag() {
                Ok(t) => DataResult::Success(t),
                Err(m) => DataResult::error(m),
            });
            id.map(|id| ClickEvent::Custom { id, payload })
        }
        other => {
            r.push_error(format!("Unknown click action: {other}"));
            None
        }
    };
    r.finish(|| ev)
}

/// Encodes `click_event` (fails for `open_file`).
pub fn encode_click_event(e: &ClickEvent) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    let s = |v: &str| Dyn::string(v);
    match e {
        ClickEvent::OpenUrl(u) => {
            b.put("action", s("open_url")).put("url", s(u));
        }
        ClickEvent::OpenFile(_) => return DataResult::error("Action not allowed: open_file"),
        ClickEvent::RunCommand(c) => {
            b.put("action", s("run_command")).put("command", s(c));
        }
        ClickEvent::SuggestCommand(c) => {
            b.put("action", s("suggest_command")).put("command", s(c));
        }
        ClickEvent::ShowDialog(d) => {
            let v = match d {
                Dialog::Id(id) => s(&id.to_string()),
                Dialog::Inline(d) => d.clone(),
            };
            b.put("action", s("show_dialog")).put("dialog", v);
        }
        ClickEvent::ChangePage(p) => {
            b.put("action", s("change_page"))
                .put("page", Dyn::Num(Num::Int(*p)));
        }
        ClickEvent::CopyToClipboard(v) => {
            b.put("action", s("copy_to_clipboard")).put("value", s(v));
        }
        ClickEvent::Custom { id, payload } => {
            b.put("action", s("custom")).put("id", s(&id.to_string()));
            if let Some(p) = payload {
                b.put("payload", Dyn::from_tag(p));
            }
        }
    }
    DataResult::Success(b.build())
}

/// Decodes `hover_event`.
pub fn decode_hover_event(d: &Dyn) -> DataResult<HoverEvent> {
    let mut r = Rec::new(d);
    let Some(action) = r.required("action", read_string) else {
        return r.finish(|| None);
    };
    let ev = match action.as_str() {
        "show_text" => r
            .required("value", decode_component)
            .map(|c| HoverEvent::ShowText(Box::new(c))),
        "show_item" => match ItemTemplate::decode_record(d) {
            DataResult::Success(t) => Some(HoverEvent::ShowItem(Box::new(t))),
            DataResult::Error { message, .. } => {
                r.push_error(message);
                None
            }
            DataResult::Failure(m) => return DataResult::Failure(m),
        },
        "show_entity" => {
            let id = r.required("id", read_identifier);
            let uuid = r.required("uuid", read_uuid);
            let name = r.optional_opt("name", decode_component);
            match (id, uuid) {
                (Some(id), Some(uuid)) => Some(HoverEvent::ShowEntity {
                    id,
                    uuid,
                    name: name.map(Box::new),
                }),
                _ => None,
            }
        }
        other => {
            r.push_error(format!("Unknown hover action: {other}"));
            None
        }
    };
    r.finish(|| ev)
}

/// Encodes `hover_event`.
pub fn encode_hover_event(e: &HoverEvent) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    match e {
        HoverEvent::ShowText(c) => {
            let v = match encode_component(c).demand() {
                Ok(v) => v,
                Err(e) => return DataResult::error(e.message),
            };
            b.put("action", Dyn::string("show_text")).put("value", v);
        }
        HoverEvent::ShowItem(t) => {
            let rec = match t.encode_record().demand() {
                Ok(v) => v,
                Err(e) => return DataResult::error(e.message),
            };
            b.put("action", Dyn::string("show_item"));
            if let Dyn::Map(m) = rec {
                for (k, v) in m {
                    b.put(k, v);
                }
            }
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            b.put("action", Dyn::string("show_entity"))
                .put("id", Dyn::string(id.to_string()))
                .put("uuid", Dyn::IntArray(uuid.to_vec()));
            if let Some(n) = name {
                match encode_component(n).demand() {
                    Ok(v) => {
                        b.put("name", v);
                    }
                    Err(e) => return DataResult::error(e.message),
                }
            }
        }
    }
    DataResult::Success(b.build())
}

/// Tooltip lines of a `show_entity` event after the optional name (type line key and lower-case UUID).
pub fn entity_tooltip_uuid_line(uuid: [i32; 4]) -> String {
    format_uuid(uuid)
}
