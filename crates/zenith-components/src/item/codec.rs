//! Persistent (JSON / NBT) codecs for all 119 persistent item data component types (spec B.7).

use std::collections::BTreeMap;

use zenith_codec::codec::DataResult;
use zenith_nbt::{Compound, Tag, parse_snbt_compound};
use zenith_registry::Identifier;

use super::registry::ComponentTypeId;
use super::stack::ItemTemplate;
use super::types::*;
use crate::dynval::{
    Dyn, MapBuilder, Num, Rec, read_bool, read_double, read_float, read_identifier, read_int,
    read_list, read_long, read_map, read_string,
};
use crate::profile::{decode_profile, encode_profile};
use crate::registry_context::{
    RegistryContext, check_registry_id, ids as reg_ids, read_registry_id,
};
use crate::text::{Component, decode_book_page, decode_component, encode_component};

/// Decodes a persistent component value for `ty`. Rejects transient component types.
pub fn decode_component_value(
    ty: ComponentTypeId,
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<ComponentValue> {
    if ty.is_transient() {
        return DataResult::error(format!("{ty} is not a persistent component"));
    }
    use ComponentTypeId::*;
    match ty {
        CustomData | BucketEntityData => decode_compound_or_snbt(d).map(ComponentValue::Compound),
        MaxStackSize => read_int_range(d, 1, 99).map(ComponentValue::Int),
        MaxDamage => read_int_min(d, 1).map(ComponentValue::Int),
        Damage | RepairCost => read_int_min(d, 0).map(ComponentValue::Int),
        Unbreakable | IntangibleProjectile | Glider | Waxed => {
            decode_unit(d).map(ComponentValue::Unit)
        }
        UseEffects => decode_use_effects(d).map(ComponentValue::UseEffects),
        CustomName | ItemName => {
            decode_component(d, registries).map(|c| ComponentValue::Text(Box::new(c)))
        }
        MinimumAttackCharge => read_float_range(d, 0.0, 1.0).map(ComponentValue::Float),
        ItemModel | TooltipStyle | NoteBlockSound => read_identifier(d).map(ComponentValue::Id),
        DamageType => {
            read_registry_id(d, reg_ids::damage_type(), registries).map(ComponentValue::Id)
        }
        BlockTransformer => {
            read_registry_id(d, reg_ids::block_transformer(), registries).map(ComponentValue::Id)
        }
        JukeboxPlayable => {
            read_registry_id(d, reg_ids::jukebox_song(), registries).map(ComponentValue::Id)
        }
        ProvidesPotteryPattern => read_registry_id(d, reg_ids::decorated_pot_pattern(), registries)
            .map(ComponentValue::Id),
        VillagerVariant => {
            read_registry_id(d, reg_ids::villager_type(), registries).map(ComponentValue::Id)
        }
        WolfVariant => {
            read_registry_id(d, reg_ids::wolf_variant(), registries).map(ComponentValue::Id)
        }
        WolfSoundVariant => {
            read_registry_id(d, reg_ids::wolf_sound_variant(), registries).map(ComponentValue::Id)
        }
        PigVariant => {
            read_registry_id(d, reg_ids::pig_variant(), registries).map(ComponentValue::Id)
        }
        PigSoundVariant => {
            read_registry_id(d, reg_ids::pig_sound_variant(), registries).map(ComponentValue::Id)
        }
        CowVariant => {
            read_registry_id(d, reg_ids::cow_variant(), registries).map(ComponentValue::Id)
        }
        CowSoundVariant => {
            read_registry_id(d, reg_ids::cow_sound_variant(), registries).map(ComponentValue::Id)
        }
        ChickenVariant => {
            read_registry_id(d, reg_ids::chicken_variant(), registries).map(ComponentValue::Id)
        }
        ChickenSoundVariant => read_registry_id(d, reg_ids::chicken_sound_variant(), registries)
            .map(ComponentValue::Id),
        ZombieNautilusVariant => {
            read_registry_id(d, reg_ids::zombie_nautilus_variant(), registries)
                .map(ComponentValue::Id)
        }
        FrogVariant => {
            read_registry_id(d, reg_ids::frog_variant(), registries).map(ComponentValue::Id)
        }
        PaintingVariant => {
            read_registry_id(d, reg_ids::painting_variant(), registries).map(ComponentValue::Id)
        }
        CatVariant => {
            read_registry_id(d, reg_ids::cat_variant(), registries).map(ComponentValue::Id)
        }
        CatSoundVariant => {
            read_registry_id(d, reg_ids::cat_sound_variant(), registries).map(ComponentValue::Id)
        }
        Lore => decode_sized_list(d, 0, 256, |v| decode_component(v, registries))
            .map(ComponentValue::Lore),
        Rarity => decode_rarity(d).map(ComponentValue::Rarity),
        Enchantments | StoredEnchantments => {
            decode_enchantments(d, registries).map(ComponentValue::Enchantments)
        }
        CanPlaceOn | CanBreak => decode_block_predicates(d).map(ComponentValue::BlockPredicates),
        AttributeModifiers => {
            decode_attribute_modifiers(d, registries).map(ComponentValue::AttributeModifiers)
        }
        CustomModelData => decode_custom_model_data(d).map(ComponentValue::CustomModelData),
        TooltipDisplay => decode_tooltip_display(d, registries).map(ComponentValue::TooltipDisplay),
        EnchantmentGlintOverride => read_bool(d).map(ComponentValue::Bool),
        Food => decode_food(d).map(ComponentValue::Food),
        Consumable => decode_consumable(d, registries).map(ComponentValue::Consumable),
        UseRemainder | SulfurCubeContent => {
            ItemTemplate::decode(d, registries).map(|t| ComponentValue::ItemTemplate(Box::new(t)))
        }
        UseCooldown => decode_use_cooldown(d).map(ComponentValue::UseCooldown),
        DamageResistant => {
            decode_damage_resistant(d, registries).map(ComponentValue::DamageResistant)
        }
        Tool => decode_tool(d, registries).map(ComponentValue::Tool),
        Weapon => decode_weapon(d).map(ComponentValue::Weapon),
        AttackRange => decode_attack_range(d).map(ComponentValue::AttackRange),
        Enchantable => decode_enchantable(d).map(ComponentValue::Enchantable),
        Equippable => decode_equippable(d, registries).map(ComponentValue::Equippable),
        Repairable => decode_repairable(d, registries).map(ComponentValue::Repairable),
        DeathProtection => {
            decode_death_protection(d, registries).map(ComponentValue::DeathProtection)
        }
        BlocksAttacks => decode_blocks_attacks(d, registries).map(ComponentValue::BlocksAttacks),
        PiercingWeapon => decode_piercing_weapon(d, registries).map(ComponentValue::PiercingWeapon),
        KineticWeapon => decode_kinetic_weapon(d, registries).map(ComponentValue::KineticWeapon),
        AttackAnimation | InteractAnimation => {
            decode_swing_animation(d).map(ComponentValue::SwingAnimation)
        }
        VillagerFood => decode_villager_food(d).map(ComponentValue::VillagerFood),
        Dye
        | BaseColor
        | WolfCollar
        | CatCollar
        | SheepColor
        | ShulkerColor
        | TropicalFishBaseColor
        | TropicalFishPatternColor
        | CushionColor => decode_dye(d).map(ComponentValue::Dye),
        DyedColor => decode_rgb_color(d).map(ComponentValue::Int),
        MapId => read_int(d).map(ComponentValue::Int),
        MapDecorations => decode_map_decorations(d, registries).map(ComponentValue::MapDecorations),
        ChargedProjectiles => {
            decode_sized_list(d, 0, 1024, |v| ItemTemplate::decode(v, registries))
                .map(ComponentValue::ChargedProjectiles)
        }
        BundleContents => decode_list_of(d, |v| ItemTemplate::decode(v, registries))
            .map(ComponentValue::BundleContents),
        PotionContents => decode_potion_contents(d, registries).map(ComponentValue::PotionContents),
        PotionDurationScale => read_float_min(d, 0.0).map(ComponentValue::Float),
        SuspiciousStewEffects => {
            decode_list_of(d, |v| decode_suspicious_stew_effect(v, registries))
                .map(ComponentValue::SuspiciousStewEffects)
        }
        WritableBookContent => {
            decode_writable_book_content(d).map(ComponentValue::WritableBookContent)
        }
        WrittenBookContent => {
            decode_written_book_content(d, registries).map(ComponentValue::WrittenBookContent)
        }
        Trim => decode_trim(d, registries).map(ComponentValue::Trim),
        DebugStickState => {
            decode_debug_stick_state(d, registries).map(ComponentValue::DebugStickState)
        }
        EntityData => decode_typed_entity_data(d, reg_ids::entity_type(), registries)
            .map(ComponentValue::TypedEntityData),
        BlockEntityData => decode_typed_entity_data(d, reg_ids::block_entity_type(), registries)
            .map(ComponentValue::TypedEntityData),
        Instrument => {
            decode_holder_ref(d, reg_ids::instrument(), registries).map(ComponentValue::Holder)
        }
        ProvidesTrimMaterial => {
            decode_holder_ref(d, reg_ids::trim_material(), registries).map(ComponentValue::Holder)
        }
        BreakSound => {
            decode_holder_ref(d, reg_ids::sound_event(), registries).map(ComponentValue::Holder)
        }
        OminousBottleAmplifier => read_int_range(d, 0, 4).map(ComponentValue::Int),
        ProvidesBannerPatterns => decode_holder_set(d, reg_ids::banner_pattern(), registries)
            .map(ComponentValue::HolderSet),
        Recipes => decode_list_of(d, read_identifier).map(ComponentValue::IdList),
        LodestoneTracker => decode_lodestone_tracker(d).map(ComponentValue::LodestoneTracker),
        FireworkExplosion => decode_firework_explosion(d).map(ComponentValue::FireworkExplosion),
        Fireworks => decode_fireworks(d).map(ComponentValue::Fireworks),
        Profile => decode_profile(d).map(ComponentValue::Profile),
        BannerPatterns => decode_list_of(d, |v| decode_banner_pattern_layer(v, registries))
            .map(ComponentValue::BannerPatterns),
        PotDecorations => decode_pot_decorations(d, registries).map(ComponentValue::PotDecorations),
        Container => decode_container(d, registries).map(ComponentValue::Container),
        BlockState => decode_block_state(d).map(ComponentValue::BlockState),
        Bees => {
            decode_list_of(d, |v| decode_beehive_occupant(v, registries)).map(ComponentValue::Bees)
        }
        Lock => {
            if d.is_map() {
                DataResult::Success(ComponentValue::Lock(d.clone()))
            } else {
                DataResult::error(format!("Not a map: {}", d.kind()))
            }
        }
        ContainerLoot => decode_container_loot(d).map(ComponentValue::ContainerLoot),
        Compostable => decode_compostable(d).map(ComponentValue::Compostable),
        CookingFuel => decode_cooking_fuel(d).map(ComponentValue::CookingFuel),
        BrewingFuel => decode_brewing_fuel(d).map(ComponentValue::BrewingFuel),
        MobVisibility => decode_mob_visibility(d, registries).map(ComponentValue::MobVisibility),
        FoxVariant | SalmonSize | ParrotVariant | TropicalFishPattern | MooshroomVariant
        | RabbitVariant | HorseVariant | LlamaVariant | AxolotlVariant => {
            decode_fixed_enum(ty, d).map(ComponentValue::EnumName)
        }
        SignTextFront | SignTextBack => {
            decode_sign_text(d, registries).map(|st| ComponentValue::SignText(Box::new(st)))
        }
        CreativeSlotLock | AdditionalTradeCost | MapPostProcessing => {
            unreachable!("checked is_transient above")
        }
    }
}

/// Encodes a persistent component value to [`Dyn`]. Rejects transient component types.
pub fn encode_component_value(ty: ComponentTypeId, value: &ComponentValue) -> DataResult<Dyn> {
    if ty.is_transient() {
        return DataResult::error(format!("{ty} is not a persistent component"));
    }
    match value {
        ComponentValue::Unit(_) => DataResult::Success(Dyn::empty_map()),
        ComponentValue::Bool(b) => DataResult::Success(Dyn::Bool(*b)),
        ComponentValue::Int(i) => DataResult::Success(Dyn::Num(Num::Int(*i))),
        ComponentValue::Float(f) => DataResult::Success(Dyn::Num(Num::Float(*f))),
        ComponentValue::EnumName(s) => DataResult::Success(Dyn::string(s)),
        ComponentValue::Id(id) => DataResult::Success(Dyn::string(id.to_string())),
        ComponentValue::Holder(h) => DataResult::Success(encode_holder_ref(h)),
        ComponentValue::HolderSet(hs) => DataResult::Success(encode_holder_set(hs)),
        ComponentValue::IdList(ids) => DataResult::Success(Dyn::List(
            ids.iter().map(|id| Dyn::string(id.to_string())).collect(),
        )),
        ComponentValue::Compound(c) => {
            DataResult::Success(Dyn::from_tag(&Tag::Compound(c.clone())))
        }
        ComponentValue::Text(c) => encode_component(c),
        ComponentValue::Lore(lines) => encode_list(lines, encode_component),
        ComponentValue::Rarity(r) => DataResult::Success(Dyn::string(r.as_str())),
        ComponentValue::Dye(d) => DataResult::Success(Dyn::string(d.as_str())),
        ComponentValue::UseEffects(u) => DataResult::Success(encode_use_effects(u)),
        ComponentValue::Enchantments(e) => DataResult::Success(encode_enchantments(e)),
        ComponentValue::BlockPredicates(preds) => {
            if preds.len() == 1 {
                DataResult::Success(preds[0].clone())
            } else {
                DataResult::Success(Dyn::List(preds.clone()))
            }
        }
        ComponentValue::AttributeModifiers(mods) => encode_list(mods, encode_attribute_modifier),
        ComponentValue::CustomModelData(cmd) => DataResult::Success(encode_custom_model_data(cmd)),
        ComponentValue::TooltipDisplay(td) => DataResult::Success(encode_tooltip_display(td)),
        ComponentValue::Food(f) => DataResult::Success(encode_food(f)),
        ComponentValue::Consumable(c) => DataResult::Success(encode_consumable(c)),
        ComponentValue::ItemTemplate(t) => t.encode(),
        ComponentValue::UseCooldown(uc) => DataResult::Success(encode_use_cooldown(uc)),
        ComponentValue::DamageResistant(hs) => {
            let mut b = MapBuilder::new();
            b.put("types", encode_holder_set(hs));
            DataResult::Success(b.build())
        }
        ComponentValue::Tool(t) => DataResult::Success(encode_tool(t)),
        ComponentValue::Weapon(w) => DataResult::Success(encode_weapon(w)),
        ComponentValue::AttackRange(a) => DataResult::Success(encode_attack_range(a)),
        ComponentValue::Enchantable(v) => {
            let mut b = MapBuilder::new();
            b.put("value", Dyn::Num(Num::Int(*v)));
            DataResult::Success(b.build())
        }
        ComponentValue::Equippable(e) => DataResult::Success(encode_equippable(e)),
        ComponentValue::Repairable(hs) => {
            let mut b = MapBuilder::new();
            b.put("items", encode_holder_set(hs));
            DataResult::Success(b.build())
        }
        ComponentValue::DeathProtection(effects) => {
            let mut b = MapBuilder::new();
            if !effects.is_empty() {
                b.put(
                    "death_effects",
                    Dyn::List(effects.iter().map(encode_consume_effect).collect()),
                );
            }
            DataResult::Success(b.build())
        }
        ComponentValue::BlocksAttacks(ba) => DataResult::Success(encode_blocks_attacks(ba)),
        ComponentValue::PiercingWeapon(pw) => DataResult::Success(encode_piercing_weapon(pw)),
        ComponentValue::KineticWeapon(kw) => DataResult::Success(encode_kinetic_weapon(kw)),
        ComponentValue::SwingAnimation(sa) => DataResult::Success(encode_swing_animation(sa)),
        ComponentValue::VillagerFood(n) => {
            let mut b = MapBuilder::new();
            b.put("nutrition", Dyn::Num(Num::Int(*n)));
            DataResult::Success(b.build())
        }
        ComponentValue::MapDecorations(m) => DataResult::Success(encode_map_decorations(m)),
        ComponentValue::MapPostProcessing(_) => {
            DataResult::error("minecraft:map_post_processing is not a persistent component")
        }
        ComponentValue::ChargedProjectiles(items) | ComponentValue::BundleContents(items) => {
            encode_list(items, ItemTemplate::encode)
        }
        ComponentValue::PotionContents(pc) => DataResult::Success(encode_potion_contents(pc)),
        ComponentValue::SuspiciousStewEffects(effects) => DataResult::Success(Dyn::List(
            effects.iter().map(encode_suspicious_stew_effect).collect(),
        )),
        ComponentValue::WritableBookContent(wbc) => {
            DataResult::Success(encode_writable_book_content(wbc))
        }
        ComponentValue::WrittenBookContent(wbc) => encode_written_book_content(wbc),
        ComponentValue::Trim(t) => {
            let mut b = MapBuilder::new();
            b.put("material", encode_holder_ref(&t.material));
            b.put("pattern", encode_holder_ref(&t.pattern));
            DataResult::Success(b.build())
        }
        ComponentValue::DebugStickState(m) => {
            let mut b = MapBuilder::new();
            for (k, v) in m {
                b.put(k.to_string(), Dyn::string(v));
            }
            DataResult::Success(b.build())
        }
        ComponentValue::TypedEntityData(ted) => DataResult::Success(encode_typed_entity_data(ted)),
        ComponentValue::LodestoneTracker(lt) => DataResult::Success(encode_lodestone_tracker(lt)),
        ComponentValue::FireworkExplosion(fe) => DataResult::Success(encode_firework_explosion(fe)),
        ComponentValue::Fireworks(fw) => DataResult::Success(encode_fireworks(fw)),
        ComponentValue::Profile(p) => encode_profile(p),
        ComponentValue::BannerPatterns(layers) => DataResult::Success(Dyn::List(
            layers.iter().map(encode_banner_pattern_layer).collect(),
        )),
        ComponentValue::PotDecorations(pd) => encode_pot_decorations(pd),
        ComponentValue::Container(c) => encode_container(c),
        ComponentValue::BlockState(m) => {
            let mut b = MapBuilder::new();
            for (k, v) in m {
                b.put(k.as_str(), Dyn::string(v));
            }
            DataResult::Success(b.build())
        }
        ComponentValue::Bees(bees) => DataResult::Success(Dyn::List(
            bees.iter().map(encode_beehive_occupant).collect(),
        )),
        ComponentValue::Lock(d) => DataResult::Success(d.clone()),
        ComponentValue::ContainerLoot(cl) => {
            let mut b = MapBuilder::new();
            b.put("loot_table", Dyn::string(cl.loot_table.to_string()));
            if cl.seed != 0 {
                b.put("seed", Dyn::Num(Num::Long(cl.seed)));
            }
            DataResult::Success(b.build())
        }
        ComponentValue::Compostable(c) => {
            let mut b = MapBuilder::new();
            b.put("layers", encode_int_or_provider(&c.layers));
            DataResult::Success(b.build())
        }
        ComponentValue::CookingFuel(cf) => {
            let mut b = MapBuilder::new();
            b.put("burn_time", encode_int_or_provider(&cf.burn_time));
            b.put(
                "speed_multiplier",
                encode_float_or_provider(&cf.speed_multiplier),
            );
            DataResult::Success(b.build())
        }
        ComponentValue::BrewingFuel(bf) => {
            let mut b = MapBuilder::new();
            b.put("uses", encode_int_or_provider(&bf.uses));
            b.put(
                "speed_multiplier",
                encode_float_or_provider(&bf.speed_multiplier),
            );
            DataResult::Success(b.build())
        }
        ComponentValue::MobVisibility(mv) => {
            let mut b = MapBuilder::new();
            b.put(
                "targeting_entity_types",
                encode_holder_set(&mv.targeting_entity_types),
            );
            b.put("visibility", Dyn::Num(Num::Float(mv.visibility)));
            DataResult::Success(b.build())
        }
        ComponentValue::SignText(st) => encode_sign_text(st),
    }
}

// ----------------------------------------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------------------------------------

fn read_int_range(d: &Dyn, min: i32, max: i32) -> DataResult<i32> {
    read_int(d).and_then(|v| {
        if (min..=max).contains(&v) {
            DataResult::Success(v)
        } else {
            DataResult::error(format!("Value {v} outside range {min}..={max}"))
        }
    })
}

fn read_int_min(d: &Dyn, min: i32) -> DataResult<i32> {
    read_int(d).and_then(|v| {
        if v >= min {
            DataResult::Success(v)
        } else {
            DataResult::error(format!("Value {v} must be at least {min}"))
        }
    })
}

fn read_float_range(d: &Dyn, min: f32, max: f32) -> DataResult<f32> {
    read_float(d).and_then(|v| {
        if v >= min && v <= max {
            DataResult::Success(v)
        } else {
            DataResult::error(format!("Value {v} outside range {min}..={max}"))
        }
    })
}

fn read_float_min(d: &Dyn, min: f32) -> DataResult<f32> {
    read_float(d).and_then(|v| {
        if v >= min {
            DataResult::Success(v)
        } else {
            DataResult::error(format!("Value {v} must be at least {min}"))
        }
    })
}

fn read_float_gt(d: &Dyn, min_exclusive: f32) -> DataResult<f32> {
    read_float(d).and_then(|v| {
        if v > min_exclusive {
            DataResult::Success(v)
        } else {
            DataResult::error(format!("Value {v} must be greater than {min_exclusive}"))
        }
    })
}

fn decode_unit(d: &Dyn) -> DataResult<UnitValue> {
    if d.is_map() {
        DataResult::Success(UnitValue)
    } else {
        DataResult::error(format!(
            "Expected empty map for unit value, got {}",
            d.kind()
        ))
    }
}

fn decode_list_of<T>(d: &Dyn, f: impl Fn(&Dyn) -> DataResult<T>) -> DataResult<Vec<T>> {
    decode_sized_list(d, 0, usize::MAX, f)
}

fn decode_sized_list<T>(
    d: &Dyn,
    min: usize,
    max: usize,
    f: impl Fn(&Dyn) -> DataResult<T>,
) -> DataResult<Vec<T>> {
    let items = match read_list(d) {
        DataResult::Success(i) => i,
        other => return other.map(|_| Vec::new()),
    };
    let mut good = Vec::with_capacity(items.len());
    let mut errors = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match f(item) {
            DataResult::Success(v) => good.push(v),
            DataResult::Error { message, .. } => errors.push(format!("Element {i}: {message}")),
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    if items.len() < min {
        errors.push(format!(
            "List too short: {}, expected at least {min}",
            items.len()
        ));
    }
    if items.len() > max {
        errors.push(format!(
            "List too long: {}, expected at most {max}",
            items.len()
        ));
        good.truncate(max);
    }
    if errors.is_empty() {
        DataResult::Success(good)
    } else {
        DataResult::error_with_partial(errors.join("; "), good)
    }
}

fn encode_list<T>(items: &[T], f: impl Fn(&T) -> DataResult<Dyn>) -> DataResult<Dyn> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match f(item) {
            DataResult::Success(v) => out.push(v),
            other => return other,
        }
    }
    DataResult::Success(Dyn::List(out))
}

fn decode_compound_or_snbt(d: &Dyn) -> DataResult<Compound> {
    match d {
        Dyn::Map(_) => match d.to_tag() {
            Ok(Tag::Compound(c)) => DataResult::Success(c),
            Ok(_) => DataResult::error("Expected compound"),
            Err(m) => DataResult::error(m),
        },
        Dyn::Str(s) => match parse_snbt_compound(&s.to_string_lossy()) {
            Ok(c) => DataResult::Success(c),
            Err(e) => DataResult::error(format!("Invalid SNBT compound: {e}")),
        },
        other => DataResult::error(format!(
            "Expected compound or SNBT string, got {}",
            other.kind()
        )),
    }
}

fn decode_holder_ref(
    d: &Dyn,
    registry: &Identifier,
    registries: &dyn RegistryContext,
) -> DataResult<HolderRef> {
    match d {
        Dyn::Str(_) => read_registry_id(d, registry, registries).map(HolderRef::Id),
        Dyn::Map(_) => DataResult::Success(HolderRef::Inline(d.clone())),
        other => DataResult::error(format!(
            "Expected identifier or inline definition, got {}",
            other.kind()
        )),
    }
}

fn encode_holder_ref(h: &HolderRef) -> Dyn {
    match h {
        HolderRef::Id(id) => Dyn::string(id.to_string()),
        HolderRef::Inline(d) => d.clone(),
    }
}

fn decode_holder_set(
    d: &Dyn,
    registry: &Identifier,
    registries: &dyn RegistryContext,
) -> DataResult<HolderSetRef> {
    match d {
        Dyn::Str(s) => {
            let s = s.to_string_lossy();
            if let Some(tag) = s.strip_prefix('#') {
                match Identifier::parse(tag) {
                    Ok(id) => {
                        // Hook for future tag validation (spec B.7): tags are not enforced until
                        // data-pack tag tables are loaded into RegistryContext.
                        let _ = registries.lookup_tag(registry, &id);
                        DataResult::Success(HolderSetRef::Tag(id))
                    }
                    Err(_) => DataResult::error(format!("Invalid tag id: {s}")),
                }
            } else {
                match Identifier::parse(&s) {
                    Ok(id) => match check_registry_id(registry, &id, registries) {
                        Ok(()) => DataResult::Success(HolderSetRef::Ids(vec![id])),
                        Err(m) => DataResult::error(m),
                    },
                    Err(_) => DataResult::error(format!("Not a valid resource location: {s}")),
                }
            }
        }
        Dyn::List(_) => {
            decode_list_of(d, |v| read_registry_id(v, registry, registries)).map(HolderSetRef::Ids)
        }
        other => DataResult::error(format!(
            "Expected tag, id, or id list for holder set, got {}",
            other.kind()
        )),
    }
}

fn encode_holder_set(hs: &HolderSetRef) -> Dyn {
    match hs {
        HolderSetRef::Tag(id) => Dyn::string(format!("#{id}")),
        HolderSetRef::Ids(ids) if ids.len() == 1 => Dyn::string(ids[0].to_string()),
        HolderSetRef::Ids(ids) => {
            Dyn::List(ids.iter().map(|id| Dyn::string(id.to_string())).collect())
        }
    }
}

fn decode_int_or_provider(d: &Dyn) -> DataResult<IntOrProvider> {
    match d {
        Dyn::Num(_) => read_int(d).map(IntOrProvider::Fixed),
        Dyn::Str(_) => read_identifier(d).map(IntOrProvider::Provider),
        other => DataResult::error(format!("Expected int or provider id, got {}", other.kind())),
    }
}

fn encode_int_or_provider(v: &IntOrProvider) -> Dyn {
    match v {
        IntOrProvider::Fixed(i) => Dyn::Num(Num::Int(*i)),
        IntOrProvider::Provider(id) => Dyn::string(id.to_string()),
    }
}

fn decode_float_or_provider(d: &Dyn) -> DataResult<FloatOrProvider> {
    match d {
        Dyn::Num(_) => read_float(d).map(FloatOrProvider::Fixed),
        Dyn::Str(_) => read_identifier(d).map(FloatOrProvider::Provider),
        other => DataResult::error(format!(
            "Expected float or provider id, got {}",
            other.kind()
        )),
    }
}

fn encode_float_or_provider(v: &FloatOrProvider) -> Dyn {
    match v {
        FloatOrProvider::Fixed(f) => Dyn::Num(Num::Float(*f)),
        FloatOrProvider::Provider(id) => Dyn::string(id.to_string()),
    }
}

fn decode_rgb_color(d: &Dyn) -> DataResult<i32> {
    if matches!(d, Dyn::Num(_)) {
        return read_int(d);
    }
    if matches!(
        d,
        Dyn::List(_) | Dyn::ByteArray(_) | Dyn::IntArray(_) | Dyn::LongArray(_)
    ) {
        return read_list(d).and_then(|items| {
            if items.len() != 3 {
                return DataResult::error(format!(
                    "Expected 3 floats for RGB colour, got {}",
                    items.len()
                ));
            }
            let mut vals = [0.0f32; 3];
            for (i, item) in items.iter().enumerate() {
                match read_float(item) {
                    DataResult::Success(f) => vals[i] = f,
                    other => return other.map(|_| 0),
                }
            }
            DataResult::Success(rgb_from_floats(vals[0], vals[1], vals[2]))
        });
    }
    DataResult::error(format!(
        "Expected RGB int or 3-float list, got {}",
        d.kind()
    ))
}

fn decode_filterable<T>(
    d: &Dyn,
    inner: impl Fn(&Dyn) -> DataResult<T>,
) -> DataResult<Filterable<T>> {
    // Try bare value first; if that fails and it's a map with `raw`, decode object form.
    // Note: for components, `{raw: ...}` is NOT a valid text component (no content key), so trying
    // bare value first works cleanly for both strings and components!
    if let Dyn::Map(_) = d
        && d.get("raw").is_some()
    {
        let mut r = Rec::new(d);
        let raw = r.required("raw", &inner);
        let filtered = r.optional_opt("filtered", &inner);
        return r.finish(|| {
            Some(Filterable {
                raw: raw?,
                filtered,
            })
        });
    }
    inner(d).map(Filterable::raw)
}

fn encode_filterable<T>(f: &Filterable<T>, enc: impl Fn(&T) -> DataResult<Dyn>) -> DataResult<Dyn> {
    let raw = match enc(&f.raw) {
        DataResult::Success(v) => v,
        other => return other,
    };
    let mut b = MapBuilder::new();
    b.put("raw", raw);
    if let Some(filt) = &f.filtered {
        match enc(filt) {
            DataResult::Success(v) => {
                b.put("filtered", v);
            }
            other => return other,
        }
    }
    DataResult::Success(b.build())
}

// ----------------------------------------------------------------------------------------------------------
// Individual component decoders / encoders
// ----------------------------------------------------------------------------------------------------------

fn decode_rarity(d: &Dyn) -> DataResult<Rarity> {
    read_string(d).and_then(|s| match Rarity::parse(&s) {
        Some(r) => DataResult::Success(r),
        None => DataResult::error(format!("Unknown rarity: {s}")),
    })
}

fn decode_dye(d: &Dyn) -> DataResult<DyeColor> {
    read_string(d).and_then(|s| match DyeColor::parse(&s) {
        Some(c) => DataResult::Success(c),
        None => DataResult::error(format!("Unknown dye colour: {s}")),
    })
}

fn decode_fixed_enum(ty: ComponentTypeId, d: &Dyn) -> DataResult<String> {
    let allowed = valid_fixed_enum_values(ty).expect("fixed enum component type");
    read_string(d).and_then(|s| {
        if allowed.contains(&s.as_str()) {
            DataResult::Success(s)
        } else {
            DataResult::error(format!("Unknown value '{s}' for {ty}"))
        }
    })
}

fn decode_use_effects(d: &Dyn) -> DataResult<UseEffects> {
    let mut r = Rec::new(d);
    let can_sprint = r.optional("can_sprint", false, read_bool);
    let interact_vibrations = r.optional("interact_vibrations", true, read_bool);
    let speed_multiplier = r.optional("speed_multiplier", 0.2, |v| read_float_range(v, 0.0, 1.0));
    r.finish(|| {
        Some(UseEffects {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        })
    })
}

fn encode_use_effects(u: &UseEffects) -> Dyn {
    let mut b = MapBuilder::new();
    if u.can_sprint {
        b.put("can_sprint", Dyn::Bool(true));
    }
    if !u.interact_vibrations {
        b.put("interact_vibrations", Dyn::Bool(false));
    }
    if u.speed_multiplier != 0.2 {
        b.put("speed_multiplier", Dyn::Num(Num::Float(u.speed_multiplier)));
    }
    b.build()
}

fn decode_enchantments(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Enchantments> {
    let entries = match read_map(d) {
        DataResult::Success(m) => m,
        other => return other.map(|_| Enchantments::default()),
    };
    let mut levels = BTreeMap::new();
    let mut errors = Vec::new();
    for (k, v) in entries {
        let id = match k.as_str().map(Identifier::parse) {
            Some(Ok(id)) => id,
            _ => {
                errors.push(format!("Invalid enchantment id: {k}"));
                continue;
            }
        };
        if let Err(m) = check_registry_id(reg_ids::enchantment(), &id, registries) {
            errors.push(m);
            continue;
        }
        match read_int_range(v, 1, 255) {
            DataResult::Success(lvl) => {
                levels.insert(id, lvl);
            }
            DataResult::Error { message, .. } => errors.push(format!("{id}: {message}")),
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    if errors.is_empty() {
        DataResult::Success(Enchantments { levels })
    } else {
        DataResult::error_with_partial(errors.join("; "), Enchantments { levels })
    }
}

fn encode_enchantments(e: &Enchantments) -> Dyn {
    let mut b = MapBuilder::new();
    for (id, lvl) in &e.levels {
        b.put(id.to_string(), Dyn::Num(Num::Int(*lvl)));
    }
    b.build()
}

fn decode_block_predicates(d: &Dyn) -> DataResult<Vec<Dyn>> {
    match d {
        Dyn::Map(_) => DataResult::Success(vec![d.clone()]),
        Dyn::List(_) => decode_sized_list(d, 1, usize::MAX, |v| {
            if v.is_map() {
                DataResult::Success(v.clone())
            } else {
                DataResult::error(format!("Expected block predicate map, got {}", v.kind()))
            }
        }),
        other => DataResult::error(format!(
            "Expected block predicate or non-empty list, got {}",
            other.kind()
        )),
    }
}

fn decode_attribute_modifiers(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<Vec<AttributeModifierEntry>> {
    decode_list_of(d, |entry| {
        let mut r = Rec::new(entry);
        let attribute = r.required("type", |v| {
            read_registry_id(v, reg_ids::attribute(), registries)
        });
        let id = r.required("id", read_identifier);
        let amount = r.required("amount", read_double);
        let operation = r.required("operation", |v| {
            read_string(v).and_then(|s| match AttributeOperation::parse(&s) {
                Some(op) => DataResult::Success(op),
                None => DataResult::error(format!("Unknown attribute operation: {s}")),
            })
        });
        let slot = r.optional("slot", EquipmentSlotGroup::Any, |v| {
            read_string(v).and_then(|s| match EquipmentSlotGroup::parse(&s) {
                Some(sl) => DataResult::Success(sl),
                None => DataResult::error(format!("Unknown slot group: {s}")),
            })
        });
        let display = r.optional("display", AttributeDisplay::Default, |v| {
            decode_attribute_display(v, registries)
        });
        r.finish(|| {
            Some(AttributeModifierEntry {
                attribute: attribute?,
                id: id?,
                amount: amount?,
                operation: operation?,
                slot,
                display,
            })
        })
    })
}

fn decode_attribute_display(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<AttributeDisplay> {
    let mut r = Rec::new(d);
    let Some(ty) = r.required("type", read_string) else {
        return r.finish(|| None);
    };
    let val = match ty.as_str() {
        "default" => Some(AttributeDisplay::Default),
        "hidden" => Some(AttributeDisplay::Hidden),
        "override" => r
            .required("value", |v| decode_component(v, registries))
            .map(|c| AttributeDisplay::Override(Box::new(c))),
        other => {
            r.push_error(format!("Unknown attribute display type: {other}"));
            None
        }
    };
    r.finish(|| val)
}

fn encode_attribute_modifier(m: &AttributeModifierEntry) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    b.put("type", Dyn::string(m.attribute.to_string()));
    b.put("id", Dyn::string(m.id.to_string()));
    b.put("amount", Dyn::Num(Num::Double(m.amount)));
    b.put("operation", Dyn::string(m.operation.as_str()));
    if m.slot != EquipmentSlotGroup::Any {
        b.put("slot", Dyn::string(m.slot.as_str()));
    }
    match &m.display {
        AttributeDisplay::Default => {}
        AttributeDisplay::Hidden => {
            let mut db = MapBuilder::new();
            db.put("type", Dyn::string("hidden"));
            b.put("display", db.build());
        }
        AttributeDisplay::Override(c) => {
            let v = match encode_component(c) {
                DataResult::Success(v) => v,
                other => return other,
            };
            let mut db = MapBuilder::new();
            db.put("value", v);
            db.put("type", Dyn::string("override"));
            b.put("display", db.build());
        }
    }
    DataResult::Success(b.build())
}

fn decode_custom_model_data(d: &Dyn) -> DataResult<CustomModelData> {
    let mut r = Rec::new(d);
    let floats = r.optional("floats", Vec::new(), |v| decode_list_of(v, read_float));
    let flags = r.optional("flags", Vec::new(), |v| decode_list_of(v, read_bool));
    let strings = r.optional("strings", Vec::new(), |v| decode_list_of(v, read_string));
    let colors = r.optional("colors", Vec::new(), |v| {
        decode_list_of(v, decode_rgb_color)
    });
    r.finish(|| {
        Some(CustomModelData {
            floats,
            flags,
            strings,
            colors,
        })
    })
}

fn encode_custom_model_data(cmd: &CustomModelData) -> Dyn {
    let mut b = MapBuilder::new();
    if !cmd.floats.is_empty() {
        b.put(
            "floats",
            Dyn::List(
                cmd.floats
                    .iter()
                    .map(|f| Dyn::Num(Num::Float(*f)))
                    .collect(),
            ),
        );
    }
    if !cmd.flags.is_empty() {
        b.put(
            "flags",
            Dyn::List(cmd.flags.iter().map(|f| Dyn::Bool(*f)).collect()),
        );
    }
    if !cmd.strings.is_empty() {
        b.put(
            "strings",
            Dyn::List(cmd.strings.iter().map(Dyn::string).collect()),
        );
    }
    if !cmd.colors.is_empty() {
        b.put(
            "colors",
            Dyn::List(cmd.colors.iter().map(|c| Dyn::Num(Num::Int(*c))).collect()),
        );
    }
    b.build()
}

fn decode_tooltip_display(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<TooltipDisplay> {
    let mut r = Rec::new(d);
    let hide_tooltip = r.optional("hide_tooltip", false, read_bool);
    let hidden_components = r.optional("hidden_components", Vec::new(), |v| {
        decode_list_of(v, |iv| {
            read_registry_id(iv, reg_ids::data_component_type(), registries)
        })
    });
    r.finish(|| {
        Some(TooltipDisplay {
            hide_tooltip,
            hidden_components,
        })
    })
}

fn encode_tooltip_display(td: &TooltipDisplay) -> Dyn {
    let mut b = MapBuilder::new();
    if td.hide_tooltip {
        b.put("hide_tooltip", Dyn::Bool(true));
    }
    if !td.hidden_components.is_empty() {
        b.put(
            "hidden_components",
            Dyn::List(
                td.hidden_components
                    .iter()
                    .map(|id| Dyn::string(id.to_string()))
                    .collect(),
            ),
        );
    }
    b.build()
}

fn decode_food(d: &Dyn) -> DataResult<FoodProperties> {
    let mut r = Rec::new(d);
    let nutrition = r.required("nutrition", |v| read_int_min(v, 0));
    let saturation = r.required("saturation", read_float);
    let can_always_eat = r.optional("can_always_eat", false, read_bool);
    r.finish(|| {
        Some(FoodProperties {
            nutrition: nutrition?,
            saturation: saturation?,
            can_always_eat,
        })
    })
}

fn encode_food(f: &FoodProperties) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("nutrition", Dyn::Num(Num::Int(f.nutrition)));
    b.put("saturation", Dyn::Num(Num::Float(f.saturation)));
    if f.can_always_eat {
        b.put("can_always_eat", Dyn::Bool(true));
    }
    b.build()
}

fn decode_mob_effect(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<MobEffectInstance> {
    let mut r = Rec::new(d);
    let id = r.required("id", |v| {
        read_registry_id(v, reg_ids::mob_effect(), registries)
    });
    let amplifier = r.optional("amplifier", 0u8, |v| {
        read_int_range(v, 0, 255).map(|n| n as u8)
    });
    let duration = r.optional("duration", 0, read_int);
    let ambient = r.optional("ambient", false, read_bool);
    let show_particles = r.optional("show_particles", true, read_bool);
    let show_icon = r.optional_opt("show_icon", read_bool);
    let hidden_effect = r
        .optional_opt("hidden_effect", |v| decode_mob_effect(v, registries))
        .map(Box::new);
    r.finish(|| {
        Some(MobEffectInstance {
            id: id?,
            amplifier,
            duration,
            ambient,
            show_particles,
            show_icon,
            hidden_effect,
        })
    })
}

fn encode_mob_effect(e: &MobEffectInstance) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("id", Dyn::string(e.id.to_string()));
    if e.amplifier != 0 {
        b.put("amplifier", Dyn::Num(Num::Int(i32::from(e.amplifier))));
    }
    if e.duration != 0 {
        b.put("duration", Dyn::Num(Num::Int(e.duration)));
    }
    if e.ambient {
        b.put("ambient", Dyn::Bool(true));
    }
    if !e.show_particles {
        b.put("show_particles", Dyn::Bool(false));
    }
    if let Some(si) = e.show_icon {
        b.put("show_icon", Dyn::Bool(si));
    }
    if let Some(h) = &e.hidden_effect {
        b.put("hidden_effect", encode_mob_effect(h));
    }
    b.build()
}

fn decode_consume_effect(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<ConsumeEffect> {
    let mut r = Rec::new(d);
    let Some(ty) = r.required("type", read_identifier) else {
        return r.finish(|| None);
    };
    let eff = match ty.to_string().as_str() {
        "minecraft:apply_effects" => {
            let effects = r.required("effects", |v| {
                if v.is_map() {
                    decode_mob_effect(v, registries).map(|e| vec![e])
                } else {
                    decode_list_of(v, |ev| decode_mob_effect(ev, registries))
                }
            });
            let probability = r.optional("probability", 1.0, |v| read_float_range(v, 0.0, 1.0));
            effects.map(|effects| ConsumeEffect::ApplyEffects {
                effects,
                probability,
            })
        }
        "minecraft:remove_effects" => r
            .required("effects", |v| {
                decode_holder_set(v, reg_ids::mob_effect(), registries)
            })
            .map(|effects| ConsumeEffect::RemoveEffects { effects }),
        "minecraft:clear_all_effects" => Some(ConsumeEffect::ClearAllEffects),
        "minecraft:teleport_randomly" => {
            let diameter = r.optional("diameter", 16.0, |v| read_float_gt(v, 0.0));
            Some(ConsumeEffect::TeleportRandomly { diameter })
        }
        "minecraft:play_sound" => r
            .required("sound", |v| {
                decode_holder_ref(v, reg_ids::sound_event(), registries)
            })
            .map(|sound| ConsumeEffect::PlaySound { sound }),
        other => {
            r.push_error(format!("Unknown consume effect type: {other}"));
            None
        }
    };
    r.finish(|| eff)
}

fn encode_consume_effect(e: &ConsumeEffect) -> Dyn {
    let mut b = MapBuilder::new();
    match e {
        ConsumeEffect::ApplyEffects {
            effects,
            probability,
        } => {
            b.put(
                "effects",
                Dyn::List(effects.iter().map(encode_mob_effect).collect()),
            );
            if *probability != 1.0 {
                b.put("probability", Dyn::Num(Num::Float(*probability)));
            }
            b.put("type", Dyn::string("minecraft:apply_effects"));
        }
        ConsumeEffect::RemoveEffects { effects } => {
            b.put("effects", encode_holder_set(effects));
            b.put("type", Dyn::string("minecraft:remove_effects"));
        }
        ConsumeEffect::ClearAllEffects => {
            b.put("type", Dyn::string("minecraft:clear_all_effects"));
        }
        ConsumeEffect::TeleportRandomly { diameter } => {
            if *diameter != 16.0 {
                b.put("diameter", Dyn::Num(Num::Float(*diameter)));
            }
            b.put("type", Dyn::string("minecraft:teleport_randomly"));
        }
        ConsumeEffect::PlaySound { sound } => {
            b.put("sound", encode_holder_ref(sound));
            b.put("type", Dyn::string("minecraft:play_sound"));
        }
    }
    b.build()
}

fn decode_consumable(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Consumable> {
    let def = Consumable::default();
    let mut r = Rec::new(d);
    let consume_seconds = r.optional("consume_seconds", def.consume_seconds, |v| {
        read_float_min(v, 0.0)
    });
    let animation = r.optional("animation", def.animation, |v| {
        read_string(v).and_then(|s| match ConsumeAnimation::parse(&s) {
            Some(a) => DataResult::Success(a),
            None => DataResult::error(format!("Unknown consume animation: {s}")),
        })
    });
    let sound = r.optional("sound", def.sound, |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    let has_consume_particles = r.optional("has_consume_particles", true, read_bool);
    let on_consume_effects = r.optional("on_consume_effects", Vec::new(), |v| {
        decode_list_of(v, |ev| decode_consume_effect(ev, registries))
    });
    r.finish(|| {
        Some(Consumable {
            consume_seconds,
            animation,
            sound,
            has_consume_particles,
            on_consume_effects,
        })
    })
}

fn encode_consumable(c: &Consumable) -> Dyn {
    let def = Consumable::default();
    let mut b = MapBuilder::new();
    if c.consume_seconds != def.consume_seconds {
        b.put("consume_seconds", Dyn::Num(Num::Float(c.consume_seconds)));
    }
    if c.animation != def.animation {
        b.put("animation", Dyn::string(c.animation.as_str()));
    }
    if c.sound != def.sound {
        b.put("sound", encode_holder_ref(&c.sound));
    }
    if c.has_consume_particles != def.has_consume_particles {
        b.put("has_consume_particles", Dyn::Bool(c.has_consume_particles));
    }
    if !c.on_consume_effects.is_empty() {
        b.put(
            "on_consume_effects",
            Dyn::List(
                c.on_consume_effects
                    .iter()
                    .map(encode_consume_effect)
                    .collect(),
            ),
        );
    }
    b.build()
}

fn decode_use_cooldown(d: &Dyn) -> DataResult<UseCooldown> {
    let mut r = Rec::new(d);
    let seconds = r.required("seconds", |v| read_float_gt(v, 0.0));
    let cooldown_group = r.optional_opt("cooldown_group", read_identifier);
    r.finish(|| {
        Some(UseCooldown {
            seconds: seconds?,
            cooldown_group,
        })
    })
}

fn encode_use_cooldown(uc: &UseCooldown) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("seconds", Dyn::Num(Num::Float(uc.seconds)));
    if let Some(g) = &uc.cooldown_group {
        b.put("cooldown_group", Dyn::string(g.to_string()));
    }
    b.build()
}

fn decode_damage_resistant(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<HolderSetRef> {
    let mut r = Rec::new(d);
    let types = r.required("types", |v| {
        decode_holder_set(v, reg_ids::damage_type(), registries)
    });
    r.finish(|| types)
}

fn decode_tool(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<ToolProperties> {
    let mut r = Rec::new(d);
    let rules = r.required("rules", |v| {
        decode_list_of(v, |rule| {
            let mut rr = Rec::new(rule);
            let blocks = rr.required("blocks", |bv| {
                decode_holder_set(bv, reg_ids::block(), registries)
            });
            let speed = rr.optional_opt("speed", |sv| read_float_gt(sv, 0.0));
            let correct_for_drops = rr.optional_opt("correct_for_drops", read_bool);
            rr.finish(|| {
                Some(ToolRule {
                    blocks: blocks?,
                    speed,
                    correct_for_drops,
                })
            })
        })
    });
    let default_mining_speed = r.optional("default_mining_speed", 1.0, read_float);
    let damage_per_block = r.optional("damage_per_block", 1, |v| read_int_min(v, 0));
    let can_destroy_blocks_in_creative =
        r.optional("can_destroy_blocks_in_creative", true, read_bool);
    r.finish(|| {
        Some(ToolProperties {
            rules: rules?,
            default_mining_speed,
            damage_per_block,
            can_destroy_blocks_in_creative,
        })
    })
}

fn encode_tool(t: &ToolProperties) -> Dyn {
    let mut b = MapBuilder::new();
    let rules = t
        .rules
        .iter()
        .map(|r| {
            let mut rb = MapBuilder::new();
            rb.put("blocks", encode_holder_set(&r.blocks));
            if let Some(s) = r.speed {
                rb.put("speed", Dyn::Num(Num::Float(s)));
            }
            if let Some(c) = r.correct_for_drops {
                rb.put("correct_for_drops", Dyn::Bool(c));
            }
            rb.build()
        })
        .collect();
    b.put("rules", Dyn::List(rules));
    if t.default_mining_speed != 1.0 {
        b.put(
            "default_mining_speed",
            Dyn::Num(Num::Float(t.default_mining_speed)),
        );
    }
    if t.damage_per_block != 1 {
        b.put("damage_per_block", Dyn::Num(Num::Int(t.damage_per_block)));
    }
    if !t.can_destroy_blocks_in_creative {
        b.put("can_destroy_blocks_in_creative", Dyn::Bool(false));
    }
    b.build()
}

fn decode_weapon(d: &Dyn) -> DataResult<WeaponProperties> {
    let mut r = Rec::new(d);
    let item_damage_per_attack = r.optional("item_damage_per_attack", 1, |v| read_int_min(v, 0));
    let disable_blocking_for_seconds = r.optional("disable_blocking_for_seconds", 0.0, |v| {
        read_float_min(v, 0.0)
    });
    r.finish(|| {
        Some(WeaponProperties {
            item_damage_per_attack,
            disable_blocking_for_seconds,
        })
    })
}

fn encode_weapon(w: &WeaponProperties) -> Dyn {
    let mut b = MapBuilder::new();
    if w.item_damage_per_attack != 1 {
        b.put(
            "item_damage_per_attack",
            Dyn::Num(Num::Int(w.item_damage_per_attack)),
        );
    }
    if w.disable_blocking_for_seconds != 0.0 {
        b.put(
            "disable_blocking_for_seconds",
            Dyn::Num(Num::Float(w.disable_blocking_for_seconds)),
        );
    }
    b.build()
}

fn decode_attack_range(d: &Dyn) -> DataResult<AttackRange> {
    let def = AttackRange::default();
    let mut r = Rec::new(d);
    let min_reach = r.optional("min_reach", def.min_reach, |v| {
        read_float_range(v, 0.0, 64.0)
    });
    let max_reach = r.optional("max_reach", def.max_reach, |v| {
        read_float_range(v, 0.0, 64.0)
    });
    let min_creative_reach = r.optional("min_creative_reach", def.min_creative_reach, |v| {
        read_float_range(v, 0.0, 64.0)
    });
    let max_creative_reach = r.optional("max_creative_reach", def.max_creative_reach, |v| {
        read_float_range(v, 0.0, 64.0)
    });
    let hitbox_margin = r.optional("hitbox_margin", def.hitbox_margin, |v| {
        read_float_range(v, 0.0, 1.0)
    });
    let mob_factor = r.optional("mob_factor", def.mob_factor, |v| {
        read_float_range(v, 0.0, 2.0)
    });
    r.finish(|| {
        Some(AttackRange {
            min_reach,
            max_reach,
            min_creative_reach,
            max_creative_reach,
            hitbox_margin,
            mob_factor,
        })
    })
}

fn encode_attack_range(a: &AttackRange) -> Dyn {
    let def = AttackRange::default();
    let mut b = MapBuilder::new();
    if a.min_reach != def.min_reach {
        b.put("min_reach", Dyn::Num(Num::Float(a.min_reach)));
    }
    if a.max_reach != def.max_reach {
        b.put("max_reach", Dyn::Num(Num::Float(a.max_reach)));
    }
    if a.min_creative_reach != def.min_creative_reach {
        b.put(
            "min_creative_reach",
            Dyn::Num(Num::Float(a.min_creative_reach)),
        );
    }
    if a.max_creative_reach != def.max_creative_reach {
        b.put(
            "max_creative_reach",
            Dyn::Num(Num::Float(a.max_creative_reach)),
        );
    }
    if a.hitbox_margin != def.hitbox_margin {
        b.put("hitbox_margin", Dyn::Num(Num::Float(a.hitbox_margin)));
    }
    if a.mob_factor != def.mob_factor {
        b.put("mob_factor", Dyn::Num(Num::Float(a.mob_factor)));
    }
    b.build()
}

fn decode_enchantable(d: &Dyn) -> DataResult<i32> {
    let mut r = Rec::new(d);
    let value = r.required("value", |v| read_int_min(v, 1));
    r.finish(|| value)
}

fn default_equip_sound() -> HolderRef {
    HolderRef::Id(Identifier::parse("minecraft:item.armor.equip_generic").expect("valid constant"))
}

fn default_shearing_sound() -> HolderRef {
    HolderRef::Id(Identifier::parse("minecraft:item.shears.snip").expect("valid constant"))
}

fn decode_equippable(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Equippable> {
    let mut r = Rec::new(d);
    let slot = r.required("slot", |v| {
        read_string(v).and_then(|s| match EquipmentSlot::parse(&s) {
            Some(sl) => DataResult::Success(sl),
            None => DataResult::error(format!("Unknown equipment slot: {s}")),
        })
    });
    let equip_sound = r.optional("equip_sound", default_equip_sound(), |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    let asset_id = r.optional_opt("asset_id", read_identifier);
    let camera_overlay = r.optional_opt("camera_overlay", read_identifier);
    let allowed_entities = r.optional_opt("allowed_entities", |v| {
        decode_holder_set(v, reg_ids::entity_type(), registries)
    });
    let dispensable = r.optional("dispensable", true, read_bool);
    let swappable = r.optional("swappable", true, read_bool);
    let damage_on_hurt = r.optional("damage_on_hurt", true, read_bool);
    let equip_on_interact = r.optional("equip_on_interact", false, read_bool);
    let can_be_sheared = r.optional("can_be_sheared", false, read_bool);
    let shearing_sound = r.optional("shearing_sound", default_shearing_sound(), |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    r.finish(|| {
        Some(Equippable {
            slot: slot?,
            equip_sound,
            asset_id,
            camera_overlay,
            allowed_entities,
            dispensable,
            swappable,
            damage_on_hurt,
            equip_on_interact,
            can_be_sheared,
            shearing_sound,
        })
    })
}

fn encode_equippable(e: &Equippable) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("slot", Dyn::string(e.slot.as_str()));
    if e.equip_sound != default_equip_sound() {
        b.put("equip_sound", encode_holder_ref(&e.equip_sound));
    }
    if let Some(a) = &e.asset_id {
        b.put("asset_id", Dyn::string(a.to_string()));
    }
    if let Some(c) = &e.camera_overlay {
        b.put("camera_overlay", Dyn::string(c.to_string()));
    }
    if let Some(ae) = &e.allowed_entities {
        b.put("allowed_entities", encode_holder_set(ae));
    }
    if !e.dispensable {
        b.put("dispensable", Dyn::Bool(false));
    }
    if !e.swappable {
        b.put("swappable", Dyn::Bool(false));
    }
    if !e.damage_on_hurt {
        b.put("damage_on_hurt", Dyn::Bool(false));
    }
    if e.equip_on_interact {
        b.put("equip_on_interact", Dyn::Bool(true));
    }
    if e.can_be_sheared {
        b.put("can_be_sheared", Dyn::Bool(true));
    }
    if e.shearing_sound != default_shearing_sound() {
        b.put("shearing_sound", encode_holder_ref(&e.shearing_sound));
    }
    b.build()
}

fn decode_repairable(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<HolderSetRef> {
    let mut r = Rec::new(d);
    let items = r.required("items", |v| {
        decode_holder_set(v, reg_ids::item(), registries)
    });
    r.finish(|| items)
}

fn decode_death_protection(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<Vec<ConsumeEffect>> {
    let mut r = Rec::new(d);
    let effects = r.optional("death_effects", Vec::new(), |v| {
        decode_list_of(v, |ev| decode_consume_effect(ev, registries))
    });
    r.finish(|| Some(effects))
}

fn decode_blocks_attacks(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<BlocksAttacks> {
    let def = BlocksAttacks::default();
    let mut r = Rec::new(d);
    let block_delay_seconds = r.optional("block_delay_seconds", def.block_delay_seconds, |v| {
        read_float_min(v, 0.0)
    });
    let disable_cooldown_scale =
        r.optional("disable_cooldown_scale", def.disable_cooldown_scale, |v| {
            read_float_min(v, 0.0)
        });
    let damage_reductions = r.optional("damage_reductions", def.damage_reductions, |v| {
        decode_list_of(v, |dr| {
            let mut rr = Rec::new(dr);
            let horizontal_blocking_angle = rr.optional("horizontal_blocking_angle", 90.0, |fv| {
                read_float_gt(fv, 0.0)
            });
            let damage_type = rr.optional_opt("type", |dv| {
                decode_holder_set(dv, reg_ids::damage_type(), registries)
            });
            let base = rr.required("base", read_float);
            let factor = rr.required("factor", read_float);
            rr.finish(|| {
                Some(DamageReduction {
                    horizontal_blocking_angle,
                    damage_type,
                    base: base?,
                    factor: factor?,
                })
            })
        })
    });
    let item_damage = r.optional("item_damage", def.item_damage, |v| {
        let mut ir = Rec::new(v);
        let threshold = ir.required("threshold", |fv| read_float_min(fv, 0.0));
        let base = ir.required("base", read_float);
        let factor = ir.required("factor", read_float);
        ir.finish(|| {
            Some(ItemDamageFunction {
                threshold: threshold?,
                base: base?,
                factor: factor?,
            })
        })
    });
    let bypassed_by = r.optional_opt("bypassed_by", |v| {
        decode_holder_set(v, reg_ids::damage_type(), registries)
    });
    let block_sound = r.optional_opt("block_sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    let disabled_sound = r.optional_opt("disabled_sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    r.finish(|| {
        Some(BlocksAttacks {
            block_delay_seconds,
            disable_cooldown_scale,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        })
    })
}

fn encode_blocks_attacks(ba: &BlocksAttacks) -> Dyn {
    let def = BlocksAttacks::default();
    let mut b = MapBuilder::new();
    if ba.block_delay_seconds != def.block_delay_seconds {
        b.put(
            "block_delay_seconds",
            Dyn::Num(Num::Float(ba.block_delay_seconds)),
        );
    }
    if ba.disable_cooldown_scale != def.disable_cooldown_scale {
        b.put(
            "disable_cooldown_scale",
            Dyn::Num(Num::Float(ba.disable_cooldown_scale)),
        );
    }
    if ba.damage_reductions != def.damage_reductions {
        let drs = ba
            .damage_reductions
            .iter()
            .map(|dr| {
                let mut db = MapBuilder::new();
                if dr.horizontal_blocking_angle != 90.0 {
                    db.put(
                        "horizontal_blocking_angle",
                        Dyn::Num(Num::Float(dr.horizontal_blocking_angle)),
                    );
                }
                if let Some(dt) = &dr.damage_type {
                    db.put("type", encode_holder_set(dt));
                }
                db.put("base", Dyn::Num(Num::Float(dr.base)));
                db.put("factor", Dyn::Num(Num::Float(dr.factor)));
                db.build()
            })
            .collect();
        b.put("damage_reductions", Dyn::List(drs));
    }
    if ba.item_damage != def.item_damage {
        let mut ib = MapBuilder::new();
        ib.put("threshold", Dyn::Num(Num::Float(ba.item_damage.threshold)));
        ib.put("base", Dyn::Num(Num::Float(ba.item_damage.base)));
        ib.put("factor", Dyn::Num(Num::Float(ba.item_damage.factor)));
        b.put("item_damage", ib.build());
    }
    if let Some(bp) = &ba.bypassed_by {
        b.put("bypassed_by", encode_holder_set(bp));
    }
    if let Some(s) = &ba.block_sound {
        b.put("block_sound", encode_holder_ref(s));
    }
    if let Some(s) = &ba.disabled_sound {
        b.put("disabled_sound", encode_holder_ref(s));
    }
    b.build()
}

fn decode_piercing_weapon(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<PiercingWeapon> {
    let mut r = Rec::new(d);
    let deals_knockback = r.optional("deals_knockback", true, read_bool);
    let dismounts = r.optional("dismounts", false, read_bool);
    let sound = r.optional_opt("sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    let hit_sound = r.optional_opt("hit_sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    r.finish(|| {
        Some(PiercingWeapon {
            deals_knockback,
            dismounts,
            sound,
            hit_sound,
        })
    })
}

fn encode_piercing_weapon(pw: &PiercingWeapon) -> Dyn {
    let mut b = MapBuilder::new();
    if !pw.deals_knockback {
        b.put("deals_knockback", Dyn::Bool(false));
    }
    if pw.dismounts {
        b.put("dismounts", Dyn::Bool(true));
    }
    if let Some(s) = &pw.sound {
        b.put("sound", encode_holder_ref(s));
    }
    if let Some(s) = &pw.hit_sound {
        b.put("hit_sound", encode_holder_ref(s));
    }
    b.build()
}

fn decode_kinetic_condition(d: &Dyn) -> DataResult<KineticCondition> {
    let mut r = Rec::new(d);
    let max_duration_ticks = r.required("max_duration_ticks", |v| read_int_min(v, 0));
    let min_speed = r.optional("min_speed", 0.0, read_float);
    let min_relative_speed = r.optional("min_relative_speed", 0.0, read_float);
    r.finish(|| {
        Some(KineticCondition {
            max_duration_ticks: max_duration_ticks?,
            min_speed,
            min_relative_speed,
        })
    })
}

fn encode_kinetic_condition(kc: &KineticCondition) -> Dyn {
    let mut b = MapBuilder::new();
    b.put(
        "max_duration_ticks",
        Dyn::Num(Num::Int(kc.max_duration_ticks)),
    );
    if kc.min_speed != 0.0 {
        b.put("min_speed", Dyn::Num(Num::Float(kc.min_speed)));
    }
    if kc.min_relative_speed != 0.0 {
        b.put(
            "min_relative_speed",
            Dyn::Num(Num::Float(kc.min_relative_speed)),
        );
    }
    b.build()
}

fn decode_kinetic_weapon(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<KineticWeapon> {
    let def = KineticWeapon::default();
    let mut r = Rec::new(d);
    let contact_cooldown_ticks =
        r.optional("contact_cooldown_ticks", def.contact_cooldown_ticks, |v| {
            read_int_min(v, 0)
        });
    let delay_ticks = r.optional("delay_ticks", def.delay_ticks, |v| read_int_min(v, 0));
    let dismount_conditions = r.optional_opt("dismount_conditions", decode_kinetic_condition);
    let knockback_conditions = r.optional_opt("knockback_conditions", decode_kinetic_condition);
    let damage_conditions = r.optional_opt("damage_conditions", decode_kinetic_condition);
    let forward_movement = r.optional("forward_movement", def.forward_movement, read_float);
    let damage_multiplier = r.optional("damage_multiplier", def.damage_multiplier, read_float);
    let sound = r.optional_opt("sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    let hit_sound = r.optional_opt("hit_sound", |v| {
        decode_holder_ref(v, reg_ids::sound_event(), registries)
    });
    r.finish(|| {
        Some(KineticWeapon {
            contact_cooldown_ticks,
            delay_ticks,
            dismount_conditions,
            knockback_conditions,
            damage_conditions,
            forward_movement,
            damage_multiplier,
            sound,
            hit_sound,
        })
    })
}

fn encode_kinetic_weapon(kw: &KineticWeapon) -> Dyn {
    let def = KineticWeapon::default();
    let mut b = MapBuilder::new();
    if kw.contact_cooldown_ticks != def.contact_cooldown_ticks {
        b.put(
            "contact_cooldown_ticks",
            Dyn::Num(Num::Int(kw.contact_cooldown_ticks)),
        );
    }
    if kw.delay_ticks != def.delay_ticks {
        b.put("delay_ticks", Dyn::Num(Num::Int(kw.delay_ticks)));
    }
    if let Some(c) = &kw.dismount_conditions {
        b.put("dismount_conditions", encode_kinetic_condition(c));
    }
    if let Some(c) = &kw.knockback_conditions {
        b.put("knockback_conditions", encode_kinetic_condition(c));
    }
    if let Some(c) = &kw.damage_conditions {
        b.put("damage_conditions", encode_kinetic_condition(c));
    }
    if kw.forward_movement != def.forward_movement {
        b.put(
            "forward_movement",
            Dyn::Num(Num::Float(kw.forward_movement)),
        );
    }
    if kw.damage_multiplier != def.damage_multiplier {
        b.put(
            "damage_multiplier",
            Dyn::Num(Num::Float(kw.damage_multiplier)),
        );
    }
    if let Some(s) = &kw.sound {
        b.put("sound", encode_holder_ref(s));
    }
    if let Some(s) = &kw.hit_sound {
        b.put("hit_sound", encode_holder_ref(s));
    }
    b.build()
}

fn decode_swing_animation(d: &Dyn) -> DataResult<SwingAnimation> {
    let def = SwingAnimation::default();
    let mut r = Rec::new(d);
    let kind = r.optional("type", def.kind, |v| {
        read_string(v).and_then(|s| match SwingAnimationType::parse(&s) {
            Some(k) => DataResult::Success(k),
            None => DataResult::error(format!("Unknown animation type: {s}")),
        })
    });
    let duration = r.optional("duration", def.duration, |v| read_int_min(v, 0));
    r.finish(|| Some(SwingAnimation { kind, duration }))
}

fn encode_swing_animation(sa: &SwingAnimation) -> Dyn {
    let def = SwingAnimation::default();
    let mut b = MapBuilder::new();
    if sa.kind != def.kind {
        b.put("type", Dyn::string(sa.kind.as_str()));
    }
    if sa.duration != def.duration {
        b.put("duration", Dyn::Num(Num::Int(sa.duration)));
    }
    b.build()
}

fn decode_villager_food(d: &Dyn) -> DataResult<i32> {
    let mut r = Rec::new(d);
    let nutrition = r.required("nutrition", |v| read_int_min(v, 1));
    r.finish(|| nutrition)
}

fn decode_map_decorations(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<BTreeMap<String, MapDecoration>> {
    let entries = match read_map(d) {
        DataResult::Success(m) => m,
        other => return other.map(|_| BTreeMap::new()),
    };
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    for (k, v) in entries {
        let mut r = Rec::new(v);
        let decoration_type = r.required("type", |tv| {
            read_registry_id(tv, reg_ids::map_decoration_type(), registries)
        });
        let x = r.required("x", read_double);
        let z = r.required("z", read_double);
        let rotation = r.required("rotation", read_float);
        match r.finish(|| {
            Some(MapDecoration {
                decoration_type: decoration_type?,
                x: x?,
                z: z?,
                rotation: rotation?,
            })
        }) {
            DataResult::Success(md) => {
                out.insert(k.to_string_lossy().into_owned(), md);
            }
            DataResult::Error { message, .. } => errors.push(format!("{k}: {message}")),
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    if errors.is_empty() {
        DataResult::Success(out)
    } else {
        DataResult::error_with_partial(errors.join("; "), out)
    }
}

fn encode_map_decorations(m: &BTreeMap<String, MapDecoration>) -> Dyn {
    let mut b = MapBuilder::new();
    for (k, md) in m {
        let mut eb = MapBuilder::new();
        eb.put("type", Dyn::string(md.decoration_type.to_string()));
        eb.put("x", Dyn::Num(Num::Double(md.x)));
        eb.put("z", Dyn::Num(Num::Double(md.z)));
        eb.put("rotation", Dyn::Num(Num::Float(md.rotation)));
        b.put(k.as_str(), eb.build());
    }
    b.build()
}

fn decode_potion_contents(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<PotionContents> {
    // Spec B.7 & edge case 44: bare potion id string OR object; encodes as object.
    if matches!(d, Dyn::Str(_)) {
        return read_registry_id(d, reg_ids::potion(), registries).map(|id| PotionContents {
            potion: Some(id),
            ..PotionContents::default()
        });
    }
    let mut r = Rec::new(d);
    let potion = r.optional_opt("potion", |v| {
        read_registry_id(v, reg_ids::potion(), registries)
    });
    let custom_color = r.optional_opt("custom_color", read_int);
    let custom_effects = r.optional("custom_effects", Vec::new(), |v| {
        decode_list_of(v, |ev| decode_mob_effect(ev, registries))
    });
    let custom_name = r.optional_opt("custom_name", read_string);
    r.finish(|| {
        Some(PotionContents {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        })
    })
}

fn encode_potion_contents(pc: &PotionContents) -> Dyn {
    let mut b = MapBuilder::new();
    if let Some(p) = &pc.potion {
        b.put("potion", Dyn::string(p.to_string()));
    }
    if let Some(c) = pc.custom_color {
        b.put("custom_color", Dyn::Num(Num::Int(c)));
    }
    if !pc.custom_effects.is_empty() {
        b.put(
            "custom_effects",
            Dyn::List(pc.custom_effects.iter().map(encode_mob_effect).collect()),
        );
    }
    if let Some(n) = &pc.custom_name {
        b.put("custom_name", Dyn::string(n));
    }
    b.build()
}

fn decode_suspicious_stew_effect(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<SuspiciousStewEffect> {
    let mut r = Rec::new(d);
    let id = r.required("id", |v| {
        read_registry_id(v, reg_ids::mob_effect(), registries)
    });
    let duration = r.lenient("duration", 160, read_int);
    r.finish(|| Some(SuspiciousStewEffect { id: id?, duration }))
}

fn encode_suspicious_stew_effect(e: &SuspiciousStewEffect) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("id", Dyn::string(e.id.to_string()));
    if e.duration != 160 {
        b.put("duration", Dyn::Num(Num::Int(e.duration)));
    }
    b.build()
}

fn decode_writable_book_content(d: &Dyn) -> DataResult<WritableBookContent> {
    let mut r = Rec::new(d);
    let pages = r.optional("pages", Vec::new(), |v| {
        decode_sized_list(v, 0, 100, |p| {
            decode_filterable(p, |s| {
                read_string(s).and_then(|txt| {
                    if txt.encode_utf16().count() <= 1024 {
                        DataResult::Success(txt)
                    } else {
                        DataResult::error("Writable book page exceeds 1024 characters")
                    }
                })
            })
        })
    });
    r.finish(|| Some(WritableBookContent { pages }))
}

fn encode_writable_book_content(wbc: &WritableBookContent) -> Dyn {
    let mut b = MapBuilder::new();
    if !wbc.pages.is_empty() {
        let pages = wbc
            .pages
            .iter()
            .map(|p| {
                encode_filterable(p, |s| DataResult::Success(Dyn::string(s)))
                    .success()
                    .expect("string encoding succeeds")
            })
            .collect();
        b.put("pages", Dyn::List(pages));
    }
    b.build()
}

fn decode_written_book_content(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<WrittenBookContent> {
    let mut r = Rec::new(d);
    let title = r.required("title", |v| {
        decode_filterable(v, |s| {
            read_string(s).and_then(|txt| {
                if txt.encode_utf16().count() <= 32 {
                    DataResult::Success(txt)
                } else {
                    DataResult::error("Written book title exceeds 32 characters")
                }
            })
        })
    });
    let author = r.required("author", read_string);
    let generation = r.optional("generation", 0, |v| read_int_range(v, 0, 3));
    let pages = r.optional("pages", Vec::new(), |v| {
        decode_list_of(v, |p| {
            decode_filterable(p, |pv| decode_book_page(pv, registries))
        })
    });
    let resolved = r.optional("resolved", false, read_bool);
    r.finish(|| {
        Some(WrittenBookContent {
            title: title?,
            author: author?,
            generation,
            pages,
            resolved,
        })
    })
}

fn encode_written_book_content(wbc: &WrittenBookContent) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    let title = match encode_filterable(&wbc.title, |s| DataResult::Success(Dyn::string(s))) {
        DataResult::Success(v) => v,
        other => return other,
    };
    b.put("title", title);
    b.put("author", Dyn::string(&wbc.author));
    if wbc.generation != 0 {
        b.put("generation", Dyn::Num(Num::Int(wbc.generation)));
    }
    if !wbc.pages.is_empty() {
        let mut pages = Vec::with_capacity(wbc.pages.len());
        for p in &wbc.pages {
            match encode_filterable(p, encode_component) {
                DataResult::Success(v) => pages.push(v),
                other => return other,
            }
        }
        b.put("pages", Dyn::List(pages));
    }
    if wbc.resolved {
        b.put("resolved", Dyn::Bool(true));
    }
    DataResult::Success(b.build())
}

fn decode_trim(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<ArmorTrim> {
    let mut r = Rec::new(d);
    let material = r.required("material", |v| {
        decode_holder_ref(v, reg_ids::trim_material(), registries)
    });
    let pattern = r.required("pattern", |v| {
        decode_holder_ref(v, reg_ids::trim_pattern(), registries)
    });
    r.finish(|| {
        Some(ArmorTrim {
            material: material?,
            pattern: pattern?,
        })
    })
}

fn decode_debug_stick_state(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<BTreeMap<Identifier, String>> {
    let entries = match read_map(d) {
        DataResult::Success(m) => m,
        other => return other.map(|_| BTreeMap::new()),
    };
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    for (k, v) in entries {
        let block_id = match k.as_str().map(Identifier::parse) {
            Some(Ok(id)) => id,
            _ => {
                errors.push(format!("Invalid block id: {k}"));
                continue;
            }
        };
        if let Err(m) = check_registry_id(reg_ids::block(), &block_id, registries) {
            errors.push(m);
            continue;
        }
        match read_string(v) {
            DataResult::Success(prop) => {
                if !zenith_registry::is_valid_property_name(&prop) {
                    errors.push(format!("{block_id}: invalid property name '{prop}'"));
                } else {
                    out.insert(block_id, prop);
                }
            }
            DataResult::Error { message, .. } => errors.push(format!("{block_id}: {message}")),
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    if errors.is_empty() {
        DataResult::Success(out)
    } else {
        DataResult::error_with_partial(errors.join("; "), out)
    }
}

fn decode_typed_entity_data(
    d: &Dyn,
    registry: &Identifier,
    registries: &dyn RegistryContext,
) -> DataResult<TypedEntityData> {
    decode_compound_or_snbt(d).and_then(|mut c| {
        let Some(Tag::String(raw_id)) = c.remove("id") else {
            return DataResult::error("Entity/block-entity data must contain a string 'id'");
        };
        let id_str = raw_id.to_string_lossy();
        match Identifier::parse(&id_str) {
            Ok(id) => match check_registry_id(registry, &id, registries) {
                Ok(()) => DataResult::Success(TypedEntityData { id, payload: c }),
                Err(m) => DataResult::error(m),
            },
            Err(_) => DataResult::error(format!("Invalid entity/block-entity id: {id_str}")),
        }
    })
}

fn encode_typed_entity_data(ted: &TypedEntityData) -> Dyn {
    let mut c = ted.payload.clone();
    c.insert("id", Tag::String(ted.id.to_string().into()));
    Dyn::from_tag(&Tag::Compound(c))
}

fn decode_lodestone_tracker(d: &Dyn) -> DataResult<LodestoneTracker> {
    let mut r = Rec::new(d);
    let target = r.optional_opt("target", |v| {
        let mut tr = Rec::new(v);
        let dimension = tr.required("dimension", read_identifier);
        let pos = tr.required("pos", |pv| {
            read_list(pv).and_then(|items| {
                if items.len() != 3 {
                    return DataResult::error(format!(
                        "Expected 3 ints for block pos, got {}",
                        items.len()
                    ));
                }
                let mut arr = [0; 3];
                for (i, it) in items.iter().enumerate() {
                    match read_int(it) {
                        DataResult::Success(n) => arr[i] = n,
                        other => return other.map(|_| [0; 3]),
                    }
                }
                DataResult::Success(arr)
            })
        });
        tr.finish(|| {
            Some(GlobalPos {
                dimension: dimension?,
                pos: pos?,
            })
        })
    });
    let tracked = r.optional("tracked", true, read_bool);
    r.finish(|| Some(LodestoneTracker { target, tracked }))
}

fn encode_lodestone_tracker(lt: &LodestoneTracker) -> Dyn {
    let mut b = MapBuilder::new();
    if let Some(t) = &lt.target {
        let mut tb = MapBuilder::new();
        tb.put("dimension", Dyn::string(t.dimension.to_string()));
        tb.put("pos", Dyn::IntArray(t.pos.to_vec()));
        b.put("target", tb.build());
    }
    if !lt.tracked {
        b.put("tracked", Dyn::Bool(false));
    }
    b.build()
}

fn decode_firework_explosion(d: &Dyn) -> DataResult<FireworkExplosion> {
    let mut r = Rec::new(d);
    let shape = r.required("shape", |v| {
        read_string(v).and_then(|s| match FireworkShape::parse(&s) {
            Some(sh) => DataResult::Success(sh),
            None => DataResult::error(format!("Unknown firework shape: {s}")),
        })
    });
    let colors = r.optional("colors", Vec::new(), |v| decode_list_of(v, read_int));
    let fade_colors = r.optional("fade_colors", Vec::new(), |v| decode_list_of(v, read_int));
    let has_trail = r.optional("has_trail", false, read_bool);
    let has_twinkle = r.optional("has_twinkle", false, read_bool);
    r.finish(|| {
        Some(FireworkExplosion {
            shape: shape?,
            colors,
            fade_colors,
            has_trail,
            has_twinkle,
        })
    })
}

fn encode_firework_explosion(fe: &FireworkExplosion) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("shape", Dyn::string(fe.shape.as_str()));
    if !fe.colors.is_empty() {
        b.put("colors", Dyn::IntArray(fe.colors.clone()));
    }
    if !fe.fade_colors.is_empty() {
        b.put("fade_colors", Dyn::IntArray(fe.fade_colors.clone()));
    }
    if fe.has_trail {
        b.put("has_trail", Dyn::Bool(true));
    }
    if fe.has_twinkle {
        b.put("has_twinkle", Dyn::Bool(true));
    }
    b.build()
}

fn decode_fireworks(d: &Dyn) -> DataResult<Fireworks> {
    let mut r = Rec::new(d);
    let flight_duration = r.optional("flight_duration", 0u8, |v| {
        read_int_range(v, 0, 255).map(|n| n as u8)
    });
    let explosions = r.optional("explosions", Vec::new(), |v| {
        decode_sized_list(v, 0, 256, decode_firework_explosion)
    });
    r.finish(|| {
        Some(Fireworks {
            flight_duration,
            explosions,
        })
    })
}

fn encode_fireworks(fw: &Fireworks) -> Dyn {
    let mut b = MapBuilder::new();
    if fw.flight_duration != 0 {
        b.put(
            "flight_duration",
            Dyn::Num(Num::Int(i32::from(fw.flight_duration))),
        );
    }
    if !fw.explosions.is_empty() {
        b.put(
            "explosions",
            Dyn::List(
                fw.explosions
                    .iter()
                    .map(encode_firework_explosion)
                    .collect(),
            ),
        );
    }
    b.build()
}

fn decode_banner_pattern_layer(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<BannerPatternLayer> {
    let mut r = Rec::new(d);
    let pattern = r.required("pattern", |v| {
        decode_holder_ref(v, reg_ids::banner_pattern(), registries)
    });
    let color = r.required("color", decode_dye);
    r.finish(|| {
        Some(BannerPatternLayer {
            pattern: pattern?,
            color: color?,
        })
    })
}

fn encode_banner_pattern_layer(layer: &BannerPatternLayer) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("pattern", encode_holder_ref(&layer.pattern));
    b.put("color", Dyn::string(layer.color.as_str()));
    b.build()
}

fn decode_pot_decorations(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<PotDecorations> {
    let mut r = Rec::new(d);
    let back = r
        .optional_opt("back", |v| ItemTemplate::decode(v, registries))
        .map(Box::new);
    let left = r
        .optional_opt("left", |v| ItemTemplate::decode(v, registries))
        .map(Box::new);
    let right = r
        .optional_opt("right", |v| ItemTemplate::decode(v, registries))
        .map(Box::new);
    let front = r
        .optional_opt("front", |v| ItemTemplate::decode(v, registries))
        .map(Box::new);
    r.finish(|| {
        Some(PotDecorations {
            back,
            left,
            right,
            front,
        })
    })
}

fn encode_pot_decorations(pd: &PotDecorations) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    for (k, v) in [
        ("back", &pd.back),
        ("left", &pd.left),
        ("right", &pd.right),
        ("front", &pd.front),
    ] {
        if let Some(t) = v {
            match t.encode() {
                DataResult::Success(val) => {
                    b.put(k, val);
                }
                other => return other,
            }
        }
    }
    DataResult::Success(b.build())
}

fn decode_container(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<ItemContainerContents> {
    decode_sized_list(d, 0, 256, |entry| {
        let mut r = Rec::new(entry);
        let slot = r.required("slot", |v| read_int_range(v, 0, 255).map(|n| n as u8));
        let item = r.required("item", |v| ItemTemplate::decode(v, registries));
        r.finish(|| Some((slot?, item?)))
    })
    .map(ItemContainerContents::from_entries)
}

fn encode_container(c: &ItemContainerContents) -> DataResult<Dyn> {
    let mut out = Vec::new();
    for (slot, item) in c.non_empty_entries() {
        let encoded_item = match item.encode() {
            DataResult::Success(v) => v,
            other => return other,
        };
        let mut b = MapBuilder::new();
        b.put("slot", Dyn::Num(Num::Int(i32::from(slot))));
        b.put("item", encoded_item);
        out.push(b.build());
    }
    DataResult::Success(Dyn::List(out))
}

fn decode_block_state(d: &Dyn) -> DataResult<BTreeMap<String, String>> {
    let entries = match read_map(d) {
        DataResult::Success(m) => m,
        other => return other.map(|_| BTreeMap::new()),
    };
    let mut out = BTreeMap::new();
    let mut errors = Vec::new();
    for (k, v) in entries {
        match read_string(v) {
            DataResult::Success(val) => {
                out.insert(k.to_string_lossy().into_owned(), val);
            }
            DataResult::Error { message, .. } => errors.push(format!("{k}: {message}")),
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    if errors.is_empty() {
        DataResult::Success(out)
    } else {
        DataResult::error_with_partial(errors.join("; "), out)
    }
}

fn decode_beehive_occupant(
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<BeehiveOccupant> {
    let mut r = Rec::new(d);
    let entity_data = r.required("entity_data", |v| {
        decode_typed_entity_data(v, reg_ids::entity_type(), registries)
    });
    let ticks_in_hive = r.required("ticks_in_hive", read_int);
    let min_ticks_in_hive = r.required("min_ticks_in_hive", read_int);
    r.finish(|| {
        Some(BeehiveOccupant {
            entity_data: entity_data?,
            ticks_in_hive: ticks_in_hive?,
            min_ticks_in_hive: min_ticks_in_hive?,
        })
    })
}

fn encode_beehive_occupant(occ: &BeehiveOccupant) -> Dyn {
    let mut b = MapBuilder::new();
    b.put("entity_data", encode_typed_entity_data(&occ.entity_data));
    b.put("ticks_in_hive", Dyn::Num(Num::Int(occ.ticks_in_hive)));
    b.put(
        "min_ticks_in_hive",
        Dyn::Num(Num::Int(occ.min_ticks_in_hive)),
    );
    b.build()
}

fn decode_container_loot(d: &Dyn) -> DataResult<ContainerLoot> {
    let mut r = Rec::new(d);
    let loot_table = r.required("loot_table", read_identifier);
    let seed = r.optional("seed", 0, read_long);
    r.finish(|| {
        Some(ContainerLoot {
            loot_table: loot_table?,
            seed,
        })
    })
}

fn decode_compostable(d: &Dyn) -> DataResult<Compostable> {
    let mut r = Rec::new(d);
    let layers = r.required("layers", decode_int_or_provider);
    r.finish(|| Some(Compostable { layers: layers? }))
}

fn decode_cooking_fuel(d: &Dyn) -> DataResult<CookingFuel> {
    let mut r = Rec::new(d);
    let burn_time = r.required("burn_time", decode_int_or_provider);
    let speed_multiplier = r.required("speed_multiplier", decode_float_or_provider);
    r.finish(|| {
        Some(CookingFuel {
            burn_time: burn_time?,
            speed_multiplier: speed_multiplier?,
        })
    })
}

fn decode_brewing_fuel(d: &Dyn) -> DataResult<BrewingFuel> {
    let mut r = Rec::new(d);
    let uses = r.required("uses", decode_int_or_provider);
    let speed_multiplier = r.required("speed_multiplier", decode_float_or_provider);
    r.finish(|| {
        Some(BrewingFuel {
            uses: uses?,
            speed_multiplier: speed_multiplier?,
        })
    })
}

fn decode_mob_visibility(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<MobVisibility> {
    let mut r = Rec::new(d);
    let targeting_entity_types = r.required("targeting_entity_types", |v| {
        decode_holder_set(v, reg_ids::entity_type(), registries)
    });
    let visibility = r.required("visibility", |v| read_float_range(v, 0.0, 10.0));
    r.finish(|| {
        Some(MobVisibility {
            targeting_entity_types: targeting_entity_types?,
            visibility: visibility?,
        })
    })
}

fn decode_four_components(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<[Component; 4]> {
    decode_sized_list(d, 4, 4, |v| decode_component(v, registries)).and_then(|v| {
        match <[Component; 4]>::try_from(v) {
            Ok(arr) => DataResult::Success(arr),
            Err(_) => DataResult::error("Expected exactly 4 sign message lines"),
        }
    })
}

fn decode_sign_text(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<SignText> {
    let mut r = Rec::new(d);
    let messages = r.required("messages", |v| decode_four_components(v, registries));
    let filtered_messages = r.lenient("filtered_messages", None, |v| {
        decode_four_components(v, registries).map(Some)
    });
    let color = r.optional("color", DyeColor::Black, decode_dye);
    let has_glowing_text = r.optional("has_glowing_text", false, read_bool);
    r.finish(|| {
        Some(SignText {
            messages: messages?,
            filtered_messages,
            color,
            has_glowing_text,
        })
    })
}

fn encode_sign_text(st: &SignText) -> DataResult<Dyn> {
    let mut b = MapBuilder::new();
    let msgs = match encode_list(&st.messages, encode_component) {
        DataResult::Success(v) => v,
        other => return other,
    };
    b.put("messages", msgs);
    if let Some(filt) = &st.filtered_messages {
        match encode_list(filt, encode_component) {
            DataResult::Success(v) => {
                b.put("filtered_messages", v);
            }
            other => return other,
        }
    }
    // Spec B.7 & edge case 46: `color` and `has_glowing_text` are ALWAYS written even at defaults.
    b.put("color", Dyn::string(st.color.as_str()));
    b.put("has_glowing_text", Dyn::Bool(st.has_glowing_text));
    DataResult::Success(b.build())
}
