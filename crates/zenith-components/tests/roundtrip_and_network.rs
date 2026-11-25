//! Comprehensive JSON <-> model <-> NBT round trips, registry checks, patched-map semantics,
//! bundle weight validation, and network / hashed stack vectors.

use zenith_codec::json::{ParseMode, parse};
use zenith_nbt::{Compound, NbtString, Tag};
use zenith_registry::Identifier;

use zenith_components::dynval::Dyn;
use zenith_components::item::*;
use zenith_components::text::*;

fn id(s: &str) -> Identifier {
    Identifier::parse(s).unwrap()
}

fn roundtrip_text(json_str: &str) -> Component {
    let c1 = decode_component_str_json(json_str).demand().unwrap();
    // JSON -> model -> JSON -> model
    let j = encode_component_json(&c1).demand().unwrap();
    let c2 = decode_component_json(&j).demand().unwrap();
    assert_eq!(c1, c2, "JSON roundtrip mismatch for {json_str}");
    // model -> NBT -> model
    let nbt = encode_component_nbt(&c1).demand().unwrap();
    let c3 = decode_component_nbt(&nbt).demand().unwrap();
    assert_eq!(c1, c3, "NBT roundtrip mismatch for {json_str}");
    // model -> network NBT bytes -> model
    let net = encode_component_network_nbt(&c1).unwrap();
    let c4 = decode_component_network_nbt(&net).unwrap();
    assert_eq!(c1, c4, "Network NBT roundtrip mismatch for {json_str}");
    c1
}

#[test]
fn text_json_model_nbt_roundtrip_all_kinds_styles_and_events() {
    roundtrip_text(r#""plain text""#);
    roundtrip_text(
        r##"{"text":"styled","color":"gold","shadow_color":16711935,"bold":true,"italic":false,"underlined":true,"strikethrough":false,"obfuscated":true,"insertion":"shift","font":"minecraft:uniform","extra":["child",{"text":"leaf","color":"#1234AB"}]}"##,
    );
    roundtrip_text(
        r#"{"translate":"chat.type.text","fallback":"<%s> %s","with":["Steve",{"text":"Hi!","bold":true},42,1.5]}"#,
    );
    // JSON `true` stays Bool(true) across JSON round-trip, and becomes Byte(1) across NBT (spec A.7.3, edge case 28).
    let with_bool = decode_component_str_json(r#"{"translate":"%s","with":[true]}"#)
        .demand()
        .unwrap();
    let j_bool = encode_component_json(&with_bool).demand().unwrap();
    assert_eq!(decode_component_json(&j_bool).demand().unwrap(), with_bool);
    let nbt_bool = encode_component_nbt(&with_bool).demand().unwrap();
    let from_nbt_bool = decode_component_nbt(&nbt_bool).demand().unwrap();
    assert_eq!(flatten_plain(&from_nbt_bool, &EmptyLanguage, None), "1");
    roundtrip_text(r#"{"keybind":"key.jump","color":"yellow"}"#);
    roundtrip_text(r#"{"score":{"name":"@p","objective":"health"}}"#);
    roundtrip_text(r#"{"score":{"name":"*","objective":"deaths"}}"#);
    roundtrip_text(
        r#"{"selector":"@a[limit=3,sort=nearest]","separator":{"text":" | ","color":"dark_gray"}}"#,
    );
    roundtrip_text(r#"{"nbt":"Pos[0]","entity":"@s","plain":true}"#);
    roundtrip_text(r#"{"nbt":"Items[0].id","block":"~ ~1 ~","interpret":true,"separator":", "}"#);
    roundtrip_text(r#"{"nbt":"data.name","storage":"demo:store"}"#);
    roundtrip_text(
        r#"{"sprite":"minecraft:item/diamond","atlas":"minecraft:items","fallback":"[gem]"}"#,
    );
    roundtrip_text(r#"{"player":{"name":"Alex"},"hat":false}"#);

    // Click events
    roundtrip_text(
        r#"{"text":"u","click_event":{"action":"open_url","url":"https://example.org/path?a=1"}}"#,
    );
    roundtrip_text(r#"{"text":"r","click_event":{"action":"run_command","command":"/help"}}"#);
    roundtrip_text(
        r#"{"text":"s","click_event":{"action":"suggest_command","command":"/msg Steve "}}"#,
    );
    roundtrip_text(
        r#"{"text":"d","click_event":{"action":"show_dialog","dialog":"minecraft:notice"}}"#,
    );
    roundtrip_text(r#"{"text":"p","click_event":{"action":"change_page","page":3}}"#);
    roundtrip_text(r#"{"text":"c","click_event":{"action":"copy_to_clipboard","value":"abc"}}"#);
    roundtrip_text(
        r#"{"text":"x","click_event":{"action":"custom","id":"demo:ev","payload":{"k":1}}}"#,
    );

    // Hover events
    roundtrip_text(
        r#"{"text":"t","hover_event":{"action":"show_text","value":{"text":"tip","color":"aqua"}}}"#,
    );
    roundtrip_text(
        r#"{"text":"i","hover_event":{"action":"show_item","id":"minecraft:stone","count":5,"components":{"minecraft:repair_cost":2}}}"#,
    );
    roundtrip_text(
        r#"{"text":"e","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[1,2,3,4],"name":"Porky"}}"#,
    );
}

#[test]
fn text_unpaired_surrogates_roundtrip_json_and_nbt() {
    let c = roundtrip_text(r#""\ud800\udc00x""#);
    assert_eq!(c.as_plain_literal().unwrap().as_str(), Some("\u{10000}x"));

    // Lone high and lone low surrogate survive JSON -> model -> JSON and model -> NBT -> model!
    let c_hi = roundtrip_text(r#""\ud800""#);
    assert_eq!(c_hi.as_plain_literal().unwrap().to_utf16(), vec![0xD800]);
    assert_eq!(
        encode_component_json_string(&c_hi).demand().unwrap(),
        r#""\ud800""#
    );

    let c_lo = roundtrip_text(r#""\udc00x""#);
    assert_eq!(
        c_lo.as_plain_literal().unwrap().to_utf16(),
        vec![0xDC00, b'x' as u16]
    );
    assert_eq!(
        encode_component_json_string(&c_lo).demand().unwrap(),
        r#""\udc00x""#
    );
}

#[test]
fn text_truncation_and_flat_list() {
    let c = Component::literal("Hello ")
        .with_style(Style::empty().with_color(TextColor::named("green").unwrap()))
        .append(Component::literal("World!").with_style(Style::empty().with_bold(true)));

    assert_eq!(flatten_truncated(&c, 8, &EmptyLanguage, None), "Hello Wo");
    assert_eq!(flatten_truncated(&c, 0, &EmptyLanguage, None), "");

    let flat = flatten_to_list(&c, &Style::empty(), &EmptyLanguage, None);
    assert_eq!(flat.len(), 2);
    assert_eq!(flat[0].content, Content::Literal(NbtString::from("Hello ")));
    assert_eq!(flat[0].style.color, TextColor::named("green"));
    assert_eq!(flat[1].content, Content::Literal(NbtString::from("World!")));
    assert_eq!(flat[1].style.color, TextColor::named("green"));
    assert_eq!(flat[1].style.bold, Some(true));
}

#[test]
fn component_type_registry_122_entries_and_flags() {
    assert_eq!(ComponentTypeId::ALL.len(), COMPONENT_TYPE_COUNT);
    assert_eq!(COMPONENT_TYPE_COUNT, 122);
    for (idx, &ty) in ComponentTypeId::ALL.iter().enumerate() {
        assert_eq!(ty.protocol_id() as usize, idx);
        assert_eq!(ComponentTypeId::from_protocol_id(idx as u32), Some(ty));
        assert_eq!(ComponentTypeId::parse_id(ty.id_str()).unwrap(), ty);
        let short = ty.id_str().strip_prefix("minecraft:").unwrap();
        assert_eq!(ComponentTypeId::parse_id(short).unwrap(), ty);
    }
    let transient: Vec<ComponentTypeId> = ComponentTypeId::ALL
        .iter()
        .copied()
        .filter(|t| t.is_transient())
        .collect();
    assert_eq!(
        transient,
        vec![
            ComponentTypeId::CreativeSlotLock,
            ComponentTypeId::AdditionalTradeCost,
            ComponentTypeId::MapPostProcessing,
        ]
    );
    let nbt_net: Vec<ComponentTypeId> = ComponentTypeId::ALL
        .iter()
        .copied()
        .filter(|t| t.uses_nbt_network_codec())
        .collect();
    assert_eq!(
        nbt_net,
        vec![
            ComponentTypeId::CustomData,
            ComponentTypeId::IntangibleProjectile,
            ComponentTypeId::MapDecorations,
            ComponentTypeId::DebugStickState,
            ComponentTypeId::Recipes,
            ComponentTypeId::Lock,
            ComponentTypeId::ContainerLoot,
        ]
    );
}

#[test]
fn component_catalog_json_model_nbt_roundtrips() {
    let cases: &[(ComponentTypeId, &str)] = &[
        (ComponentTypeId::CustomData, r#"{"foo":"bar","n":7}"#),
        (ComponentTypeId::MaxStackSize, "16"),
        (ComponentTypeId::MaxDamage, "500"),
        (ComponentTypeId::Damage, "12"),
        (ComponentTypeId::Unbreakable, "{}"),
        (
            ComponentTypeId::UseEffects,
            r#"{"can_sprint":true,"interact_vibrations":false,"speed_multiplier":0.5}"#,
        ),
        (
            ComponentTypeId::CustomName,
            r#"{"text":"Excalibur","color":"gold"}"#,
        ),
        (ComponentTypeId::MinimumAttackCharge, "0.75"),
        (ComponentTypeId::DamageType, r#""minecraft:spear""#),
        (
            ComponentTypeId::Lore,
            r#"["line 1",{"text":"line 2","bold":true}]"#,
        ),
        (ComponentTypeId::Rarity, r#""epic""#),
        (
            ComponentTypeId::Enchantments,
            r#"{"minecraft:sharpness":5,"minecraft:unbreaking":3}"#,
        ),
        (
            ComponentTypeId::CanPlaceOn,
            r#"{"blocks":"minecraft:stone"}"#,
        ),
        (
            ComponentTypeId::AttributeModifiers,
            r#"[{"type":"minecraft:attack_damage","id":"demo:dmg","amount":4.5,"operation":"add_value","slot":"mainhand","display":{"type":"hidden"}}]"#,
        ),
        (
            ComponentTypeId::CustomModelData,
            r#"{"floats":[1.5],"flags":[true],"strings":["a"],"colors":[[1.0,0.0,0.0]]}"#,
        ),
        (
            ComponentTypeId::TooltipDisplay,
            r#"{"hide_tooltip":true,"hidden_components":["minecraft:lore"]}"#,
        ),
        (ComponentTypeId::RepairCost, "4"),
        (ComponentTypeId::EnchantmentGlintOverride, "true"),
        (
            ComponentTypeId::Food,
            r#"{"nutrition":6,"saturation":0.8,"can_always_eat":true}"#,
        ),
        (
            ComponentTypeId::Consumable,
            r#"{"consume_seconds":2.0,"animation":"drink","on_consume_effects":[{"type":"minecraft:clear_all_effects"}]}"#,
        ),
        (ComponentTypeId::UseRemainder, r#"{"id":"minecraft:bowl"}"#),
        (
            ComponentTypeId::UseCooldown,
            r#"{"seconds":1.5,"cooldown_group":"demo:cd"}"#,
        ),
        (
            ComponentTypeId::DamageResistant,
            r##"{"types":"#minecraft:is_fire"}"##,
        ),
        (
            ComponentTypeId::Tool,
            r##"{"rules":[{"blocks":"#minecraft:mineable/pickaxe","speed":8.0,"correct_for_drops":true}],"damage_per_block":2}"##,
        ),
        (
            ComponentTypeId::Weapon,
            r#"{"item_damage_per_attack":2,"disable_blocking_for_seconds":5.0}"#,
        ),
        (
            ComponentTypeId::AttackRange,
            r#"{"min_reach":1.0,"max_reach":4.5,"hitbox_margin":0.2,"mob_factor":0.5}"#,
        ),
        (ComponentTypeId::Enchantable, r#"{"value":15}"#),
        (
            ComponentTypeId::Equippable,
            r#"{"slot":"chest","asset_id":"minecraft:diamond","allowed_entities":["minecraft:player"]}"#,
        ),
        (
            ComponentTypeId::Repairable,
            r##"{"items":"#minecraft:planks"}"##,
        ),
        (ComponentTypeId::Glider, "{}"),
        (
            ComponentTypeId::DeathProtection,
            r#"{"death_effects":[{"type":"minecraft:teleport_randomly","diameter":8.0}]}"#,
        ),
        (
            ComponentTypeId::BlocksAttacks,
            r##"{"block_delay_seconds":0.25,"bypassed_by":"#minecraft:bypasses_shield"}"##,
        ),
        (
            ComponentTypeId::PiercingWeapon,
            r#"{"deals_knockback":false,"dismounts":true,"sound":"minecraft:item.spear.attack"}"#,
        ),
        (
            ComponentTypeId::KineticWeapon,
            r#"{"contact_cooldown_ticks":5,"damage_conditions":{"max_duration_ticks":100,"min_relative_speed":3.0}}"#,
        ),
        (
            ComponentTypeId::AttackAnimation,
            r#"{"type":"stab","duration":8}"#,
        ),
        (ComponentTypeId::VillagerFood, r#"{"nutrition":3}"#),
        (ComponentTypeId::Dye, r#""cyan""#),
        (ComponentTypeId::DyedColor, "[0.0, 1.0, 0.0]"),
        (ComponentTypeId::MapId, "42"),
        (
            ComponentTypeId::MapDecorations,
            r#"{"home":{"type":"minecraft:red_x","x":10.5,"z":-20.0,"rotation":90.0}}"#,
        ),
        (
            ComponentTypeId::ChargedProjectiles,
            r#"["minecraft:arrow"]"#,
        ),
        (
            ComponentTypeId::BundleContents,
            r#"[{"id":"minecraft:stone","count":16}]"#,
        ),
        (
            ComponentTypeId::PotionContents,
            r#"{"potion":"minecraft:healing","custom_color":16711680,"custom_effects":[{"id":"minecraft:speed","amplifier":1,"duration":200}]}"#,
        ),
        (ComponentTypeId::PotionDurationScale, "0.5"),
        (
            ComponentTypeId::SuspiciousStewEffects,
            r#"[{"id":"minecraft:regeneration","duration":"bad"}]"#,
        ),
        (
            ComponentTypeId::WritableBookContent,
            r#"{"pages":["hello",{"raw":"a","filtered":"b"}]}"#,
        ),
        (
            ComponentTypeId::WrittenBookContent,
            r#"{"title":"My Book","author":"Steve","generation":1,"pages":["page 1"]}"#,
        ),
        (
            ComponentTypeId::Trim,
            r#"{"material":"minecraft:gold","pattern":"minecraft:spire"}"#,
        ),
        (
            ComponentTypeId::DebugStickState,
            r#"{"minecraft:lever":"powered"}"#,
        ),
        (
            ComponentTypeId::EntityData,
            r#"{"id":"minecraft:pig","CustomName":"oink"}"#,
        ),
        (ComponentTypeId::OminousBottleAmplifier, "3"),
        (
            ComponentTypeId::ProvidesBannerPatterns,
            r##""#minecraft:pattern_item/globe""##,
        ),
        (ComponentTypeId::Recipes, r#"["minecraft:oak_planks"]"#),
        (
            ComponentTypeId::LodestoneTracker,
            r#"{"target":{"dimension":"minecraft:overworld","pos":[1,64,-3]},"tracked":false}"#,
        ),
        (
            ComponentTypeId::FireworkExplosion,
            r#"{"shape":"star","colors":[16711680],"has_trail":true}"#,
        ),
        (
            ComponentTypeId::Fireworks,
            r#"{"flight_duration":2,"explosions":[{"shape":"burst","has_twinkle":true}]}"#,
        ),
        (
            ComponentTypeId::BannerPatterns,
            r#"[{"pattern":"minecraft:stripe_top","color":"red"}]"#,
        ),
        (
            ComponentTypeId::PotDecorations,
            r#"{"back":"minecraft:brick","front":{"id":"minecraft:angler_pottery_sherd"}}"#,
        ),
        (ComponentTypeId::BlockState, r#"{"honey_level":"5"}"#),
        (
            ComponentTypeId::Bees,
            r#"[{"entity_data":{"id":"minecraft:bee"},"ticks_in_hive":10,"min_ticks_in_hive":2400}]"#,
        ),
        (
            ComponentTypeId::ContainerLoot,
            r#"{"loot_table":"minecraft:chests/simple_dungeon","seed":999}"#,
        ),
        (
            ComponentTypeId::Compostable,
            r#"{"layers":"minecraft:compostable/low"}"#,
        ),
        (
            ComponentTypeId::CookingFuel,
            r#"{"burn_time":200,"speed_multiplier":"minecraft:cooking/speed_default"}"#,
        ),
        (
            ComponentTypeId::BrewingFuel,
            r#"{"uses":"minecraft:brewing/uses_default","speed_multiplier":1.0}"#,
        ),
        (
            ComponentTypeId::MobVisibility,
            r#"{"targeting_entity_types":"minecraft:creeper","visibility":0.5}"#,
        ),
        (ComponentTypeId::FoxVariant, r#""snow""#),
        (
            ComponentTypeId::SignTextBack,
            r#"{"messages":["1","2","3","4"],"color":"red","has_glowing_text":true}"#,
        ),
        (ComponentTypeId::Waxed, "{}"),
    ];

    for &(ty, raw_json) in cases {
        let j = parse(raw_json, ParseMode::Data).unwrap();
        let val = decode_component_value(ty, &Dyn::from_json(&j))
            .demand()
            .unwrap_or_else(|e| panic!("{ty} decode failed on {raw_json}: {e}"));
        let enc_dyn = encode_component_value(ty, &val)
            .demand()
            .unwrap_or_else(|e| panic!("{ty} encode failed: {e}"));
        // Re-decode from JSON
        let enc_json = enc_dyn.to_json().unwrap();
        let val_from_json = decode_component_value(ty, &Dyn::from_json(&enc_json))
            .demand()
            .unwrap_or_else(|e| panic!("{ty} re-decode JSON failed: {e}"));
        assert_eq!(val, val_from_json, "JSON roundtrip mismatch for {ty}");
        // Re-decode from NBT
        let enc_nbt = enc_dyn.to_tag().unwrap();
        let val_from_nbt = decode_component_value(ty, &Dyn::from_tag(&enc_nbt))
            .demand()
            .unwrap_or_else(|e| panic!("{ty} re-decode NBT failed: {e}"));
        assert_eq!(val, val_from_nbt, "NBT roundtrip mismatch for {ty}");
    }
}

#[test]
fn patched_map_semantics_and_iteration_order() {
    let mut proto = ComponentMap::new();
    proto.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    proto.insert(ComponentTypeId::RepairCost, ComponentValue::Int(0));
    proto.insert(
        ComponentTypeId::Rarity,
        ComponentValue::Rarity(Rarity::Common),
    );

    let mut m = PatchedComponentMap::from_prototype(proto.clone());
    // Setting default-equal value leaves patch empty.
    m.set(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    assert!(m.patch().is_empty());

    // Setting a new value and overriding an existing value puts them in patch order.
    m.set(ComponentTypeId::Damage, ComponentValue::Int(5));
    m.set(ComponentTypeId::RepairCost, ComponentValue::Int(3));
    m.remove(ComponentTypeId::Rarity);

    assert_eq!(
        m.get(ComponentTypeId::Damage),
        Some(&ComponentValue::Int(5))
    );
    assert_eq!(
        m.get(ComponentTypeId::RepairCost),
        Some(&ComponentValue::Int(3))
    );
    assert_eq!(m.get(ComponentTypeId::Rarity), None);
    assert_eq!(
        m.get(ComponentTypeId::MaxStackSize),
        Some(&ComponentValue::Int(64))
    );

    // Iteration lists patched set values first (in patch order), then untouched prototype entries.
    let keys = m.keys();
    assert_eq!(
        keys,
        vec![
            ComponentTypeId::Damage,
            ComponentTypeId::RepairCost,
            ComponentTypeId::MaxStackSize,
        ]
    );

    // Reverting to prototype drops the patch entry; equality holds across insertion orders.
    let mut m2 = PatchedComponentMap::from_prototype(proto);
    m2.remove(ComponentTypeId::Rarity);
    m2.set(ComponentTypeId::RepairCost, ComponentValue::Int(3));
    m2.set(ComponentTypeId::Damage, ComponentValue::Int(5));
    assert_eq!(m, m2);
}

#[test]
fn bundle_weight_calculation_and_display_name_helpers() {
    let mut protos = PrototypeRegistry::new();

    let mut stone = ComponentMap::new();
    stone.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    stone.insert(
        ComponentTypeId::ItemName,
        ComponentValue::Text(Box::new(Component::literal("Stone"))),
    );
    stone.insert(
        ComponentTypeId::Rarity,
        ComponentValue::Rarity(Rarity::Rare),
    );
    protos.bind(id("minecraft:stone"), stone);

    let mut nest = ComponentMap::new();
    nest.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(1));
    protos.bind(id("minecraft:bee_nest"), nest);

    let mut bundle = ComponentMap::new();
    bundle.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(1));
    bundle.insert(
        ComponentTypeId::BundleContents,
        ComponentValue::BundleContents(Vec::new()),
    );
    protos.bind(id("minecraft:bundle"), bundle);

    // 16 stone = 16/64 = 1/4; empty nested bundle = 1/16; bee nest with 1 bee = 1.
    let mut bee_tpl = ItemTemplate::new(id("minecraft:bee_nest"));
    bee_tpl.components.set(
        ComponentTypeId::Bees,
        ComponentValue::Bees(vec![BeehiveOccupant {
            entity_data: TypedEntityData {
                id: id("minecraft:bee"),
                payload: Compound::new(),
            },
            ticks_in_hive: 0,
            min_ticks_in_hive: 100,
        }]),
    );
    let mut stone_tpl = ItemTemplate::new(id("minecraft:stone"));
    stone_tpl.count = 16;
    let nested_bundle_tpl = ItemTemplate::new(id("minecraft:bundle"));

    let w = compute_bundle_weight(&[stone_tpl, nested_bundle_tpl, bee_tpl], &protos).unwrap();
    // 1/4 + 1/16 + 1 = 21/16
    assert_eq!(w, WeightFraction { num: 21, den: 16 });

    // Hover name + styling: default uses item_name + rarity color, NOT italic; custom_name IS italic.
    let mut st = ItemStack::of(id("minecraft:stone"), &protos).unwrap();
    let styled_default = st.styled_hover_name();
    assert_eq!(styled_default.style.color, TextColor::named("aqua"));
    assert_eq!(styled_default.style.italic, None);
    assert_eq!(
        flatten_plain(&styled_default, &EmptyLanguage, None),
        "Stone"
    );

    st.components_mut().unwrap().set(
        ComponentTypeId::CustomName,
        ComponentValue::Text(Box::new(Component::literal("Pet Rock"))),
    );
    let styled_custom = st.styled_hover_name();
    assert_eq!(styled_custom.style.italic, Some(true));
    assert_eq!(
        flatten_plain(&styled_custom, &EmptyLanguage, None),
        "Pet Rock"
    );
    assert_eq!(lore_base_style().color, TextColor::named("dark_purple"));
    assert_eq!(lore_base_style().italic, Some(true));
}

#[test]
fn network_patch_delimited_transient_and_hashed_stack_vectors() {
    // Transient components round-trip over the wire even though persistent codec rejects them.
    let mut patch = ComponentPatch::new();
    patch.set(
        ComponentTypeId::CreativeSlotLock,
        ComponentValue::Unit(UnitValue),
    );
    patch.set(ComponentTypeId::AdditionalTradeCost, ComponentValue::Int(5));
    patch.set(
        ComponentTypeId::MapPostProcessing,
        ComponentValue::MapPostProcessing(MapPostProcessing::Scale),
    );
    let mut custom = Compound::new();
    custom.insert("k", Tag::Int(99));
    patch.set(
        ComponentTypeId::CustomData,
        ComponentValue::Compound(custom),
    );
    patch.remove(ComponentTypeId::RepairCost);

    let mut wire = Vec::new();
    encode_patch_network(&patch, &mut wire).unwrap();
    let (dec, used) = decode_patch_network(&wire).unwrap();
    assert_eq!(used, wire.len());
    assert_eq!(dec, patch);

    // Delimited patch: trailing unconsumed byte inside a delimited slice fails!
    let mut delim = Vec::new();
    encode_patch_delimited(&patch, &mut delim).unwrap();
    let (dec_d, used_d) = decode_patch_delimited(&delim).unwrap();
    assert_eq!(used_d, delim.len());
    assert_eq!(dec_d, patch);

    // Corrupt a delimited length by +1 (extra byte) -> must fail.
    let bad_delim = vec![1, 0, 1, 2, 16, 0]; // 1 set, 0 rem, id=1 (max_stack_size), len=2, val=16 + extra 0
    assert!(decode_patch_delimited(&bad_delim).is_err());

    // HashedStack matching and network round-trip.
    struct DummyHasher;
    impl ComponentHasher for DummyHasher {
        fn hash_component(
            &self,
            ty: ComponentTypeId,
            value: &ComponentValue,
        ) -> Result<i32, String> {
            let mut h = ty.protocol_id() as i32 * 31;
            if let ComponentValue::Int(n) = value {
                h = h.wrapping_add(*n);
            }
            Ok(h)
        }
    }

    let mut protos = PrototypeRegistry::new();
    let mut stone = ComponentMap::new();
    stone.insert(ComponentTypeId::MaxStackSize, ComponentValue::Int(64));
    stone.insert(ComponentTypeId::RepairCost, ComponentValue::Int(0));
    protos.bind(id("minecraft:stone"), stone);

    let mut items = SimpleItemRegistry::new();
    items.insert(7, id("minecraft:stone"));

    let mut p = ComponentPatch::new();
    p.set(ComponentTypeId::MaxStackSize, ComponentValue::Int(16));
    p.remove(ComponentTypeId::RepairCost);
    let stack = ItemStack::from_parts(id("minecraft:stone"), 5, &p, &protos).unwrap();

    let hashed = HashedStack::from_stack(&stack, &DummyHasher).unwrap();
    assert!(hashed.matches(&stack, &DummyHasher).unwrap());

    let mut hbuf = Vec::new();
    hashed.encode_network(&items, &mut hbuf).unwrap();
    let (hashed_dec, hused) = HashedStack::decode_network(&hbuf, &items).unwrap();
    assert_eq!(hused, hbuf.len());
    assert_eq!(hashed_dec, hashed);
    assert!(hashed_dec.matches(&stack, &DummyHasher).unwrap());

    // Mismatch if stack count differs
    let other_stack = ItemStack::from_parts(id("minecraft:stone"), 4, &p, &protos).unwrap();
    assert!(!hashed.matches(&other_stack, &DummyHasher).unwrap());
}
