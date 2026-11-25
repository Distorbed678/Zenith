//! Spec edge cases 37–47 (Part B: Item data components) from `docs/specs/codec/text-and-data-components.md`.

use zenith_codec::json::{ParseMode, parse};
use zenith_nbt::parse_snbt;
use zenith_registry::Identifier;

use zenith_components::dynval::Dyn;
use zenith_components::item::*;
use zenith_components::text::Component;

fn id(s: &str) -> Identifier {
    Identifier::parse(s).unwrap()
}

fn dyn_json(s: &str) -> Dyn {
    Dyn::from_json(&parse(s, ParseMode::Data).unwrap())
}

fn dyn_snbt(s: &str) -> Dyn {
    Dyn::from_tag(&parse_snbt(s).unwrap())
}

fn sample_prototypes() -> PrototypeRegistry {
    let mut reg = PrototypeRegistry::new();

    let mut stone = ComponentMap::new();
    stone.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    stone.insert(
        ComponentTypeId::ItemName,
        ComponentValue::Text(Box::new(Component::translatable(
            "block.minecraft.stone",
            vec![],
        ))),
    );
    stone.insert(
        ComponentTypeId::Rarity,
        ComponentValue::Rarity(Rarity::Common),
    );
    reg.bind(id("minecraft:stone"), stone);

    let mut sword = ComponentMap::new();
    sword.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(1));
    sword.insert(ComponentTypeId::MaxDamage, ComponentValue::Int(1561));
    sword.insert(ComponentTypeId::Damage, ComponentValue::Int(0));
    sword.insert(
        ComponentTypeId::Rarity,
        ComponentValue::Rarity(Rarity::Common),
    );
    reg.bind(id("minecraft:diamond_sword"), sword);

    reg
}

#[test]
fn edge_37_patch_sanitization_drops_default_equals_and_noop_removals() {
    let protos = sample_prototypes();
    let stack = ItemStack::decode(
        &dyn_json(
            r#"{"id":"minecraft:stone","count":1,"components":{"minecraft:max_stack_size":64,"!minecraft:food":{}}}"#,
        ),
        &protos,
    )
    .demand()
    .unwrap();
    assert!(stack.components().unwrap().patch().is_empty());
    let enc = stack.encode_json().demand().unwrap();
    assert!(enc.get("components").is_none());
}

#[test]
fn edge_38_removal_and_unit_components_require_a_map() {
    assert!(
        ComponentPatch::decode(&dyn_json(r#"{"!minecraft:damage":0}"#))
            .demand()
            .is_err()
    );
    let ok_rem = ComponentPatch::decode(&dyn_json(r#"{"!minecraft:damage":{"x":1}}"#))
        .demand()
        .unwrap();
    assert_eq!(
        ok_rem.get(ComponentTypeId::Damage),
        Some(&PatchEntry::Removed)
    );

    let ok_unit = ComponentPatch::decode(&dyn_json(r#"{"minecraft:unbreakable":{}}"#))
        .demand()
        .unwrap();
    assert_eq!(
        ok_unit.get(ComponentTypeId::Unbreakable),
        Some(&PatchEntry::Set(Box::new(ComponentValue::Unit(UnitValue))))
    );
    assert!(
        ComponentPatch::decode(&dyn_json(r#"{"minecraft:unbreakable":true}"#))
            .demand()
            .is_err()
    );
}

#[test]
fn edge_39_transient_invalid_and_unknown_patch_keys_fail() {
    assert!(
        ComponentPatch::decode(&dyn_json(r#"{"minecraft:creative_slot_lock":{}}"#))
            .demand()
            .is_err()
    );
    assert!(
        ComponentPatch::decode(&dyn_json(r#"{"Minecraft:damage":1}"#))
            .demand()
            .is_err()
    );
    assert!(
        ComponentPatch::decode(&dyn_json(r#"{"minecraft:does_not_exist":1}"#))
            .demand()
            .is_err()
    );
}

#[test]
fn edge_40_patch_with_both_set_and_remove_last_processed_wins() {
    let p1 = ComponentPatch::decode(&dyn_json(
        r#"{"minecraft:damage":5,"!minecraft:damage":{}}"#,
    ))
    .demand()
    .unwrap();
    assert_eq!(p1.get(ComponentTypeId::Damage), Some(&PatchEntry::Removed));

    let p2 = ComponentPatch::decode(&dyn_json(
        r#"{"!minecraft:damage":{},"minecraft:damage":5}"#,
    ))
    .demand()
    .unwrap();
    assert_eq!(
        p2.get(ComponentTypeId::Damage),
        Some(&PatchEntry::Set(Box::new(ComponentValue::Int(5))))
    );
}

#[test]
fn edge_41_stack_always_writes_count_template_omits_1_and_accepts_bare_id() {
    let protos = sample_prototypes();
    let stack = ItemStack::of(id("minecraft:stone"), &protos).unwrap();
    let stack_json = stack.encode_json().demand().unwrap();
    assert_eq!(
        stack_json.get("count").unwrap().as_number().unwrap().text(),
        "1"
    );

    let tpl = ItemTemplate::decode(&dyn_json(r#""minecraft:stone""#))
        .demand()
        .unwrap();
    assert_eq!(tpl.id, id("minecraft:stone"));
    assert_eq!(tpl.count, 1);
    let tpl_json = tpl.encode().demand().unwrap().to_json().unwrap();
    assert!(tpl_json.as_object().is_some());
    assert!(tpl_json.get("count").is_none());

    for bad in [
        r#"{"id":"minecraft:stone","count":0}"#,
        r#"{"id":"minecraft:stone","count":100}"#,
        r#"{"id":"minecraft:air","count":1}"#,
    ] {
        assert!(
            ItemTemplate::decode(&dyn_json(bad)).demand().is_err(),
            "{bad}"
        );
        assert!(
            ItemStack::decode(&dyn_json(bad), &protos).demand().is_err(),
            "{bad}"
        );
    }
}

#[test]
fn edge_42_damageable_and_stackable_decodes_from_nbt_but_fails_validation() {
    let protos = sample_prototypes();
    let stack = ItemStack::decode(
        &dyn_snbt(r#"{id:"minecraft:diamond_sword",components:{"minecraft:max_stack_size":64}}"#),
        &protos,
    )
    .demand()
    .unwrap();
    // Plain NBT decode succeeds, stack validation rejects.
    assert!(stack.validate(&protos).is_err());

    // Template -> stack runs validation and yields empty stack + warning.
    let tpl = ItemTemplate::decode(&dyn_snbt(
        r#"{id:"minecraft:diamond_sword",components:{"minecraft:max_stack_size":64}}"#,
    ))
    .demand()
    .unwrap();
    let (empty_stack, warn) = tpl.to_stack_or_empty(&protos);
    assert!(empty_stack.is_empty());
    assert!(warn.is_some());

    // Validated patch application restores the old patch on failure.
    let mut good_sword = ItemStack::of(id("minecraft:diamond_sword"), &protos).unwrap();
    let mut bad_patch = ComponentPatch::new();
    bad_patch.set(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    assert!(
        good_sword
            .apply_patch_validated(&bad_patch, &protos)
            .is_err()
    );
    assert_eq!(good_sword.max_stack_size(), 1);

    // Creative-slot packet does NOT run stack validation, only checks persistent encodability.
    let mut items = SimpleItemRegistry::new();
    items.insert(1, id("minecraft:diamond_sword"));
    let mut wire = Vec::new();
    encode_creative_slot_stack(&stack, &items, &mut wire).unwrap();
    let (decoded, used) = decode_creative_slot_stack(&wire, &items, &protos).unwrap();
    assert_eq!(used, wire.len());
    assert_eq!(decoded, stack);
}

#[test]
fn edge_43_container_slots_gaps_duplicates_and_slot_256_fails() {
    let val = decode_component_value(
        ComponentTypeId::Container,
        &dyn_json(
            r#"[
                {"slot":0,"item":"minecraft:stone"},
                {"slot":5,"item":"minecraft:diamond_sword"},
                {"slot":0,"item":{"id":"minecraft:stone","count":3}}
            ]"#,
        ),
    )
    .demand()
    .unwrap();
    let ComponentValue::Container(c) = val else {
        panic!("expected container");
    };
    assert_eq!(c.slots.len(), 6);
    assert_eq!(c.slots[0].as_ref().unwrap().count, 3);
    assert!(c.slots[1].is_none());
    assert!(c.slots[2].is_none());
    assert!(c.slots[3].is_none());
    assert!(c.slots[4].is_none());
    assert_eq!(
        c.slots[5].as_ref().unwrap().id,
        id("minecraft:diamond_sword")
    );

    assert!(
        decode_component_value(
            ComponentTypeId::Container,
            &dyn_json(r#"[{"slot":256,"item":"minecraft:stone"}]"#)
        )
        .demand()
        .is_err()
    );
}

#[test]
fn edge_44_potion_contents_short_string_reencodes_as_object() {
    let val = decode_component_value(
        ComponentTypeId::PotionContents,
        &dyn_json(r#""minecraft:swiftness""#),
    )
    .demand()
    .unwrap();
    let enc = encode_component_value(ComponentTypeId::PotionContents, &val)
        .demand()
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(enc.to_json_string(), r#"{"potion":"minecraft:swiftness"}"#);
}

#[test]
fn edge_45_profile_short_string_and_invalid_names() {
    let val = decode_component_value(ComponentTypeId::Profile, &dyn_json(r#""Steve""#))
        .demand()
        .unwrap();
    let ComponentValue::Profile(p) = val else {
        panic!("expected profile");
    };
    assert_eq!(p.name.as_deref(), Some("Steve"));
    assert!(p.is_partial());

    assert!(
        decode_component_value(ComponentTypeId::Profile, &dyn_json(r#""Bad Name""#))
            .demand()
            .is_err()
    );
    assert!(
        decode_component_value(
            ComponentTypeId::Profile,
            &dyn_json(r#""12345678901234567""#)
        )
        .demand()
        .is_err()
    );
}

#[test]
fn edge_46_sign_text_requires_4_messages_and_always_writes_color_and_glow() {
    assert!(
        decode_component_value(
            ComponentTypeId::SignTextFront,
            &dyn_json(r#"{"messages":["a","b","c"]}"#)
        )
        .demand()
        .is_err()
    );

    let val = decode_component_value(
        ComponentTypeId::SignTextFront,
        &dyn_json(r#"{"messages":["a","b","c","d"]}"#),
    )
    .demand()
    .unwrap();
    let enc = encode_component_value(ComponentTypeId::SignTextFront, &val)
        .demand()
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(enc.get("color").unwrap().as_str(), Some("black"));
    assert_eq!(enc.get("has_glowing_text").unwrap().as_bool(), Some(false));
}

#[test]
fn edge_47_network_patch_sets_before_removals_empty_and_non_positive_count() {
    // Empty patch is two zero VarInts.
    let mut empty_buf = Vec::new();
    encode_patch_network(&ComponentPatch::new(), &mut empty_buf).unwrap();
    assert_eq!(empty_buf, vec![0x00, 0x00]);
    let (decoded_empty, used) = decode_patch_network(&empty_buf).unwrap();
    assert_eq!(used, 2);
    assert!(decoded_empty.is_empty());

    // Insert a removal BEFORE a set in memory: wire format still writes sets first!
    let mut patch = ComponentPatch::new();
    patch.remove(ComponentTypeId::Damage); // id 3
    patch.set(ComponentTypeId::MaxStackSize, ComponentValue::Int(16)); // id 1, value 16
    let mut buf = Vec::new();
    encode_patch_network(&patch, &mut buf).unwrap();
    // Expected: set_count=1, rem_count=1, set(id=1, val=16), rem(id=3)
    assert_eq!(buf, vec![1, 1, 1, 16, 3]);
    let (decoded_patch, used) = decode_patch_network(&buf).unwrap();
    assert_eq!(used, buf.len());
    assert_eq!(decoded_patch, patch);

    // Item count <= 0 on the wire is the empty stack.
    let protos = sample_prototypes();
    let mut items = SimpleItemRegistry::new();
    items.insert(1, id("minecraft:stone"));
    for count in [0, -1, -50] {
        let mut sbuf = Vec::new();
        write_varint(&mut sbuf, count);
        let (stack, used) = decode_optional_stack_network(&sbuf, &items, &protos).unwrap();
        assert_eq!(used, sbuf.len());
        assert!(stack.is_empty());
        assert!(decode_stack_network(&sbuf, &items, &protos).is_err());
    }
}
