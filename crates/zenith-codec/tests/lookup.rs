mod common;

use std::sync::Arc;

use common::*;
use zenith_codec::codec::{Codec, DataResult};
use zenith_codec::filter::PackFilterCodec;
use zenith_codec::format::PackType;
use zenith_codec::json::{ParseMode, parse};
use zenith_codec::log::{Level, MemoryLog, null_log};
use zenith_codec::lookup::{FileToIdConverter, ResourceStack, StackPack};
use zenith_codec::pack::PackSource;
use zenith_codec::resources::{
    FolderPackResources, MemoryPack, PackResources, PathRules, ResourceError, ZipPackResources,
};
use zenith_registry::Identifier;

fn id(s: &str) -> Identifier {
    Identifier::parse(s).unwrap()
}

fn filter(json: &str) -> Option<Arc<zenith_codec::filter::PackFilter>> {
    let v = parse(json, ParseMode::Data).unwrap();
    match PackFilterCodec.decode(&v) {
        DataResult::Success(f) => Some(Arc::new(f)),
        other => panic!("{other:?}"),
    }
}

fn sp(
    id: &str,
    res: impl PackResources + 'static,
    filter: Option<Arc<zenith_codec::filter::PackFilter>>,
) -> StackPack {
    StackPack {
        id: id.to_owned(),
        resources: Arc::new(res),
        filter,
    }
}

fn mem(id: &str, files: &[(&str, &str)]) -> MemoryPack {
    let mut p = MemoryPack::new(id, PathRules::Folder, null_log());
    for (path, text) in files {
        p = p.with_file(*path, text.as_bytes().to_vec());
    }
    p
}

fn stack(packs: Vec<StackPack>) -> ResourceStack {
    ResourceStack::new(PackType::Data, packs, null_log())
}

fn text(h: &zenith_codec::lookup::ResourceHandle) -> String {
    String::from_utf8(h.read().unwrap()).unwrap()
}

// --- the same pack laid out as memory, folder and zip ------------------------------------------------------------

struct Fixture {
    _dir: tempfile::TempDir,
    packs: Vec<(&'static str, Arc<dyn PackResources>)>,
}

/// Builds the same `files` as an in-memory pack (folder rules), a folder pack and a zip pack.
fn three_backings(files: &[(&str, &str)]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let log = null_log();
    let mut m = MemoryPack::new("mem", PathRules::Folder, log.clone());
    for (p, t) in files {
        m = m.with_file(*p, t.as_bytes().to_vec());
        write_file(&dir.path().join("folder"), p, t.as_bytes());
    }
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(p, t)| (*p, t.as_bytes())).collect();
    let zip_path = dir.path().join("pack.zip");
    std::fs::write(&zip_path, make_zip(&entries)).unwrap();
    Fixture {
        packs: vec![
            ("memory", Arc::new(m)),
            (
                "folder",
                Arc::new(FolderPackResources::new(
                    "folder",
                    dir.path().join("folder"),
                    log.clone(),
                )),
            ),
            (
                "zip",
                Arc::new(ZipPackResources::open("zip", &zip_path, log)),
            ),
        ],
        _dir: dir,
    }
}

#[test]
fn all_backings_agree_on_basic_reads_namespaces_and_listing() {
    let fx = three_backings(&[
        ("data/minecraft/recipe/a.json", "A"),
        ("data/minecraft/recipe/sub/dir/b.json", "B"),
        ("data/minecraft/recipe/c.txt", "C"),
        ("data/minecraft/tags/block/t.json", "T"),
        ("data/other/recipe/z.json", "Z"),
    ]);
    for (name, res) in &fx.packs {
        let ns: Vec<String> = res.namespaces(PackType::Data).into_iter().collect();
        assert_eq!(ns, ["minecraft", "other"], "{name}");
        assert_eq!(res.get(PackType::Data, &id("a")).as_deref(), None, "{name}");
        assert_eq!(
            res.get(PackType::Data, &id("recipe/a.json")).as_deref(),
            Some(&b"A"[..]),
            "{name}"
        );
        assert!(
            res.has(PackType::Data, &id("other:recipe/z.json")),
            "{name}"
        );
        assert!(!res.has(PackType::Data, &id("recipe/nope.json")), "{name}");
        let listed: Vec<String> = res
            .list(PackType::Data, "minecraft", "recipe")
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            listed,
            [
                "minecraft:recipe/a.json",
                "minecraft:recipe/c.txt",
                "minecraft:recipe/sub/dir/b.json"
            ],
            "{name}"
        );
        // Listing a nested directory.
        let nested = res.list(PackType::Data, "minecraft", "tags/block").unwrap();
        assert_eq!(nested.len(), 1, "{name}");
        // A missing directory gives nothing.
        assert!(
            res.list(PackType::Data, "minecraft", "nope")
                .unwrap()
                .is_empty(),
            "{name}"
        );
        // A trailing slash is an internal error.
        assert_eq!(
            res.list(PackType::Data, "minecraft", "recipe/"),
            Err(ResourceError::TrailingSlash("recipe/".to_owned())),
            "{name}"
        );
        // Resource packs use `assets/`.
        assert!(res.namespaces(PackType::Resource).is_empty(), "{name}");
    }
}

#[test]
fn edge_16_folder_versus_zip_path_rules() {
    let log = MemoryLog::shared();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("f");
    write_file(&folder, "data/ns/a/b.json", b"folder-b");
    write_file(&folder, "data/foo", b"a plain file named foo");
    std::fs::create_dir_all(folder.join("data/Bad Ns")).unwrap();
    let f = FolderPackResources::new("f", &folder, log.clone());

    // Zip with the same names, including an odd literal entry.
    let zbytes = make_zip(&[
        ("data/ns/a/b.json", b"zip-b"),
        ("data/ns/a/../c.json", b"zip-odd"),
        ("data/foo", b"a plain file named foo"),
        ("data/Bad Ns/x.json", b"x"),
    ]);
    let z = ZipPackResources::from_bytes("z", zbytes, log.clone());

    // A zip counts the plain file `data/foo` as namespace `foo`; a folder pack rejects it (with a warning).
    let fz: Vec<String> = z.namespaces(PackType::Data).into_iter().collect();
    assert_eq!(fz, ["foo", "ns"]);
    let ff: Vec<String> = f.namespaces(PackType::Data).into_iter().collect();
    assert_eq!(ff, ["ns"]);
    assert!(log.contains(Level::Warn, "data/foo"));
    // Invalid namespace names are logged and ignored in both.
    assert!(log.contains(Level::Warn, "Bad Ns"));

    // A zip lookup does not validate path segments: `a/../c.json` is the literal entry name.
    let odd = id("ns:a/../c.json");
    assert_eq!(
        z.get(PackType::Data, &odd).as_deref(),
        Some(&b"zip-odd"[..])
    );
    // A folder lookup rejects it ("invalid path") even if the normalized file exists.
    write_file(&folder, "data/ns/c.json", b"folder-c");
    assert_eq!(f.get(PackType::Data, &odd), None);
    assert!(log.contains(Level::Warn, "Invalid path"));
    // Empty segments and `.` are rejected by the folder too.
    assert_eq!(f.get(PackType::Data, &id("ns:a//b.json")), None);
    assert_eq!(f.get(PackType::Data, &id("ns:./a/b.json")), None);
    assert_eq!(
        f.get(PackType::Data, &id("ns:a/b.json")).as_deref(),
        Some(&b"folder-b"[..])
    );
}

#[test]
fn edge_17_uppercase_names_are_skipped_and_plural_folders_are_not_read() {
    let files = [
        ("data/minecraft/recipe/foo.json", "ok"),
        ("data/minecraft/recipe/Foo.json", "upper"),
        ("data/minecraft/recipes/legacy.json", "legacy"),
        ("data/minecraft/tags/blocks/legacy.json", "legacy"),
        ("data/minecraft/tags/block/new.json", "new"),
    ];
    let fx = three_backings(&files);
    for (name, res) in &fx.packs {
        let log = MemoryLog::shared();
        let s = ResourceStack::new(
            PackType::Data,
            vec![StackPack {
                id: "p".into(),
                resources: res.clone(),
                filter: None,
            }],
            log.clone(),
        );
        let listed = s.list("recipe", |i| i.path().ends_with(".json")).unwrap();
        let ids: Vec<String> = listed.iter().map(|h| h.id.to_string()).collect();
        assert_eq!(ids, ["minecraft:recipe/foo.json"], "{name}");
        let tags = s
            .list("tags/block", |i| i.path().ends_with(".json"))
            .unwrap();
        assert_eq!(tags.len(), 1, "{name}");
    }
    // The skipped name is logged by the pack that listed it.
    let log = MemoryLog::shared();
    let m = MemoryPack::new("m", PathRules::Folder, log.clone())
        .with_file("data/ns/recipe/Foo.json", "x");
    assert!(m.list(PackType::Data, "ns", "recipe").unwrap().is_empty());
    assert!(log.contains(Level::Warn, "Foo.json"));
    // A file named exactly `.json` gives an element with an empty path.
    let conv = FileToIdConverter::new("recipe", ".json");
    assert_eq!(conv.file_to_id(&id("recipe/.json")), Some(id("minecraft:")));
    assert_eq!(conv.file_to_id(&id("recipe/a/b.json")), Some(id("a/b")));
    assert_eq!(conv.file_to_id(&id("recipes/a.json")), None);
    assert_eq!(conv.file_to_id(&id("recipe/a.txt")), None);
    assert_eq!(conv.id_to_file(&id("x:a/b")), Some(id("x:recipe/a/b.json")));
}

// --- single and stack lookups, precedence --------------------------------------------------------------------------

#[test]
fn single_lookup_topmost_pack_wins() {
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/recipe/a.json", "p0"),
                    ("data/ns/recipe/only0.json", "o0"),
                ],
            ),
            None,
        ),
        sp("p1", mem("p1", &[("data/ns/recipe/a.json", "p1")]), None),
        sp("p2", mem("p2", &[("data/other/x/y.json", "p2")]), None),
    ]);
    let h = s.get(&id("ns:recipe/a.json")).unwrap();
    assert_eq!((h.pack_id.as_str(), text(&h).as_str()), ("p1", "p1"));
    assert_eq!(s.get(&id("ns:recipe/only0.json")).unwrap().pack_id, "p0");
    assert!(s.get(&id("ns:recipe/missing.json")).is_none());
    // A namespace no pack provides finds nothing.
    assert!(s.get(&id("nowhere:recipe/a.json")).is_none());
    assert_eq!(s.namespaces(), ["ns", "other"]);
    // Stack lookup: every version, bottom-up.
    let versions: Vec<String> = s
        .get_stack(&id("ns:recipe/a.json"))
        .iter()
        .map(|h| h.pack_id.clone())
        .collect();
    assert_eq!(versions, ["p0", "p1"]);
}

#[test]
fn overlay_precedence_inside_one_pack() {
    // Active overlays sit above the base, later-listed overlays win; namespaces of overlays count.
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    let mcmeta = r#"{"pack":{"description":"d","min_format":121,"max_format":121},
        "overlays":{"entries":[
            {"directory":"ov_a","min_format":121,"max_format":121},
            {"directory":"ov_b","min_format":121,"max_format":121},
            {"directory":"ov_old","min_format":100,"max_format":110}]}}"#;
    let res = mem_pack("file/p", mcmeta, &log)
        .with_file("data/ns/recipe/x.json", "base")
        .with_file("data/ns/recipe/base_only.json", "base_only")
        .with_file("ov_a/data/ns/recipe/x.json", "a")
        .with_file("ov_a/data/ns/recipe/a_only.json", "a_only")
        .with_file("ov_b/data/ns/recipe/x.json", "b")
        .with_file("ov_b/data/ovns/recipe/b_ns.json", "b_ns")
        .with_file("ov_old/data/ns/recipe/x.json", "old")
        .with_file("ov_b/pack.mcmeta", "{\"pack\":{}}");
    let pack = make_pack(
        "file/p",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        res,
        &ctx,
    );
    assert_eq!(pack.metadata.overlays, ["ov_a", "ov_b"]);
    let s = ResourceStack::from_packs(PackType::Data, &[&pack], log.clone());
    assert_eq!(text(&s.get(&id("ns:recipe/x.json")).unwrap()), "b");
    assert_eq!(
        text(&s.get(&id("ns:recipe/a_only.json")).unwrap()),
        "a_only"
    );
    assert_eq!(
        text(&s.get(&id("ns:recipe/base_only.json")).unwrap()),
        "base_only"
    );
    assert_eq!(text(&s.get(&id("ovns:recipe/b_ns.json")).unwrap()), "b_ns");
    assert_eq!(s.namespaces(), ["ns", "ovns"]);
    let listed = s.list("recipe", |_| true).unwrap();
    assert_eq!(listed.len(), 4); // x, a_only, base_only and ovns:recipe/b_ns.json
}

#[test]
fn overlays_in_folder_and_zip_packs() {
    let dir = tempfile::tempdir().unwrap();
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    let mcmeta = r#"{"pack":{"description":"d","min_format":121,"max_format":121},
        "overlays":{"entries":[{"directory":"Ov.1","min_format":121,"max_format":121},
                               {"directory":"missing_dir","min_format":121,"max_format":121}]}}"#;
    // Folder pack.
    let root = dir.path().join("fp");
    write_file(&root, "pack.mcmeta", mcmeta.as_bytes());
    write_file(&root, "data/ns/recipe/x.json", b"base");
    write_file(&root, "Ov.1/data/ns/recipe/x.json", b"overlay");
    let folder = make_pack(
        "file/fp",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        FolderPackResources::new("file/fp", &root, log.clone()),
        &ctx,
    );
    // Zip pack.
    let zbytes = make_zip(&[
        ("pack.mcmeta", mcmeta.as_bytes()),
        ("data/ns/recipe/x.json", b"base"),
        ("Ov.1/data/ns/recipe/x.json", b"overlay"),
    ]);
    let zip = make_pack(
        "file/zp",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        ZipPackResources::from_bytes("file/zp", zbytes, log.clone()),
        &ctx,
    );
    for pack in [&folder, &zip] {
        let s = ResourceStack::from_packs(PackType::Data, &[pack], log.clone());
        assert_eq!(
            text(&s.get(&id("ns:recipe/x.json")).unwrap()),
            "overlay",
            "{}",
            pack.id
        );
    }
    // The overlay's own pack.mcmeta is ignored; metadata comes from the base root.
    assert!(folder.open().root_resource("pack.mcmeta").is_some());
}

#[test]
fn edge_12_overlay_dot_is_the_pack_root_and_dotdot_is_refused_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    // datapacks/ (the parent) holds a file that `..` would expose.
    let datapacks = dir.path().join("datapacks");
    write_file(&datapacks, "data/ns/recipe/escaped.json", b"SECRET");
    let root = datapacks.join("evil");
    write_file(
        &root,
        "pack.mcmeta",
        br#"{"pack":{"description":"d","min_format":121,"max_format":121},
            "overlays":{"entries":[{"directory":"..","min_format":121,"max_format":121},
                                    {"directory":".","min_format":121,"max_format":121}]}}"#,
    );
    write_file(&root, "data/ns/recipe/own.json", b"own");
    let pack = make_pack(
        "file/evil",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        FolderPackResources::new("file/evil", &root, log.clone()),
        &ctx,
    );
    // The entries pass metadata validation (both active) ...
    assert_eq!(pack.metadata.overlays, ["..", "."]);
    // ... but the `..` overlay is refused: nothing outside the pack is readable, and a warning names the pack.
    let open = pack.open();
    assert!(!open.has(PackType::Data, &id("ns:recipe/escaped.json")));
    assert!(open.has(PackType::Data, &id("ns:recipe/own.json")));
    assert!(log.contains(Level::Warn, "file/evil"));
    assert!(log.contains(Level::Warn, "deviation #1"));
    // `.` resolves to the pack root itself: harmless.
    assert!(open.list(PackType::Data, "ns", "recipe").unwrap().len() == 1);
    // Same for an in-memory folder-rules pack.
    let log2 = MemoryLog::shared();
    let ctx2 = data_ctx(&log2);
    let mcmeta = r#"{"pack":{"description":"d","min_format":121,"max_format":121},"overlays":{"entries":[{"directory":"..","min_format":121,"max_format":121}]}}"#;
    let m = mem_pack("file/m", mcmeta, &log2).with_file("data/ns/x.json", "x");
    let p = make_pack(
        "file/m",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        m,
        &ctx2,
    );
    let _ = p.open();
    assert!(log2.contains(Level::Warn, "file/m"));
    // Zip packs use literal entry-name prefixes: `../` cannot escape the archive.
    let log3 = MemoryLog::shared();
    let ctx3 = data_ctx(&log3);
    let z = ZipPackResources::from_bytes(
        "file/z",
        make_zip(&[("pack.mcmeta", mcmeta.as_bytes()), ("data/ns/x.json", b"x")]),
        log3.clone(),
    );
    let p = make_pack(
        "file/z",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        z,
        &ctx3,
    );
    assert!(p.open().has(PackType::Data, &id("ns:x.json")));
}

// --- filters (spec 5.3) --------------------------------------------------------------------------------------------

#[test]
fn edge_14_filters_hide_lower_packs_only_never_their_own_or_higher() {
    let own = stack(vec![
        sp("p0", mem("p0", &[("data/ns/recipe/a.json", "p0a")]), None),
        sp(
            "p1",
            mem("p1", &[("data/ns/recipe/a.json", "p1a")]),
            filter(r#"{"block":[{"path":"recipe/a\\.json"}]}"#),
        ),
    ]);
    // p1's own a.json is not hidden by its own filter.
    assert_eq!(own.get(&id("ns:recipe/a.json")).unwrap().pack_id, "p1");
    // Single lookup for something only p0 has and p1 filters: not found.
    let log = MemoryLog::shared();
    let s2 = ResourceStack::new(
        PackType::Data,
        vec![
            StackPack {
                id: "p0".into(),
                resources: Arc::new(mem(
                    "p0",
                    &[
                        ("data/ns/recipe/a.json", "p0a"),
                        ("data/ns/recipe/z.json", "p0z"),
                    ],
                )),
                filter: None,
            },
            StackPack {
                id: "p1".into(),
                resources: Arc::new(mem("p1", &[("data/ns/recipe/b.json", "p1b")])),
                filter: filter(r#"{"block":[{"path":"recipe/a\\.json"}]}"#),
            },
            StackPack {
                id: "p2".into(),
                resources: Arc::new(mem("p2", &[("data/ns/recipe/c.json", "p2c")])),
                filter: None,
            },
        ],
        log.clone(),
    );
    assert!(s2.get(&id("ns:recipe/a.json")).is_none());
    assert!(log.contains(Level::Info, "filtered by pack p1"));
    assert_eq!(s2.get(&id("ns:recipe/z.json")).unwrap().pack_id, "p0");
    assert_eq!(s2.get(&id("ns:recipe/b.json")).unwrap().pack_id, "p1");
    assert_eq!(s2.get(&id("ns:recipe/c.json")).unwrap().pack_id, "p2");
    let listed: Vec<String> = s2
        .list("recipe", |_| true)
        .unwrap()
        .iter()
        .map(|h| h.id.path().to_owned())
        .collect();
    assert_eq!(listed, ["recipe/b.json", "recipe/c.json", "recipe/z.json"]);
    // Stack lookups keep the filtering layer's own version and exclude lower ones.
    let s3 = stack(vec![
        sp("p0", mem("p0", &[("data/ns/recipe/a.json", "p0a")]), None),
        sp(
            "p1",
            mem("p1", &[("data/ns/recipe/a.json", "p1a")]),
            filter(r#"{"block":[{"path":"a\\.json"}]}"#),
        ),
        sp("p2", mem("p2", &[("data/ns/recipe/a.json", "p2a")]), None),
    ]);
    let v: Vec<String> = s3
        .get_stack(&id("ns:recipe/a.json"))
        .iter()
        .map(|h| h.pack_id.clone())
        .collect();
    assert_eq!(v, ["p1", "p2"]);
    let stacks = s3.list_stacks("recipe", |_| true).unwrap();
    assert_eq!(stacks.len(), 1);
    let v: Vec<&str> = stacks[0].1.iter().map(|h| h.pack_id.as_str()).collect();
    assert_eq!(v, ["p1", "p2"]);
}

#[test]
fn edge_13_filter_cross_product_and_unanchored_search() {
    let lower = |extra: Vec<StackPack>| {
        let mut v = vec![sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/a/recipe/x.json", "a"),
                    ("data/other/recipe/y.json", "o"),
                ],
            ),
            None,
        )];
        v.extend(extra);
        stack(v)
    };
    // [{namespace:"a"},{path:"x"}] is active for every namespace (the second entry has no namespace) and blocks
    // every path (the first entry has no path): it hides EVERYTHING below.
    let s = lower(vec![sp(
        "p1",
        mem("p1", &[]),
        filter(r#"{"block":[{"namespace":"a"},{"path":"x"}]}"#),
    )]);
    assert!(s.get(&id("a:recipe/x.json")).is_none());
    assert!(s.get(&id("other:recipe/y.json")).is_none());
    assert!(s.list("recipe", |_| true).unwrap().is_empty());
    // [{}] hides everything too.
    let s = lower(vec![sp("p1", mem("p1", &[]), filter(r#"{"block":[{}]}"#))]);
    assert!(s.get(&id("a:recipe/x.json")).is_none());
    // [] hides nothing.
    let s = lower(vec![sp("p1", mem("p1", &[]), filter(r#"{"block":[]}"#))]);
    assert!(s.get(&id("a:recipe/x.json")).is_some());
    // A single entry with both patterns: namespace "a" only, path "x": active only in namespace a.
    let s = lower(vec![sp(
        "p1",
        mem("p1", &[]),
        filter(r#"{"block":[{"namespace":"^a$","path":"x"}]}"#),
    )]);
    assert!(s.get(&id("a:recipe/x.json")).is_none());
    assert!(s.get(&id("other:recipe/y.json")).is_some());
    // Unanchored namespace search: "the" matches namespace "xtheme".
    let s = stack(vec![
        sp("p0", mem("p0", &[("data/xtheme/t/a.json", "a")]), None),
        sp(
            "p1",
            mem("p1", &[]),
            filter(r#"{"block":[{"namespace":"the"}]}"#),
        ),
    ]);
    assert!(s.get(&id("xtheme:t/a.json")).is_none());
    // Unanchored path search against the FULL path (category directory and extension included).
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/recipe/stone.json", "a"),
                    ("data/ns/loot_table/stone.json", "b"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem("p1", &[]),
            filter(r#"{"block":[{"path":"^recipe/stone\\.json$"}]}"#),
        ),
    ]);
    assert!(s.get(&id("ns:recipe/stone.json")).is_none());
    assert!(s.get(&id("ns:loot_table/stone.json")).is_some());
    // The pack lacking namespace N but with an active filter contributes a filter-only layer.
    let s = stack(vec![
        sp("p0", mem("p0", &[("data/ns/recipe/a.json", "a")]), None),
        sp(
            "p1",
            mem("p1", &[("data/zz/recipe/q.json", "q")]),
            filter(r#"{"block":[{"namespace":"^ns$","path":"a"}]}"#),
        ),
    ]);
    assert!(s.get(&id("ns:recipe/a.json")).is_none());
    assert!(s.get(&id("zz:recipe/q.json")).is_some());
}

#[test]
fn invalid_filter_is_ignored_and_logged_when_the_stack_is_built() {
    let log = MemoryLog::shared();
    let ctx = data_ctx(&log);
    let mcmeta = r#"{"pack":{"description":"d","min_format":121,"max_format":121},"filter":{"block":[{"namespace":"("}]}}"#;
    let lower = make_pack(
        "file/lo",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        mem_pack("file/lo", &data_mcmeta(&[]), &log).with_file("data/ns/a.json", "a"),
        &ctx,
    );
    let upper = make_pack(
        "file/up",
        PackSource::World,
        zenith_codec::pack::SelectionConfig::OPTIONAL_TOP,
        mem_pack("file/up", mcmeta, &log),
        &ctx,
    );
    let s = ResourceStack::from_packs(PackType::Data, &[&lower, &upper], log.clone());
    assert!(s.get(&id("ns:a.json")).is_some());
    assert!(log.contains(Level::Error, "file/up"));
}

// --- sidecars (spec 5.4-5.8) ---------------------------------------------------------------------------------------

#[test]
fn edge_15_sidecar_ownership_differs_by_lookup_mode() {
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/tex/x.png", "png"),
                    ("data/ns/tex/x.png.mcmeta", "meta0"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem("p1", &[("data/ns/tex/x.png.mcmeta", "meta1")]),
            None,
        ),
    ]);
    // Single lookup: a higher pack's sidecar attaches to a lower pack's file, even though p1 lacks the file.
    let h = s.get(&id("ns:tex/x.png")).unwrap();
    assert_eq!(h.pack_id, "p0");
    assert_eq!(h.sidecar_pack(), Some("p1"));
    assert_eq!(
        String::from_utf8(h.read_sidecar().unwrap()).unwrap(),
        "meta1"
    );
    // A sidecar from a LOWER pack never attaches to a higher pack's file.
    let s_low = stack(vec![
        sp(
            "p0",
            mem("p0", &[("data/ns/tex/x.png.mcmeta", "meta0")]),
            None,
        ),
        sp("p1", mem("p1", &[("data/ns/tex/x.png", "png")]), None),
    ]);
    let h = s_low.get(&id("ns:tex/x.png")).unwrap();
    assert_eq!(h.pack_id, "p1");
    assert!(h.sidecar().is_none());
    // Stack lookup: each version uses only its own pack's sidecar.
    let versions = s.get_stack(&id("ns:tex/x.png"));
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].sidecar_pack(), Some("p0"));
    let s_two = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/tex/x.png", "png0"),
                    ("data/ns/tex/x.png.mcmeta", "meta0"),
                ],
            ),
            None,
        ),
        sp("p1", mem("p1", &[("data/ns/tex/x.png", "png1")]), None),
        sp(
            "p2",
            mem(
                "p2",
                &[
                    ("data/ns/tex/x.png", "png2"),
                    ("data/ns/tex/x.png.mcmeta", "meta2"),
                ],
            ),
            None,
        ),
    ]);
    let v = s_two.get_stack(&id("ns:tex/x.png"));
    let owners: Vec<(&str, Option<&str>)> = v
        .iter()
        .map(|h| (h.pack_id.as_str(), h.sidecar_pack()))
        .collect();
    assert_eq!(
        owners,
        [("p0", Some("p0")), ("p1", None), ("p2", Some("p2"))]
    );
    // Single lookup on the same stack: p2 wins with its own sidecar.
    let h = s_two.get(&id("ns:tex/x.png")).unwrap();
    assert_eq!((h.pack_id.as_str(), h.sidecar_pack()), ("p2", Some("p2")));
}

#[test]
fn sidecar_search_stops_at_a_filter_that_blocks_the_sidecar_path() {
    // p1 (above the file's pack) blocks sidecars; p2 (top) has a sidecar. The top-down sidecar search finds p2's
    // sidecar first, before reaching p1's filter.
    let s = stack(vec![
        sp("p0", mem("p0", &[("data/ns/tex/x.png", "png")]), None),
        sp(
            "p1",
            mem("p1", &[]),
            filter(r#"{"block":[{"path":"\\.mcmeta$"}]}"#),
        ),
        sp(
            "p2",
            mem("p2", &[("data/ns/tex/x.png.mcmeta", "meta2")]),
            None,
        ),
    ]);
    assert_eq!(
        s.get(&id("ns:tex/x.png")).unwrap().sidecar_pack(),
        Some("p2")
    );
    // With the sidecar owner BELOW the blocking filter, the search stops first and finds nothing.
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/tex/x.png", "png"),
                    ("data/ns/tex/x.png.mcmeta", "meta0"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem("p1", &[]),
            filter(r#"{"block":[{"path":"\\.mcmeta$"}]}"#),
        ),
    ]);
    let h = s.get(&id("ns:tex/x.png"));
    // The filter also does not block the *file* path, so p0's file is still reachable; its sidecar is hidden.
    // (The path filter only matches `.mcmeta`, but the single lookup of x.png reaches p1 first: not blocked.)
    let h = h.unwrap();
    assert_eq!(h.pack_id, "p0");
    assert!(h.sidecar().is_none());
    // Stack lookup: a higher layer's filter blocking the sidecar path removes lower versions' sidecars.
    let v = s.get_stack(&id("ns:tex/x.png"));
    assert_eq!(v.len(), 1);
    assert!(v[0].sidecar().is_none());
}

#[test]
fn edge_15_listing_never_lists_mcmeta_and_attaches_sidecars_from_same_or_higher_layers() {
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/tex/a.png", "a0"),
                    ("data/ns/tex/a.png.mcmeta", "m0"),
                    ("data/ns/tex/b.png", "b0"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem(
                "p1",
                &[
                    ("data/ns/tex/b.png.mcmeta", "m1"),
                    ("data/ns/tex/c.png.mcmeta", "orphan"),
                ],
            ),
            None,
        ),
        sp(
            "p2",
            mem(
                "p2",
                &[
                    ("data/ns/tex/d.png", "d2"),
                    ("data/ns/tex/d.png.mcmeta", "m2"),
                ],
            ),
            None,
        ),
    ]);
    let l = s.list("tex", |i| i.path().ends_with(".png")).unwrap();
    let ids: Vec<String> = l.iter().map(|h| h.id.path().to_owned()).collect();
    // Orphan sidecar (c.png.mcmeta) is not a resource; mcmeta files are never listed as data.
    assert_eq!(ids, ["tex/a.png", "tex/b.png", "tex/d.png"]);
    let owner = |h: &zenith_codec::lookup::ResourceHandle| h.sidecar_pack().map(str::to_owned);
    assert_eq!(owner(&l[0]), Some("p0".into())); // same layer
    assert_eq!(owner(&l[1]), Some("p1".into())); // higher layer attaches to a lower file
    assert_eq!(owner(&l[2]), Some("p2".into()));
    // A sidecar from a LOWER layer than the file is not attached.
    let s = stack(vec![
        sp("p0", mem("p0", &[("data/ns/tex/a.png.mcmeta", "m0")]), None),
        sp("p1", mem("p1", &[("data/ns/tex/a.png", "a1")]), None),
    ]);
    let l = s.list("tex", |i| i.path().ends_with(".png")).unwrap();
    assert!(l[0].sidecar().is_none());
    // Without any predicate every non-mcmeta file lists; names ending .mcmeta never do.
    let l = s.list("tex", |_| true).unwrap();
    assert!(l.iter().all(|h| !h.id.path().ends_with(".mcmeta")));
    // The predicate is tested on the name with `.mcmeta` removed: a sidecar of a non-matching file is dropped.
    let s = stack(vec![sp(
        "p0",
        mem(
            "p0",
            &[
                ("data/ns/tex/a.txt", "t"),
                ("data/ns/tex/a.png", "p"),
                ("data/ns/tex/a.png.mcmeta", "m"),
            ],
        ),
        None,
    )]);
    let l = s.list("tex", |i| i.path().ends_with(".png")).unwrap();
    assert_eq!(l.len(), 1);
    assert!(l[0].sidecar().is_some());
    let l = s.list("tex", |i| i.path().ends_with(".txt")).unwrap();
    assert_eq!(l.len(), 1);
    assert!(l[0].sidecar().is_none());
}

// --- listings ---------------------------------------------------------------------------------------------------

#[test]
fn listing_one_winner_applies_filters_bottom_up_and_sorts_path_first() {
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/b/recipe/z.json", "0"),
                    ("data/a/recipe/a.json", "0"),
                    ("data/a/recipe/gone.json", "0"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem("p1", &[("data/a/recipe/a.json", "1")]),
            filter(r#"{"block":[{"path":"gone"}]}"#),
        ),
        sp("p2", mem("p2", &[("data/a/recipe/new.json", "2")]), None),
    ]);
    let l = s.list("recipe", |i| i.path().ends_with(".json")).unwrap();
    let rows: Vec<(String, String)> = l
        .iter()
        .map(|h| (h.id.to_string(), h.pack_id.clone()))
        .collect();
    // Sorted by path first, then namespace: a:recipe/a.json < a:recipe/new.json < b:recipe/z.json.
    // `gone.json` is blocked by p1's filter. p1's own a.json survives and beats p0's.
    assert_eq!(
        rows,
        [
            ("a:recipe/a.json".to_owned(), "p1".to_owned()),
            ("a:recipe/new.json".to_owned(), "p2".to_owned()),
            ("b:recipe/z.json".to_owned(), "p0".to_owned()),
        ]
    );
    // Natural order puts a lower path in a higher namespace first.
    let s = stack(vec![sp(
        "p",
        mem("p", &[("data/a/x/z.json", "1"), ("data/b/x/a.json", "2")]),
        None,
    )]);
    let ids: Vec<String> = s
        .list("x", |_| true)
        .unwrap()
        .iter()
        .map(|h| h.id.to_string())
        .collect();
    assert_eq!(ids, ["b:x/a.json", "a:x/z.json"]);
}

#[test]
fn listing_stacks_clears_lower_versions_but_keeps_the_filtering_layers_own() {
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[
                    ("data/ns/tags/block/t.json", "0"),
                    ("data/ns/tags/block/u.json", "0u"),
                ],
            ),
            None,
        ),
        sp(
            "p1",
            mem("p1", &[("data/ns/tags/block/t.json", "1")]),
            filter(r#"{"block":[{"path":"t\\.json"}]}"#),
        ),
        sp("p2", mem("p2", &[("data/ns/tags/block/t.json", "2")]), None),
    ]);
    let l = s
        .list_stacks("tags/block", |i| i.path().ends_with(".json"))
        .unwrap();
    let rows: Vec<(String, Vec<String>)> = l
        .iter()
        .map(|(id, v)| {
            (
                id.to_string(),
                v.iter().map(|h| h.pack_id.clone()).collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "ns:tags/block/t.json".to_owned(),
                vec!["p1".to_owned(), "p2".to_owned()]
            ),
            ("ns:tags/block/u.json".to_owned(), vec!["p0".to_owned()]),
        ]
    );
    // A file whose version list ends up empty is omitted.
    let s = stack(vec![
        sp("p0", mem("p0", &[("data/ns/tags/block/t.json", "0")]), None),
        sp(
            "p1",
            mem("p1", &[]),
            filter(r#"{"block":[{"path":"t\\.json"}]}"#),
        ),
    ]);
    assert!(s.list_stacks("tags/block", |_| true).unwrap().is_empty());
    // Sidecar versions use their own pack's sidecar.
    let s = stack(vec![
        sp(
            "p0",
            mem(
                "p0",
                &[("data/ns/d/x.json", "0"), ("data/ns/d/x.json.mcmeta", "m0")],
            ),
            None,
        ),
        sp("p1", mem("p1", &[("data/ns/d/x.json", "1")]), None),
    ]);
    let l = s.list_stacks("d", |_| true).unwrap();
    let owners: Vec<Option<&str>> = l[0].1.iter().map(|h| h.sidecar_pack()).collect();
    assert_eq!(owners, [Some("p0"), None]);
}

#[test]
fn listing_trailing_slash_is_an_internal_error() {
    let s = stack(vec![sp(
        "p0",
        mem("p0", &[("data/ns/a/b.json", "x")]),
        None,
    )]);
    assert!(matches!(
        s.list("a/", |_| true),
        Err(ResourceError::TrailingSlash(_))
    ));
    assert!(matches!(
        s.list_stacks("a/", |_| true),
        Err(ResourceError::TrailingSlash(_))
    ));
}

// --- the same lookups over temp-dir folder and zip packs ----------------------------------------------------------

#[test]
fn stack_over_folder_and_zip_packs() {
    let fx = three_backings(&[
        ("data/ns/recipe/a.json", "A"),
        ("data/ns/recipe/x.png", "png"),
        ("data/ns/recipe/x.png.mcmeta", "meta"),
    ]);
    for (name, res) in &fx.packs {
        let s = ResourceStack::new(
            PackType::Data,
            vec![
                StackPack {
                    id: "lower".into(),
                    resources: Arc::new(mem("lower", &[("data/ns/recipe/a.json", "L")])),
                    filter: None,
                },
                StackPack {
                    id: (*name).into(),
                    resources: res.clone(),
                    filter: None,
                },
            ],
            null_log(),
        );
        assert_eq!(s.get(&id("ns:recipe/a.json")).unwrap().pack_id, *name);
        assert_eq!(text(&s.get(&id("ns:recipe/a.json")).unwrap()), "A");
        let h = s.get(&id("ns:recipe/x.png")).unwrap();
        assert_eq!(h.sidecar_pack(), Some(*name));
        assert_eq!(
            String::from_utf8(h.read_sidecar().unwrap()).unwrap(),
            "meta"
        );
        let listed = s.list("recipe", |i| i.path().ends_with(".json")).unwrap();
        assert_eq!(listed.len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn folder_listing_does_not_follow_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    write_file(&root, "data/ns/recipe/real.json", b"r");
    write_file(dir.path(), "outside/secret.json", b"s");
    std::os::unix::fs::symlink(
        dir.path().join("outside"),
        root.join("data/ns/recipe/linkdir"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("outside/secret.json"),
        root.join("data/ns/recipe/linkfile.json"),
    )
    .unwrap();
    let f = FolderPackResources::new("p", &root, null_log());
    let l: Vec<String> = f
        .list(PackType::Data, "ns", "recipe")
        .unwrap()
        .iter()
        .map(|i| i.path().to_owned())
        .collect();
    assert_eq!(l, ["recipe/real.json"]);
}

#[test]
fn zip_packs_and_resource_pack_type() {
    let z = ZipPackResources::from_bytes(
        "z",
        make_zip(&[
            ("assets/minecraft/textures/a.png", b"png"),
            ("assets/minecraft/textures/", b""),
            ("data/x/y.json", b""),
        ]),
        null_log(),
    );
    assert_eq!(
        z.namespaces(PackType::Resource)
            .into_iter()
            .collect::<Vec<_>>(),
        ["minecraft"]
    );
    assert_eq!(
        z.namespaces(PackType::Data).into_iter().collect::<Vec<_>>(),
        ["x"]
    );
    // Directory entries are not listed.
    assert_eq!(
        z.list(PackType::Resource, "minecraft", "textures")
            .unwrap()
            .len(),
        1
    );
    // A broken archive logs an error once and behaves as an empty pack.
    let log = MemoryLog::shared();
    let bad = ZipPackResources::from_bytes("bad", b"not a zip".to_vec(), log.clone());
    assert!(bad.namespaces(PackType::Data).is_empty());
    assert!(bad.root_resource("pack.mcmeta").is_none());
    assert_eq!(
        log.entries()
            .iter()
            .filter(|e| e.level == Level::Error)
            .count(),
        1
    );
    let missing =
        ZipPackResources::open("m", std::path::Path::new("/nonexistent/x.zip"), log.clone());
    assert!(missing.namespaces(PackType::Data).is_empty());
}
