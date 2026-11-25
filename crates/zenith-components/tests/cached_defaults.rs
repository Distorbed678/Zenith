//! Cache-gated test verifying per-item default component loading and protocol IDs against the
//! locally ingested 26.3 data-generator reports (`~/.cache/zenith/versions/26.3/reports/`).

use std::fs;

use zenith_codec::json::{ParseMode, parse_bytes};
use zenith_registry::Identifier;

use zenith_components::item::*;
use zenith_components::text::{EmptyLanguage, flatten_plain};

#[test]
fn cached_26_3_item_defaults_and_registry_ids_load_cleanly() {
    let item_dir = cached_item_components_dir("26.3");
    if !item_dir.exists() {
        eprintln!(
            "skipping cached_26_3_item_defaults_and_registry_ids_load_cleanly: {} not present",
            item_dir.display()
        );
        return;
    }

    // 1. Verify all 122 component type IDs and protocol IDs against reports/registries.json.
    let reg_path = zenith_data::version_dir("26.3")
        .join("reports")
        .join("registries.json");
    if reg_path.exists() {
        let bytes = fs::read(&reg_path).expect("read registries.json");
        let root = parse_bytes(&bytes, ParseMode::Data).expect("parse registries.json");
        let entries = root
            .get("minecraft:data_component_type")
            .and_then(|v| v.get("entries"))
            .and_then(|v| v.as_object())
            .expect("minecraft:data_component_type entries");
        assert_eq!(entries.len(), COMPONENT_TYPE_COUNT);
        for (k, v) in entries.iter() {
            let pid = v
                .get("protocol_id")
                .and_then(|n| n.as_number())
                .and_then(|n| n.to_i32().ok())
                .expect("protocol_id") as u32;
            let ty = ComponentTypeId::from_protocol_id(pid)
                .unwrap_or_else(|| panic!("missing ComponentTypeId for protocol_id {pid}"));
            assert_eq!(ty.id_str(), k.as_str().unwrap());
        }
    }

    // 2. Load EVERY per-item default component file in reports/minecraft/components/item/*.json.
    let prototypes = load_prototypes_for_version("26.3")
        .expect("every 26.3 item default component file must decode cleanly");
    eprintln!("loaded {} cached 26.3 item prototypes", prototypes.len());
    assert!(
        prototypes.len() > 1000,
        "expected >1000 items, got {}",
        prototypes.len()
    );

    // 3. Spot-check representative vanilla items.
    let stone_id = Identifier::parse("minecraft:stone").unwrap();
    let stone = prototypes
        .get(&stone_id)
        .expect("minecraft:stone prototype");
    assert_eq!(
        stone.get(ComponentTypeId::MaxStackSize),
        Some(&ComponentValue::Int(64))
    );
    assert_eq!(
        stone.get(ComponentTypeId::Rarity),
        Some(&ComponentValue::Rarity(Rarity::Common))
    );
    let stone_stack = ItemStack::of(stone_id, &prototypes).unwrap();
    assert!(stone_stack.validate(&prototypes).is_ok());
    assert_eq!(
        flatten_plain(&stone_stack.hover_name(), &EmptyLanguage, None),
        "block.minecraft.stone"
    );

    let sword_id = Identifier::parse("minecraft:diamond_sword").unwrap();
    let sword = prototypes
        .get(&sword_id)
        .expect("minecraft:diamond_sword prototype");
    assert_eq!(
        sword.get(ComponentTypeId::MaxDamage),
        Some(&ComponentValue::Int(1561))
    );
    assert_eq!(
        sword.get(ComponentTypeId::MaxStackSize),
        Some(&ComponentValue::Int(1))
    );
    assert!(
        ItemStack::of(sword_id, &prototypes)
            .unwrap()
            .validate(&prototypes)
            .is_ok()
    );
}
