//! Registry context for validating registry references by id during decoding (spec A.6.4, B.7).

use std::sync::LazyLock;

use zenith_codec::codec::DataResult;
use zenith_registry::Identifier;

use crate::dynval::{Dyn, read_identifier};

/// Result of looking up an entry in a registry (spec A.6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryLookup {
    /// The registry itself is not available in this context (e.g. a dynamic registry when
    /// decoding without registry access).
    Unavailable,
    /// The registry is available, and the entry is not registered in it.
    Missing,
    /// The registry is available, and the entry is registered in it.
    Present,
}

/// Answers, per registry, whether it is available and whether an entry id is registered in it
/// (spec A.6.4).
pub trait RegistryContext {
    /// Looks up `id` in `registry`.
    fn lookup(&self, registry: &Identifier, id: &Identifier) -> RegistryLookup;

    /// Looks up a `#tag` in `registry`.
    ///
    /// Hook for future tag validation: component holder-set codecs do not reject tags yet
    /// until data-pack tags are loaded into the context.
    fn lookup_tag(&self, _registry: &Identifier, _tag: &Identifier) -> RegistryLookup {
        RegistryLookup::Present
    }
}

/// A [`RegistryContext`] in which every registry is [`RegistryLookup::Unavailable`].
///
/// Useful for unit tests and contexts without registry access (spec A.6.4: inline definitions
/// still decode, while references by id fail).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRegistries;

impl RegistryContext for NoRegistries {
    fn lookup(&self, _registry: &Identifier, _id: &Identifier) -> RegistryLookup {
        RegistryLookup::Unavailable
    }
}

/// A [`RegistryContext`] in which every registry and entry is [`RegistryLookup::Present`].
///
/// Useful for schema-only tests or offline loading where full registry contents are not loaded.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllRegistries;

impl RegistryContext for AllowAllRegistries {
    fn lookup(&self, _registry: &Identifier, _id: &Identifier) -> RegistryLookup {
        RegistryLookup::Present
    }
}

/// Checks that `id` is [`RegistryLookup::Present`] in `registry` under `ctx`.
pub fn check_registry_id(
    registry: &Identifier,
    id: &Identifier,
    ctx: &dyn RegistryContext,
) -> Result<(), String> {
    match ctx.lookup(registry, id) {
        RegistryLookup::Present => Ok(()),
        RegistryLookup::Missing => Err(format!("Unknown registry entry {id} in {registry}")),
        RegistryLookup::Unavailable => Err(format!("Registry {registry} does not exist")),
    }
}

/// Reads an identifier from `d` and verifies that it is [`RegistryLookup::Present`] in `registry`.
pub fn read_registry_id(
    d: &Dyn,
    registry: &Identifier,
    ctx: &dyn RegistryContext,
) -> DataResult<Identifier> {
    read_identifier(d).and_then(|id| match check_registry_id(registry, &id, ctx) {
        Ok(()) => DataResult::Success(id),
        Err(m) => DataResult::error(m),
    })
}

/// Standard registry identifiers referenced by text and item component codecs (spec A.4, A.5, B.7).
pub mod ids {
    use super::{Identifier, LazyLock};

    macro_rules! registry_ids {
        ($( $fn_name:ident => $id:literal ; )*) => {
            $(
                #[doc = concat!("Registry identifier `", $id, "`.")]
                pub fn $fn_name() -> &'static Identifier {
                    static ID: LazyLock<Identifier> = LazyLock::new(|| {
                        Identifier::parse($id).expect("valid built-in registry id")
                    });
                    &ID
                }
            )*
        };
    }

    registry_ids! {
        attribute => "minecraft:attribute";
        banner_pattern => "minecraft:banner_pattern";
        block => "minecraft:block";
        block_entity_type => "minecraft:block_entity_type";
        block_transformer => "minecraft:block_transformer";
        cat_sound_variant => "minecraft:cat_sound_variant";
        cat_variant => "minecraft:cat_variant";
        chicken_sound_variant => "minecraft:chicken_sound_variant";
        chicken_variant => "minecraft:chicken_variant";
        cow_sound_variant => "minecraft:cow_sound_variant";
        cow_variant => "minecraft:cow_variant";
        damage_type => "minecraft:damage_type";
        data_component_type => "minecraft:data_component_type";
        decorated_pot_pattern => "minecraft:decorated_pot_pattern";
        dialog => "minecraft:dialog";
        enchantment => "minecraft:enchantment";
        entity_type => "minecraft:entity_type";
        frog_variant => "minecraft:frog_variant";
        instrument => "minecraft:instrument";
        item => "minecraft:item";
        jukebox_song => "minecraft:jukebox_song";
        map_decoration_type => "minecraft:map_decoration_type";
        mob_effect => "minecraft:mob_effect";
        painting_variant => "minecraft:painting_variant";
        pig_sound_variant => "minecraft:pig_sound_variant";
        pig_variant => "minecraft:pig_variant";
        potion => "minecraft:potion";
        sound_event => "minecraft:sound_event";
        trim_material => "minecraft:trim_material";
        trim_pattern => "minecraft:trim_pattern";
        villager_type => "minecraft:villager_type";
        wolf_sound_variant => "minecraft:wolf_sound_variant";
        wolf_variant => "minecraft:wolf_variant";
        zombie_nautilus_variant => "minecraft:zombie_nautilus_variant";
    }
}
