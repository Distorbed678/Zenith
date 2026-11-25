//! Optional `serde` feature round-trip tests (`to_tag` / `from_tag`).
#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use zenith_nbt::{Compound, Tag, from_tag, to_tag};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PlayerData {
    name: String,
    health: f32,
    pos: [f64; 3],
    alive: bool,
    level: i32,
    xp: i64,
    slot: i8,
    air: i16,
    #[serde(default)]
    nickname: Option<String>,
    tags: Vec<String>,
    mode: GameMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum GameMode {
    Survival,
    Creative,
    Custom(i32),
}

#[test]
fn serde_struct_roundtrip() {
    let player = PlayerData {
        name: "Alex".into(),
        health: 20.0,
        pos: [10.5, 64.0, -32.25],
        alive: true,
        level: 30,
        xp: 12_345_678_901,
        slot: 4,
        air: 300,
        nickname: None,
        tags: vec!["builder".into(), "explorer".into()],
        mode: GameMode::Survival,
    };

    let tag = to_tag(&player).unwrap();
    let compound = tag.as_compound().unwrap();
    assert_eq!(compound.get("name"), Some(&Tag::from("Alex")));
    assert_eq!(compound.get("alive"), Some(&Tag::Byte(1)));
    assert_eq!(compound.get("nickname"), None);

    let back: PlayerData = from_tag(tag).unwrap();
    assert_eq!(back, player);
}

#[test]
fn serde_enum_and_typed_array_deserialization() {
    let custom = GameMode::Custom(7);
    let tag = to_tag(&custom).unwrap();
    assert_eq!(from_tag::<GameMode>(tag).unwrap(), custom);

    // Typed IntArray can deserialize directly into a `Vec<i32>` field
    #[derive(Debug, PartialEq, Deserialize)]
    struct WithUuid {
        uuid: Vec<i32>,
    }
    let mut c = Compound::new();
    c.insert("uuid", Tag::IntArray(vec![1, 2, 3, 4]));
    let decoded: WithUuid = from_tag(Tag::Compound(c)).unwrap();
    assert_eq!(decoded.uuid, vec![1, 2, 3, 4]);
}
