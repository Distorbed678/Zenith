//! Comprehensive tests covering all 15 spec edge cases plus identifier fuzz-style tables
//! and subsystem invariants.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use zenith_registry::{
    BlockDef, BlockStateError, BlockStateTable, ClientRegistryAssembler,
    DEFAULTED_BUILTIN_REGISTRIES, DataPackFeatureInfo, FEATURE_FILTERED_REGISTRIES, FeatureElement,
    FeatureError, FeatureFilteredRegistry, FeatureFlagSet, FeatureUniverse, FeatureUniverseBuilder,
    Holder, HolderError, HolderSet, INTRUSIVE_HOLDER_REGISTRIES, Identifier, IdentifierError,
    KnownPack, Lifecycle, MAX_FLAGS_PER_UNIVERSE, MemoryTagSource, NETWORK_MAX_UTF16_UNITS,
    NON_EMPTY_SYNCHRONIZED_REGISTRIES, PackSelectionMode, PropertyDef, RELOADABLE_REGISTRIES,
    RegistrationInfo, Registry, RegistryError, ResourceKey, SYNCHRONIZED_DYNAMIC_REGISTRIES,
    SyncRegistryEntry, TagDiagnostic, TagEntry, TagFile, TagKey, TagLoader, TagParseError,
    TagWireError, assign_data_driven_ids, build_sync_entries, decode_holder_registry_only,
    decode_holder_set, decode_holder_with_inline, decode_registry_tags,
    encode_holder_registry_only, encode_holder_set, encode_holder_with_inline,
    encode_registry_tags, encode_update_tags_payload, escape_java_string, is_valid_command_char,
    is_valid_namespace_char, is_valid_path_char, negotiate_known_packs, select_data_packs,
};

fn id(s: &str) -> Identifier {
    Identifier::parse(s).unwrap_or_else(|e| panic!("expected valid identifier {s:?}: {e}"))
}

// =============================================================================
// Edge case 1: Identifier empty parts and colon handling
// =============================================================================

#[test]
fn edge_case_01_identifier_empty_parts_and_colons() {
    let empty = Identifier::parse("").unwrap();
    assert_eq!(empty.namespace(), "minecraft");
    assert_eq!(empty.path(), "");
    assert_eq!(empty.to_string(), "minecraft:");
    assert_eq!(empty.to_short_string(), "");

    let colon = Identifier::parse(":").unwrap();
    assert_eq!(colon, empty);
    assert_eq!(colon.namespace(), "minecraft");
    assert_eq!(colon.path(), "");

    let leading_colon = Identifier::parse(":foo").unwrap();
    assert_eq!(leading_colon.namespace(), "minecraft");
    assert_eq!(leading_colon.path(), "foo");
    assert_eq!(leading_colon.to_string(), "minecraft:foo");
    assert_eq!(leading_colon.to_short_string(), "foo");

    // Two-argument construction with empty namespace succeeds.
    let empty_ns = Identifier::from_parts("", "foo").unwrap();
    assert_eq!(empty_ns.namespace(), "");
    assert_eq!(empty_ns.path(), "foo");
    assert_eq!(empty_ns.to_string(), ":foo");
    assert_eq!(empty_ns.to_short_string(), ":foo");

    // Two-argument construction with empty path succeeds.
    let empty_path = Identifier::from_parts("ns", "").unwrap();
    assert_eq!(empty_path.namespace(), "ns");
    assert_eq!(empty_path.path(), "");
    assert_eq!(empty_path.to_string(), "ns:");

    // Split on first colon: "a:b:c" has ns="a", path="b:c" -> fails path validation.
    let err = Identifier::parse("a:b:c").unwrap_err();
    assert!(matches!(err, IdentifierError::InvalidPath { .. }));

    // "::" -> ns="minecraft", path=":" -> fails path validation.
    let err2 = Identifier::parse("::").unwrap_err();
    assert!(matches!(err2, IdentifierError::InvalidPath { .. }));
}

// =============================================================================
// Edge case 2: Namespace ".." rejection vs other dots + filesystem root resolution
// =============================================================================

#[test]
fn edge_case_02_namespace_dots_and_filesystem_resolution() {
    let err = Identifier::parse("..:foo").unwrap_err();
    assert!(matches!(err, IdentifierError::InvalidNamespace { .. }));
    assert!(matches!(
        Identifier::from_parts("..", "foo"),
        Err(IdentifierError::InvalidNamespace { .. })
    ));

    // Other dot-containing namespaces pass validation.
    for valid in [".:foo", "...:foo", "a..b:foo", ".a:foo", "a.:foo"] {
        assert!(Identifier::parse(valid).is_ok(), "expected {valid} to pass");
    }

    // Path traversal passes identifier validation, rejected by filesystem root resolution.
    let traversal = Identifier::parse("minecraft:../secret").unwrap();
    let root = Path::new("/pack/data");
    assert!(matches!(
        traversal.resolve_under(root),
        Err(IdentifierError::PathEscapesRoot { .. })
    ));

    let double_traversal = Identifier::parse("minecraft:../../etc/passwd").unwrap();
    assert!(matches!(
        double_traversal.resolve_under(root),
        Err(IdentifierError::PathEscapesRoot { .. })
    ));

    let leading_slash = Identifier::parse("minecraft:/etc/passwd").unwrap();
    assert!(matches!(
        leading_slash.resolve_under(root),
        Err(IdentifierError::PathEscapesRoot { .. })
    ));

    let normal = Identifier::parse("minecraft:tags/block/mineable/pickaxe.json").unwrap();
    assert_eq!(
        normal.resolve_under(root).unwrap(),
        Path::new("/pack/data/minecraft/tags/block/mineable/pickaxe.json")
    );
}

// =============================================================================
// Edge case 3: Uppercase / non-ASCII rejection and Java-style escaped error messages
// =============================================================================

#[test]
fn edge_case_03_uppercase_non_ascii_and_escaped_errors() {
    let upper = Identifier::parse("Minecraft:stone").unwrap_err();
    assert!(matches!(upper, IdentifierError::InvalidNamespace { .. }));
    assert!(upper.to_string().contains("Minecraft:stone"));

    let upper_path = Identifier::parse("minecraft:Stone").unwrap_err();
    assert!(matches!(upper_path, IdentifierError::InvalidPath { .. }));

    // Control characters and non-ASCII must be Java-escaped, never raw.
    let ctrl = Identifier::parse("minecraft:bad\n\t\x00\u{00E9}\u{1F600}").unwrap_err();
    let msg = ctrl.to_string();
    assert!(!msg.contains('\n'), "newline must be escaped: {msg}");
    assert!(!msg.contains('\t'), "tab must be escaped: {msg}");
    assert!(!msg.contains('\0'), "NUL must be escaped: {msg}");
    assert!(msg.is_ascii(), "non-ASCII must be escaped: {msg}");
    assert!(msg.contains("\\n"));
    assert!(msg.contains("\\t"));
    assert!(msg.contains("\\u0000"));
    assert!(msg.contains("\\u00E9"));
    // U+1F600 -> surrogate pair \uD83D\uDE00
    assert!(msg.contains("\\uD83D\\uDE00"));

    assert_eq!(
        escape_java_string("a\"\\b\r\x08\x0c"),
        "a\\\"\\\\b\\r\\b\\f"
    );

    // Serde JSON decode wraps identifier error with "Not a valid resource location".
    let json_err = serde_json::from_str::<Identifier>("\"Minecraft:stone\"").unwrap_err();
    assert!(
        json_err
            .to_string()
            .contains("Not a valid resource location")
    );
}

// =============================================================================
// Edge case 4: Identifier natural ordering (path first, then namespace)
// =============================================================================

#[test]
fn edge_case_04_identifier_natural_ordering() {
    assert!(id("z:a") < id("a:b"));
    assert!(id("alpha:foo") < id("minecraft:foo"));
    assert!(id("minecraft:a") < id("a:b"));
    assert!(id("a:same") < id("b:same"));

    let mut list = vec![
        id("minecraft:foo"),
        id("a:b"),
        id("z:a"),
        id("alpha:foo"),
        id("b:a"),
    ];
    list.sort();
    assert_eq!(
        list,
        vec![
            id("b:a"),
            id("z:a"),
            id("a:b"),
            id("alpha:foo"),
            id("minecraft:foo"),
        ]
    );
}

// =============================================================================
// Identifier fuzz-style tables (exhaustive ASCII + formatting + command + wire + keys)
// =============================================================================

#[test]
fn identifier_ascii_fuzz_table_and_formatting() {
    // Exhaustively test every ASCII byte 0x00..=0x7F in namespace, path, and command stream.
    for byte in 0u8..=127u8 {
        let ch = byte as char;
        let ns_allowed = matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.');
        let path_allowed = ns_allowed || ch == '/';
        let cmd_allowed = path_allowed || ch == ':';

        assert_eq!(is_valid_namespace_char(ch), ns_allowed, "ns byte {byte:#x}");
        assert_eq!(is_valid_path_char(ch), path_allowed, "path byte {byte:#x}");
        assert_eq!(is_valid_command_char(ch), cmd_allowed, "cmd byte {byte:#x}");

        let ns_res = Identifier::from_parts(ch.to_string(), "ok");
        assert_eq!(ns_res.is_ok(), ns_allowed, "from_parts ns {ch:?}");

        let path_res = Identifier::from_parts("ok", ch.to_string());
        assert_eq!(path_res.is_ok(), path_allowed, "from_parts path {ch:?}");

        if ch != ':' {
            let single = format!("a{ch}b:c");
            assert_eq!(
                Identifier::parse(&single).is_ok(),
                ns_allowed,
                "parse ns {single:?}"
            );
            let single_p = format!("a:b{ch}c");
            assert_eq!(
                Identifier::parse(&single_p).is_ok(),
                path_allowed,
                "parse path {single_p:?}"
            );
        }
    }

    // Formatting forms (Section 1.4).
    let vanilla = id("minecraft:block/stone");
    assert_eq!(vanilla.to_string(), "minecraft:block/stone");
    assert_eq!(vanilla.to_short_string(), "block/stone");
    assert_eq!(vanilla.to_language_key(), "minecraft.block/stone");
    assert_eq!(vanilla.to_short_language_key(), "block/stone");
    assert_eq!(
        vanilla.to_language_key_prefixed("block"),
        "block.minecraft.block/stone"
    );
    assert_eq!(
        vanilla.to_language_key_prefixed_suffixed("block", "desc"),
        "block.minecraft.block/stone.desc"
    );
    assert_eq!(vanilla.to_debug_file_name(), "minecraft_block_stone");

    let modded = id("zenith:sub/thing");
    assert_eq!(modded.to_short_string(), "zenith:sub/thing");
    assert_eq!(modded.to_short_language_key(), "zenith.sub/thing");
    assert_eq!(modded.to_debug_file_name(), "zenith_sub_thing");

    // Command-stream parsing (Section 1.2.2).
    assert_eq!(
        Identifier::parse_command("minecraft:stone extra", true).unwrap(),
        (id("minecraft:stone"), 15)
    );
    assert_eq!(
        Identifier::parse_command(" extra", false).unwrap(),
        (id("minecraft:"), 0)
    );
    assert_eq!(
        Identifier::parse_command(" extra", true).unwrap_err(),
        IdentifierError::EmptyCommandIdentifier
    );
    assert!(Identifier::parse_command("a:b:c rest", true).is_err());
    assert!(Identifier::parse_command("..:foo rest", true).is_err());

    // Network wire length limits (Section 1.3).
    let mut wire = Vec::new();
    vanilla.encode_network(&mut wire).unwrap();
    let (decoded, used) = Identifier::decode_network(&wire).unwrap();
    assert_eq!(decoded, vanilla);
    assert_eq!(used, wire.len());

    // Overlong identifier (> 32,767 chars) rejected on encode and decode.
    let too_long_path = "a".repeat(NETWORK_MAX_UTF16_UNITS);
    let too_long = Identifier::with_default_namespace(too_long_path).unwrap();
    assert!(matches!(
        too_long.validate_network_len(),
        Err(IdentifierError::NetworkEncoding { .. })
    ));
    // Negative length on wire rejected before identifier validation.
    let neg_varint = [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]; // -1
    assert!(matches!(
        Identifier::decode_network(&neg_varint),
        Err(IdentifierError::NetworkEncoding { .. })
    ));

    // ResourceKey & TagKey (Section 1.6).
    let block_reg_key = ResourceKey::registry_key(id("minecraft:block"));
    assert_eq!(block_reg_key.registry(), &id("minecraft:root"));
    assert_eq!(block_reg_key.location(), &id("minecraft:block"));
    let stone_key = ResourceKey::for_registry(&block_reg_key, id("minecraft:stone"));
    assert!(stone_key.is_for(&block_reg_key));

    let tag_hashed = TagKey::parse_hashed(block_reg_key.clone(), "#logs").unwrap();
    assert_eq!(tag_hashed.location(), &id("minecraft:logs"));
    assert_eq!(tag_hashed.to_hashed_string(), "#minecraft:logs");
    assert!(matches!(
        TagKey::parse_hashed(block_reg_key, "logs"),
        Err(IdentifierError::NotATagId { .. })
    ));
}

// =============================================================================
// Edge case 5: Data-driven numeric ID ordering with .json suffix attached
// =============================================================================

#[test]
fn edge_case_05_data_driven_ids_sorted_with_json_suffix() {
    let files = vec![
        id("minecraft:damage_type/foo.json"),
        id("minecraft:damage_type/foo_bar.json"),
        id("minecraft:damage_type/foo/bar.json"),
        id("minecraft:damage_type/foo.a.json"),
        id("minecraft:damage_type/foo-bar.json"),
    ];

    let assigned = assign_data_driven_ids("damage_type", &files).unwrap();
    let expected = vec![
        (0, id("minecraft:foo-bar")), // '-' = 45
        (1, id("minecraft:foo.a")),   // '.' = 46, 'a' < 'j'
        (2, id("minecraft:foo")),     // ".json"
        (3, id("minecraft:foo/bar")), // '/' = 47
        (4, id("minecraft:foo_bar")), // '_' = 95
    ];
    assert_eq!(assigned, expected);

    // Cross-namespace tie-breaking: path first, namespace second.
    let cross_ns = vec![
        id("minecraft:worldgen/biome/plains.json"),
        id("alpha:worldgen/biome/plains.json"),
        id("zebra:worldgen/biome/badlands.json"),
    ];
    let assigned_ns = assign_data_driven_ids("worldgen/biome", &cross_ns).unwrap();
    assert_eq!(
        assigned_ns,
        vec![
            (0, id("zebra:badlands")),
            (1, id("alpha:plains")),
            (2, id("minecraft:plains")),
        ]
    );
}

// =============================================================================
// Edge case 6: Defaulted registry selective fallbacks
// =============================================================================

#[test]
fn edge_case_06_defaulted_registry_selective_fallbacks() {
    let mut reg: Registry<&str> =
        Registry::new_defaulted(id("minecraft:block"), id("minecraft:air"));
    let stone_arc = Arc::new("stone");
    let air_arc = Arc::new("air");
    let unregistered_arc = Arc::new("unregistered");

    reg.register_arc(
        id("minecraft:stone"),
        Arc::clone(&stone_arc),
        RegistrationInfo::built_in(),
    )
    .unwrap();
    reg.register_arc(
        id("minecraft:air"),
        Arc::clone(&air_arc),
        RegistrationInfo::built_in(),
    )
    .unwrap();
    reg.freeze().unwrap();
    reg.validate(true).unwrap();

    let missing = id("minecraft:nonexistent");
    let missing_key = ResourceKey::for_registry(reg.key(), missing.clone());

    // Direct value lookup by identifier -> FALLS BACK to default ("air").
    assert!(Arc::ptr_eq(reg.get_value(&missing).unwrap(), &air_arc));
    // Optional value lookup by identifier -> EMPTY (NO fallback).
    assert!(reg.get_optional(&missing).is_none());
    // Holder lookups (by id, key, numeric ID) -> EMPTY (NO fallback).
    assert!(reg.get_holder(&missing).is_none());
    assert!(reg.get_holder_by_key(&missing_key).is_none());
    assert!(reg.get_holder_by_id(-1).is_none());
    assert!(reg.get_holder_by_id(99).is_none());
    // Value lookup by ResourceKey -> ABSENT (NO fallback).
    assert!(reg.get_value_by_key(&missing_key).is_none());
    // Value lookup by out-of-range numeric ID -> FALLS BACK to default.
    assert!(Arc::ptr_eq(reg.by_id(-1).unwrap(), &air_arc));
    assert!(Arc::ptr_eq(reg.by_id(99).unwrap(), &air_arc));
    // Numeric ID lookup by unregistered value -> default value's numeric ID (1, since stone=0, air=1).
    assert_eq!(reg.id_of(&unregistered_arc), 1);
    assert_eq!(reg.id_of_opt(None), 1);
    // Identifier lookup by unregistered value -> default identifier.
    assert_eq!(
        reg.identifier_of(&unregistered_arc),
        Some(&id("minecraft:air"))
    );
    // Any-entry lookup -> default entry (id 1 = air, NOT id 0 = stone).
    assert_eq!(
        reg.get_any().unwrap().key().unwrap().unwrap().location(),
        &id("minecraft:air")
    );

    // Compare against a non-defaulted registry.
    let mut non_def: Registry<&str> = Registry::new(id("minecraft:sound_event"));
    non_def
        .register_arc(
            id("minecraft:a"),
            Arc::clone(&stone_arc),
            RegistrationInfo::built_in(),
        )
        .unwrap();
    non_def.freeze().unwrap();

    assert!(non_def.get_value(&missing).is_none());
    assert!(non_def.by_id(99).is_none());
    assert_eq!(non_def.id_of(&unregistered_arc), -1);
    assert_eq!(non_def.id_of_opt(None), -1);
    assert!(non_def.identifier_of(&unregistered_arc).is_none());
    assert_eq!(
        non_def
            .get_any()
            .unwrap()
            .key()
            .unwrap()
            .unwrap()
            .location(),
        &id("minecraft:a")
    );
}

#[test]
fn registry_invariants_intrusive_freezing_and_empty_reload_quirk() {
    // Duplicate ID and duplicate Arc identity fail.
    let mut reg: Registry<String> = Registry::new(id("minecraft:test"));
    let v1 = Arc::new("one".to_owned());
    reg.register_arc(
        id("minecraft:one"),
        Arc::clone(&v1),
        RegistrationInfo::built_in(),
    )
    .unwrap();
    assert!(matches!(
        reg.register(
            id("minecraft:one"),
            "other".to_owned(),
            RegistrationInfo::built_in()
        ),
        Err(RegistryError::DuplicateIdentifier { .. })
    ));
    assert!(matches!(
        reg.register_arc(
            id("minecraft:two"),
            Arc::clone(&v1),
            RegistrationInfo::built_in()
        ),
        Err(RegistryError::DuplicateValue { .. })
    ));

    // Lifecycle combines: adding experimental makes registry experimental.
    assert_eq!(reg.lifecycle(), Lifecycle::Stable);
    reg.register(
        id("minecraft:exp"),
        "exp".to_owned(),
        RegistrationInfo::from_pack(None),
    )
    .unwrap();
    assert_eq!(reg.lifecycle(), Lifecycle::Experimental);

    // Pre-created unbound tag fails freeze until bound.
    let pre_tag = TagKey::new(reg.key().clone(), id("minecraft:pre"));
    let pre_set = reg.get_or_create_tag(&pre_tag).unwrap();
    assert!(pre_set.contents().is_err());
    assert!(matches!(
        reg.freeze(),
        Err(RegistryError::UnboundTags { .. })
    ));

    reg.bind_empty_pre_referenced_tags().unwrap();
    reg.freeze().unwrap();
    // Freezing again is an idempotent no-op.
    reg.freeze().unwrap();
    assert_eq!(pre_set.contents().unwrap().len(), 0);
    // Registration after freeze fails.
    assert!(matches!(
        reg.register(
            id("minecraft:late"),
            "late".to_owned(),
            RegistrationInfo::built_in()
        ),
        Err(RegistryError::Frozen { .. })
    ));

    // Empty-reload quirk (Section 4.5): reloading with 0 tags leaves existing tags untouched!
    assert!(!reg.apply_tag_reload(BTreeMap::new()).unwrap());
    assert_eq!(reg.tag_count(), 1);
    assert!(reg.get_tag(&pre_tag).is_some());

    // Intrusive holder registry rules (Section 2.3).
    let mut intrusive: Registry<u32> =
        Registry::with_options(id("minecraft:block"), Lifecycle::Stable, None, true);
    assert!(matches!(
        intrusive.get_or_create_placeholder(&id("minecraft:stone")),
        Err(RegistryError::IntrusiveRegistryRejectsPlaceholder { .. })
    ));
    assert!(matches!(
        intrusive.register(id("minecraft:stone"), 1, RegistrationInfo::built_in()),
        Err(RegistryError::MissingIntrusiveHolder { .. })
    ));
    let val = Arc::new(42u32);
    let h = intrusive.create_intrusive_holder(Arc::clone(&val)).unwrap();
    assert!(matches!(h.key(), Err(HolderError::UnboundKey)));
    // Freeze fails while intrusive holder is unregistered.
    assert!(matches!(
        intrusive.freeze(),
        Err(RegistryError::UnregisteredIntrusiveHolders { count: 1, .. })
    ));
    intrusive
        .register_arc(id("minecraft:stone"), val, RegistrationInfo::built_in())
        .unwrap();
    assert_eq!(h.key().unwrap().unwrap().location(), &id("minecraft:stone"));
    intrusive.freeze().unwrap();

    // Spec constant counts sanity check.
    assert_eq!(DEFAULTED_BUILTIN_REGISTRIES.len(), 10);
    assert_eq!(INTRUSIVE_HOLDER_REGISTRIES.len(), 5);
    assert_eq!(FEATURE_FILTERED_REGISTRIES.len(), 7);
    assert_eq!(SYNCHRONIZED_DYNAMIC_REGISTRIES.len(), 32);
    assert_eq!(NON_EMPTY_SYNCHRONIZED_REGISTRIES.len(), 13);
    assert_eq!(RELOADABLE_REGISTRIES.len(), 8);
}

// =============================================================================
// Edge case 7: Built-in (frozen) vs dynamic (loading) tag missing required element
// =============================================================================

#[test]
fn edge_case_07_static_vs_dynamic_missing_required_element() {
    let mut source = MemoryTagSource::new().with_pack_name(0, "vanilla");
    source.add_json(
        0,
        id("minecraft:broken"),
        r#"{"values": ["minecraft:present", "minecraft:missing_elem"]}"#,
    );
    source.add_json(
        0,
        id("minecraft:good"),
        r#"{"values": ["minecraft:present"]}"#,
    );
    source.add_json(
        0,
        id("minecraft:depends_on_broken"),
        r##"{"values": ["#minecraft:broken"]}"##,
    );
    source.add_json(
        0,
        id("minecraft:optional_missing"),
        r#"{"values": ["minecraft:present", {"id": "minecraft:missing_opt", "required": false}]}"#,
    );

    // 1. Frozen static registry: missing required element drops ONLY the affected tag
    //    (and tags requiring it); healthy tags load fine.
    let mut static_reg: Registry<&str> = Registry::new(id("minecraft:block"));
    static_reg
        .register(
            id("minecraft:present"),
            "present",
            RegistrationInfo::built_in(),
        )
        .unwrap();
    static_reg.freeze().unwrap();

    let mut static_lookup = &static_reg;
    let static_res = TagLoader::load_and_resolve(&source, &mut static_lookup);
    assert!(!static_res.resolved.contains_key(&id("minecraft:broken")));
    assert!(
        !static_res
            .resolved
            .contains_key(&id("minecraft:depends_on_broken"))
    );
    assert!(static_res.resolved.contains_key(&id("minecraft:good")));
    assert!(
        static_res
            .resolved
            .contains_key(&id("minecraft:optional_missing"))
    );
    assert_eq!(
        static_res.resolved[&id("minecraft:optional_missing")].len(),
        1
    );
    assert!(static_res.diagnostics.iter().any(|d| matches!(
        d,
        TagDiagnostic::MissingReferences { tag_id, .. } if tag_id == &id("minecraft:broken")
    )));

    // 2. Loading dynamic registry: missing REQUIRED element creates an unbound placeholder
    //    holder, tag appears to build, and then registry.freeze() FAILS with UnboundKeys!
    let mut dyn_reg: Registry<&str> = Registry::new(id("minecraft:worldgen/biome"));
    dyn_reg
        .register(
            id("minecraft:present"),
            "present",
            RegistrationInfo::built_in(),
        )
        .unwrap();

    let dyn_res = {
        let mut dyn_lookup = &mut dyn_reg;
        TagLoader::load_and_resolve(&source, &mut dyn_lookup)
    };
    assert!(dyn_res.resolved.contains_key(&id("minecraft:broken")));
    assert!(
        dyn_res
            .resolved
            .contains_key(&id("minecraft:depends_on_broken"))
    );
    assert!(
        dyn_res
            .resolved
            .contains_key(&id("minecraft:optional_missing"))
    );
    // Optional missing did NOT create a placeholder!
    dyn_reg.bind_tags(dyn_res.resolved).unwrap();
    let freeze_err = dyn_reg.freeze().unwrap_err();
    assert_eq!(
        freeze_err,
        RegistryError::UnboundKeys {
            registry: id("minecraft:worldgen/biome"),
            keys: vec![id("minecraft:missing_elem")],
        }
    );

    // Confirm optional missing alone on dynamic registry freezes cleanly!
    let mut opt_only_source = MemoryTagSource::new();
    opt_only_source.add_json(
        0,
        id("minecraft:opt_only"),
        r#"{"values": ["minecraft:present", {"id": "minecraft:missing_opt", "required": false}]}"#,
    );
    let mut dyn_reg_ok: Registry<&str> = Registry::new(id("minecraft:worldgen/biome"));
    dyn_reg_ok
        .register(
            id("minecraft:present"),
            "present",
            RegistrationInfo::built_in(),
        )
        .unwrap();
    let ok_res = {
        let mut lookup = &mut dyn_reg_ok;
        TagLoader::load_and_resolve(&opt_only_source, &mut lookup)
    };
    dyn_reg_ok.bind_tags(ok_res.resolved).unwrap();
    dyn_reg_ok.freeze().unwrap();
}

// =============================================================================
// Edge case 8: Tag replace: true across packs and parse-failure quirks
// =============================================================================

#[test]
fn edge_case_08_tag_replace_and_parse_failure_quirks() {
    let mut reg: Registry<&str> = Registry::new(id("minecraft:item"));
    for name in ["a", "b", "c"] {
        reg.register(id(name), name, RegistrationInfo::built_in())
            .unwrap();
    }
    reg.freeze().unwrap();

    let mut source = MemoryTagSource::new()
        .with_pack_name(0, "vanilla")
        .with_pack_name(1, "pack1")
        .with_pack_name(2, "pack2");

    // 1. Higher-priority pack with replace: true clears lower-priority entries.
    source.add_json(0, id("minecraft:replaced"), r#"{"values": ["a", "b"]}"#);
    source.add_json(
        1,
        id("minecraft:replaced"),
        r#"{"replace": true, "values": ["c"]}"#,
    );

    // 2. Higher-priority pack with malformed JSON does NOT clear lower-priority entries.
    source.add_json(0, id("minecraft:kept_after_syntax"), r#"{"values": ["a"]}"#);
    source.add_json(1, id("minecraft:kept_after_syntax"), r#"{ invalid json"#);

    // 3. Higher-priority pack with invalid tag schema (even with replace: true!) does NOT clear.
    source.add_json(0, id("minecraft:kept_after_schema"), r#"{"values": ["a"]}"#);
    source.add_json(
        1,
        id("minecraft:kept_after_schema"),
        r#"{"replace": true, "values": "not_an_array"}"#,
    );

    // 4. Valid JSON failing schema alone DOES insert an empty tag entry!
    source.add_json(0, id("minecraft:empty_from_schema_fail"), r#"{}"#);

    // 5. Malformed JSON alone does NOT create an entry at all!
    source.add_json(0, id("minecraft:absent_from_syntax_fail"), r#"{ bad"#);

    // 6. Strict RFC 8259 rejects trailing commas and trailing tokens.
    assert!(matches!(
        TagFile::parse_json(br#"{"values": ["a",]}"#),
        Err(TagParseError::JsonSyntax(_))
    ));
    assert!(matches!(
        TagFile::parse_json(br#"{"values": ["a"]} trailing"#),
        Err(TagParseError::JsonSyntax(_))
    ));

    // 7. TagEntry shorthand and object parsing + pack filter clearing lower-priority files.
    let parsed_file = TagFile::parse_json(
        br##"{"values": ["a", "#logs", {"id": "b", "required": false}, {"id": "#other"}]}"##,
    )
    .unwrap();
    assert_eq!(
        parsed_file.values,
        vec![
            TagEntry::element(id("minecraft:a")),
            TagEntry::tag(id("minecraft:logs")),
            TagEntry::optional_element(id("minecraft:b")),
            TagEntry::tag(id("minecraft:other")),
        ]
    );
    source.add_json(0, id("minecraft:filtered_out"), r#"{"values": ["a"]}"#);
    source.add_filter(1, id("minecraft:filtered_out"));
    source.add_json(2, id("minecraft:filtered_out"), r#"{"values": ["c"]}"#);

    let mut lookup = &reg;
    let result = TagLoader::load_and_resolve(&source, &mut lookup);

    let names = |tag: &str| -> Vec<String> {
        result.resolved[&id(tag)]
            .iter()
            .map(|h| h.display_name())
            .collect()
    };

    assert_eq!(names("minecraft:replaced"), vec!["minecraft:c"]);
    assert_eq!(names("minecraft:kept_after_syntax"), vec!["minecraft:a"]);
    assert_eq!(names("minecraft:kept_after_schema"), vec!["minecraft:a"]);
    assert_eq!(names("minecraft:filtered_out"), vec!["minecraft:c"]);
    assert!(
        result
            .resolved
            .contains_key(&id("minecraft:empty_from_schema_fail"))
    );
    assert!(result.resolved[&id("minecraft:empty_from_schema_fail")].is_empty());
    assert!(
        !result
            .resolved
            .contains_key(&id("minecraft:absent_from_syntax_fail"))
    );
}

// =============================================================================
// Edge case 9: Tag deduplication and first-seen order
// =============================================================================

#[test]
fn edge_case_09_tag_dedup_first_seen_order() {
    let mut reg: Registry<&str> = Registry::new(id("minecraft:block"));
    for name in ["a", "b", "c", "d"] {
        reg.register(id(name), name, RegistrationInfo::built_in())
            .unwrap();
    }
    reg.freeze().unwrap();

    let mut source = MemoryTagSource::new();
    source.add_json(0, id("minecraft:sub"), r#"{"values": ["b", "c", "a"]}"#);
    source.add_json(
        0,
        id("minecraft:top"),
        r##"{"values": ["c", "a", "#minecraft:sub", "d", "b", "c"]}"##,
    );

    let mut lookup = &reg;
    let res = TagLoader::load_and_resolve(&source, &mut lookup);
    let top: Vec<String> = res.resolved[&id("minecraft:top")]
        .iter()
        .map(|h| h.display_name())
        .collect();
    // First seen: c, then a, then from #sub: b (c and a already seen), then d (b and c already seen).
    assert_eq!(
        top,
        vec!["minecraft:c", "minecraft:a", "minecraft:b", "minecraft:d"]
    );
}

// =============================================================================
// Edge case 10: Tag cycles (all four combinations)
// =============================================================================

#[test]
fn edge_case_10_tag_cycles_all_four_combinations() {
    let mut reg: Registry<&str> = Registry::new(id("minecraft:block"));
    for name in ["a", "b", "c", "d"] {
        reg.register(id(name), name, RegistrationInfo::built_in())
            .unwrap();
    }
    reg.freeze().unwrap();

    // 1. Required self-reference fails.
    {
        let mut src = MemoryTagSource::new();
        src.add_json(
            0,
            id("minecraft:self_req"),
            r##"{"values": ["a", "#self_req"]}"##,
        );
        let mut lookup = &reg;
        let res = TagLoader::load_and_resolve(&src, &mut lookup);
        assert!(!res.resolved.contains_key(&id("minecraft:self_req")));
    }

    // 2. Optional self-reference succeeds (ignored, keeps remaining entries).
    {
        let mut src = MemoryTagSource::new();
        src.add_json(
            0,
            id("minecraft:self_opt"),
            r##"{"values": ["a", {"id": "#self_opt", "required": false}]}"##,
        );
        let mut lookup = &reg;
        let res = TagLoader::load_and_resolve(&src, &mut lookup);
        let elems: Vec<String> = res.resolved[&id("minecraft:self_opt")]
            .iter()
            .map(|h| h.display_name())
            .collect();
        assert_eq!(elems, vec!["minecraft:a"]);
    }

    // 3. Mutual required cycle drops BOTH tags.
    {
        let mut src = MemoryTagSource::new();
        src.add_json(0, id("minecraft:cyc_a"), r##"{"values": ["a", "#cyc_b"]}"##);
        src.add_json(0, id("minecraft:cyc_b"), r##"{"values": ["b", "#cyc_a"]}"##);
        let mut lookup = &reg;
        let res = TagLoader::load_and_resolve(&src, &mut lookup);
        assert!(!res.resolved.contains_key(&id("minecraft:cyc_a")));
        assert!(!res.resolved.contains_key(&id("minecraft:cyc_b")));
    }

    // 4. Mixed required/optional cycle: A requires #B, B optionally references #A ->
    //    deterministically resolves B before A (regardless of alphabetical ordering of names!).
    for (req_name, opt_name) in [
        ("minecraft:alpha", "minecraft:zeta"),
        ("minecraft:zeta", "minecraft:alpha"),
    ] {
        let mut src = MemoryTagSource::new();
        src.add_json(
            0,
            id(req_name),
            format!(r##"{{"values": ["a", "#{opt_name}"]}}"##),
        );
        src.add_json(
            0,
            id(opt_name),
            format!(r##"{{"values": ["b", {{"id": "#{req_name}", "required": false}}]}}"##),
        );
        let mut lookup = &reg;
        let res = TagLoader::load_and_resolve(&src, &mut lookup);

        let opt_elems: Vec<String> = res.resolved[&id(opt_name)]
            .iter()
            .map(|h| h.display_name())
            .collect();
        let req_elems: Vec<String> = res.resolved[&id(req_name)]
            .iter()
            .map(|h| h.display_name())
            .collect();
        assert_eq!(opt_elems, vec!["minecraft:b"]);
        assert_eq!(req_elems, vec!["minecraft:a", "minecraft:b"]);
    }

    // 5. Mutual optional cycle: both succeed.
    {
        let mut src = MemoryTagSource::new();
        src.add_json(
            0,
            id("minecraft:opt_a"),
            r##"{"values": ["a", {"id": "#opt_b", "required": false}]}"##,
        );
        src.add_json(
            0,
            id("minecraft:opt_b"),
            r##"{"values": ["b", {"id": "#opt_a", "required": false}]}"##,
        );
        let mut lookup = &reg;
        let res = TagLoader::load_and_resolve(&src, &mut lookup);
        assert!(res.resolved.contains_key(&id("minecraft:opt_a")));
        assert!(res.resolved.contains_key(&id("minecraft:opt_b")));
    }
}

// =============================================================================
// Edge case 11: Tag network deserialization with out-of-range numeric IDs vs named HolderSet
// =============================================================================

#[test]
fn edge_case_11_tag_wire_out_of_range_and_missing_named_holder_set() {
    let mut reg: Registry<&str> = Registry::new(id("minecraft:block"));
    let h0 = reg
        .register(id("minecraft:air"), "air", RegistrationInfo::built_in())
        .unwrap();
    let h1 = reg
        .register(id("minecraft:stone"), "stone", RegistrationInfo::built_in())
        .unwrap();
    let mut tags = BTreeMap::new();
    tags.insert(id("minecraft:present"), vec![h0.clone(), h1.clone()]);
    tags.insert(id("minecraft:empty_tag"), Vec::new());
    reg.bind_tags(tags).unwrap();
    reg.freeze().unwrap();

    // Roundtrip encode_registry_tags & encode_update_tags_payload (empty registry omitted, empty tag kept).
    let mut empty_reg: Registry<&str> = Registry::new(id("minecraft:item"));
    empty_reg.freeze().unwrap();
    let mut payload = Vec::new();
    encode_update_tags_payload(&[&reg, &empty_reg], &mut payload).unwrap();
    // Payload starts with VarInt(1) because empty_reg was omitted.
    assert_eq!(payload[0], 1);

    // Manually craft wire bytes for 1 tag containing valid ID 1 and out-of-range IDs -5 and 99.
    let mut raw = Vec::new();
    raw.push(1); // 1 tag
    id("minecraft:filtered").encode_network(&mut raw).unwrap();
    raw.push(3); // 3 element IDs
    raw.push(1); // valid: minecraft:stone
    raw.push(99); // out of range -> silently dropped
    raw.push(0); // valid: minecraft:air

    let (decoded, used) = decode_registry_tags(&reg, &raw).unwrap();
    assert_eq!(used, raw.len());
    let holders = &decoded[&id("minecraft:filtered")];
    assert_eq!(holders.len(), 2);
    assert_eq!(holders[0].display_name(), "minecraft:stone");
    assert_eq!(holders[1].display_name(), "minecraft:air");

    // Meanwhile, decoding a named HolderSet wire payload referencing a MISSING tag MUST FAIL.
    let mut missing_set_wire = Vec::new();
    missing_set_wire.push(0); // 0 = named tag
    id("minecraft:does_not_exist")
        .encode_network(&mut missing_set_wire)
        .unwrap();
    assert!(matches!(
        decode_holder_set(&reg, &missing_set_wire),
        Err(HolderError::MissingTarget(_))
    ));

    // Encoding a tag map succeeds; direct holders cannot be bound into registry tags.
    let mut reg_check: Registry<&str> = Registry::new(id("minecraft:block"));
    let mut bad = BTreeMap::new();
    bad.insert(id("minecraft:bad"), vec![Holder::direct("inline")]);
    assert!(matches!(
        reg_check.bind_tags(bad),
        Err(RegistryError::InvalidTagHolder { .. })
    ));

    // Also verify encode_registry_tags roundtrip directly.
    let mut encoded = Vec::new();
    encode_registry_tags(&reg, &mut encoded).unwrap();
    let (roundtrip, len) = decode_registry_tags(&reg, &encoded).unwrap();
    assert_eq!(len, encoded.len());
    assert_eq!(roundtrip[&id("minecraft:present")].len(), 2);
    assert_eq!(roundtrip[&id("minecraft:empty_tag")].len(), 0);
    let _ = TagWireError::Wire("unused".to_owned());
}

// =============================================================================
// Edge case 12: Known-pack exact-list negotiation & client packet arrival order
// =============================================================================

#[test]
fn edge_case_12_known_pack_exact_negotiation_and_client_arrival_order() {
    let core = KnownPack::new("minecraft", "core", "26.3");
    let extra = KnownPack::new("custom", "pack", "1.0");
    let requested = vec![core.clone(), extra.clone()];

    // Exact match -> negotiated.
    assert_eq!(
        negotiate_known_packs(&requested, &[core.clone(), extra.clone()]),
        requested
    );
    // Strict subset -> EMPTY negotiated set!
    assert!(negotiate_known_packs(&requested, std::slice::from_ref(&core)).is_empty());
    // Reordered list -> EMPTY negotiated set!
    assert!(negotiate_known_packs(&requested, &[extra.clone(), core.clone()]).is_empty());

    // Build server registry with core known-pack provenance.
    let mut server_reg: Registry<String> = Registry::new(id("minecraft:damage_type"));
    server_reg
        .register(
            id("minecraft:zeta"),
            "zeta_val".to_owned(),
            RegistrationInfo::from_pack(Some(core.clone())),
        )
        .unwrap();
    server_reg
        .register(
            id("minecraft:alpha"),
            "alpha_val".to_owned(),
            RegistrationInfo::from_pack(Some(extra.clone())),
        )
        .unwrap();
    server_reg.freeze().unwrap();

    // When strict subset returned (negotiated = empty), server transmits full payload for ALL entries.
    let negotiated_empty = negotiate_known_packs(&requested, std::slice::from_ref(&core));
    let sync_full = build_sync_entries(&server_reg, &negotiated_empty, |v| v.clone());
    assert!(sync_full.iter().all(|e| e.payload.is_some()));

    // When exact list matched, payloads for negotiated packs are omitted.
    let negotiated_all = negotiate_known_packs(&requested, &requested);
    let sync_omitted = build_sync_entries(&server_reg, &negotiated_all, |v| v.clone());
    assert!(sync_omitted.iter().all(|e| e.payload.is_none()));

    // Client assembly across MULTIPLE registry_data packets preserves EXACT arrival order
    // (NOT re-sorted alphabetically), and assigns network registration info.
    let mut assembler: ClientRegistryAssembler<String> = ClientRegistryAssembler::new();
    let reg_id = id("minecraft:damage_type");
    assembler.push_registry_data(
        reg_id.clone(),
        vec![SyncRegistryEntry {
            id: id("minecraft:zeta"),
            payload: Some("zeta_net".to_owned()),
        }],
    );
    assembler.push_registry_data(
        reg_id.clone(),
        vec![SyncRegistryEntry {
            id: id("minecraft:alpha"),
            payload: None, // resolved from local known pack
        }],
    );

    // Later update_tags payload replaces earlier one.
    let mut first_tags = BTreeMap::new();
    first_tags.insert(id("minecraft:old"), vec![0]);
    assembler.push_tag_payload(reg_id.clone(), first_tags);

    let mut second_tags = BTreeMap::new();
    second_tags.insert(id("minecraft:latest"), vec![1, 999]); // 999 dropped
    assembler.push_tag_payload(reg_id.clone(), second_tags);

    let client_reg = assembler
        .assemble::<String, String>(
            &reg_id,
            |_, payload| Ok(payload.clone()),
            |elem_id| Ok(format!("local:{elem_id}")),
        )
        .unwrap();

    let entries: Vec<(u32, String, String)> = client_reg
        .iter()
        .map(|(nid, ident, val)| (nid, ident.to_string(), (**val).clone()))
        .collect();
    assert_eq!(
        entries,
        vec![
            (0, "minecraft:zeta".to_owned(), "zeta_net".to_owned()),
            (
                1,
                "minecraft:alpha".to_owned(),
                "local:minecraft:alpha".to_owned()
            ),
        ]
    );
    assert_eq!(
        client_reg.registration_info(&id("minecraft:zeta")),
        Some(&RegistrationInfo::network())
    );
    assert!(client_reg.get_tag_by_id(&id("minecraft:old")).is_none());
    let latest = client_reg
        .get_tag_by_id(&id("minecraft:latest"))
        .unwrap()
        .contents()
        .unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].display_name(), "minecraft:alpha");
}

// =============================================================================
// Edge case 13: Block-state enumeration order, air = 0, and fallbacks
// =============================================================================

#[test]
fn edge_case_13_block_state_enumeration_and_fallbacks() {
    // Property validation rules (Section 3.1).
    assert!(matches!(
        PropertyDef::boolean("BadName"),
        Err(BlockStateError::InvalidPropertyName { .. })
    ));
    assert!(matches!(
        PropertyDef::enumeration("axis", vec!["x".to_owned()]),
        Err(BlockStateError::InsufficientCardinality { .. })
    ));
    assert!(matches!(
        PropertyDef::integer("age", 3, 3),
        Err(BlockStateError::InvalidIntegerRange { .. })
    ));

    // Declare properties deliberately out of alphabetical order ("b" before "a"):
    // BlockDef must sort them alphabetically: a in [true, false], b in [0, 1, 2].
    let air = BlockDef::new(id("minecraft:air"), vec![]).unwrap();
    let example = BlockDef::new(
        id("minecraft:example"),
        vec![
            PropertyDef::integer("b", 0, 2).unwrap(),
            PropertyDef::boolean("a").unwrap(),
        ],
    )
    .unwrap();

    let table = BlockStateTable::from_blocks(vec![air, example]).unwrap();
    assert_eq!(table.len(), 7);

    // State 0 is minecraft:air.
    assert_eq!(table.state_by_id(0).unwrap().block_id, id("minecraft:air"));

    // States 1..=6 follow odometer order: (true,0), (true,1), (true,2), (false,0), (false,1), (false,2).
    let expected = [
        (1, "true", "0"),
        (2, "true", "1"),
        (3, "true", "2"),
        (4, "false", "0"),
        (5, "false", "1"),
        (6, "false", "2"),
    ];
    for (gid, a_val, b_val) in expected {
        let s = table.state_by_id_exact(gid).unwrap();
        assert_eq!(s.block_id, id("minecraft:example"));
        assert_eq!(s.property_value("a"), Some(a_val));
        assert_eq!(s.property_value("b"), Some(b_val));
        // Lookup in any property order returns the exact global ID.
        assert_eq!(
            table.id_of(&id("minecraft:example"), &[("b", b_val), ("a", a_val)]),
            gid as u32
        );
    }

    // Fallbacks (Section 3.2.3): absent / out-of-range -> 0 / minecraft:air.
    assert_eq!(table.id_of(&id("minecraft:missing"), &[]), 0);
    assert_eq!(
        table.id_of(&id("minecraft:example"), &[("a", "invalid"), ("b", "0")]),
        0
    );
    assert_eq!(table.state_by_id(-1).unwrap().block_id, id("minecraft:air"));
    assert_eq!(
        table.state_by_id(999).unwrap().block_id,
        id("minecraft:air")
    );
    assert!(table.state_by_id_exact(-1).is_none());
    assert!(table.state_by_id_exact(999).is_none());
}

// =============================================================================
// Edge case 14: Feature flags (no ID shifting, tags unfiltered, pack gating)
// =============================================================================

#[derive(Debug, PartialEq, Eq)]
struct GatedItem {
    name: &'static str,
    required: FeatureFlagSet,
}

impl FeatureElement for GatedItem {
    fn required_features(&self) -> &FeatureFlagSet {
        &self.required
    }
}

#[test]
fn edge_case_14_feature_flags_and_filtered_views() {
    let main = FeatureUniverse::builtin_main();
    assert_eq!(main.flags().len(), 4);
    let vanilla = main.flag(&id("minecraft:vanilla")).unwrap();
    let trade = main.flag(&id("minecraft:trade_rebalance")).unwrap();
    let redstone = main.flag(&id("minecraft:redstone_experiments")).unwrap();
    let minecart = main.flag(&id("minecraft:minecart_improvements")).unwrap();

    assert_eq!(vanilla.bit_index(), 0);
    assert_eq!(trade.bit_index(), 1);
    assert_eq!(redstone.bit_index(), 2);
    assert_eq!(minecart.bit_index(), 3);

    let vanilla_set = FeatureFlagSet::of(vanilla);
    let trade_set = FeatureFlagSet::of(trade);
    let both = vanilla_set.join(&trade_set).unwrap();

    assert!(!main.is_experimental(&FeatureFlagSet::empty()));
    assert!(!main.is_experimental(&vanilla_set));
    assert!(main.is_experimental(&trade_set));
    assert!(main.is_experimental(&both));

    // Bitset operations & universe mismatch.
    let mut other_builder = FeatureUniverseBuilder::new("other");
    let other_flag = other_builder.add_flag(id("other:x")).unwrap();
    let other_set = FeatureFlagSet::of(&other_flag);
    assert!(!other_set.contains(vanilla));
    assert!(FeatureFlagSet::empty().is_subset_of(&other_set));
    assert!(!vanilla_set.is_subset_of(&other_set));
    assert!(!vanilla_set.intersects(&other_set));
    assert!(matches!(
        vanilla_set.join(&other_set),
        Err(FeatureError::UniverseMismatch { .. })
    ));
    assert!(matches!(
        vanilla_set.subtract(&other_set),
        Err(FeatureError::UniverseMismatch { .. })
    ));
    assert_eq!(both.subtract(&trade_set).unwrap(), vanilla_set);
    assert_eq!(
        vanilla_set.subtract(&vanilla_set).unwrap(),
        FeatureFlagSet::empty()
    );

    // 64-flag universe limit: 64 succeed, 65th fails; duplicate fails.
    let mut full_builder = FeatureUniverseBuilder::new("limit");
    for i in 0..MAX_FLAGS_PER_UNIVERSE {
        full_builder
            .add_flag(Identifier::with_default_namespace(format!("flag_{i}")).unwrap())
            .unwrap();
    }
    assert!(matches!(
        full_builder.add_flag(id("minecraft:flag_0")),
        Err(FeatureError::DuplicateFlag { .. })
    ));
    assert!(matches!(
        full_builder.add_flag(id("minecraft:flag_64")),
        Err(FeatureError::TooManyFlags { .. })
    ));

    // Partial error vs lenient decoding (Section 5.3).
    let partial_err = main
        .from_names(&[id("minecraft:vanilla"), id("minecraft:unknown_flag")])
        .unwrap_err();
    assert_eq!(partial_err.unknown, vec![id("minecraft:unknown_flag")]);
    assert_eq!(partial_err.partial, vanilla_set);
    let (lenient_set, unknown) =
        main.from_names_lenient(&[id("minecraft:vanilla"), id("minecraft:unknown_flag")]);
    assert_eq!(lenient_set, vanilla_set);
    assert_eq!(unknown, vec![id("minecraft:unknown_flag")]);

    // Registry with disabled experimental element: NO ID shifting and tags stay UNFILTERED!
    let mut reg: Registry<GatedItem> = Registry::new(id("minecraft:item"));
    let h0 = reg
        .register(
            id("minecraft:stone"),
            GatedItem {
                name: "stone",
                required: vanilla_set.clone(),
            },
            RegistrationInfo::built_in(),
        )
        .unwrap();
    let h1 = reg
        .register(
            id("minecraft:exp_item"),
            GatedItem {
                name: "exp_item",
                required: trade_set.clone(),
            },
            RegistrationInfo::built_in(),
        )
        .unwrap();
    let h2 = reg
        .register(
            id("minecraft:dirt"),
            GatedItem {
                name: "dirt",
                required: vanilla_set.clone(),
            },
            RegistrationInfo::built_in(),
        )
        .unwrap();

    let all_tag = TagKey::new(reg.key().clone(), id("minecraft:all_items"));
    let mut tag_map = BTreeMap::new();
    tag_map.insert(id("minecraft:all_items"), vec![h0, h1, h2]);
    reg.bind_tags(tag_map).unwrap();
    reg.freeze().unwrap();

    let view = FeatureFilteredRegistry::new(&reg, vanilla_set.clone());
    // Single-key lookups hide disabled entry.
    assert!(view.get_optional(&id("minecraft:stone")).is_some());
    assert!(view.get_optional(&id("minecraft:exp_item")).is_none());
    assert!(view.get_holder(&id("minecraft:exp_item")).is_none());
    assert!(view.get_holder_by_id(1).is_none());
    assert!(view.get_holder_by_id(2).is_some());
    // Element iteration skips disabled entry WITHOUT shifting numeric IDs (0 and 2, NOT 0 and 1!).
    let visible: Vec<(u32, &str)> = view.iter().map(|(nid, _, v)| (nid, v.name)).collect();
    assert_eq!(visible, vec![(0, "stone"), (2, "dirt")]);
    // Tag lookup & listing remain UNFILTERED (all 3 items including disabled exp_item!).
    let tag_contents = view.get_tag(&all_tag).unwrap().contents().unwrap();
    assert_eq!(tag_contents.len(), 3);
    assert_eq!(tag_contents[1].display_name(), "minecraft:exp_item");
    assert_eq!(view.tags().count(), 1);

    // Existing world pack selection automatically disables packs requesting unenabled flags.
    let available = vec![
        DataPackFeatureInfo {
            id: "vanilla".to_owned(),
            requested_features: vanilla_set.clone(),
            auto_enabled: true,
            is_builtin_feature_source: true,
        },
        DataPackFeatureInfo {
            id: "trade_rebalance".to_owned(),
            requested_features: trade_set.clone(),
            auto_enabled: false,
            is_builtin_feature_source: true,
        },
        DataPackFeatureInfo {
            id: "file/needs_redstone.zip".to_owned(),
            requested_features: FeatureFlagSet::of(redstone),
            auto_enabled: false,
            is_builtin_feature_source: false,
        },
    ];
    let selection = select_data_packs(
        &available,
        &["vanilla".to_owned(), "file/needs_redstone.zip".to_owned()],
        &PackSelectionMode::ExistingWorld {
            persisted_enabled: both.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        selection.auto_disabled_packs,
        vec!["file/needs_redstone.zip".to_owned()]
    );
    // "trade_rebalance" built-in feature source auto-enabled to cover forced trade_rebalance flag.
    let selected_set: HashSet<&str> = selection
        .selected_packs
        .iter()
        .map(String::as_str)
        .collect();
    assert!(selected_set.contains("vanilla"));
    assert!(selected_set.contains("trade_rebalance"));
    assert!(!selected_set.contains("file/needs_redstone.zip"));
    assert_eq!(selection.enabled_features, both);
}

// =============================================================================
// Edge case 15: Holder and holder-set wire encoding off-by-one
// =============================================================================

#[test]
fn edge_case_15_holder_and_holder_set_wire_off_by_one() {
    let mut reg: Registry<String> = Registry::new(id("minecraft:instrument"));
    let h0 = reg
        .register(
            id("minecraft:ponder"),
            "ponder".to_owned(),
            RegistrationInfo::built_in(),
        )
        .unwrap();
    let h1 = reg
        .register(
            id("minecraft:sing"),
            "sing".to_owned(),
            RegistrationInfo::built_in(),
        )
        .unwrap();
    let tag_key = TagKey::new(reg.key().clone(), id("minecraft:goat_horns"));
    let mut tags = BTreeMap::new();
    tags.insert(id("minecraft:goat_horns"), vec![h0.clone(), h1.clone()]);
    reg.bind_tags(tags).unwrap();
    reg.freeze().unwrap();

    // 1. Registry-only holder: VarInt(id).
    let mut buf = Vec::new();
    encode_holder_registry_only(&reg, &h0, &mut buf).unwrap();
    assert_eq!(buf, vec![0]);
    let (dec, used) = decode_holder_registry_only(&reg, &buf).unwrap();
    assert_eq!(used, 1);
    assert!(dec.matches_id(&id("minecraft:ponder")));

    // 2. Inline-capable holder: 0 + payload for Direct; id + 1 for Reference!
    let direct = Holder::direct("custom_horn".to_owned());
    let mut direct_buf = Vec::new();
    encode_holder_with_inline(
        &reg,
        &direct,
        &mut direct_buf,
        |s, out| -> Result<(), String> {
            out.push(s.len() as u8);
            out.extend_from_slice(s.as_bytes());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(direct_buf[0], 0); // 0 = direct!
    let (dec_direct, used) = decode_holder_with_inline(
        &reg,
        &direct_buf,
        |bytes| -> Result<(String, usize), String> {
            let len = bytes[0] as usize;
            let s = String::from_utf8(bytes[1..1 + len].to_vec()).map_err(|e| e.to_string())?;
            Ok((s, 1 + len))
        },
    )
    .unwrap();
    assert_eq!(used, direct_buf.len());
    assert!(dec_direct.is_direct());
    assert_eq!(dec_direct.display_name(), "[unregistered]");
    assert_eq!(dec_direct.value().unwrap().as_str(), "custom_horn");

    let mut ref_buf = Vec::new();
    encode_holder_with_inline(&reg, &h0, &mut ref_buf, |_, _| -> Result<(), String> {
        unreachable!("reference holder should not invoke inline encoder")
    })
    .unwrap();
    assert_eq!(ref_buf, vec![1]); // id 0 -> 0 + 1 = 1!
    let (dec_ref, used) =
        decode_holder_with_inline(&reg, &ref_buf, |_| -> Result<(String, usize), String> {
            unreachable!("reference holder should not invoke inline decoder")
        })
        .unwrap();
    assert_eq!(used, 1);
    assert!(dec_ref.matches_id(&id("minecraft:ponder")));

    // 3. HolderSet: 0 + tag_id for Named; count + 1 + count registry-only IDs for Direct!
    let named_set = HolderSet::Named(reg.get_tag(&tag_key).unwrap());
    let mut named_buf = Vec::new();
    encode_holder_set(&reg, &named_set, &mut named_buf).unwrap();
    assert_eq!(named_buf[0], 0); // 0 = named!
    let (dec_named, used) = decode_holder_set(&reg, &named_buf).unwrap();
    assert_eq!(used, named_buf.len());
    assert_eq!(dec_named.tag_key(), Some(&tag_key));
    assert_eq!(dec_named.contents().unwrap().len(), 2);

    let direct_set = HolderSet::direct(vec![h1.clone(), h0.clone()]);
    let mut direct_set_buf = Vec::new();
    encode_holder_set(&reg, &direct_set, &mut direct_set_buf).unwrap();
    // count = 2 -> prefix VarInt(3), followed by IDs [1, 0].
    assert_eq!(direct_set_buf, vec![3, 1, 0]);
    let (dec_direct_set, used) = decode_holder_set(&reg, &direct_set_buf).unwrap();
    assert_eq!(used, 3);
    assert!(dec_direct_set.tag_key().is_none());
    let contents = dec_direct_set.contents().unwrap();
    assert_eq!(contents[0].display_name(), "minecraft:sing");
    assert_eq!(contents[1].display_name(), "minecraft:ponder");

    // Empty direct holder set uses prefix VarInt(0 + 1) = 1.
    let empty_direct_set: HolderSet<String> = HolderSet::direct(Vec::new());
    let mut empty_buf = Vec::new();
    encode_holder_set(&reg, &empty_direct_set, &mut empty_buf).unwrap();
    assert_eq!(empty_buf, vec![1]);
    let (dec_empty, used) = decode_holder_set(&reg, &empty_buf).unwrap();
    assert_eq!(used, 1);
    assert!(dec_empty.contents().unwrap().is_empty());

    // NamedHolderSet::contains checks reverse tag-membership set.
    let named = reg.get_tag(&tag_key).unwrap();
    assert!(named.contains(&h0).unwrap());
    assert!(named.contains(&h1).unwrap());
    assert!(
        !named
            .contains(&Holder::direct("ponder".to_owned()))
            .unwrap()
    );
}

#[test]
fn additional_invariants_multi_missing_3_cycle_and_pack_modes() {
    // 1. Tag resolution collects ALL missing entries together rather than stopping at the first.
    let mut reg: Registry<&str> = Registry::new(id("minecraft:block"));
    let h_ok = reg
        .register(id("minecraft:ok"), "ok", RegistrationInfo::built_in())
        .unwrap();
    // Querying tag membership before freeze/tag-binding fails with TagsNotBound.
    let dummy_tag = TagKey::new(reg.key().clone(), id("minecraft:any"));
    assert_eq!(h_ok.matches_tag(&dummy_tag), Err(HolderError::TagsNotBound));
    reg.freeze().unwrap();
    assert_eq!(h_ok.matches_tag(&dummy_tag), Ok(false));

    let mut src = MemoryTagSource::new()
        .with_pack_name(0, "base")
        .with_pack_name(1, "addon");
    src.add_json(
        0,
        id("minecraft:multi_miss"),
        r#"{"values": ["missing_1"]}"#,
    );
    src.add_json(
        1,
        id("minecraft:multi_miss"),
        r##"{"values": ["ok", "#missing_tag", "missing_2"]}"##,
    );
    // 3-node required cycle: a -> #b -> #c -> #a (all 3 must fail).
    src.add_json(0, id("minecraft:c1"), r##"{"values": ["#c2"]}"##);
    src.add_json(0, id("minecraft:c2"), r##"{"values": ["#c3"]}"##);
    src.add_json(0, id("minecraft:c3"), r##"{"values": ["#c1"]}"##);

    let mut lookup = &reg;
    let res = TagLoader::load_and_resolve(&src, &mut lookup);
    assert!(!res.resolved.contains_key(&id("minecraft:multi_miss")));
    assert!(!res.resolved.contains_key(&id("minecraft:c1")));
    assert!(!res.resolved.contains_key(&id("minecraft:c2")));
    assert!(!res.resolved.contains_key(&id("minecraft:c3")));

    let multi_diag = res
        .diagnostics
        .iter()
        .find_map(|d| match d {
            TagDiagnostic::MissingReferences { tag_id, missing }
                if tag_id == &id("minecraft:multi_miss") =>
            {
                Some(missing)
            }
            _ => None,
        })
        .expect("expected MissingReferences diagnostic for multi_miss");
    assert_eq!(
        multi_diag,
        &vec![
            ("minecraft:missing_1".to_owned(), "base".to_owned()),
            ("#minecraft:missing_tag".to_owned(), "addon".to_owned()),
            ("minecraft:missing_2".to_owned(), "addon".to_owned()),
        ]
    );

    // 2. PackSelectionMode::SafeMode and NewWorld + FeatureUniverse::to_names ordering.
    let main = FeatureUniverse::builtin_main();
    let vanilla = FeatureFlagSet::of(main.flag(&id("minecraft:vanilla")).unwrap());
    let trade = FeatureFlagSet::of(main.flag(&id("minecraft:trade_rebalance")).unwrap());
    let minecart = FeatureFlagSet::of(main.flag(&id("minecraft:minecart_improvements")).unwrap());

    let combined = minecart.join(&vanilla).unwrap();
    assert_eq!(
        main.to_names(&combined),
        vec![
            id("minecraft:vanilla"),
            id("minecraft:minecart_improvements")
        ]
    );

    let packs = vec![
        DataPackFeatureInfo {
            id: "vanilla".to_owned(),
            requested_features: vanilla.clone(),
            auto_enabled: true,
            is_builtin_feature_source: true,
        },
        DataPackFeatureInfo {
            id: "trade_rebalance".to_owned(),
            requested_features: trade.clone(),
            auto_enabled: false,
            is_builtin_feature_source: true,
        },
        DataPackFeatureInfo {
            id: "custom".to_owned(),
            requested_features: FeatureFlagSet::empty(),
            auto_enabled: false,
            is_builtin_feature_source: false,
        },
    ];

    let safe = select_data_packs(
        &packs,
        &["vanilla".to_owned(), "custom".to_owned()],
        &PackSelectionMode::SafeMode {
            forced_features: vanilla.join(&trade).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(safe.selected_packs, vec!["vanilla", "trade_rebalance"]);
    assert_eq!(safe.enabled_features, vanilla.join(&trade).unwrap());

    let new_world = select_data_packs(
        &packs,
        &["custom".to_owned(), "trade_rebalance".to_owned()],
        &PackSelectionMode::NewWorld {
            all_features: main.all_flags(),
        },
    )
    .unwrap();
    assert!(new_world.selected_packs.contains(&"vanilla".to_owned()));
    assert!(new_world.selected_packs.contains(&"custom".to_owned()));
    assert!(
        new_world
            .selected_packs
            .contains(&"trade_rebalance".to_owned())
    );
    assert_eq!(new_world.enabled_features, vanilla.join(&trade).unwrap());

    // 3. Truncated network identifier buffer fails decoding before identifier parsing.
    let truncated = [10u8, b'a', b'b'];
    assert!(matches!(
        Identifier::decode_network(&truncated),
        Err(IdentifierError::NetworkEncoding { .. })
    ));
}
