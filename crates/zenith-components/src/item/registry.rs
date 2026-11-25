//! The built-in `minecraft:data_component_type` registry (26.3: 122 entries, 119 persistent, 3 transient;
//! spec B.1, B.5, Constants and IDs).

use std::fmt;

use zenith_registry::Identifier;

/// Total number of built-in item data component types in 26.3.
pub const COMPONENT_TYPE_COUNT: usize = 122;

macro_rules! define_component_types {
    ($( $variant:ident = $id:expr => $name:literal $(, transient = $tr:literal)? $(, nbt_net = $nn:literal)? ; )*) => {
        /// Built-in item data component type in `minecraft:data_component_type` (26.3).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u32)]
        pub enum ComponentTypeId {
            $(
                #[doc = concat!("`", $name, "` (protocol id ", stringify!($id), ").")]
                $variant = $id,
            )*
        }

        impl ComponentTypeId {
            /// All 122 built-in component types in protocol ID order (`0..=121`).
            pub const ALL: &'static [ComponentTypeId] = &[
                $( Self::$variant, )*
            ];

            /// The 26.3 protocol ID (`0..=121`).
            #[inline]
            pub const fn protocol_id(self) -> u32 {
                self as u32
            }

            /// Looks up a component type by its protocol ID (`0..=121`).
            pub const fn from_protocol_id(id: u32) -> Option<Self> {
                match id {
                    $( $id => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// Canonical `minecraft:...` identifier string.
            pub const fn id_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $name, )*
                }
            }

            /// Canonical [`Identifier`].
            pub fn identifier(self) -> Identifier {
                Identifier::parse(self.id_str()).expect("built-in component id is valid")
            }

            /// Looks up a component type by canonical [`Identifier`].
            pub fn from_identifier(id: &Identifier) -> Option<Self> {
                if id.namespace() != "minecraft" {
                    return None;
                }
                let full = id.to_string();
                match full.as_str() {
                    $( $name => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// Parses an identifier string (default namespace `minecraft`) and looks up the component type.
            pub fn parse_id(s: &str) -> Result<Self, String> {
                let id = Identifier::parse(s)
                    .map_err(|_| format!("Not a valid resource location: {s}"))?;
                Self::from_identifier(&id)
                    .ok_or_else(|| format!("No component with type {id}"))
            }

            /// Whether this component type is transient (network-only, never saved, rejected in serialized patches).
            pub const fn is_transient(self) -> bool {
                match self {
                    $( $( Self::$variant => $tr, )? )*
                    _ => false,
                }
            }

            /// Whether this component type is persistent (saved in JSON/NBT patches).
            #[inline]
            pub const fn is_persistent(self) -> bool {
                !self.is_transient()
            }

            /// Whether this component type has no dedicated network codec and is sent on the wire as an NBT tag
            /// produced by its persistent codec (spec B.5).
            pub const fn uses_nbt_network_codec(self) -> bool {
                match self {
                    $( $( Self::$variant => $nn, )? )*
                    _ => false,
                }
            }
        }
    };
}

define_component_types! {
    CustomData = 0 => "minecraft:custom_data", nbt_net = true;
    MaxStackSize = 1 => "minecraft:max_stack_size";
    MaxDamage = 2 => "minecraft:max_damage";
    Damage = 3 => "minecraft:damage";
    Unbreakable = 4 => "minecraft:unbreakable";
    UseEffects = 5 => "minecraft:use_effects";
    CustomName = 6 => "minecraft:custom_name";
    MinimumAttackCharge = 7 => "minecraft:minimum_attack_charge";
    DamageType = 8 => "minecraft:damage_type";
    ItemName = 9 => "minecraft:item_name";
    ItemModel = 10 => "minecraft:item_model";
    Lore = 11 => "minecraft:lore";
    Rarity = 12 => "minecraft:rarity";
    Enchantments = 13 => "minecraft:enchantments";
    CanPlaceOn = 14 => "minecraft:can_place_on";
    CanBreak = 15 => "minecraft:can_break";
    AttributeModifiers = 16 => "minecraft:attribute_modifiers";
    CustomModelData = 17 => "minecraft:custom_model_data";
    TooltipDisplay = 18 => "minecraft:tooltip_display";
    RepairCost = 19 => "minecraft:repair_cost";
    CreativeSlotLock = 20 => "minecraft:creative_slot_lock", transient = true;
    EnchantmentGlintOverride = 21 => "minecraft:enchantment_glint_override";
    IntangibleProjectile = 22 => "minecraft:intangible_projectile", nbt_net = true;
    Food = 23 => "minecraft:food";
    Consumable = 24 => "minecraft:consumable";
    UseRemainder = 25 => "minecraft:use_remainder";
    UseCooldown = 26 => "minecraft:use_cooldown";
    DamageResistant = 27 => "minecraft:damage_resistant";
    Tool = 28 => "minecraft:tool";
    Weapon = 29 => "minecraft:weapon";
    AttackRange = 30 => "minecraft:attack_range";
    Enchantable = 31 => "minecraft:enchantable";
    Equippable = 32 => "minecraft:equippable";
    Repairable = 33 => "minecraft:repairable";
    Glider = 34 => "minecraft:glider";
    TooltipStyle = 35 => "minecraft:tooltip_style";
    DeathProtection = 36 => "minecraft:death_protection";
    BlocksAttacks = 37 => "minecraft:blocks_attacks";
    PiercingWeapon = 38 => "minecraft:piercing_weapon";
    KineticWeapon = 39 => "minecraft:kinetic_weapon";
    AttackAnimation = 40 => "minecraft:attack_animation";
    InteractAnimation = 41 => "minecraft:interact_animation";
    AdditionalTradeCost = 42 => "minecraft:additional_trade_cost", transient = true;
    BlockTransformer = 43 => "minecraft:block_transformer";
    VillagerFood = 44 => "minecraft:villager_food";
    StoredEnchantments = 45 => "minecraft:stored_enchantments";
    Dye = 46 => "minecraft:dye";
    DyedColor = 47 => "minecraft:dyed_color";
    MapId = 48 => "minecraft:map_id";
    MapDecorations = 49 => "minecraft:map_decorations", nbt_net = true;
    MapPostProcessing = 50 => "minecraft:map_post_processing", transient = true;
    ChargedProjectiles = 51 => "minecraft:charged_projectiles";
    BundleContents = 52 => "minecraft:bundle_contents";
    PotionContents = 53 => "minecraft:potion_contents";
    PotionDurationScale = 54 => "minecraft:potion_duration_scale";
    SuspiciousStewEffects = 55 => "minecraft:suspicious_stew_effects";
    WritableBookContent = 56 => "minecraft:writable_book_content";
    WrittenBookContent = 57 => "minecraft:written_book_content";
    Trim = 58 => "minecraft:trim";
    DebugStickState = 59 => "minecraft:debug_stick_state", nbt_net = true;
    EntityData = 60 => "minecraft:entity_data";
    BucketEntityData = 61 => "minecraft:bucket_entity_data";
    BlockEntityData = 62 => "minecraft:block_entity_data";
    Instrument = 63 => "minecraft:instrument";
    ProvidesTrimMaterial = 64 => "minecraft:provides_trim_material";
    OminousBottleAmplifier = 65 => "minecraft:ominous_bottle_amplifier";
    JukeboxPlayable = 66 => "minecraft:jukebox_playable";
    ProvidesBannerPatterns = 67 => "minecraft:provides_banner_patterns";
    Recipes = 68 => "minecraft:recipes", nbt_net = true;
    LodestoneTracker = 69 => "minecraft:lodestone_tracker";
    FireworkExplosion = 70 => "minecraft:firework_explosion";
    Fireworks = 71 => "minecraft:fireworks";
    Profile = 72 => "minecraft:profile";
    NoteBlockSound = 73 => "minecraft:note_block_sound";
    BannerPatterns = 74 => "minecraft:banner_patterns";
    BaseColor = 75 => "minecraft:base_color";
    PotDecorations = 76 => "minecraft:pot_decorations";
    Container = 77 => "minecraft:container";
    BlockState = 78 => "minecraft:block_state";
    Bees = 79 => "minecraft:bees";
    SulfurCubeContent = 80 => "minecraft:sulfur_cube_content";
    Lock = 81 => "minecraft:lock", nbt_net = true;
    ContainerLoot = 82 => "minecraft:container_loot", nbt_net = true;
    BreakSound = 83 => "minecraft:break_sound";
    Compostable = 84 => "minecraft:compostable";
    CookingFuel = 85 => "minecraft:cooking_fuel";
    BrewingFuel = 86 => "minecraft:brewing_fuel";
    MobVisibility = 87 => "minecraft:mob_visibility";
    VillagerVariant = 88 => "minecraft:villager/variant";
    WolfVariant = 89 => "minecraft:wolf/variant";
    WolfSoundVariant = 90 => "minecraft:wolf/sound_variant";
    WolfCollar = 91 => "minecraft:wolf/collar";
    FoxVariant = 92 => "minecraft:fox/variant";
    SalmonSize = 93 => "minecraft:salmon/size";
    ParrotVariant = 94 => "minecraft:parrot/variant";
    TropicalFishPattern = 95 => "minecraft:tropical_fish/pattern";
    TropicalFishBaseColor = 96 => "minecraft:tropical_fish/base_color";
    TropicalFishPatternColor = 97 => "minecraft:tropical_fish/pattern_color";
    MooshroomVariant = 98 => "minecraft:mooshroom/variant";
    RabbitVariant = 99 => "minecraft:rabbit/variant";
    PigVariant = 100 => "minecraft:pig/variant";
    PigSoundVariant = 101 => "minecraft:pig/sound_variant";
    CowVariant = 102 => "minecraft:cow/variant";
    CowSoundVariant = 103 => "minecraft:cow/sound_variant";
    ChickenVariant = 104 => "minecraft:chicken/variant";
    ChickenSoundVariant = 105 => "minecraft:chicken/sound_variant";
    ZombieNautilusVariant = 106 => "minecraft:zombie_nautilus/variant";
    FrogVariant = 107 => "minecraft:frog/variant";
    HorseVariant = 108 => "minecraft:horse/variant";
    PaintingVariant = 109 => "minecraft:painting/variant";
    LlamaVariant = 110 => "minecraft:llama/variant";
    AxolotlVariant = 111 => "minecraft:axolotl/variant";
    CatVariant = 112 => "minecraft:cat/variant";
    CatSoundVariant = 113 => "minecraft:cat/sound_variant";
    CatCollar = 114 => "minecraft:cat/collar";
    SheepColor = 115 => "minecraft:sheep/color";
    ShulkerColor = 116 => "minecraft:shulker/color";
    ProvidesPotteryPattern = 117 => "minecraft:provides_pottery_pattern";
    SignTextFront = 118 => "minecraft:sign_text_front";
    SignTextBack = 119 => "minecraft:sign_text_back";
    Waxed = 120 => "minecraft:waxed";
    CushionColor = 121 => "minecraft:cushion/color";
}

impl fmt::Display for ComponentTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id_str())
    }
}
