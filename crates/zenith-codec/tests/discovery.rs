mod common;

use std::sync::Arc;

use common::*;
use zenith_codec::format::{Compatibility, PackType};
use zenith_codec::log::{Level, MemoryLog};
use zenith_codec::pack::{
    PackRepository, PackSource, SelectionConfig, SymlinkAllowList, discover_feature_packs_in_dir,
    discover_feature_packs_in_zip, discover_folder_packs, discover_world_packs, vanilla_data_pack,
};
use zenith_codec::resources::{MemoryPack, PathRules, ZipPackResources};
use zenith_registry::Identifier;

fn mcmeta() -> String {
    data_mcmeta(&[])
}

#[test]
fn world_folder_discovery_rules() {
    let world = tempfile::tempdir().unwrap();
    let dp = world.path().join("datapacks");
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);

    // A folder pack, a folder without pack.mcmeta, a folder whose pack.mcmeta is itself a directory.
    write_file(&dp, "dir_pack/pack.mcmeta", mcmeta().as_bytes());
    write_file(&dp, "dir_pack/data/ns/recipe/a.json", b"{}");
    write_file(&dp, "no_meta/data/ns/recipe/a.json", b"{}");
    std::fs::create_dir_all(dp.join("meta_is_dir/pack.mcmeta")).unwrap();
    // Zip packs: valid, `.ZIP` (ignored), corrupt, and one with an unusable pack.mcmeta.
    let good = make_zip(&[
        ("pack.mcmeta", mcmeta().as_bytes()),
        ("data/ns/recipe/z.json", b"{}"),
    ]);
    std::fs::write(dp.join("good.zip"), &good).unwrap();
    std::fs::write(dp.join("UPPER.ZIP"), &good).unwrap();
    std::fs::write(dp.join("corrupt.zip"), b"garbage").unwrap();
    std::fs::write(dp.join("bad_meta.zip"), make_zip(&[("pack.mcmeta", b"{")])).unwrap();
    // A plain file and a folder pack with invalid metadata.
    std::fs::write(dp.join("readme.txt"), b"hi").unwrap();
    write_file(&dp, "broken/pack.mcmeta", b"{\"pack\":{}}");
    // A kept-but-incompatible pack.
    write_file(
        &dp,
        "old/pack.mcmeta",
        br#"{"pack":{"description":"d","pack_format":20}}"#,
    );

    let packs = discover_world_packs(world.path(), &SymlinkAllowList::none(), &ctx);
    let mut ids: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    // World-folder ids are `file/<entry name>` verbatim: a zip keeps its extension.
    assert_eq!(ids, ["file/dir_pack", "file/good.zip", "file/old"]);
    let good = packs.iter().find(|p| p.id == "file/good.zip").unwrap();
    assert_eq!(good.source, PackSource::World);
    assert_eq!(good.selection, SelectionConfig::OPTIONAL_TOP);
    assert!(good.known_pack.is_none());
    assert_eq!(good.title.0.as_str(), Some("good.zip"));
    assert_eq!(
        packs
            .iter()
            .find(|p| p.id == "file/old")
            .unwrap()
            .compatibility(),
        Compatibility::TooOld
    );
    // The corrupt zip logged an error once and was dropped as an empty pack.
    assert_eq!(
        log.entries()
            .iter()
            .filter(|e| e.level == Level::Error && e.message.contains("corrupt.zip"))
            .count(),
        1
    );
    assert!(log.contains_any("file/broken"));
    // Contents are reachable.
    let stack = zenith_codec::lookup::ResourceStack::from_packs(
        PackType::Data,
        &packs.iter().collect::<Vec<_>>(),
        log.clone(),
    );
    assert!(
        stack
            .get(&Identifier::parse("ns:recipe/a.json").unwrap())
            .is_some()
    );
    assert!(
        stack
            .get(&Identifier::parse("ns:recipe/z.json").unwrap())
            .is_some()
    );
}

#[test]
fn world_folder_is_created_when_missing_and_symlinked_folder_is_resolved() {
    let world = tempfile::tempdir().unwrap();
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    assert!(discover_world_packs(world.path(), &SymlinkAllowList::none(), &ctx).is_empty());
    assert!(world.path().join("datapacks").is_dir());
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        write_file(elsewhere.path(), "p/pack.mcmeta", mcmeta().as_bytes());
        let world2 = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), world2.path().join("datapacks")).unwrap();
        let packs = discover_world_packs(world2.path(), &SymlinkAllowList::none(), &ctx);
        assert_eq!(packs.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_entries_need_the_allow_list() {
    let base = tempfile::tempdir().unwrap();
    let world = base.path().join("world");
    let dp = world.join("datapacks");
    let outside = base.path().join("outside");
    write_file(&outside, "ext_pack/pack.mcmeta", mcmeta().as_bytes());
    write_file(&outside, "ext_pack/data/ns/recipe/e.json", b"{}");
    write_file(
        &outside,
        "ext.zip",
        &make_zip(&[("pack.mcmeta", mcmeta().as_bytes())]),
    );
    std::fs::create_dir_all(&dp).unwrap();
    std::os::unix::fs::symlink(outside.join("ext_pack"), dp.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("ext.zip"), dp.join("linked.zip")).unwrap();
    // A normal pack that contains a symlink inside.
    write_file(&dp, "inner/pack.mcmeta", mcmeta().as_bytes());
    std::os::unix::fs::symlink(outside.join("ext_pack"), dp.join("inner/escape")).unwrap();
    write_file(&dp, "fine/pack.mcmeta", mcmeta().as_bytes());

    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    // Default: nothing allowed. Symlink entries are ignored; a pack with an inner symlink is ignored entirely.
    let packs = discover_world_packs(&world, &SymlinkAllowList::none(), &ctx);
    let ids: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["file/fine"]);
    assert!(log.contains(Level::Warn, "linked"));
    assert!(log.contains(Level::Warn, "inner"));

    // Allowed by prefix.
    let outside_real = std::fs::canonicalize(&outside).unwrap();
    let allow = SymlinkAllowList::parse(
        &format!("# comment\n\n{}\n", outside_real.display()),
        &(log.clone() as _),
    );
    let packs = discover_world_packs(&world, &allow, &ctx);
    let mut ids: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["file/fine", "file/inner", "file/linked", "file/linked.zip"]
    );
    // Allowed by regex and glob.
    let allow = SymlinkAllowList::parse(
        &format!(
            "[regex]^{}/.*\n",
            fancy_escape(&outside_real.display().to_string())
        ),
        &(log.clone() as _),
    );
    assert!(allow.allows(&outside_real.join("x")));
    assert!(!allow.allows(std::path::Path::new("/elsewhere/x")));
    let allow = SymlinkAllowList::parse(
        &format!("[glob]{}/**\n[bogus]x\n[regex](\n", outside_real.display()),
        &(log.clone() as _),
    );
    assert!(allow.allows(&outside_real.join("a/b")));
    assert!(!allow.allows(&outside_real));
    assert!(log.contains(Level::Warn, "bogus"));
}

#[cfg(unix)]
fn fancy_escape(s: &str) -> String {
    fancy_regex::escape(s).into_owned()
}

#[test]
fn feature_packs_in_the_jar_and_in_a_directory() {
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    let tr = data_mcmeta(&["minecraft:trade_rebalance"]);
    let jar = make_zip(&[
        ("data/minecraft/recipe/stone.json", b"{}"),
        (
            "data/minecraft/datapacks/trade_rebalance/pack.mcmeta",
            tr.as_bytes(),
        ),
        (
            "data/minecraft/datapacks/trade_rebalance/data/minecraft/villager_trade/x.json",
            b"{}",
        ),
        ("data/minecraft/datapacks/not_a_pack/data/x.json", b"{}"),
        ("data/minecraft/datapacks/archive.zip", b"zipzip"),
        ("data/minecraft/datapacks/readme.txt", b"hi"),
    ]);
    let jar = ZipPackResources::from_bytes("vanilla", jar, log.clone());
    let packs = discover_feature_packs_in_zip(&jar, &ctx);
    assert_eq!(packs.len(), 1);
    let p = &packs[0];
    assert_eq!(p.id, "trade_rebalance");
    assert_eq!(p.source, PackSource::Feature);
    assert!(!p.source.auto_add_when_new());
    assert_eq!(
        p.known_pack
            .as_ref()
            .map(|k| (k.namespace.as_str(), k.id.as_str(), k.version.as_str())),
        Some(("minecraft", "trade_rebalance", "26.3"))
    );
    assert_eq!(p.title.0.as_str(), Some("trade_rebalance"));
    assert!(p.open().has(
        PackType::Data,
        &Identifier::parse("villager_trade/x.json").unwrap()
    ));
    // A `.zip` found inside the jar is refused.
    assert!(log.contains(Level::Warn, "archive.zip"));

    // Directory variant: same entry rules; zip is refused; ids drop a trailing `.zip`.
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "redstone_experiments/pack.mcmeta",
        data_mcmeta(&["minecraft:redstone_experiments"]).as_bytes(),
    );
    std::fs::write(
        dir.path().join("zipped.zip"),
        make_zip(&[("pack.mcmeta", tr.as_bytes())]),
    )
    .unwrap();
    let packs = discover_feature_packs_in_dir(dir.path(), &ctx);
    assert_eq!(
        packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["redstone_experiments"]
    );
}

#[test]
fn vanilla_pack_prefers_a_root_mcmeta_section_by_section_and_exposes_only_minecraft() {
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    // No root pack.mcmeta (the 26.3 jar): synthetic metadata.
    let jar = MemoryPack::new("vanilla", PathRules::Zip, log.clone())
        .with_file("data/minecraft/recipe/a.json", "{}")
        .with_file("data/other/recipe/b.json", "{}");
    let v = vanilla_data_pack(Arc::new(jar), &ctx);
    assert_eq!(v.id, "vanilla");
    assert_eq!(v.source, PackSource::BuiltIn);
    assert_eq!(v.compatibility(), Compatibility::Compatible);
    assert_eq!(
        v.metadata
            .description
            .0
            .get("translate")
            .and_then(|j| j.as_str()),
        Some("dataPack.vanilla.description")
    );
    let open = v.open();
    assert_eq!(
        open.namespaces(PackType::Data)
            .into_iter()
            .collect::<Vec<_>>(),
        ["minecraft"]
    );
    assert!(!open.has(
        PackType::Data,
        &Identifier::parse("other:recipe/b.json").unwrap()
    ));
    assert!(open.has(PackType::Data, &Identifier::parse("recipe/a.json").unwrap()));
    // A root pack.mcmeta is preferred; a section that fails falls back to the synthetic one.
    let jar = MemoryPack::new("vanilla", PathRules::Zip, log.clone())
        .with_mcmeta(r#"{"pack":{"description":"custom","pack_format":999},"features":{"enabled":["minecraft:nope"]}}"#);
    let v = vanilla_data_pack(Arc::new(jar), &ctx);
    assert_eq!(v.metadata.range.to_string(), "[121.0, 121.*]"); // pack section failed -> synthetic
    assert_eq!(
        v.requested_features().mask(),
        ctx.universe.vanilla_set().unwrap().mask()
    );
    let jar = MemoryPack::new("vanilla", PathRules::Zip, log.clone())
        .with_mcmeta(r#"{"pack":{"description":"custom","min_format":121,"max_format":125}}"#);
    let v = vanilla_data_pack(Arc::new(jar), &ctx);
    assert_eq!(v.metadata.description.0.as_str(), Some("custom"));
    assert_eq!(v.metadata.range.to_string(), "[121.0, 125.*]");
}

#[test]
fn client_resourcepacks_folder_uses_default_source() {
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "mypack/pack.mcmeta",
        br#"{"pack":{"description":"d","min_format":97,"max_format":97}}"#,
    );
    let log = MemoryLog::shared();
    let ctx = resource_ctx(&log);
    let packs = discover_folder_packs(
        dir.path(),
        PackSource::Default,
        &SymlinkAllowList::none(),
        &ctx,
    );
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].id, "file/mypack");
    assert_eq!(packs[0].source, PackSource::Default);
    assert!(packs[0].source.auto_add_when_new());
    assert!(packs[0].is_compatible());
    let repo = PackRepository::from_sources([packs]);
    assert!(repo.contains("file/mypack"));
}
