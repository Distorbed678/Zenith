//! Value types for all 122 item data components in 26.3 (spec B.7).

use std::collections::BTreeMap;

use zenith_nbt::Compound;
use zenith_registry::Identifier;

use super::stack::ItemTemplate;
use crate::dynval::Dyn;
use crate::profile::Profile;
use crate::text::{Component, TextColor};

/// A unit value (`{}`). Any map decodes; non-maps fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct UnitValue;

/// A registry holder that accepts either an identifier or an inline definition (spec B.7).
#[derive(Debug, Clone, PartialEq)]
pub enum HolderRef {
    /// Registered entry identifier.
    Id(Identifier),
    /// Inline unregistered definition.
    Inline(Dyn),
}

/// A holder set: either `#tag` or a direct list of identifiers (single id decodes as a 1-element list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderSetRef {
    /// Named tag (`#namespace:path`).
    Tag(Identifier),
    /// Direct list of identifiers.
    Ids(Vec<Identifier>),
}

/// Either an inline integer or a `context_int_provider` identifier (spec B.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntOrProvider {
    /// Constant integer.
    Fixed(i32),
    /// Registry reference to a `context_int_provider`.
    Provider(Identifier),
}

/// Either an inline float or a `context_float_provider` identifier (spec B.7).
#[derive(Debug, Clone, PartialEq)]
pub enum FloatOrProvider {
    /// Constant float.
    Fixed(f32),
    /// Registry reference to a `context_float_provider`.
    Provider(Identifier),
}

/// A filterable value (book pages / titles): decodes bare `T` or `{raw, filtered?}`, encodes as object.
#[derive(Debug, Clone, PartialEq)]
pub struct Filterable<T> {
    /// Unfiltered value.
    pub raw: T,
    /// Optional chat-filtered value.
    pub filtered: Option<T>,
}

impl<T> Filterable<T> {
    /// Creates an unfiltered [`Filterable`].
    pub fn raw(raw: T) -> Self {
        Self {
            raw,
            filtered: None,
        }
    }
}

/// Item rarity (`rarity`, spec B.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rarity {
    /// `common` (white).
    Common,
    /// `uncommon` (yellow).
    Uncommon,
    /// `rare` (aqua).
    Rare,
    /// `epic` (light_purple).
    Epic,
}

impl Rarity {
    /// Parses a rarity name (case-sensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "common" => Some(Self::Common),
            "uncommon" => Some(Self::Uncommon),
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Epic => "epic",
        }
    }

    /// Text colour associated with this rarity for item display names (spec B.6).
    pub fn color(self) -> TextColor {
        let name = match self {
            Self::Common => "white",
            Self::Uncommon => "yellow",
            Self::Rare => "aqua",
            Self::Epic => "light_purple",
        };
        TextColor::named(name).expect("valid rarity color")
    }
}

/// The 16 dye colours (spec Constants and IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DyeColor {
    /// `white`
    White,
    /// `orange`
    Orange,
    /// `magenta`
    Magenta,
    /// `light_blue`
    LightBlue,
    /// `yellow`
    Yellow,
    /// `lime`
    Lime,
    /// `pink`
    Pink,
    /// `gray`
    Gray,
    /// `light_gray`
    LightGray,
    /// `cyan`
    Cyan,
    /// `purple`
    Purple,
    /// `blue`
    Blue,
    /// `brown`
    Brown,
    /// `green`
    Green,
    /// `red`
    Red,
    /// `black`
    Black,
}

impl DyeColor {
    /// All 16 dye colours in protocol order.
    pub const ALL: [DyeColor; 16] = [
        Self::White,
        Self::Orange,
        Self::Magenta,
        Self::LightBlue,
        Self::Yellow,
        Self::Lime,
        Self::Pink,
        Self::Gray,
        Self::LightGray,
        Self::Cyan,
        Self::Purple,
        Self::Blue,
        Self::Brown,
        Self::Green,
        Self::Red,
        Self::Black,
    ];

    /// Parses a dye colour name (case-sensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "white" => Some(Self::White),
            "orange" => Some(Self::Orange),
            "magenta" => Some(Self::Magenta),
            "light_blue" => Some(Self::LightBlue),
            "yellow" => Some(Self::Yellow),
            "lime" => Some(Self::Lime),
            "pink" => Some(Self::Pink),
            "gray" => Some(Self::Gray),
            "light_gray" => Some(Self::LightGray),
            "cyan" => Some(Self::Cyan),
            "purple" => Some(Self::Purple),
            "blue" => Some(Self::Blue),
            "brown" => Some(Self::Brown),
            "green" => Some(Self::Green),
            "red" => Some(Self::Red),
            "black" => Some(Self::Black),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Orange => "orange",
            Self::Magenta => "magenta",
            Self::LightBlue => "light_blue",
            Self::Yellow => "yellow",
            Self::Lime => "lime",
            Self::Pink => "pink",
            Self::Gray => "gray",
            Self::LightGray => "light_gray",
            Self::Cyan => "cyan",
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Brown => "brown",
            Self::Green => "green",
            Self::Red => "red",
            Self::Black => "black",
        }
    }

    /// Protocol ordinal (`0..=15`).
    pub const fn id(self) -> u32 {
        self as u32
    }

    /// Looks up by protocol ordinal (`0..=15`).
    pub const fn from_id(id: u32) -> Option<Self> {
        if (id as usize) < Self::ALL.len() {
            Some(Self::ALL[id as usize])
        } else {
            None
        }
    }
}

/// `use_effects` component value (spec B.7).
#[derive(Debug, Clone, PartialEq)]
pub struct UseEffects {
    /// Whether the player may sprint while using the item (default `false`).
    pub can_sprint: bool,
    /// Whether using the item emits vibrations (default `true`).
    pub interact_vibrations: bool,
    /// Movement speed multiplier in `0..=1` (default `0.2`).
    pub speed_multiplier: f32,
}

impl Default for UseEffects {
    fn default() -> Self {
        Self {
            can_sprint: false,
            interact_vibrations: true,
            speed_multiplier: 0.2,
        }
    }
}

/// `enchantments` / `stored_enchantments` (enchantment id → level `1..=255`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Enchantments {
    /// Enchantment levels keyed by enchantment identifier.
    pub levels: BTreeMap<Identifier, i32>,
}

/// Attribute modifier operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeOperation {
    /// `add_value`
    AddValue,
    /// `add_multiplied_base`
    AddMultipliedBase,
    /// `add_multiplied_total`
    AddMultipliedTotal,
}

impl AttributeOperation {
    /// Parses an operation name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add_value" => Some(Self::AddValue),
            "add_multiplied_base" => Some(Self::AddMultipliedBase),
            "add_multiplied_total" => Some(Self::AddMultipliedTotal),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AddValue => "add_value",
            Self::AddMultipliedBase => "add_multiplied_base",
            Self::AddMultipliedTotal => "add_multiplied_total",
        }
    }
}

/// Equipment slot group for `attribute_modifiers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EquipmentSlotGroup {
    /// `any`
    #[default]
    Any,
    /// `mainhand`
    Mainhand,
    /// `offhand`
    Offhand,
    /// `hand`
    Hand,
    /// `feet`
    Feet,
    /// `legs`
    Legs,
    /// `chest`
    Chest,
    /// `head`
    Head,
    /// `armor`
    Armor,
    /// `body`
    Body,
    /// `saddle`
    Saddle,
}

impl EquipmentSlotGroup {
    /// Parses a slot group name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "any" => Some(Self::Any),
            "mainhand" => Some(Self::Mainhand),
            "offhand" => Some(Self::Offhand),
            "hand" => Some(Self::Hand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "armor" => Some(Self::Armor),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Hand => "hand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Armor => "armor",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }
}

/// Display mode of an attribute modifier entry.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AttributeDisplay {
    /// `{type: "default"}`
    #[default]
    Default,
    /// `{type: "hidden"}`
    Hidden,
    /// `{type: "override", value: <component>}`
    Override(Box<Component>),
}

/// One entry in `attribute_modifiers`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifierEntry {
    /// Attribute identifier (`type`).
    pub attribute: Identifier,
    /// Modifier identifier (`id`).
    pub id: Identifier,
    /// Numeric amount.
    pub amount: f64,
    /// Operation.
    pub operation: AttributeOperation,
    /// Slot group (default `any`).
    pub slot: EquipmentSlotGroup,
    /// Tooltip display (default `default`).
    pub display: AttributeDisplay,
}

/// `custom_model_data` component value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomModelData {
    /// Float values.
    pub floats: Vec<f32>,
    /// Boolean flags.
    pub flags: Vec<bool>,
    /// Strings.
    pub strings: Vec<String>,
    /// RGB colours (stored as integers).
    pub colors: Vec<i32>,
}

/// `tooltip_display` component value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TooltipDisplay {
    /// Whether the entire tooltip is hidden (default `false`).
    pub hide_tooltip: bool,
    /// Component IDs hidden from the tooltip (default empty).
    pub hidden_components: Vec<Identifier>,
}

/// `food` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct FoodProperties {
    /// Hunger points restored (`>= 0`).
    pub nutrition: i32,
    /// Saturation modifier.
    pub saturation: f32,
    /// Whether the food can be eaten when full (default `false`).
    pub can_always_eat: bool,
}

/// Consumable animation (`consumable.animation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConsumeAnimation {
    /// `none`
    None,
    /// `eat` (default)
    #[default]
    Eat,
    /// `drink`
    Drink,
    /// `block`
    Block,
    /// `bow`
    Bow,
    /// `trident`
    Trident,
    /// `crossbow`
    Crossbow,
    /// `spyglass`
    Spyglass,
    /// `toot_horn`
    TootHorn,
    /// `brush`
    Brush,
    /// `bundle`
    Bundle,
    /// `spear`
    Spear,
}

impl ConsumeAnimation {
    /// Parses an animation name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "eat" => Some(Self::Eat),
            "drink" => Some(Self::Drink),
            "block" => Some(Self::Block),
            "bow" => Some(Self::Bow),
            "trident" => Some(Self::Trident),
            "crossbow" => Some(Self::Crossbow),
            "spyglass" => Some(Self::Spyglass),
            "toot_horn" => Some(Self::TootHorn),
            "brush" => Some(Self::Brush),
            "bundle" => Some(Self::Bundle),
            "spear" => Some(Self::Spear),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Eat => "eat",
            Self::Drink => "drink",
            Self::Block => "block",
            Self::Bow => "bow",
            Self::Trident => "trident",
            Self::Crossbow => "crossbow",
            Self::Spyglass => "spyglass",
            Self::TootHorn => "toot_horn",
            Self::Brush => "brush",
            Self::Bundle => "bundle",
            Self::Spear => "spear",
        }
    }
}

/// Status effect instance used by `potion_contents`, `consumable`, `death_protection`.
#[derive(Debug, Clone, PartialEq)]
pub struct MobEffectInstance {
    /// Effect identifier.
    pub id: Identifier,
    /// Amplifier `0..=255` (default `0`).
    pub amplifier: u8,
    /// Duration in ticks (default `0`).
    pub duration: i32,
    /// Ambient flag (default `false`).
    pub ambient: bool,
    /// Show particles (default `true`).
    pub show_particles: bool,
    /// Optional show icon override.
    pub show_icon: Option<bool>,
    /// Optional hidden nested effect.
    pub hidden_effect: Option<Box<MobEffectInstance>>,
}

/// An effect triggered on consume or death protection.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsumeEffect {
    /// `minecraft:apply_effects`
    ApplyEffects {
        /// Effects to apply.
        effects: Vec<MobEffectInstance>,
        /// Probability `0..=1` (default `1.0`).
        probability: f32,
    },
    /// `minecraft:remove_effects`
    RemoveEffects {
        /// Effects holder set.
        effects: HolderSetRef,
    },
    /// `minecraft:clear_all_effects`
    ClearAllEffects,
    /// `minecraft:teleport_randomly`
    TeleportRandomly {
        /// Diameter `> 0` (default `16.0`).
        diameter: f32,
    },
    /// `minecraft:play_sound`
    PlaySound {
        /// Sound event (id/inline).
        sound: HolderRef,
    },
}

/// `consumable` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct Consumable {
    /// Consume duration in seconds (`>= 0`, default `1.6`).
    pub consume_seconds: f32,
    /// Consume animation (default `eat`).
    pub animation: ConsumeAnimation,
    /// Sound event (default `minecraft:entity.generic.eat`).
    pub sound: HolderRef,
    /// Whether particles spawn (default `true`).
    pub has_consume_particles: bool,
    /// Effects triggered on completion.
    pub on_consume_effects: Vec<ConsumeEffect>,
}

impl Default for Consumable {
    fn default() -> Self {
        Self {
            consume_seconds: 1.6,
            animation: ConsumeAnimation::Eat,
            sound: HolderRef::Id(
                Identifier::parse("minecraft:entity.generic.eat").expect("valid constant"),
            ),
            has_consume_particles: true,
            on_consume_effects: Vec::new(),
        }
    }
}

/// `use_cooldown` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct UseCooldown {
    /// Cooldown duration in seconds (`> 0`).
    pub seconds: f32,
    /// Optional cooldown group identifier.
    pub cooldown_group: Option<Identifier>,
}

/// One mining rule inside `tool`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRule {
    /// Matched blocks holder set.
    pub blocks: HolderSetRef,
    /// Optional mining speed (`> 0`).
    pub speed: Option<f32>,
    /// Optional correct-for-drops flag.
    pub correct_for_drops: Option<bool>,
}

/// `tool` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolProperties {
    /// Mining rules.
    pub rules: Vec<ToolRule>,
    /// Default mining speed (default `1.0`).
    pub default_mining_speed: f32,
    /// Damage taken per broken block (`>= 0`, default `1`).
    pub damage_per_block: i32,
    /// Whether the tool can destroy blocks in creative mode (default `true`).
    pub can_destroy_blocks_in_creative: bool,
}

/// `weapon` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponProperties {
    /// Durability damage per attack (`>= 0`, default `1`).
    pub item_damage_per_attack: i32,
    /// Shield-disable duration in seconds (`>= 0`, default `0.0`).
    pub disable_blocking_for_seconds: f32,
}

impl Default for WeaponProperties {
    fn default() -> Self {
        Self {
            item_damage_per_attack: 1,
            disable_blocking_for_seconds: 0.0,
        }
    }
}

/// `attack_range` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct AttackRange {
    /// `0..=64` (default `0.0`).
    pub min_reach: f32,
    /// `0..=64` (default `3.0`).
    pub max_reach: f32,
    /// `0..=64` (default `0.0`).
    pub min_creative_reach: f32,
    /// `0..=64` (default `5.0`).
    pub max_creative_reach: f32,
    /// `0..=1` (default `0.3`).
    pub hitbox_margin: f32,
    /// `0..=2` (default `1.0`).
    pub mob_factor: f32,
}

impl Default for AttackRange {
    fn default() -> Self {
        Self {
            min_reach: 0.0,
            max_reach: 3.0,
            min_creative_reach: 0.0,
            max_creative_reach: 5.0,
            hitbox_margin: 0.3,
            mob_factor: 1.0,
        }
    }
}

/// Equipment slot for `equippable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    /// `mainhand`
    Mainhand,
    /// `offhand`
    Offhand,
    /// `feet`
    Feet,
    /// `legs`
    Legs,
    /// `chest`
    Chest,
    /// `head`
    Head,
    /// `body`
    Body,
    /// `saddle`
    Saddle,
}

impl EquipmentSlot {
    /// Parses an equipment slot name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mainhand" => Some(Self::Mainhand),
            "offhand" => Some(Self::Offhand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }
}

/// `equippable` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct Equippable {
    /// Target equipment slot.
    pub slot: EquipmentSlot,
    /// Equip sound event (default `minecraft:item.armor.equip_generic`).
    pub equip_sound: HolderRef,
    /// Optional equipment model asset id.
    pub asset_id: Option<Identifier>,
    /// Optional camera overlay texture id.
    pub camera_overlay: Option<Identifier>,
    /// Optional allowed entities holder set.
    pub allowed_entities: Option<HolderSetRef>,
    /// Dispensable flag (default `true`).
    pub dispensable: bool,
    /// Swappable flag (default `true`).
    pub swappable: bool,
    /// Damage on hurt flag (default `true`).
    pub damage_on_hurt: bool,
    /// Equip on interact flag (default `false`).
    pub equip_on_interact: bool,
    /// Can be sheared flag (default `false`).
    pub can_be_sheared: bool,
    /// Shearing sound event (default `minecraft:item.shears.snip`).
    pub shearing_sound: HolderRef,
}

/// One damage-reduction rule in `blocks_attacks`.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageReduction {
    /// Horizontal blocking angle (`> 0`, default `90.0`).
    pub horizontal_blocking_angle: f32,
    /// Optional damage-type holder set.
    pub damage_type: Option<HolderSetRef>,
    /// Base flat reduction.
    pub base: f32,
    /// Multiplicative factor.
    pub factor: f32,
}

impl Default for DamageReduction {
    fn default() -> Self {
        Self {
            horizontal_blocking_angle: 90.0,
            damage_type: None,
            base: 0.0,
            factor: 1.0,
        }
    }
}

/// Item-damage function in `blocks_attacks`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemDamageFunction {
    /// Minimum damage threshold (`>= 0`, default `1.0`).
    pub threshold: f32,
    /// Base item damage (default `0.0`).
    pub base: f32,
    /// Factor (default `1.0`).
    pub factor: f32,
}

impl Default for ItemDamageFunction {
    fn default() -> Self {
        Self {
            threshold: 1.0,
            base: 0.0,
            factor: 1.0,
        }
    }
}

/// `blocks_attacks` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct BlocksAttacks {
    /// Delay before blocking activates (`>= 0`, default `0.0`).
    pub block_delay_seconds: f32,
    /// Cooldown scale when disabled (`>= 0`, default `1.0`).
    pub disable_cooldown_scale: f32,
    /// Damage reduction rules.
    pub damage_reductions: Vec<DamageReduction>,
    /// Item damage function.
    pub item_damage: ItemDamageFunction,
    /// Optional bypassed-by damage type tag/holder set.
    pub bypassed_by: Option<HolderSetRef>,
    /// Optional block sound event.
    pub block_sound: Option<HolderRef>,
    /// Optional disabled sound event.
    pub disabled_sound: Option<HolderRef>,
}

impl Default for BlocksAttacks {
    fn default() -> Self {
        Self {
            block_delay_seconds: 0.0,
            disable_cooldown_scale: 1.0,
            damage_reductions: vec![DamageReduction::default()],
            item_damage: ItemDamageFunction::default(),
            bypassed_by: None,
            block_sound: None,
            disabled_sound: None,
        }
    }
}

/// `piercing_weapon` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct PiercingWeapon {
    /// Whether attacks deal knockback (default `true`).
    pub deals_knockback: bool,
    /// Whether attacks dismount targets (default `false`).
    pub dismounts: bool,
    /// Optional attack sound.
    pub sound: Option<HolderRef>,
    /// Optional hit sound.
    pub hit_sound: Option<HolderRef>,
}

impl Default for PiercingWeapon {
    fn default() -> Self {
        Self {
            deals_knockback: true,
            dismounts: false,
            sound: None,
            hit_sound: None,
        }
    }
}

/// Condition inside `kinetic_weapon`.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticCondition {
    /// Maximum duration in ticks (`>= 0`).
    pub max_duration_ticks: i32,
    /// Minimum speed (default `0.0`).
    pub min_speed: f32,
    /// Minimum relative speed (default `0.0`).
    pub min_relative_speed: f32,
}

/// `kinetic_weapon` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct KineticWeapon {
    /// Contact cooldown in ticks (`>= 0`, default `10`).
    pub contact_cooldown_ticks: i32,
    /// Delay in ticks (`>= 0`, default `0`).
    pub delay_ticks: i32,
    /// Optional dismount condition.
    pub dismount_conditions: Option<KineticCondition>,
    /// Optional knockback condition.
    pub knockback_conditions: Option<KineticCondition>,
    /// Optional damage condition.
    pub damage_conditions: Option<KineticCondition>,
    /// Forward movement (default `0.0`).
    pub forward_movement: f32,
    /// Damage multiplier (default `1.0`).
    pub damage_multiplier: f32,
    /// Optional sound.
    pub sound: Option<HolderRef>,
    /// Optional hit sound.
    pub hit_sound: Option<HolderRef>,
}

impl Default for KineticWeapon {
    fn default() -> Self {
        Self {
            contact_cooldown_ticks: 10,
            delay_ticks: 0,
            dismount_conditions: None,
            knockback_conditions: None,
            damage_conditions: None,
            forward_movement: 0.0,
            damage_multiplier: 1.0,
            sound: None,
            hit_sound: None,
        }
    }
}

/// Animation type for `attack_animation` and `interact_animation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SwingAnimationType {
    /// `none`
    None,
    /// `whack` (default)
    #[default]
    Whack,
    /// `stab`
    Stab,
}

impl SwingAnimationType {
    /// Parses a swing animation type name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "whack" => Some(Self::Whack),
            "stab" => Some(Self::Stab),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Whack => "whack",
            Self::Stab => "stab",
        }
    }
}

/// `attack_animation` / `interact_animation` component value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    /// Animation type (default `whack`).
    pub kind: SwingAnimationType,
    /// Duration in ticks (`>= 0`, default `6`).
    pub duration: i32,
}

impl Default for SwingAnimation {
    fn default() -> Self {
        Self {
            kind: SwingAnimationType::Whack,
            duration: 6,
        }
    }
}

/// One entry in `map_decorations`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapDecoration {
    /// Decoration type id.
    pub decoration_type: Identifier,
    /// X coordinate.
    pub x: f64,
    /// Z coordinate.
    pub z: f64,
    /// Rotation angle.
    pub rotation: f32,
}

/// Transient `map_post_processing` component value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MapPostProcessing {
    /// Lock map (`0`).
    Lock = 0,
    /// Scale map (`1`).
    Scale = 1,
}

impl MapPostProcessing {
    /// Looks up by network ordinal.
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Lock),
            1 => Some(Self::Scale),
            _ => None,
        }
    }
}

/// `potion_contents` component value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PotionContents {
    /// Base potion id.
    pub potion: Option<Identifier>,
    /// Custom ARGB/RGB colour override.
    pub custom_color: Option<i32>,
    /// Custom status effects.
    pub custom_effects: Vec<MobEffectInstance>,
    /// Optional custom translation suffix name.
    pub custom_name: Option<String>,
}

/// One entry in `suspicious_stew_effects`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspiciousStewEffect {
    /// Effect identifier.
    pub id: Identifier,
    /// Duration in ticks (default `160`, lenient).
    pub duration: i32,
}

/// `writable_book_content` component value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WritableBookContent {
    /// At most 100 pages, each 0..=1024 UTF-16 chars.
    pub pages: Vec<Filterable<String>>,
}

/// `written_book_content` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenBookContent {
    /// Title (≤ 32 UTF-16 chars).
    pub title: Filterable<String>,
    /// Author name.
    pub author: String,
    /// Generation `0..=3` (default `0`).
    pub generation: i32,
    /// Pages (each ≤ 32,767 compact JSON UTF-16 chars).
    pub pages: Vec<Filterable<Component>>,
    /// Whether text components have been resolved (default `false`).
    pub resolved: bool,
}

/// `trim` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorTrim {
    /// Trim material (id/inline).
    pub material: HolderRef,
    /// Trim pattern (id/inline).
    pub pattern: HolderRef,
}

/// `entity_data` / `block_entity_data`: compound with a mandatory `id` identifier.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedEntityData {
    /// Entity type or block-entity type identifier.
    pub id: Identifier,
    /// Remaining NBT fields (excluding `id`).
    pub payload: Compound,
}

/// Global position target inside `lodestone_tracker`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPos {
    /// Dimension identifier.
    pub dimension: Identifier,
    /// Block `[x, y, z]`.
    pub pos: [i32; 3],
}

/// `lodestone_tracker` component value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LodestoneTracker {
    /// Optional target position.
    pub target: Option<GlobalPos>,
    /// Whether tracking is active (default `true`).
    pub tracked: bool,
}

impl Default for LodestoneTracker {
    fn default() -> Self {
        Self {
            target: None,
            tracked: true,
        }
    }
}

/// Shape of a firework explosion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireworkShape {
    /// `small_ball`
    SmallBall,
    /// `large_ball`
    LargeBall,
    /// `star`
    Star,
    /// `creeper`
    Creeper,
    /// `burst`
    Burst,
}

impl FireworkShape {
    /// Parses a firework shape name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "small_ball" => Some(Self::SmallBall),
            "large_ball" => Some(Self::LargeBall),
            "star" => Some(Self::Star),
            "creeper" => Some(Self::Creeper),
            "burst" => Some(Self::Burst),
            _ => None,
        }
    }

    /// Serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SmallBall => "small_ball",
            Self::LargeBall => "large_ball",
            Self::Star => "star",
            Self::Creeper => "creeper",
            Self::Burst => "burst",
        }
    }
}

/// `firework_explosion` component value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkExplosion {
    /// Explosion shape.
    pub shape: FireworkShape,
    /// Primary colours.
    pub colors: Vec<i32>,
    /// Fade colours.
    pub fade_colors: Vec<i32>,
    /// Trail flag (default `false`).
    pub has_trail: bool,
    /// Twinkle flag (default `false`).
    pub has_twinkle: bool,
}

/// `fireworks` component value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fireworks {
    /// Flight duration `0..=255` (default `0`).
    pub flight_duration: u8,
    /// Explosions (`<= 256`).
    pub explosions: Vec<FireworkExplosion>,
}

/// One layer of `banner_patterns`.
#[derive(Debug, Clone, PartialEq)]
pub struct BannerPatternLayer {
    /// Banner pattern (id/inline).
    pub pattern: HolderRef,
    /// Layer dye colour.
    pub color: DyeColor,
}

/// `pot_decorations` component value (`{back?, left?, right?, front?}`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PotDecorations {
    /// Back sherd/brick item template.
    pub back: Option<Box<ItemTemplate>>,
    /// Left sherd/brick item template.
    pub left: Option<Box<ItemTemplate>>,
    /// Right sherd/brick item template.
    pub right: Option<Box<ItemTemplate>>,
    /// Front sherd/brick item template.
    pub front: Option<Box<ItemTemplate>>,
}

/// `container` component value: dense slot list (`0..=255`), missing slots are `None`,
/// later duplicates overwrite earlier ones (spec B.7, edge case 43).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemContainerContents {
    /// Dense slot vector indexed by slot number; trailing `None`s are trimmed.
    pub slots: Vec<Option<ItemTemplate>>,
}

impl ItemContainerContents {
    /// Builds container contents from `(slot, item)` pairs, overwriting earlier duplicates and
    /// padding gaps with `None`.
    pub fn from_entries(entries: impl IntoIterator<Item = (u8, ItemTemplate)>) -> Self {
        let mut slots: Vec<Option<ItemTemplate>> = Vec::new();
        for (slot, item) in entries {
            let idx = usize::from(slot);
            if idx >= slots.len() {
                slots.resize(idx + 1, None);
            }
            slots[idx] = Some(item);
        }
        Self { slots }
    }

    /// Iterates non-empty `(slot, &ItemTemplate)` entries in ascending slot order.
    pub fn non_empty_entries(&self) -> impl Iterator<Item = (u8, &ItemTemplate)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|item| (i as u8, item)))
    }
}

/// One entry in `bees`.
#[derive(Debug, Clone, PartialEq)]
pub struct BeehiveOccupant {
    /// Entity NBT data (must contain `id`).
    pub entity_data: TypedEntityData,
    /// Ticks spent in hive.
    pub ticks_in_hive: i32,
    /// Minimum ticks required in hive.
    pub min_ticks_in_hive: i32,
}

/// `container_loot` component value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerLoot {
    /// Loot table identifier.
    pub loot_table: Identifier,
    /// Optional RNG seed (default `0`).
    pub seed: i64,
}

/// `compostable` component value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compostable {
    /// Layers (`int` or `context_int_provider` id).
    pub layers: IntOrProvider,
}

/// `cooking_fuel` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct CookingFuel {
    /// Burn time in ticks (`int` or provider id).
    pub burn_time: IntOrProvider,
    /// Speed multiplier (`float` or provider id).
    pub speed_multiplier: FloatOrProvider,
}

/// `brewing_fuel` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct BrewingFuel {
    /// Uses (`int` or provider id).
    pub uses: IntOrProvider,
    /// Speed multiplier (`float` or provider id).
    pub speed_multiplier: FloatOrProvider,
}

/// `mob_visibility` component value.
#[derive(Debug, Clone, PartialEq)]
pub struct MobVisibility {
    /// Entity types affected.
    pub targeting_entity_types: HolderSetRef,
    /// Visibility multiplier `0..=10`.
    pub visibility: f32,
}

/// `sign_text_front` / `sign_text_back` component value (spec B.7, edge case 46).
///
/// Always serializes `color` and `has_glowing_text` even when equal to their defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct SignText {
    /// Exactly 4 lines.
    pub messages: [Component; 4],
    /// Optional filtered lines (exactly 4, lenient on decode).
    pub filtered_messages: Option<[Component; 4]>,
    /// Text dye colour (default `black`, always written).
    pub color: DyeColor,
    /// Glowing text flag (default `false`, always written).
    pub has_glowing_text: bool,
}

impl Default for SignText {
    fn default() -> Self {
        Self {
            messages: [
                Component::empty(),
                Component::empty(),
                Component::empty(),
                Component::empty(),
            ],
            filtered_messages: None,
            color: DyeColor::Black,
            has_glowing_text: false,
        }
    }
}

/// Converts three `[r, g, b]` floats in `0..=1` (wrapping via `floor(v * 255) & 0xFF`) to an RGB int.
pub fn rgb_from_floats(r: f32, g: f32, b: f32) -> i32 {
    let ch = |v: f32| ((v * 255.0).floor() as i32) & 0xFF;
    (ch(r) << 16) | (ch(g) << 8) | ch(b)
}

/// Known enum values for the 9 fixed-enum entity-variant components (spec B.7).
pub fn valid_fixed_enum_values(
    ty: super::registry::ComponentTypeId,
) -> Option<&'static [&'static str]> {
    use super::registry::ComponentTypeId::*;
    match ty {
        FoxVariant => Some(&["red", "snow"]),
        SalmonSize => Some(&["small", "medium", "large"]),
        ParrotVariant => Some(&["red_blue", "blue", "green", "yellow_blue", "gray"]),
        TropicalFishPattern => Some(&[
            "kob",
            "sunstreak",
            "snooper",
            "dasher",
            "brinely",
            "spotty",
            "flopper",
            "stripey",
            "glitter",
            "blockfish",
            "betty",
            "clayfish",
        ]),
        MooshroomVariant => Some(&["red", "brown"]),
        RabbitVariant => Some(&[
            "brown",
            "white",
            "black",
            "white_splotched",
            "gold",
            "salt",
            "evil",
        ]),
        HorseVariant => Some(&[
            "white",
            "creamy",
            "chestnut",
            "brown",
            "black",
            "gray",
            "dark_brown",
        ]),
        LlamaVariant => Some(&["creamy", "white", "brown", "gray"]),
        AxolotlVariant => Some(&["lucy", "wild", "gold", "cyan", "blue"]),
        _ => None,
    }
}

/// A typed value of an item data component (covers all 122 component types).
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    /// Unit `{}` (`unbreakable`, `creative_slot_lock`, `intangible_projectile`, `glider`, `waxed`).
    Unit(UnitValue),
    /// Plain boolean (`enchantment_glint_override`).
    Bool(bool),
    /// 32-bit integer (`max_stack_size`, `max_damage`, `damage`, `repair_cost`, `additional_trade_cost`,
    /// `map_id`, `ominous_bottle_amplifier`, `dyed_color`).
    Int(i32),
    /// 32-bit float (`minimum_attack_charge`, `potion_duration_scale`).
    Float(f32),
    /// Fixed enum name (`fox/variant`, `salmon/size`, …).
    EnumName(String),
    /// Registry identifier (`item_model`, `damage_type`, `tooltip_style`, `block_transformer`,
    /// `jukebox_playable`, `note_block_sound`, `provides_pottery_pattern`, and the 14 registry-id entity variants).
    Id(Identifier),
    /// Registry id or inline definition (`instrument`, `provides_trim_material`, `break_sound`).
    Holder(HolderRef),
    /// Holder set (`provides_banner_patterns`).
    HolderSet(HolderSetRef),
    /// List of identifiers (`recipes`).
    IdList(Vec<Identifier>),
    /// Arbitrary NBT compound (`custom_data`, `bucket_entity_data`).
    Compound(Compound),
    /// Text component (`custom_name`, `item_name`).
    Text(Box<Component>),
    /// List of text components (`lore`, `<= 256`).
    Lore(Vec<Component>),
    /// `rarity`.
    Rarity(Rarity),
    /// `dye`, `base_color`, and the 7 dye-coloured entity/block components.
    Dye(DyeColor),
    /// `use_effects`.
    UseEffects(UseEffects),
    /// `enchantments`, `stored_enchantments`.
    Enchantments(Enchantments),
    /// `can_place_on`, `can_break` (non-empty list of block predicates).
    BlockPredicates(Vec<Dyn>),
    /// `attribute_modifiers`.
    AttributeModifiers(Vec<AttributeModifierEntry>),
    /// `custom_model_data`.
    CustomModelData(CustomModelData),
    /// `tooltip_display`.
    TooltipDisplay(TooltipDisplay),
    /// `food`.
    Food(FoodProperties),
    /// `consumable`.
    Consumable(Consumable),
    /// Single item template (`use_remainder`, `sulfur_cube_content`).
    ItemTemplate(Box<ItemTemplate>),
    /// `use_cooldown`.
    UseCooldown(UseCooldown),
    /// `damage_resistant`.
    DamageResistant(HolderSetRef),
    /// `tool`.
    Tool(ToolProperties),
    /// `weapon`.
    Weapon(WeaponProperties),
    /// `attack_range`.
    AttackRange(AttackRange),
    /// `enchantable` (`value >= 1`).
    Enchantable(i32),
    /// `equippable`.
    Equippable(Equippable),
    /// `repairable`.
    Repairable(HolderSetRef),
    /// `death_protection`.
    DeathProtection(Vec<ConsumeEffect>),
    /// `blocks_attacks`.
    BlocksAttacks(BlocksAttacks),
    /// `piercing_weapon`.
    PiercingWeapon(PiercingWeapon),
    /// `kinetic_weapon`.
    KineticWeapon(KineticWeapon),
    /// `attack_animation`, `interact_animation`.
    SwingAnimation(SwingAnimation),
    /// `villager_food` (`nutrition >= 1`).
    VillagerFood(i32),
    /// `map_decorations`.
    MapDecorations(BTreeMap<String, MapDecoration>),
    /// `map_post_processing` (transient).
    MapPostProcessing(MapPostProcessing),
    /// `charged_projectiles` (`<= 1024`).
    ChargedProjectiles(Vec<ItemTemplate>),
    /// `bundle_contents`.
    BundleContents(Vec<ItemTemplate>),
    /// `potion_contents`.
    PotionContents(PotionContents),
    /// `suspicious_stew_effects`.
    SuspiciousStewEffects(Vec<SuspiciousStewEffect>),
    /// `writable_book_content`.
    WritableBookContent(WritableBookContent),
    /// `written_book_content`.
    WrittenBookContent(WrittenBookContent),
    /// `trim`.
    Trim(ArmorTrim),
    /// `debug_stick_state` (block id → property name).
    DebugStickState(BTreeMap<Identifier, String>),
    /// `entity_data`, `block_entity_data`.
    TypedEntityData(TypedEntityData),
    /// `lodestone_tracker`.
    LodestoneTracker(LodestoneTracker),
    /// `firework_explosion`.
    FireworkExplosion(FireworkExplosion),
    /// `fireworks`.
    Fireworks(Fireworks),
    /// `profile`.
    Profile(Profile),
    /// `banner_patterns`.
    BannerPatterns(Vec<BannerPatternLayer>),
    /// `pot_decorations`.
    PotDecorations(PotDecorations),
    /// `container`.
    Container(ItemContainerContents),
    /// `block_state` (property name → value string).
    BlockState(BTreeMap<String, String>),
    /// `bees`.
    Bees(Vec<BeehiveOccupant>),
    /// `lock` (item predicate).
    Lock(Dyn),
    /// `container_loot`.
    ContainerLoot(ContainerLoot),
    /// `compostable`.
    Compostable(Compostable),
    /// `cooking_fuel`.
    CookingFuel(CookingFuel),
    /// `brewing_fuel`.
    BrewingFuel(BrewingFuel),
    /// `mob_visibility`.
    MobVisibility(MobVisibility),
    /// `sign_text_front`, `sign_text_back`.
    SignText(Box<SignText>),
}
