//! Item stacks, item templates, prototype registries, stack validation, and display helpers (spec B.2, B.6).

use std::collections::BTreeMap;

use zenith_codec::codec::DataResult;
use zenith_codec::json::Json;
use zenith_nbt::Tag;
use zenith_registry::Identifier;

use super::patch::{ComponentMap, ComponentPatch, PatchedComponentMap};
use super::registry::ComponentTypeId;
use super::types::{ComponentValue, Rarity};
use crate::dynval::{Dyn, MapBuilder, Num, Rec, read_identifier, read_int};
use crate::text::{Component, Style, TextColor};

/// Maximum item stack count (`1..=99`; spec B.6).
pub const MAX_ITEM_COUNT: i32 = 99;

/// Default stack size when an item prototype does not specify `max_stack_size`.
pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

/// Registry of per-item default component prototypes (spec B.2).
///
/// Prototypes are bound after registries load (and again on reload). Decoding an item stack whose item
/// has no bound prototype fails ("does not have components yet").
#[derive(Debug, Clone, Default)]
pub struct PrototypeRegistry {
    prototypes: BTreeMap<Identifier, ComponentMap>,
}

impl PrototypeRegistry {
    /// Creates an empty prototype registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `prototype` as the default component map for `item_id`.
    pub fn bind(&mut self, item_id: Identifier, prototype: ComponentMap) {
        self.prototypes.insert(item_id, prototype);
    }

    /// Looks up the bound prototype for `item_id`.
    pub fn get(&self, item_id: &Identifier) -> Option<&ComponentMap> {
        self.prototypes.get(item_id)
    }

    /// Number of bound items.
    pub fn len(&self) -> usize {
        self.prototypes.len()
    }

    /// Whether no prototypes are bound.
    pub fn is_empty(&self) -> bool {
        self.prototypes.is_empty()
    }

    /// Returns the effective `max_stack_size` of an item template (falling back to 64 if unbound).
    pub fn effective_max_stack_size(&self, id: &Identifier, patch: &ComponentPatch) -> i32 {
        if let Some(super::patch::PatchEntry::Set(val)) = patch.get(ComponentTypeId::MaxStackSize)
            && let ComponentValue::Int(n) = val.as_ref()
        {
            return *n;
        }
        if let Some(super::patch::PatchEntry::Removed) = patch.get(ComponentTypeId::MaxStackSize) {
            return DEFAULT_MAX_STACK_SIZE;
        }
        self.get(id)
            .and_then(|p| p.get(ComponentTypeId::MaxStackSize))
            .and_then(|v| match v {
                ComponentValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(DEFAULT_MAX_STACK_SIZE)
    }
}

fn is_air(id: &Identifier) -> bool {
    id.namespace() == "minecraft" && id.path() == "air"
}

fn read_item_id(d: &Dyn) -> DataResult<Identifier> {
    read_identifier(d).and_then(|id| {
        if is_air(&id) {
            DataResult::error("Item must not be minecraft:air")
        } else {
            DataResult::Success(id)
        }
    })
}

fn read_count(d: &Dyn) -> DataResult<i32> {
    read_int(d).and_then(|c| {
        if (1..=MAX_ITEM_COUNT).contains(&c) {
            DataResult::Success(c)
        } else {
            DataResult::error(format!("Item count {c} outside range 1..={MAX_ITEM_COUNT}"))
        }
    })
}

/// An item template (used inside other components such as `container`, `bundle_contents`,
/// `charged_projectiles`, `use_remainder`, `pot_decorations`, and `hover_event` `show_item`; spec B.6).
///
/// - Accepts a bare item-id string on decode (`"minecraft:stone"`).
/// - Always encodes as an object, omitting `count` when 1 and `components` when empty.
/// - Cannot be empty (`minecraft:air` is rejected).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemTemplate {
    /// Item identifier (never `minecraft:air`).
    pub id: Identifier,
    /// Item count (`1..=99`, default 1, omitted on encode when 1).
    pub count: i32,
    /// Unsanitized component patch (omitted on encode when empty).
    pub components: ComponentPatch,
}

impl ItemTemplate {
    /// Creates a template with count 1 and an empty patch.
    pub fn new(id: Identifier) -> Self {
        Self {
            id,
            count: 1,
            components: ComponentPatch::new(),
        }
    }

    /// Decodes a template from either a bare item-id string or a map.
    pub fn decode(d: &Dyn) -> DataResult<Self> {
        if matches!(d, Dyn::Str(_)) {
            return read_item_id(d).map(Self::new);
        }
        Self::decode_record(d)
    }

    /// Decodes a template from a map (`id`, optional `count` 1..=99, optional `components`).
    pub fn decode_record(d: &Dyn) -> DataResult<Self> {
        let mut r = Rec::new(d);
        let id = r.required("id", read_item_id);
        let count = r.optional("count", 1, read_count);
        let components = r.optional("components", ComponentPatch::new(), ComponentPatch::decode);
        r.finish(|| {
            Some(Self {
                id: id?,
                count,
                components,
            })
        })
    }

    /// Decodes a template from [`Json`].
    pub fn decode_json(json: &Json) -> DataResult<Self> {
        Self::decode(&Dyn::from_json(json))
    }

    /// Decodes a template from an NBT [`Tag`].
    pub fn decode_nbt(tag: &Tag) -> DataResult<Self> {
        Self::decode(&Dyn::from_tag(tag))
    }

    /// Encodes as an object (`count` omitted when 1, `components` omitted when empty).
    pub fn encode(&self) -> DataResult<Dyn> {
        self.encode_record()
    }

    /// Encodes as a record map (also used inline by `hover_event` `show_item`).
    pub fn encode_record(&self) -> DataResult<Dyn> {
        if is_air(&self.id) {
            return DataResult::error("Item must not be minecraft:air");
        }
        if !(1..=MAX_ITEM_COUNT).contains(&self.count) {
            return DataResult::error(format!(
                "Item count {} outside range 1..={MAX_ITEM_COUNT}",
                self.count
            ));
        }
        let mut b = MapBuilder::new();
        b.put("id", Dyn::string(self.id.to_string()));
        if self.count != 1 {
            b.put("count", Dyn::Num(Num::Int(self.count)));
        }
        if !self.components.is_empty() {
            match self.components.encode() {
                DataResult::Success(v) => {
                    b.put("components", v);
                }
                other => return other,
            }
        }
        DataResult::Success(b.build())
    }

    /// Materializes this template into a validated [`ItemStack`] against `prototypes` (spec B.6).
    pub fn to_stack(&self, prototypes: &PrototypeRegistry) -> Result<ItemStack, String> {
        let stack =
            ItemStack::from_parts(self.id.clone(), self.count, &self.components, prototypes)?;
        stack.validate(prototypes)?;
        Ok(stack)
    }

    /// Materializes this template into an [`ItemStack`], yielding the empty stack and a warning
    /// message if validation fails (spec B.6).
    pub fn to_stack_or_empty(&self, prototypes: &PrototypeRegistry) -> (ItemStack, Option<String>) {
        match self.to_stack(prototypes) {
            Ok(stack) => (stack, None),
            Err(warn) => (ItemStack::empty(), Some(warn)),
        }
    }
}

/// An item stack: either empty or `(id, count, sanitized PatchedComponentMap)` (spec B.6).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStack {
    inner: Option<NonEmptyStack>,
}

#[derive(Debug, Clone, PartialEq)]
struct NonEmptyStack {
    id: Identifier,
    count: i32,
    components: PatchedComponentMap,
}

impl Default for ItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

impl ItemStack {
    /// The empty stack.
    pub const fn empty() -> Self {
        Self { inner: None }
    }

    /// Whether this is the empty stack.
    pub fn is_empty(&self) -> bool {
        self.inner.is_none()
    }

    /// Builds a non-empty stack from `(id, count, patch)` using the bound prototype in `prototypes`.
    /// Does **not** run stack validation (call [`ItemStack::validate`] where validation applies).
    pub fn from_parts(
        id: Identifier,
        count: i32,
        patch: &ComponentPatch,
        prototypes: &PrototypeRegistry,
    ) -> Result<Self, String> {
        if is_air(&id) {
            return Err("Item must not be minecraft:air".to_owned());
        }
        if !(1..=MAX_ITEM_COUNT).contains(&count) {
            return Err(format!(
                "Item count {count} outside range 1..={MAX_ITEM_COUNT}"
            ));
        }
        let proto = prototypes
            .get(&id)
            .ok_or_else(|| format!("Item {id} does not have components yet"))?
            .clone();
        let components = PatchedComponentMap::from_prototype_and_patch(proto, patch);
        Ok(Self {
            inner: Some(NonEmptyStack {
                id,
                count,
                components,
            }),
        })
    }

    /// Builds a stack with count 1 and default components for `id`.
    pub fn of(id: Identifier, prototypes: &PrototypeRegistry) -> Result<Self, String> {
        Self::from_parts(id, 1, &ComponentPatch::new(), prototypes)
    }

    /// The item identifier, or `None` if empty.
    pub fn id(&self) -> Option<&Identifier> {
        self.inner.as_ref().map(|s| &s.id)
    }

    /// The item count (`0` if empty).
    pub fn count(&self) -> i32 {
        self.inner.as_ref().map_or(0, |s| s.count)
    }

    /// The patched component map, or `None` if empty.
    pub fn components(&self) -> Option<&PatchedComponentMap> {
        self.inner.as_ref().map(|s| &s.components)
    }

    /// Mutable access to the patched component map, or `None` if empty.
    pub fn components_mut(&mut self) -> Option<&mut PatchedComponentMap> {
        self.inner.as_mut().map(|s| &mut s.components)
    }

    /// Effective `max_stack_size` of this stack (`1` when `max_stack_size` component is absent).
    pub fn max_stack_size(&self) -> i32 {
        self.components()
            .and_then(|c| c.get(ComponentTypeId::MaxStackSize))
            .and_then(|v| match v {
                ComponentValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(1)
    }

    /// Applies `patch` with validation (used by the `set_components` loot function; spec B.3 rule 7).
    /// Restores the previous patch if stack validation fails.
    pub fn apply_patch_validated(
        &mut self,
        patch: &ComponentPatch,
        prototypes: &PrototypeRegistry,
    ) -> Result<(), String> {
        let Some(inner) = &mut self.inner else {
            return Ok(());
        };
        let old_patch = inner.components.patch().clone();
        inner.components.apply_patch(patch);
        if let Err(e) = self.validate(prototypes) {
            if let Some(inner) = &mut self.inner {
                inner.components.restore_patch(old_patch);
            }
            return Err(e);
        }
        Ok(())
    }

    /// Runs the four stack-validation rules of spec B.6 (edge case 42):
    /// 1. Cannot have both `max_damage` and `max_stack_size > 1`.
    /// 2. Items inside `container`, `bundle_contents`, and `charged_projectiles` must not exceed
    ///    their own max stack size.
    /// 3. `bundle_contents` total weight must be computable without arithmetic overflow.
    /// 4. Stack `count` must not exceed `max_stack_size`.
    pub fn validate(&self, prototypes: &PrototypeRegistry) -> Result<(), String> {
        let Some(inner) = &self.inner else {
            return Ok(());
        };
        let max_stack = self.max_stack_size();
        // Rule 1
        if inner.components.contains(ComponentTypeId::MaxDamage) && max_stack > 1 {
            return Err(format!(
                "Item {} cannot have both max_damage and max_stack_size > 1 ({max_stack})",
                inner.id
            ));
        }
        // Rule 2: nested items in container / bundle_contents / charged_projectiles
        if let Some(ComponentValue::Container(c)) = inner.components.get(ComponentTypeId::Container)
        {
            for (_, item) in c.non_empty_entries() {
                validate_nested_template(item, prototypes)?;
            }
        }
        if let Some(ComponentValue::ChargedProjectiles(items)) =
            inner.components.get(ComponentTypeId::ChargedProjectiles)
        {
            for item in items {
                validate_nested_template(item, prototypes)?;
            }
        }
        if let Some(ComponentValue::BundleContents(items)) =
            inner.components.get(ComponentTypeId::BundleContents)
        {
            for item in items {
                validate_nested_template(item, prototypes)?;
            }
            // Rule 3: bundle weight must be computable without overflow
            compute_bundle_weight(items, prototypes)?;
        }
        // Rule 4
        if inner.count > max_stack {
            return Err(format!(
                "Item count {} exceeds max_stack_size {max_stack}",
                inner.count
            ));
        }
        Ok(())
    }

    /// Decodes a non-empty `ItemStack` from [`Dyn`] (spec B.6).
    ///
    /// Does **not** run stack validation (edge case 42); does **not** accept bare id strings.
    pub fn decode(d: &Dyn, prototypes: &PrototypeRegistry) -> DataResult<Self> {
        let mut r = Rec::new(d);
        let id = r.required("id", read_item_id);
        let count = r.optional("count", 1, read_count);
        let components = r.optional("components", ComponentPatch::new(), ComponentPatch::decode);
        let Some(id_val) = id else {
            return r.finish(|| None);
        };
        let Some(proto) = prototypes.get(&id_val).cloned() else {
            r.push_error(format!("Item {id_val} does not have components yet"));
            return r.finish(|| None);
        };
        r.finish(|| {
            Some(Self {
                inner: Some(NonEmptyStack {
                    id: id_val,
                    count,
                    components: PatchedComponentMap::from_prototype_and_patch(proto, &components),
                }),
            })
        })
    }

    /// Decodes an optional `ItemStack`: `{}` decodes as [`ItemStack::empty`].
    pub fn decode_optional(d: &Dyn, prototypes: &PrototypeRegistry) -> DataResult<Self> {
        if let Dyn::Map(m) = d
            && m.is_empty()
        {
            return DataResult::Success(Self::empty());
        }
        Self::decode(d, prototypes)
    }

    /// Decodes from [`Json`].
    pub fn decode_json(json: &Json, prototypes: &PrototypeRegistry) -> DataResult<Self> {
        Self::decode(&Dyn::from_json(json), prototypes)
    }

    /// Decodes from an NBT [`Tag`].
    pub fn decode_nbt(tag: &Tag, prototypes: &PrototypeRegistry) -> DataResult<Self> {
        Self::decode(&Dyn::from_tag(tag), prototypes)
    }

    /// Encodes a non-empty `ItemStack` to [`Dyn`] (spec B.6, edge case 41):
    /// `count` is **always written**; `components` is omitted when the patch is empty.
    pub fn encode(&self) -> DataResult<Dyn> {
        let Some(inner) = &self.inner else {
            return DataResult::error("Cannot encode empty ItemStack with non-optional codec");
        };
        let mut b = MapBuilder::new();
        b.put("id", Dyn::string(inner.id.to_string()));
        b.put("count", Dyn::Num(Num::Int(inner.count)));
        if !inner.components.patch().is_empty() {
            match inner.components.patch().encode() {
                DataResult::Success(v) => {
                    b.put("components", v);
                }
                other => return other,
            }
        }
        DataResult::Success(b.build())
    }

    /// Encodes an optional `ItemStack`: empty stacks encode as `{}`.
    pub fn encode_optional(&self) -> DataResult<Dyn> {
        if self.is_empty() {
            return DataResult::Success(Dyn::empty_map());
        }
        self.encode()
    }

    /// Encodes to [`Json`].
    pub fn encode_json(&self) -> DataResult<Json> {
        self.encode().and_then(|d| match d.to_json() {
            Ok(j) => DataResult::Success(j),
            Err(m) => DataResult::error(m),
        })
    }

    /// Encodes to an NBT [`Tag`].
    pub fn encode_nbt(&self) -> DataResult<Tag> {
        self.encode().and_then(|d| match d.to_tag() {
            Ok(t) => DataResult::Success(t),
            Err(m) => DataResult::error(m),
        })
    }

    /// Unstyled hover name (spec B.6):
    /// `custom_name` if set, else the written-book title (if non-blank), else `item_name`, else empty.
    pub fn hover_name(&self) -> Component {
        let Some(c) = self.components() else {
            return Component::empty();
        };
        if let Some(ComponentValue::Text(name)) = c.get(ComponentTypeId::CustomName) {
            return (**name).clone();
        }
        if let Some(ComponentValue::WrittenBookContent(book)) =
            c.get(ComponentTypeId::WrittenBookContent)
            && !book.title.raw.trim().is_empty()
        {
            return Component::literal(&book.title.raw);
        }
        if let Some(ComponentValue::Text(name)) = c.get(ComponentTypeId::ItemName) {
            return (**name).clone();
        }
        Component::empty()
    }

    /// Styled hover name (spec B.6): wrapped in the stack's rarity colour, and italic **only** when
    /// `custom_name` is set.
    pub fn styled_hover_name(&self) -> Component {
        let Some(c) = self.components() else {
            return Component::empty();
        };
        let rarity = match c.get(ComponentTypeId::Rarity) {
            Some(ComponentValue::Rarity(r)) => *r,
            _ => Rarity::Common,
        };
        let has_custom_name = c.contains(ComponentTypeId::CustomName);
        let mut style = Style::empty().with_color(rarity.color());
        if has_custom_name {
            style = style.with_italic(true);
        }
        Component::empty()
            .with_style(style)
            .append(self.hover_name())
    }
}

/// Base parent style applied to every item lore line when rendering (dark purple + italic; spec B.6).
pub fn lore_base_style() -> Style {
    Style::empty()
        .with_color(TextColor::named("dark_purple").expect("valid color"))
        .with_italic(true)
}

fn validate_nested_template(
    item: &ItemTemplate,
    prototypes: &PrototypeRegistry,
) -> Result<(), String> {
    let max_stack = prototypes.effective_max_stack_size(&item.id, &item.components);
    if item.count > max_stack {
        return Err(format!(
            "Nested item {} count {} exceeds its max_stack_size {max_stack}",
            item.id, item.count
        ));
    }
    if let Some(super::patch::PatchEntry::Set(val)) =
        item.components.get(ComponentTypeId::Container)
        && let ComponentValue::Container(c) = val.as_ref()
    {
        for (_, sub) in c.non_empty_entries() {
            validate_nested_template(sub, prototypes)?;
        }
    }
    if let Some(super::patch::PatchEntry::Set(val)) =
        item.components.get(ComponentTypeId::ChargedProjectiles)
        && let ComponentValue::ChargedProjectiles(items) = val.as_ref()
    {
        for sub in items {
            validate_nested_template(sub, prototypes)?;
        }
    }
    if let Some(super::patch::PatchEntry::Set(val)) =
        item.components.get(ComponentTypeId::BundleContents)
        && let ComponentValue::BundleContents(items) = val.as_ref()
    {
        for sub in items {
            validate_nested_template(sub, prototypes)?;
        }
        compute_bundle_weight(items, prototypes)?;
    }
    Ok(())
}

/// Exact reduced signed 32-bit rational fraction (matching Apache Commons Math `Fraction` overflow semantics
/// used for bundle weights in spec B.6 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeightFraction {
    /// Numerator.
    pub num: i32,
    /// Positive denominator.
    pub den: i32,
}

impl WeightFraction {
    /// Zero (`0/1`).
    pub const ZERO: Self = Self { num: 0, den: 1 };
    /// One (`1/1`).
    pub const ONE: Self = Self { num: 1, den: 1 };
    /// Empty nested bundle overhead (`1/16`).
    pub const BUNDLE_OVERHEAD: Self = Self { num: 1, den: 16 };

    /// Creates a reduced fraction.
    pub fn new(num: i32, den: i32) -> Result<Self, String> {
        if den == 0 {
            return Err("Bundle weight division by zero".to_owned());
        }
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i32;
        let (n, d) = if den < 0 {
            (
                num.checked_neg().ok_or("Bundle weight overflow")? / g,
                den.checked_neg().ok_or("Bundle weight overflow")? / g,
            )
        } else {
            (num / g, den / g)
        };
        Ok(Self { num: n, den: d })
    }

    /// Checked addition.
    pub fn checked_add(self, rhs: Self) -> Result<Self, String> {
        let n = i64::from(self.num)
            .checked_mul(i64::from(rhs.den))
            .and_then(|a| {
                i64::from(rhs.num)
                    .checked_mul(i64::from(self.den))
                    .and_then(|b| a.checked_add(b))
            })
            .ok_or("Bundle weight arithmetic overflow")?;
        let d = i64::from(self.den)
            .checked_mul(i64::from(rhs.den))
            .ok_or("Bundle weight arithmetic overflow")?;
        let g = gcd_u64(n.unsigned_abs(), d.unsigned_abs()) as i64;
        let rn = i32::try_from(n / g).map_err(|_| "Bundle weight numerator overflow".to_owned())?;
        let rd =
            i32::try_from(d / g).map_err(|_| "Bundle weight denominator overflow".to_owned())?;
        Ok(Self { num: rn, den: rd })
    }

    /// Checked multiplication by an integer count.
    pub fn checked_mul_i32(self, count: i32) -> Result<Self, String> {
        let n = i64::from(self.num)
            .checked_mul(i64::from(count))
            .ok_or("Bundle weight arithmetic overflow")?;
        let g = gcd_u64(n.unsigned_abs(), self.den.unsigned_abs() as u64) as i64;
        let rn = i32::try_from(n / g).map_err(|_| "Bundle weight numerator overflow".to_owned())?;
        let rd = self.den / (g as i32);
        Ok(Self { num: rn, den: rd })
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

/// Computes the total bundle weight of `items` (spec B.6 rule 3):
/// - A non-empty `bees` item weighs `1`.
/// - A nested bundle weighs `own content weight + 1/16`.
/// - Any other item weighs `1 / max_stack_size` per unit.
/// - Arithmetic overflow is an error.
pub fn compute_bundle_weight(
    items: &[ItemTemplate],
    prototypes: &PrototypeRegistry,
) -> Result<WeightFraction, String> {
    let mut total = WeightFraction::ZERO;
    for item in items {
        let unit = item_bundle_unit_weight(item, prototypes)?;
        let contrib = unit.checked_mul_i32(item.count)?;
        total = total.checked_add(contrib)?;
    }
    Ok(total)
}

fn item_bundle_unit_weight(
    item: &ItemTemplate,
    prototypes: &PrototypeRegistry,
) -> Result<WeightFraction, String> {
    // Check bees (patch or prototype)
    let has_bees = match item.components.get(ComponentTypeId::Bees) {
        Some(super::patch::PatchEntry::Set(v)) => {
            matches!(v.as_ref(), ComponentValue::Bees(b) if !b.is_empty())
        }
        Some(super::patch::PatchEntry::Removed) => false,
        _ => prototypes
            .get(&item.id)
            .and_then(|p| p.get(ComponentTypeId::Bees))
            .is_some_and(|v| matches!(v, ComponentValue::Bees(b) if !b.is_empty())),
    };
    if has_bees {
        return Ok(WeightFraction::ONE);
    }

    // Check nested bundle_contents (patch or prototype)
    let nested_bundle: Option<&[ItemTemplate]> =
        match item.components.get(ComponentTypeId::BundleContents) {
            Some(super::patch::PatchEntry::Set(v)) => match v.as_ref() {
                ComponentValue::BundleContents(b) => Some(b.as_slice()),
                _ => None,
            },
            Some(super::patch::PatchEntry::Removed) => None,
            _ => prototypes
                .get(&item.id)
                .and_then(|p| p.get(ComponentTypeId::BundleContents))
                .and_then(|v| match v {
                    ComponentValue::BundleContents(b) => Some(b.as_slice()),
                    _ => None,
                }),
        };
    if let Some(nested) = nested_bundle {
        let inner_weight = compute_bundle_weight(nested, prototypes)?;
        return WeightFraction::BUNDLE_OVERHEAD.checked_add(inner_weight);
    }

    let max_stack = prototypes.effective_max_stack_size(&item.id, &item.components);
    WeightFraction::new(1, max_stack.max(1))
}
