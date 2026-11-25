mod common;

use std::sync::Arc;

use common::*;
use zenith_codec::format::PackType;
use zenith_codec::log::{Level, MemoryLog};
use zenith_codec::pack::{
    PackRepository, PackSource, SelectionConfig, feature_pack, java_string_cmp, vanilla_data_pack,
    vanilla_resource_pack,
};
use zenith_codec::resources::{MemoryPack, PathRules};
use zenith_codec::selection::*;
use zenith_nbt::{Compound, List, Tag};
use zenith_registry::{FeatureFlagSet, FeatureUniverse, Identifier};

fn universe() -> FeatureUniverse {
    FeatureUniverse::builtin_main()
}

fn flags(names: &[&str]) -> FeatureFlagSet {
    let ids: Vec<Identifier> = names
        .iter()
        .map(|n| Identifier::parse(n).unwrap())
        .collect();
    universe().from_names(&ids).unwrap()
}

/// vanilla, two feature packs, and the requested world packs.
fn repo_with(world: &[(&str, &[&str])], log: &Arc<MemoryLog>) -> PackRepository {
    let ctx = data_ctx(log);
    let vanilla = vanilla_data_pack(
        Arc::new(MemoryPack::new("vanilla", PathRules::Zip, log.clone())),
        &ctx,
    );
    let trade = feature_pack(
        "trade_rebalance",
        Arc::new(mem_pack(
            "trade_rebalance",
            &data_mcmeta(&["minecraft:trade_rebalance"]),
            log,
        )),
        &ctx,
    )
    .unwrap();
    let redstone = feature_pack(
        "redstone_experiments",
        Arc::new(mem_pack(
            "redstone_experiments",
            &data_mcmeta(&["minecraft:redstone_experiments"]),
            log,
        )),
        &ctx,
    )
    .unwrap();
    let mut world_packs = Vec::new();
    for (id, feats) in world {
        world_packs.push(data_pack(id, PackSource::World, feats, log));
    }
    PackRepository::from_sources([vec![vanilla, trade, redstone], world_packs])
}

fn config(enabled: &[&str], disabled: &[&str]) -> WorldPackConfig {
    WorldPackConfig {
        enabled: enabled.iter().map(|s| s.to_string()).collect(),
        disabled: disabled.iter().map(|s| s.to_string()).collect(),
    }
}

fn names(set: &FeatureFlagSet) -> Vec<String> {
    universe()
        .to_names(set)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn available_order_is_java_string_order() {
    let log = new_log();
    let repo = repo_with(&[("file/a", &[]), ("file/B", &[])], &log);
    assert_eq!(
        repo.ids(),
        [
            "file/B",
            "file/a",
            "redstone_experiments",
            "trade_rebalance",
            "vanilla"
        ]
    );
    assert_eq!(
        java_string_cmp("file/B", "file/a"),
        std::cmp::Ordering::Less
    );
    // UTF-16 order, not UTF-8 / code point order: U+FF5E (BMP) sorts after the surrogate pair of U+1F600.
    assert_eq!(
        java_string_cmp("\u{ff5e}", "\u{1f600}"),
        std::cmp::Ordering::Greater
    );
    assert_eq!("\u{ff5e}".cmp("\u{1f600}"), std::cmp::Ordering::Less);
}

#[test]
fn later_source_wins_on_repeated_ids() {
    let log = new_log();
    let a = data_pack("file/x", PackSource::World, &[], &log);
    let mut b = data_pack("file/x", PackSource::Default, &[], &log);
    b.title = zenith_codec::text::Component::literal("second");
    let repo = PackRepository::from_sources([vec![a], vec![b]]);
    assert_eq!(repo.available().len(), 1);
    assert_eq!(repo.get("file/x").unwrap().source, PackSource::Default);
}

#[test]
fn edge_19_selection_order() {
    let log = new_log();
    let repo = repo_with(&[("file/a", &[]), ("file/B", &[])], &log);
    let u = universe();
    // Enabled order is kept; duplicates collapse to the first occurrence; unknown ids are skipped (logged).
    let r = configure_world_packs(
        &repo,
        &config(&["file/a", "vanilla", "file/a", "nope"], &[]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    // file/a, vanilla from Enabled; file/B auto-added after; feature packs never auto-added.
    assert_eq!(r.enabled, ["file/a", "vanilla", "file/B"]);
    assert!(log.contains(Level::Warn, "nope"));
    assert!(log.contains(Level::Info, "file/B"));
    // Disabled = every available id not selected, in available order.
    assert_eq!(r.disabled, ["redstone_experiments", "trade_rebalance"]);

    // Auto-added packs are appended in UTF-16 id order: file/B, file/a, vanilla.
    let r = configure_world_packs(
        &repo,
        &config(&["missing"], &[]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["file/B", "file/a", "vanilla"]);

    // Forced feature packs come last.
    let r = configure_world_packs(
        &repo,
        &config(&["vanilla"], &[]),
        &flags(&["minecraft:vanilla", "minecraft:trade_rebalance"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(
        r.enabled,
        ["vanilla", "file/B", "file/a", "trade_rebalance"]
    );
    assert_eq!(
        names(&r.enabled_features),
        ["minecraft:vanilla", "minecraft:trade_rebalance"]
    );
    // An empty Enabled list adds vanilla.
    let r = configure_world_packs(
        &repo,
        &config(&[], &["file/a", "file/B", "vanilla"]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla"]);
}

#[test]
fn feature_packs_are_never_auto_added_in_new_worlds_and_world_packs_with_flags_depend_on_allowed() {
    let log = new_log();
    let repo = repo_with(&[("file/exp", &["minecraft:redstone_experiments"])], &log);
    let u = universe();
    // New world: all flags allowed -> the world pack is auto-added; feature packs still are not.
    let r = configure_world_packs(
        &repo,
        &WorldPackConfig::default(),
        &flags(&["minecraft:vanilla"]),
        WorldMode::New,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla", "file/exp"]);
    assert_eq!(
        names(&r.enabled_features),
        ["minecraft:vanilla", "minecraft:redstone_experiments"]
    );
    // Existing world with only vanilla: the pack requests an unallowed flag -> skipped with a log.
    let log2 = new_log();
    let repo2 = repo_with(&[("file/exp", &["minecraft:redstone_experiments"])], &log2);
    let r = configure_world_packs(
        &repo2,
        &WorldPackConfig::default(),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log2,
    );
    assert_eq!(r.enabled, ["vanilla"]);
    assert!(log2.contains(Level::Info, "file/exp"));
    assert_eq!(names(&r.enabled_features), ["minecraft:vanilla"]);
    // Listed in Enabled but its features are not allowed: removed with a warning.
    let r = configure_world_packs(
        &repo2,
        &config(&["vanilla", "file/exp"], &[]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log2,
    );
    assert_eq!(r.enabled, ["vanilla"]);
    assert!(log2.contains(Level::Warn, "file/exp"));
}

#[test]
fn edge_20_pack_in_both_enabled_and_disabled_stays_and_skips_the_feature_check() {
    let log = new_log();
    let repo = repo_with(&[("file/exp", &["minecraft:trade_rebalance"])], &log);
    let u = universe();
    let r = configure_world_packs(
        &repo,
        &config(&["vanilla", "file/exp"], &["file/exp"]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla", "file/exp"]);
    // Its requested flags are added to the world's enabled features.
    assert_eq!(
        names(&r.enabled_features),
        ["minecraft:vanilla", "minecraft:trade_rebalance"]
    );
    // Without the Disabled entry the feature check removes it.
    let r = configure_world_packs(
        &repo,
        &config(&["vanilla", "file/exp"], &[]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla"]);
    // A disabled pack that is not enabled is skipped entirely (not auto-added).
    let repo = repo_with(&[("file/p", &[])], &log);
    let r = configure_world_packs(
        &repo,
        &config(&["vanilla"], &["file/p"]),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla"]);
    assert_eq!(
        r.disabled,
        ["file/p", "redstone_experiments", "trade_rebalance"]
    );
}

#[test]
fn edge_21_safe_mode_is_not_sticky() {
    let log = new_log();
    let repo = repo_with(&[("file/a", &[]), ("file/B", &[])], &log);
    let u = universe();
    let stored_features = flags(&["minecraft:vanilla", "minecraft:trade_rebalance"]);
    let stored = config(&["vanilla", "file/a", "trade_rebalance"], &["file/B"]);
    let safe = configure_world_packs(
        &repo,
        &stored,
        &stored_features,
        WorldMode::Existing,
        true,
        &u,
        &*log,
    );
    // Vanilla plus the forced feature pack; Disabled is empty.
    assert_eq!(safe.enabled, ["vanilla", "trade_rebalance"]);
    assert!(safe.disabled.is_empty());
    assert_eq!(
        names(&safe.enabled_features),
        ["minecraft:vanilla", "minecraft:trade_rebalance"]
    );
    // Next normal load with the stored result: every world pack (but no feature pack) is auto-enabled again,
    // including the previously disabled file/B.
    let next = configure_world_packs(
        &repo,
        &safe.stored_config(),
        &safe.enabled_features,
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(
        next.enabled,
        ["vanilla", "trade_rebalance", "file/B", "file/a"]
    );
    assert!(!next.enabled.contains(&"redstone_experiments".to_owned()));
    // Safe mode of a new world forces nothing.
    let safe_new = configure_world_packs(
        &repo,
        &WorldPackConfig::default(),
        &flags(&["minecraft:vanilla"]),
        WorldMode::New,
        true,
        &u,
        &*log,
    );
    assert_eq!(safe_new.enabled, ["vanilla"]);
    assert!(safe_new.disabled.is_empty());
}

#[test]
fn forced_feature_packs_must_be_subsets_of_forced_and_stop_when_covered() {
    let log = new_log();
    let repo = repo_with(&[], &log);
    let u = universe();
    // Both flags forced: both feature packs are appended in available order.
    let r = configure_world_packs(
        &repo,
        &WorldPackConfig::default(),
        &flags(&[
            "minecraft:vanilla",
            "minecraft:trade_rebalance",
            "minecraft:redstone_experiments",
        ]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(
        r.enabled,
        ["vanilla", "redstone_experiments", "trade_rebalance"]
    );
    // A flag without a feature pack is simply kept in enabled_features.
    let r = configure_world_packs(
        &repo,
        &WorldPackConfig::default(),
        &flags(&["minecraft:vanilla", "minecraft:minecart_improvements"]),
        WorldMode::Existing,
        false,
        &u,
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla"]);
    assert_eq!(
        names(&r.enabled_features),
        ["minecraft:vanilla", "minecraft:minecart_improvements"]
    );
}

// --- level.dat (spec 4.1) --------------------------------------------------------------------------------------

fn string_list(items: &[&str]) -> Tag {
    let mut l = List::new();
    for i in items {
        l.push(Tag::String((*i).into()));
    }
    Tag::List(l)
}

#[test]
fn edge_18_level_dat_lenient_fallbacks() {
    let u = universe();
    // Both lists present: decoded.
    let mut packs = Compound::new();
    packs.insert("Enabled", string_list(&["vanilla", "file/a"]));
    packs.insert("Disabled", string_list(&["file/b"]));
    let mut data = Compound::new();
    data.insert("DataPacks", Tag::Compound(packs));
    assert_eq!(
        decode_world_pack_config(&data),
        config(&["vanilla", "file/a"], &["file/b"])
    );
    // Missing `Disabled` resets everything to the default.
    let mut packs = Compound::new();
    packs.insert("Enabled", string_list(&["vanilla", "file/a"]));
    let mut data = Compound::new();
    data.insert("DataPacks", Tag::Compound(packs));
    assert_eq!(decode_world_pack_config(&data), WorldPackConfig::default());
    // A non-string element resets to the default.
    let mut bad = List::new();
    bad.push(Tag::String("vanilla".into()));
    bad.push(Tag::Int(3));
    let mut packs = Compound::new();
    packs.insert("Enabled", Tag::List(bad));
    packs.insert("Disabled", string_list(&[]));
    let mut data = Compound::new();
    data.insert("DataPacks", Tag::Compound(packs));
    assert_eq!(decode_world_pack_config(&data), WorldPackConfig::default());
    // Missing / wrong-typed key.
    assert_eq!(
        decode_world_pack_config(&Compound::new()),
        WorldPackConfig::default()
    );
    let mut data = Compound::new();
    data.insert("DataPacks", Tag::Int(1));
    assert_eq!(decode_world_pack_config(&data), WorldPackConfig::default());

    // enabled_features: valid list decodes; one unknown flag resets to [vanilla]; [] stays empty.
    let mut data = Compound::new();
    data.insert(
        "enabled_features",
        string_list(&["minecraft:vanilla", "minecraft:trade_rebalance"]),
    );
    assert_eq!(
        names(&decode_enabled_features(&data, &u)),
        ["minecraft:vanilla", "minecraft:trade_rebalance"]
    );
    let mut data = Compound::new();
    data.insert(
        "enabled_features",
        string_list(&["minecraft:trade_rebalance", "minecraft:nope"]),
    );
    assert_eq!(
        names(&decode_enabled_features(&data, &u)),
        ["minecraft:vanilla"]
    );
    let mut data = Compound::new();
    data.insert("enabled_features", string_list(&[]));
    assert!(decode_enabled_features(&data, &u).is_empty());
    let mut data = Compound::new();
    data.insert("enabled_features", string_list(&["Bad Id"]));
    assert_eq!(
        names(&decode_enabled_features(&data, &u)),
        ["minecraft:vanilla"]
    );
    assert_eq!(
        names(&decode_enabled_features(&Compound::new(), &u)),
        ["minecraft:vanilla"]
    );
    let mut data = Compound::new();
    data.insert("enabled_features", Tag::String("minecraft:vanilla".into()));
    assert_eq!(
        names(&decode_enabled_features(&data, &u)),
        ["minecraft:vanilla"]
    );
    // Round trip of the encoder.
    let encoded = encode_world_pack_config(&config(&["vanilla"], &["x"]));
    let mut data = Compound::new();
    data.insert("DataPacks", Tag::Compound(encoded));
    assert_eq!(
        decode_world_pack_config(&data),
        config(&["vanilla"], &["x"])
    );
}

// --- dedicated servers (spec 4.3) -------------------------------------------------------------------------------

#[test]
fn edge_23_empty_initial_enabled_packs_gives_one_empty_id() {
    assert_eq!(split_pack_property(""), [""]);
    assert_eq!(server_initial_packs(Some(""), None).enabled, [""]);
    assert_eq!(server_initial_packs(None, None).enabled, ["vanilla"]);
    assert_eq!(split_pack_property(" a , b ,, c "), ["a", "b", "", "c"]);
    let log = new_log();
    let repo = repo_with(&[("file/a", &[])], &log);
    let initial = server_initial_packs(Some(""), None);
    let r = configure_world_packs(
        &repo,
        &initial,
        &flags(&["minecraft:vanilla"]),
        WorldMode::New,
        false,
        &universe(),
        &*log,
    );
    assert!(log.contains(Level::Warn, "Missing data pack"));
    // The empty id is skipped; auto-add then enables world packs and vanilla.
    assert_eq!(r.enabled, ["file/a", "vanilla"]);
    // Feature packs become active only if listed in initial-enabled-packs.
    let initial = server_initial_packs(Some("vanilla, trade_rebalance"), None);
    let r = configure_world_packs(
        &repo,
        &initial,
        &flags(&["minecraft:vanilla"]),
        WorldMode::New,
        false,
        &universe(),
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla", "trade_rebalance", "file/a"]);
    assert!(names(&r.enabled_features).contains(&"minecraft:trade_rebalance".to_owned()));
    // Safe mode on the dedicated server.
    let r = configure_world_packs(
        &repo,
        &initial,
        &flags(&["minecraft:vanilla"]),
        WorldMode::New,
        true,
        &universe(),
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla"]);
}

// --- /reload (spec 4.4) -----------------------------------------------------------------------------------------

#[test]
fn edge_22_reload_has_no_feature_or_source_check() {
    let log = new_log();
    let repo = repo_with(
        &[
            ("file/new", &["minecraft:redstone_experiments"]),
            ("file/off", &[]),
            ("file/on", &[]),
        ],
        &log,
    );
    let sel = reload_selection(
        &repo,
        &["vanilla".into(), "file/on".into()],
        &["file/off".into()],
    );
    // Current selection first, then every available id in neither the selection nor Disabled, in available order:
    // including a pack that requests flags the world lacks and a *feature* pack.
    assert_eq!(
        sel,
        [
            "vanilla",
            "file/on",
            "file/new",
            "redstone_experiments",
            "trade_rebalance"
        ]
    );
    // Stored configuration after success: Enabled = selection, Disabled = every other available id.
    let after = config_after_reload(&repo, &sel);
    assert_eq!(after.enabled, sel);
    assert_eq!(after.disabled, ["file/off"]);
    // The world's feature set is not part of the reload result: it is the caller's to keep.
}

#[test]
fn reload_after_a_world_pack_appears_enables_it() {
    let log = new_log();
    let before = repo_with(&[("file/a", &[])], &log);
    let current = vec!["vanilla".to_owned(), "file/a".to_owned()];
    let after = repo_with(&[("file/a", &[]), ("file/dropped_in", &[])], &log);
    let sel = reload_selection(&after, &current, &[]);
    assert!(sel.contains(&"file/dropped_in".to_owned()));
    let _ = before;
}

// --- required packs (spec 4.5) --------------------------------------------------------------------------------------

fn resource_repo(log: &Arc<MemoryLog>, extra: &[(&str, &str)]) -> PackRepository {
    let ctx = resource_ctx(log);
    let vanilla = vanilla_resource_pack(
        Arc::new(MemoryPack::new("vanilla", PathRules::Zip, log.clone())),
        &ctx,
    );
    let mut packs = Vec::new();
    for (id, mcmeta) in extra {
        packs.push(make_pack(
            id,
            PackSource::Default,
            SelectionConfig::OPTIONAL_TOP,
            mem_pack(id, mcmeta, log),
            &ctx,
        ));
    }
    PackRepository::from_sources([vec![vanilla], packs])
}

const RES_OK: &str = r#"{"pack":{"description":"d","min_format":97,"max_format":97}}"#;
const RES_OLD: &str = r#"{"pack":{"description":"d","pack_format":5}}"#;

#[test]
fn client_vanilla_pack_is_synthetic_required_and_bottom() {
    let log = new_log();
    let repo = resource_repo(&log, &[]);
    let v = repo.get("vanilla").unwrap();
    assert!(v.selection.required && !v.selection.fixed);
    assert_eq!(v.metadata.range.to_string(), "[97.1, 97.*]");
    assert!(v.is_compatible());
    assert_eq!(v.known_pack.as_ref().unwrap().id, "core");
    assert_eq!(v.known_pack.as_ref().unwrap().version, "26.3");
    assert!(v.metadata.requested_features.is_empty());
    // Data vanilla: synthetic range [121.0, 121.*], features {vanilla}, not required, BOTTOM.
    let log = new_log();
    let repo = repo_with(&[], &log);
    let v = repo.get("vanilla").unwrap();
    assert_eq!(v.metadata.range.to_string(), "[121.0, 121.*]");
    assert_eq!(names(v.requested_features()), ["minecraft:vanilla"]);
    assert!(!v.selection.required);
    assert_eq!(v.selection, SelectionConfig::OPTIONAL_BOTTOM);
    let t = repo.get("trade_rebalance").unwrap();
    assert_eq!(t.source, PackSource::Feature);
    assert_eq!(t.known_pack.as_ref().unwrap().id, "trade_rebalance");
}

#[test]
fn required_packs_are_inserted_by_default_position() {
    let log = new_log();
    let ctx = resource_ctx(&log);
    let vanilla = vanilla_resource_pack(
        Arc::new(MemoryPack::new("vanilla", PathRules::Zip, log.clone())),
        &ctx,
    );
    let srv = |serial: u32, id: &str| {
        zenith_codec::pack::server_resource_pack(
            serial,
            id,
            zenith_codec::text::Component::literal(id),
            Arc::new(mem_pack(id, RES_OK, &log)),
            &ctx,
        )
        .unwrap()
    };
    let s1 = srv(0xAB, "one");
    assert_eq!(s1.id, "server/000000AB/one");
    assert_eq!(s1.selection, SelectionConfig::REQUIRED_FIXED_TOP);
    let s2 = srv(2, "two");
    let user = make_pack(
        "file/u",
        PackSource::Default,
        SelectionConfig::OPTIONAL_TOP,
        mem_pack("file/u", RES_OK, &log),
        &ctx,
    );
    let repo = PackRepository::from_sources([vec![vanilla], vec![s1, s2], vec![user]]);
    let mut sel = vec!["file/u".to_owned()];
    insert_required(&repo, &mut sel);
    // Required BOTTOM goes first; required fixed TOP goes after the last non-fixed-top pack, in available order
    // ("server/00000002/two" < "server/000000AB/one"), the later one landing *below* the earlier one.
    assert_eq!(
        sel,
        [
            "vanilla",
            "file/u",
            "server/000000AB/one",
            "server/00000002/two"
        ]
    );
    // Already present: unchanged.
    let before = sel.clone();
    insert_required(&repo, &mut sel);
    assert_eq!(sel, before);
}

// --- edge 24 / client options (spec 4.7) -------------------------------------------------------------------------

#[test]
fn edge_24_client_options_quirk_and_legacy_names() {
    let log = new_log();
    let repo = resource_repo(
        &log,
        &[
            ("file/old", RES_OK),
            ("file/inc", RES_OLD),
            ("file/was_inc", RES_OK),
            ("file/inc2", RES_OLD),
        ],
    );
    assert!(!repo.get("file/inc").unwrap().is_compatible());
    let mut options = ClientPackOptions {
        resource_packs: vec![
            "old".into(),          // legacy id: resolves to file/old
            "file/inc".into(),     // incompatible and remembered as incompatible: kept
            "missing".into(),      // absent: removed
            "file/was_inc".into(), // compatible but listed as incompatible: quirk
            "file/inc2".into(),    // incompatible and not remembered: removed
        ],
        incompatible_resource_packs: vec!["file/inc".into(), "file/was_inc".into()],
    };
    let sel = select_client_resource_packs(&repo, &mut options, &*log);
    assert_eq!(sel, ["vanilla", "file/old", "file/inc"]);
    assert_eq!(options.resource_packs, ["old", "file/inc", "file/was_inc"]);
    assert_eq!(options.incompatible_resource_packs, ["file/inc"]);
    // Next start: the pack is no longer remembered as incompatible and is selected.
    let sel = select_client_resource_packs(&repo, &mut options, &*log);
    assert!(sel.contains(&"file/was_inc".to_owned()));
    // Saving: incompatible packs are also written to incompatibleResourcePacks; fixed packs are left out.
    let saved = client_options_for_selection(&repo, &sel);
    assert!(saved.resource_packs.contains(&"file/inc".to_owned()));
    assert_eq!(saved.incompatible_resource_packs, ["file/inc"]);
    // A legacy name with a `file/` pack already listed under the real id is still found.
    let mut options = ClientPackOptions {
        resource_packs: vec!["file/old".into()],
        incompatible_resource_packs: vec![],
    };
    assert_eq!(
        select_client_resource_packs(&repo, &mut options, &*log),
        ["vanilla", "file/old"]
    );
}

#[test]
fn option_id_lists_round_trip() {
    assert_eq!(
        parse_option_id_list(r#"["vanilla","file/a b"]"#),
        ["vanilla", "file/a b"]
    );
    assert!(parse_option_id_list("not json").is_empty());
    assert!(parse_option_id_list(r#"[1,2]"#).is_empty());
    assert!(parse_option_id_list("[]").is_empty());
    let ids = vec!["a".to_owned(), "file/\"q\"".to_owned()];
    assert_eq!(parse_option_id_list(&format_option_id_list(&ids)), ids);
}

#[test]
fn incompatible_packs_still_load_on_the_server() {
    // The server never filters by compatibility (spec 4.6).
    let log = new_log();
    let ctx = data_ctx(&log);
    let old = make_pack(
        "file/old",
        PackSource::World,
        SelectionConfig::OPTIONAL_TOP,
        mem_pack(
            "file/old",
            r#"{"pack":{"description":"d","pack_format":20}}"#,
            &log,
        ),
        &ctx,
    );
    let broken = make_pack(
        "file/unk",
        PackSource::World,
        SelectionConfig::OPTIONAL_TOP,
        mem_pack(
            "file/unk",
            r#"{"pack":{"description":"d","pack_format":999}}"#,
            &log,
        ),
        &ctx,
    );
    let vanilla = vanilla_data_pack(
        Arc::new(MemoryPack::new("vanilla", PathRules::Zip, log.clone())),
        &ctx,
    );
    let repo = PackRepository::from_sources([vec![vanilla, old, broken]]);
    let r = configure_world_packs(
        &repo,
        &WorldPackConfig::default(),
        &flags(&["minecraft:vanilla"]),
        WorldMode::Existing,
        false,
        &universe(),
        &*log,
    );
    assert_eq!(r.enabled, ["vanilla", "file/old", "file/unk"]);
    assert!(
        r.enabled.contains(&"file/old".to_owned()) && r.enabled.contains(&"file/unk".to_owned())
    );
    assert_eq!(PackType::Data.current_format().major, 121);
}
