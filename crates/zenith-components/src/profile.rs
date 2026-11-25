//! The `profile` shape (item component `profile` and `object` player contents; spec B.7).

use zenith_codec::codec::DataResult;
use zenith_registry::Identifier;

use crate::dynval::{Dyn, MapBuilder, Rec, read_identifier, read_list, read_string};
use crate::text::events::read_uuid;

/// A signed property of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileProperty {
    /// Property name (≤ 64 chars).
    pub name: String,
    /// Value (≤ 32,767 chars).
    pub value: String,
    /// Optional signature (≤ 1024 chars).
    pub signature: Option<String>,
}

/// A (possibly partial) player profile.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    /// Player name (≤ 16 chars, each char strictly between U+0020 and U+007F).
    pub name: Option<String>,
    /// UUID as four ints.
    pub id: Option<[i32; 4]>,
    /// Properties (≤ 16).
    pub properties: Vec<ProfileProperty>,
    /// Skin texture override.
    pub texture: Option<Identifier>,
    /// Cape texture override.
    pub cape: Option<Identifier>,
    /// Elytra texture override.
    pub elytra: Option<Identifier>,
    /// Model override.
    pub model: Option<String>,
}

impl Profile {
    /// Whether the profile still has to be resolved by a profile lookup: a bare name, or exactly one of name / id and
    /// no properties.
    pub fn is_partial(&self) -> bool {
        self.properties.is_empty() && (self.name.is_some() != self.id.is_some())
    }
}

fn valid_name(s: &str) -> Result<(), String> {
    if s.encode_utf16().count() > 16 {
        return Err(format!("Player name is too long: {s}"));
    }
    if s.chars().any(|c| c <= ' ' || c as u32 >= 127) {
        return Err(format!("Player name contains invalid characters: {s:?}"));
    }
    Ok(())
}

fn decode_property(d: &Dyn) -> DataResult<ProfileProperty> {
    let mut r = Rec::new(d);
    let name = r.required("name", |v| {
        read_string(v).and_then(|s| {
            if s.encode_utf16().count() <= 64 {
                DataResult::Success(s)
            } else {
                DataResult::error("Property name is too long")
            }
        })
    });
    let value = r.required("value", |v| {
        read_string(v).and_then(|s| {
            if s.encode_utf16().count() <= 32_767 {
                DataResult::Success(s)
            } else {
                DataResult::error("Property value is too long")
            }
        })
    });
    let signature = r.optional_opt("signature", |v| {
        read_string(v).and_then(|s| {
            if s.encode_utf16().count() <= 1024 {
                DataResult::Success(s)
            } else {
                DataResult::error("Property signature is too long")
            }
        })
    });
    r.finish(|| {
        Some(ProfileProperty {
            name: name?,
            value: value?,
            signature,
        })
    })
}

/// Decodes a profile: a bare name or the object form.
pub fn decode_profile(d: &Dyn) -> DataResult<Profile> {
    if let Dyn::Str(_) = d {
        return read_string(d).and_then(|s| match valid_name(&s) {
            Ok(()) => DataResult::Success(Profile {
                name: Some(s),
                ..Profile::default()
            }),
            Err(m) => DataResult::error(m),
        });
    }
    let mut r = Rec::new(d);
    let name = r.optional_opt("name", |v| {
        read_string(v).and_then(|s| match valid_name(&s) {
            Ok(()) => DataResult::Success(s),
            Err(m) => DataResult::error(m),
        })
    });
    let id = r.optional_opt("id", read_uuid);
    let props = r.optional("properties", Vec::new(), |v| {
        let items = match read_list(v) {
            DataResult::Success(i) => i,
            other => return other.map(|_| Vec::new()),
        };
        if items.len() > 16 {
            return DataResult::error("Too many profile properties");
        }
        let mut out = Vec::new();
        for it in &items {
            match decode_property(it) {
                DataResult::Success(p) => out.push(p),
                DataResult::Error { message, .. } => return DataResult::error(message),
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        DataResult::Success(out)
    });
    let texture = r.optional_opt("texture", read_identifier);
    let cape = r.optional_opt("cape", read_identifier);
    let elytra = r.optional_opt("elytra", read_identifier);
    let model = r.optional_opt("model", |v| {
        read_string(v).and_then(|s| {
            if s == "wide" || s == "slim" {
                DataResult::Success(s)
            } else {
                DataResult::error(format!("Unknown player model: {s}"))
            }
        })
    });
    r.finish(|| {
        Some(Profile {
            name,
            id,
            properties: props,
            texture,
            cape,
            elytra,
            model,
        })
    })
}

/// Encodes a profile in the object form.
// PARITY [oracle]: settled by the text-component suite (whether a name-only profile re-encodes as a bare string).
pub fn encode_profile(p: &Profile) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    if let Some(n) = &p.name {
        b.put("name", Dyn::string(n));
    }
    if let Some(id) = p.id {
        b.put("id", Dyn::IntArray(id.to_vec()));
    }
    if !p.properties.is_empty() {
        let list = p
            .properties
            .iter()
            .map(|pp| {
                let mut m = MapBuilder::new();
                m.put("name", Dyn::string(&pp.name));
                m.put("value", Dyn::string(&pp.value));
                if let Some(s) = &pp.signature {
                    m.put("signature", Dyn::string(s));
                }
                m.build()
            })
            .collect();
        b.put("properties", Dyn::List(list));
    }
    for (k, v) in [
        ("texture", &p.texture),
        ("cape", &p.cape),
        ("elytra", &p.elytra),
    ] {
        if let Some(id) = v {
            b.put(k, Dyn::string(id.to_string()));
        }
    }
    if let Some(m) = &p.model {
        b.put("model", Dyn::string(m));
    }
    DataResult::Success(b.build())
}
