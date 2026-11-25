mod common;

use common::*;
use zenith_codec::format::{Compatibility, FormatRange, PackFormat, PackType, WILDCARD};
use zenith_codec::json::{ParseMode, parse};
use zenith_codec::log::{Level, MemoryLog};
use zenith_codec::meta::{
    DropReason, FilterSection, McmetaFile, PackMetadata, decode_overlay_entries,
    read_pack_metadata_from, validate_overlays,
};
use zenith_registry::FeatureUniverse;

fn read(text: &str) -> Result<PackMetadata, DropReason> {
    read_with(text, PackType::Data)
}

fn read_with(text: &str, ty: PackType) -> Result<PackMetadata, DropReason> {
    let log = MemoryLog::shared();
    read_pack_metadata_from(
        &McmetaFile::from_text(text),
        "test",
        ty,
        &FeatureUniverse::builtin_main(),
        &*log,
    )
}

fn pack_obj(rest: &str) -> String {
    format!("{{\"pack\":{{\"description\":\"d\"{rest}}}}}")
}

fn range(min: (i32, i32), max: (i32, i32)) -> FormatRange {
    FormatRange::new(PackFormat::new(min.0, min.1), PackFormat::new(max.0, max.1))
}

// --- the worked-example table (spec 3.3) ---------------------------------------------------------------------

#[test]
fn format_validation_example_table() {
    use Compatibility::*;
    let w = WILDCARD;
    #[allow(clippy::type_complexity)]
    let rows: &[(&str, FormatRange, Compatibility)] = &[
        (
            r#","min_format":121,"max_format":121"#,
            range((121, 0), (121, w)),
            Compatible,
        ),
        (
            r#","min_format":[121,1],"max_format":121"#,
            range((121, 1), (121, w)),
            TooNew,
        ),
        (
            r#","min_format":[121,2147483647],"max_format":121"#,
            range((121, w), (121, w)),
            TooNew,
        ),
        (
            r#","min_format":121.7,"max_format":121.2"#,
            range((121, 0), (121, w)),
            Compatible,
        ),
        (
            r#","min_format":121,"max_format":130,"pack_format":125"#,
            range((121, 0), (130, w)),
            Compatible,
        ),
        (
            r#","min_format":80,"max_format":121,"supported_formats":[80,81],"pack_format":80"#,
            range((80, 0), (121, w)),
            Compatible,
        ),
        (
            r#","min_format":80,"max_format":121,"supported_formats":[80,121],"pack_format":80"#,
            range((80, 0), (121, w)),
            Compatible,
        ),
        (r#","pack_format":48"#, range((48, 0), (48, 0)), TooOld),
        (r#","pack_format":-3"#, range((-3, 0), (-3, 0)), TooOld),
        (
            r#","supported_formats":{"min_inclusive":40,"max_inclusive":81},"pack_format":40"#,
            range((40, 0), (81, 0)),
            TooOld,
        ),
    ];
    for (extra, expected, class) in rows {
        let m = read(&pack_obj(extra)).unwrap_or_else(|e| panic!("{extra}: {e}"));
        assert_eq!(m.range, *expected, "{extra}");
        assert_eq!(m.compatibility, *class, "{extra}");
    }
    // The error rows fall back to the unknown range but are kept (their description is valid).
    for extra in [
        r#","pack_format":121"#,
        r#","min_format":121,"max_format":121,"supported_formats":[121,121]"#,
        r#","min_format":121,"max_format":121,"pack_format":5"#,
        r#","min_format":80,"max_format":121"#,
        r#","pack_format":10,"supported_formats":[10,20]"#,
    ] {
        let m = read(&pack_obj(extra)).unwrap_or_else(|e| panic!("{extra}: {e}"));
        assert_eq!(m.range, FormatRange::UNKNOWN, "{extra}");
        assert_eq!(m.compatibility, Unknown, "{extra}");
    }
}

#[test]
fn edge_09_format_edge_cases() {
    // pack_format alone above 81 is an error; at 81 or below (even negative) it is valid.
    assert_eq!(
        read(&pack_obj(r#","pack_format":82"#))
            .unwrap()
            .compatibility,
        Compatibility::Unknown
    );
    assert_eq!(
        read(&pack_obj(r#","pack_format":81"#)).unwrap().range,
        range((81, 0), (81, 0))
    );
    assert_eq!(
        read(&pack_obj(r#","pack_format":-100"#)).unwrap().range,
        range((-100, 0), (-100, 0))
    );
    // [121,1] as min_format is TOO_NEW on 121.0.
    let m = read(&pack_obj(r#","min_format":[121,1],"max_format":[121,5]"#)).unwrap();
    assert_eq!(m.compatibility, Compatibility::TooNew);
    // A bare number: min -> n.0, max -> n.*.
    let m = read(&pack_obj(
        r#","min_format":5,"max_format":20,"supported_formats":[5,20],"pack_format":15"#,
    ))
    .unwrap();
    assert_eq!(m.range, range((5, 0), (20, w())));
    // Decimals truncate.
    let m = read(&pack_obj(r#","min_format":121.9,"max_format":121.9"#)).unwrap();
    assert_eq!(m.range, range((121, 0), (121, w())));
    // A legacy supported_formats range ends at minor 0, not *.
    let m = read(&pack_obj(
        r#","supported_formats":[20,30],"pack_format":25"#,
    ))
    .unwrap();
    assert_eq!(m.range.max, PackFormat::new(30, 0));
    // Arrays: elements after the second are ignored; empty / negative / too long / strings are errors (fallback).
    let m = read(&pack_obj(
        r#","min_format":[121,0,99,100],"max_format":[121,3,9]"#,
    ))
    .unwrap();
    assert_eq!(m.range, range((121, 0), (121, 3)));
    for bad in [
        r#","min_format":[],"max_format":121"#,
        r#","min_format":-1,"max_format":121"#,
        r#","min_format":"121","max_format":121"#,
        r#","min_format":[121,-1],"max_format":121"#,
    ] {
        assert_eq!(
            read(&pack_obj(bad)).unwrap().compatibility,
            Compatibility::Unknown,
            "{bad}"
        );
    }
    let long: Vec<String> = (0..257).map(|_| "1".to_owned()).collect();
    let long_arr = format!("[{}]", long.join(","));
    let m = read(&pack_obj(&format!(
        ",\"min_format\":{long_arr},\"max_format\":121"
    )))
    .unwrap();
    assert_eq!(m.compatibility, Compatibility::Unknown);
    // pack_format that does not decode (a string) fails the strict schema even though it is not needed.
    let m = read(&pack_obj(
        r#","min_format":121,"max_format":121,"pack_format":"x""#,
    ))
    .unwrap();
    assert_eq!(m.compatibility, Compatibility::Unknown);
    // null counts as absent.
    let m = read(&pack_obj(
        r#","min_format":121,"max_format":121,"pack_format":null,"supported_formats":null"#,
    ))
    .unwrap();
    assert_eq!(m.compatibility, Compatibility::Compatible);
    // Reversed min/max.
    let m = read(&pack_obj(r#","min_format":122,"max_format":121"#)).unwrap();
    assert_eq!(m.compatibility, Compatibility::Unknown);
}

fn w() -> i32 {
    WILDCARD
}

#[test]
fn edge_10_mixed_legacy_new_declarations() {
    // min_format <= 81 requires supported_formats starting at min.major, ending at max.major or 81, and a pack_format
    // within [min, max] and >= 15.
    let ok = |s: &str| read(&pack_obj(s)).unwrap().compatibility;
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[80,121],"pack_format":80"#),
        Compatibility::Compatible
    );
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[81,81],"pack_format":81"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[80,100],"pack_format":80"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[80,81],"pack_format":14"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[80,81],"pack_format":122"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","min_format":80,"max_format":121,"supported_formats":[80,81]"#),
        Compatibility::Unknown
    );
    // With min_format > 81, supported_formats is forbidden.
    assert_eq!(
        ok(r#","min_format":82,"max_format":121,"supported_formats":[82,121]"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","min_format":82,"max_format":121"#),
        Compatibility::Compatible
    );
    // Only supported_formats: max above 81 is an error.
    assert_eq!(
        ok(r#","supported_formats":[20,82],"pack_format":20"#),
        Compatibility::Unknown
    );
    // Only one of min/max is an error.
    assert_eq!(ok(r#","min_format":121"#), Compatibility::Unknown);
    assert_eq!(ok(r#","max_format":121"#), Compatibility::Unknown);
    // Nothing at all is an error.
    assert_eq!(ok(""), Compatibility::Unknown);
    // Integer-range encodings.
    assert_eq!(
        ok(r#","supported_formats":20,"pack_format":20"#),
        Compatibility::TooOld
    );
    assert_eq!(
        ok(r#","supported_formats":[30,20],"pack_format":25"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","supported_formats":[20,30,40],"pack_format":25"#),
        Compatibility::Unknown
    );
    assert_eq!(
        ok(r#","supported_formats":{"min_inclusive":20},"pack_format":20"#),
        Compatibility::Unknown
    );
}

#[test]
fn resource_pack_formats_use_their_own_constants() {
    let m = read_with(
        &pack_obj(r#","min_format":97,"max_format":97"#),
        PackType::Resource,
    )
    .unwrap();
    assert_eq!(m.range, range((97, 0), (97, w())));
    assert_eq!(m.compatibility, Compatibility::Compatible); // 97.1 inside [97.0, 97.*]
    let m = read_with(&pack_obj(r#","pack_format":64"#), PackType::Resource).unwrap();
    assert_eq!(m.compatibility, Compatibility::TooOld);
    let m = read_with(&pack_obj(r#","pack_format":65"#), PackType::Resource).unwrap();
    assert_eq!(m.compatibility, Compatibility::Unknown); // 65 > LPM 64
    assert_eq!(PackType::Resource.current_format().to_string(), "97.1");
    assert_eq!(PackType::Data.current_format().to_string(), "121.0");
    assert_eq!(PackFormat::wildcard(5).to_string(), "5.*");
}

// --- dropped vs kept (spec 3.2) --------------------------------------------------------------------------------

#[test]
fn edge_08_dropped_versus_incompatible_packs() {
    // Dropped.
    assert_eq!(read_missing(), DropReason::MissingMetadata);
    assert_eq!(read("{}").unwrap_err(), DropReason::MissingMetadata);
    assert!(matches!(read("{\"pack\":"), Err(DropReason::Failed(_))));
    assert!(matches!(read("[]"), Err(DropReason::Failed(_))));
    assert!(
        matches!(
            read("{\"pack\":{\"pack_format\":48}}"),
            Err(DropReason::Failed(_))
        ),
        "no description"
    );
    assert!(
        matches!(
            read(&pack_obj("").replace("\"d\"", "5")),
            Err(DropReason::Failed(_))
        ),
        "invalid description"
    );
    assert!(matches!(
        read(&format!(
            "{},\"features\":{{\"enabled\":[\"minecraft:nope\"]}}}}",
            &pack_obj("")[..pack_obj("").len() - 1]
        )),
        Err(DropReason::Failed(_))
    ));
    // `pack` null is an invalid section: the fallback fails too.
    assert!(matches!(
        read("{\"pack\":null}"),
        Err(DropReason::Failed(_))
    ));
    // Kept as UNKNOWN: invalid format declaration but a valid description.
    let m = read(&pack_obj(r#","pack_format":999"#)).unwrap();
    assert_eq!(m.compatibility, Compatibility::Unknown);
    // Kept: valid but too old / too new.
    assert_eq!(
        read(&pack_obj(r#","pack_format":20"#))
            .unwrap()
            .compatibility,
        Compatibility::TooOld
    );
    assert_eq!(
        read(&pack_obj(r#","min_format":122,"max_format":122"#))
            .unwrap()
            .compatibility,
        Compatibility::TooNew
    );
    // Unknown top-level keys are ignored; trailing text is ignored.
    assert!(
        read(&format!(
            "{} // trailing",
            pack_obj(r#","min_format":121,"max_format":121"#)
        ))
        .is_ok()
    );
    assert!(
        read("{\"other\":1,\"pack\":{\"description\":\"d\",\"pack_format\":48,\"extra\":1}}")
            .is_ok()
    );
}

fn read_missing() -> DropReason {
    let log = MemoryLog::shared();
    read_pack_metadata_from(
        &McmetaFile::Missing,
        "t",
        PackType::Data,
        &FeatureUniverse::builtin_main(),
        &*log,
    )
    .unwrap_err()
}

#[test]
fn description_forms() {
    let ok = |d: &str| {
        read(&format!(
            "{{\"pack\":{{\"description\":{d},\"pack_format\":48}}}}"
        ))
        .is_ok()
    };
    assert!(ok("\"plain\""));
    assert!(ok("[\"a\",{\"text\":\"b\"}]"));
    assert!(ok("{\"text\":\"t\",\"color\":\"red\",\"bold\":true}"));
    assert!(ok("{\"translate\":\"k\",\"with\":[\"x\",1]}"));
    assert!(ok("{\"text\":\"t\",\"color\":\"#FF00aa\"}"));
    assert!(ok("{\"text\":\"t\",\"extra\":[\"e\"]}"));
    for bad in [
        "5",
        "true",
        "{}",
        "[]",
        "null",
        "{\"text\":\"t\",\"color\":\"nope\"}",
        "{\"text\":\"t\",\"bold\":1}",
        "{\"extra\":[]}",
    ] {
        assert!(!ok(bad), "should reject description {bad}");
    }
}

#[test]
fn features_section_rules() {
    let with = |f: &str| {
        read(&format!(
            "{{\"pack\":{{\"description\":\"d\",\"pack_format\":48}},\"features\":{f}}}"
        ))
    };
    let m = with(r#"{"enabled":["minecraft:trade_rebalance","redstone_experiments"]}"#).unwrap();
    let u = FeatureUniverse::builtin_main();
    let names: Vec<String> = u
        .to_names(&m.requested_features)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        names,
        [
            "minecraft:trade_rebalance",
            "minecraft:redstone_experiments"
        ]
    );
    assert!(
        with(r#"{"enabled":[]}"#)
            .unwrap()
            .requested_features
            .is_empty()
    );
    for bad in [
        r#"{"enabled":["minecraft:nope"]}"#,
        r#"{}"#,
        r#"[]"#,
        r#"{"enabled":"x"}"#,
        r#"{"enabled":["Bad Id"]}"#,
        r#"{"enabled":[1]}"#,
        "null",
    ] {
        assert!(with(bad).is_err(), "should drop for {bad}");
    }
    // No features section: empty request.
    assert!(
        read(&pack_obj(r#","pack_format":48"#))
            .unwrap()
            .requested_features
            .is_empty()
    );
}

// --- overlays (spec 3.5) -----------------------------------------------------------------------------------

fn overlay_pack(entries: &str) -> String {
    format!(
        "{{\"pack\":{{\"description\":\"d\",\"min_format\":121,\"max_format\":121}},\"overlays\":{{\"entries\":{entries}}}}}"
    )
}

fn active(entries: &str) -> Result<Vec<String>, DropReason> {
    read(&overlay_pack(entries)).map(|m| m.overlays)
}

#[test]
fn edge_11_overlay_all_or_nothing_legacy_rule_and_examples() {
    // Example from the spec: a [120,121], b min 121 / max [121,0], c [122,130] -> active [a, b].
    let r = active(
        r#"[{"directory":"a","min_format":120,"max_format":121},
           {"directory":"b","min_format":121,"max_format":[121,0]},
           {"directory":"c","min_format":122,"max_format":130}]"#,
    )
    .unwrap();
    assert_eq!(r, ["a", "b"]);
    // {"directory":"x","formats":5} is valid but inactive.
    assert_eq!(
        active(r#"[{"directory":"x","formats":5}]"#).unwrap(),
        Vec::<String>::new()
    );
    // formats:[80,81] plus an entry with only min/max 121 is invalid (second lacks `formats`): the pack is dropped.
    assert!(matches!(
        active(
            r#"[{"directory":"a","formats":[80,81]},{"directory":"b","min_format":121,"max_format":121}]"#
        ),
        Err(DropReason::Failed(_))
    ));
    // Entries with no version fields are skipped with a warning, not rejected.
    let log = MemoryLog::shared();
    let raw = decode_overlay_entries(&parse(r#"{"entries":[{"directory":"nada"},{"directory":"ok","min_format":121,"max_format":121}]}"#, ParseMode::Data).unwrap()).demand().unwrap();
    let valid = validate_overlays(PackType::Data, &raw, &*log).unwrap();
    assert_eq!(valid.len(), 1);
    assert!(log.contains(Level::Warn, "nada"));
    // Active overlays keep listed order, the last one wins (here: reported in listed order).
    assert_eq!(
        active(r#"[{"directory":"z","min_format":121,"max_format":121},{"directory":"a","min_format":121,"max_format":121}]"#).unwrap(),
        ["z", "a"]
    );
    // Legacy entries with formats: [80, 121] min_format 80 ... valid combination.
    assert_eq!(
        active(r#"[{"directory":"old","min_format":80,"max_format":121,"formats":[80,81]}]"#)
            .unwrap(),
        ["old"]
    );
    // With formats present for a >81 entry while no entry is old: error.
    assert!(
        active(r#"[{"directory":"a","min_format":121,"max_format":121,"formats":[121,121]}]"#)
            .is_err()
    );
    // Exactly one of min/max: error. min > max: error. formats-only above 81: error.
    assert!(active(r#"[{"directory":"a","min_format":121}]"#).is_err());
    assert!(active(r#"[{"directory":"a","min_format":122,"max_format":121}]"#).is_err());
    assert!(active(r#"[{"directory":"a","formats":[100,121]}]"#).is_err());
    // `entries` is required; any entry decode error drops the pack.
    assert!(read(r#"{"pack":{"description":"d","pack_format":48},"overlays":{}}"#).is_err());
    assert!(active(r#"[{"min_format":121,"max_format":121}]"#).is_err());
    assert!(active(r#"[5]"#).is_err());
}

#[test]
fn edge_12_overlay_directory_names() {
    let ok = |d: &str| {
        active(&format!(
            r#"[{{"directory":"{d}","min_format":121,"max_format":121}}]"#
        ))
    };
    for good in ["Overlay_1", "a.b", "A-B", ".", "..", "...", "x"] {
        assert_eq!(ok(good).unwrap(), [good], "{good}");
    }
    for bad in ["a/b", "", "a b", "é", "a\\\\b"] {
        assert!(ok(bad).is_err(), "should reject {bad:?}");
    }
}

// --- filter (spec 3.7) --------------------------------------------------------------------------------------

#[test]
fn filter_section_is_kept_or_ignored() {
    let with = |f: &str| {
        read(&format!(
            "{{\"pack\":{{\"description\":\"d\",\"pack_format\":48}},\"filter\":{f}}}"
        ))
        .unwrap()
    };
    assert!(matches!(
        with(r#"{"block":[{"namespace":"a","path":"b"}]}"#).filter,
        FilterSection::Valid(_)
    ));
    assert!(matches!(
        with(r#"{"block":[]}"#).filter,
        FilterSection::Valid(_)
    ));
    // Invalid filters never drop the pack.
    for bad in [
        r#"{}"#,
        r#"[]"#,
        r#"{"block":[{"namespace":"("}]}"#,
        r#"{"block":[{"path":5}]}"#,
        r#"{"block":5}"#,
        "null",
    ] {
        assert!(
            matches!(with(bad).filter, FilterSection::Invalid(_)),
            "{bad}"
        );
    }
    let m = read(&pack_obj(r#","pack_format":48"#)).unwrap();
    assert!(matches!(m.filter, FilterSection::Absent));
}

#[test]
fn java_regex_dialect() {
    use zenith_codec::filter::JavaRegex;
    let re = |p: &str| JavaRegex::new(p).unwrap();
    // Unanchored search.
    assert!(re("tone").find_in("stone"));
    assert!(!re("^tone").find_in("stone"));
    assert!(re("^st").find_in("stone"));
    assert!(re("one$").find_in("stone"));
    assert!(!re("on$").find_in("stone"));
    // (?i), \Q..\E, look-ahead, look-behind, backreferences.
    assert!(re("(?i)STONE").find_in("stone"));
    assert!(!re("STONE").find_in("stone"));
    assert!(re(r"\Q.json\E").find_in("a.json"));
    assert!(!re(r"\Q.json\E").find_in("a_json"));
    assert!(re(r"\Qa+b\E").find_in("xa+by"));
    assert!(!re(r"\Qa+b\E").find_in("xaab"));
    assert!(re(r"^(?!keep).*").find_in("drop/x"));
    assert!(!re(r"^(?!keep).*$").find_in("keep/x"));
    assert!(re(r"(?<=recipe/)x").find_in("recipe/x"));
    assert!(re(r"(a)\1").find_in("xaay"));
    assert!(re(r"\Qx").find_in("x")); // an unterminated \Q quotes to the end
    // Invalid patterns fail.
    assert!(JavaRegex::new("(").is_err());
    assert!(JavaRegex::new("[").is_err());
    assert!(JavaRegex::new("*a").is_err());
}

#[test]
fn log_mentions_dropped_packs() {
    let log = MemoryLog::shared();
    let r = read_pack_metadata_from(
        &McmetaFile::from_text("{\"pack\":{}}"),
        "file/broken",
        PackType::Data,
        &FeatureUniverse::builtin_main(),
        &*log,
    );
    assert!(r.is_err());
    assert!(log.contains(Level::Warn, "file/broken"));
    let _ = data_ctx(&log);
}
