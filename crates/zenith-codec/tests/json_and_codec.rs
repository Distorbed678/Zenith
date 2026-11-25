mod common;

use zenith_codec::codec::*;
use zenith_codec::json::{Json, ParseMode, parse};
use zenith_codec::log::{Level, MemoryLog};
use zenith_registry::Identifier;

fn data(text: &str) -> Result<Json, String> {
    parse(text, ParseMode::Data).map_err(|e| e.to_string())
}

fn mcmeta(text: &str) -> Result<Json, String> {
    parse(text, ParseMode::Metadata).map_err(|e| e.to_string())
}

fn j(text: &str) -> Json {
    data(text).unwrap()
}

// --- edge 1 -------------------------------------------------------------------------------------------------

#[test]
fn edge_01_strict_json_but_bom_tolerant_duplicate_keys_last_wins() {
    assert!(data("\u{feff}{\"a\":1}").is_ok());
    assert!(mcmeta("\u{feff}{\"a\":1}").is_ok());
    // Last value wins, in the position of the first key.
    let v = j("{\"a\":1,\"b\":0,\"a\":2}");
    assert_eq!(v.get("a").unwrap().as_number().unwrap().text(), "2");
    assert_eq!(
        v.as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(v.as_object().unwrap().len(), 2);

    for bad in [
        "// c\n{}",
        "/* c */ {}",
        "{/*c*/}",
        "[1,2,]",
        "{\"a\":1,}",
        "[1,,2]",
        "{'a':1}",
        "{\"a\":'x'}",
        "{a:1}",
        "NaN",
        "Infinity",
        "-Infinity",
        "01",
        "-01",
        "1.",
        ".5",
        "+1",
        "1e",
        "-",
        "\"a\u{1}b\"",
        "\"a\nb\"",
        "\"a\tb\"",
        "\"it\\'s\"",
        "\"\\x41\"",
        "\"\\u12\"",
        "{\"a\" 1}",
        "{\"a\":1 \"b\":2}",
        "tru",
        "trueX",
        "nul",
        "[1 2]",
        "{",
        "[",
        "\"abc",
    ] {
        assert!(data(bad).is_err(), "should reject {bad:?}");
    }
    for good in [
        "-0",
        "1e400",
        "-1e-400",
        "0",
        "0.5",
        "1E+2",
        "[]",
        "{}",
        "\"\"",
        "true",
        "false",
        "null",
        " \t\r\n[ 1 , 2 ] ",
        "\"\\u00e9\\n\\/\\b\\f\\r\\t\\\"\\\\\"",
    ] {
        assert!(data(good).is_ok(), "should accept {good:?}");
    }
    // Key order is preserved.
    let v = j("{\"z\":1,\"a\":2,\"m\":3}");
    assert_eq!(
        v.as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["z", "a", "m"]
    );
}

#[test]
fn json_string_escapes_decode() {
    assert_eq!(j("\"a\\u00e9\\n\\\"\"").as_str(), Some("a\u{e9}\n\""));
    // A surrogate pair combines.
    assert_eq!(j("\"\\ud83d\\ude00\"").as_str(), Some("\u{1f600}"));
    // Lone surrogates are preserved losslessly as raw UTF-16 code units.
    let lone = j("\"\\ud83d\"");
    assert_eq!(lone.as_str(), None);
    assert_eq!(lone.as_json_str().unwrap().to_utf16(), vec![0xD83D]);
    assert_eq!(lone.as_json_str().unwrap().to_string_lossy(), "\u{fffd}");
    // Raw non-ASCII passes through.
    assert_eq!(
        j("\"h\u{e9}llo \u{1f600}\"").as_str(),
        Some("h\u{e9}llo \u{1f600}")
    );
}

#[test]
fn json_unpaired_surrogates_survive_parse_and_reencode() {
    use zenith_codec::json::JsonString;

    // Lone high surrogate: "\ud800"
    let hi = j("\"\\ud800\"");
    assert_eq!(hi.as_json_str().unwrap().to_utf16(), vec![0xD800]);
    assert_eq!(hi.to_json_string(), "\"\\ud800\"");
    assert_eq!(j(&hi.to_json_string()), hi);

    // Lone low surrogate followed by ASCII: "\udc00x"
    let lo_x = j("\"\\udc00x\"");
    assert_eq!(
        lo_x.as_json_str().unwrap().to_utf16(),
        vec![0xDC00, b'x' as u16]
    );
    assert_eq!(lo_x.to_json_string(), "\"\\udc00x\"");
    assert_eq!(j(&lo_x.to_json_string()), lo_x);

    // High surrogate followed by a valid surrogate pair: "\ud800\ud83d\ude00"
    let mixed = j("\"\\ud800\\ud83d\\ude00\"");
    assert_eq!(
        mixed.as_json_str().unwrap().to_utf16(),
        vec![0xD800, 0xD83D, 0xDE00]
    );
    assert_eq!(mixed.to_json_string(), "\"\\ud800\u{1f600}\"");
    assert_eq!(j(&mixed.to_json_string()), mixed);

    // Unpaired surrogates in object keys and duplicate-key replacement.
    let obj_val = j("{\"\\ud800\":1,\"\\udc00x\":\"\\ud800\",\"\\ud800\":\"\\udc00x\"}");
    let obj = obj_val.as_object().unwrap();
    assert_eq!(obj.len(), 2);
    let k_hi = JsonString::from_utf16(vec![0xD800]);
    let k_lo_x = JsonString::from_utf16(vec![0xDC00, b'x' as u16]);
    assert_eq!(
        obj.get_key(&k_hi)
            .unwrap()
            .as_json_str()
            .unwrap()
            .to_utf16(),
        vec![0xDC00, b'x' as u16]
    );
    assert_eq!(
        obj.get_key(&k_lo_x)
            .unwrap()
            .as_json_str()
            .unwrap()
            .to_utf16(),
        vec![0xD800]
    );
    let encoded = obj_val.to_json_string();
    assert_eq!(
        encoded,
        "{\"\\ud800\":\"\\udc00x\",\"\\udc00x\":\"\\ud800\"}"
    );
    assert_eq!(j(&encoded), obj_val);
}

#[test]
fn json_malformed_utf8_becomes_replacement_character() {
    let bytes = b"\"a\xffb\"";
    let v = zenith_codec::json::parse_bytes(bytes, ParseMode::Data).unwrap();
    assert_eq!(v.as_str(), Some("a\u{fffd}b"));
}

#[test]
fn json_depth_limit_is_an_error_not_a_crash() {
    let deep = "[".repeat(100_000);
    assert!(data(&deep).is_err());
}

// --- edge 2 -------------------------------------------------------------------------------------------------

#[test]
fn edge_02_mcmeta_ignores_trailing_text_but_data_files_do_not() {
    for tail in [" // comment", " garbage", " {\"second\":1}", "\n/* x */"] {
        let text = format!("{{\"a\":1}}{tail}");
        assert!(mcmeta(&text).is_ok(), "mcmeta should accept {text:?}");
        assert!(data(&text).is_err(), "data should reject {text:?}");
    }
    // Comments *inside* the object are still rejected.
    assert!(mcmeta("{/*x*/\"a\":1}").is_err());
    assert!(mcmeta("{\"a\":1,}").is_err());
    // Root must be an object, and an empty file is an error.
    assert!(mcmeta("[1]").is_err());
    assert!(mcmeta("1").is_err());
    assert!(mcmeta("").is_err());
    assert!(mcmeta("  \n ").is_err());
    // An empty data file is JSON null, which then fails its codec.
    assert_eq!(j(""), Json::Null);
    assert_eq!(j(" \n\t"), Json::Null);
    assert!(IntCodec.decode(&j("")).message().is_some());
    let r = IntCodec.parse_text(b"");
    assert!(r.message().is_some());
    // Top-level primitives and arrays are fine for data files.
    assert_eq!(j("5").as_number().unwrap().text(), "5");
    assert!(j("[1]").as_array().is_some());
}

// --- edge 3 -------------------------------------------------------------------------------------------------

fn int(text: &str) -> DataResult<i32> {
    IntCodec.decode(&j(text))
}

#[test]
fn edge_03_integers_truncate_and_wrap() {
    assert_eq!(int("1.9").success(), Some(1));
    assert_eq!(int("-1.9").success(), Some(-1));
    assert_eq!(int("3000000000").success(), Some(-1294967296));
    assert_eq!(int("1e20").success(), Some(1661992960));
    assert_eq!(int("1e2").success(), Some(100));
    assert_eq!(int("1e400").success(), Some(0));
    assert_eq!(int("-0.5").success(), Some(0));
    assert_eq!(int("-0").success(), Some(0));
    assert_eq!(int("2147483648").success(), Some(i32::MIN));
    assert_eq!(int("-2147483649").success(), Some(i32::MAX));
    assert_eq!(
        int("12345678901234567890123").success(),
        Some(wrap_big(12345678901234567890123u128))
    );
    assert_eq!(int("1.5e1").success(), Some(15));
    assert_eq!(int("123e-1").success(), Some(12));
    assert_eq!(int("0.000001e6").success(), Some(1));
    assert_eq!(int("1e-5").success(), Some(0));
    assert_eq!(ByteCodec.decode(&j("300")).success(), Some(44));
    assert_eq!(ByteCodec.decode(&j("-129")).success(), Some(127));
    assert_eq!(ShortCodec.decode(&j("70000")).success(), Some(4464));
    assert_eq!(LongCodec.decode(&j("1e3")).success(), Some(1000));
    assert_eq!(
        LongCodec.decode(&j("9223372036854775808")).success(),
        Some(i64::MIN)
    );
    assert_eq!(LongCodec.decode(&j("1.5")).success(), Some(1));
    assert_eq!(
        LongCodec.decode(&j("1e20")).success(),
        Some(1e20_f64 as u128 as i64)
    );
}

fn wrap_big(v: u128) -> i32 {
    v as u32 as i32
}

#[test]
fn edge_03_floats_parse_directly_from_decimal_text() {
    assert_eq!(FloatCodec.decode(&j("1e40")).success(), Some(f32::INFINITY));
    assert_eq!(
        FloatCodec.decode(&j("-1e40")).success(),
        Some(f32::NEG_INFINITY)
    );
    assert_eq!(
        DoubleCodec.decode(&j("1e400")).success(),
        Some(f64::INFINITY)
    );
    assert_eq!(DoubleCodec.decode(&j("1.5")).success(), Some(1.5));
    // 16777217.0000000001 is just above the midpoint between two floats; a double-rounding implementation
    // (text -> f64 -> f32) lands on the even neighbour 16777216 instead of 16777218.
    assert_eq!(
        FloatCodec.decode(&j("16777217.0000000001")).success(),
        Some(16_777_218.0_f32)
    );
    assert_eq!(FloatCodec.decode(&j("1e-60")).success(), Some(0.0));
    let z = FloatCodec.decode(&j("-0")).success().unwrap();
    assert!(z == 0.0 && z.is_sign_negative());
}

#[test]
fn edge_03_absurd_exponents_are_a_hard_failure_not_a_decode_error() {
    for text in ["1e10001", "1e-10001", "1e99999999999999999999", "5E+100000"] {
        match int(text) {
            DataResult::Failure(_) => {}
            other => panic!("{text}: expected hard failure, got {other:?}"),
        }
    }
    // An exponent magnitude of 10000 is already rejected (oracle-verified, datapack-load 2026-11-25); 9999 is not.
    for text in ["1e10000", "1e-10000"] {
        assert!(matches!(int(text), DataResult::Failure(_)), "{text}");
    }
    assert_eq!(int("1e9999").success(), Some(0));
    assert_eq!(int("1e-9999").success(), Some(0));
    // Floats never throw.
    assert!(FloatCodec.decode(&j("1e99999999999999999999")).is_success());
    // The hard failure aborts record decoding immediately and propagates.
    let obj = j("{\"a\":\"bad\",\"b\":1e20000}");
    let mut r = RecordReader::new(&obj);
    let _ = r.required("a", &IntCodec);
    let _ = r.required("b", &IntCodec);
    assert!(matches!(r.finish(|| Some(())), DataResult::Failure(_)));
    // Consumers catch it per element.
    let mut errors = RegistryLoadErrors::new();
    let reg = Identifier::parse("worldgen/biome").unwrap();
    let el = Identifier::parse("x").unwrap();
    assert!(
        errors
            .load_element(&reg, &el, "p", b"1e20000", &IntCodec)
            .is_none()
    );
    assert_eq!(errors.len(), 1);
}

// --- edge 4 -------------------------------------------------------------------------------------------------

#[test]
fn edge_04_no_type_coercion() {
    assert!(BoolCodec.decode(&j("1")).message().is_some());
    assert!(BoolCodec.decode(&j("\"true\"")).message().is_some());
    assert_eq!(BoolCodec.decode(&j("true")).success(), Some(true));
    assert!(StringCodec.decode(&j("5")).message().is_some());
    assert!(StringCodec.decode(&j("true")).message().is_some());
    assert!(StringCodec.decode(&j("[\"a\"]")).message().is_some());
    assert_eq!(
        StringCodec.decode(&j("\"5\"")).success(),
        Some("5".to_owned())
    );
    assert!(IntCodec.decode(&j("\"5\"")).message().is_some());
    assert!(IntCodec.decode(&j("true")).message().is_some());
    assert!(FloatCodec.decode(&j("\"1.0\"")).message().is_some());
    assert!(DoubleCodec.decode(&j("false")).message().is_some());
    assert!(IntCodec.decode(&j("null")).message().is_some());
    // The description of a pack must not be a number.
    assert!(
        zenith_codec::text::ComponentCodec
            .decode(&j("5"))
            .message()
            .is_some()
    );
}

// --- edge 5 / 6: records -------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone)]
struct Rec {
    a: i32,
    b: i32,
    c: Option<String>,
    d: Vec<i32>,
}

fn decode_rec(input: &Json) -> DataResult<Rec> {
    let mut r = RecordReader::new(input);
    let a = r.required("a", &IntCodec);
    let b = r.optional("b", &IntCodec, 7);
    let c = r.optional_opt("c", &StringCodec);
    let d = r.lenient("d", &list(IntCodec), vec![9]);
    r.finish(|| Some(Rec { a: a?, b, c, d }))
}

#[test]
fn edge_05_null_counts_as_absent() {
    // Required field set to null: "no key".
    let r = decode_rec(&j("{\"a\":null}"));
    assert!(r.message().unwrap().contains("No key a"));
    // Optional field set to null: default.
    let r = decode_rec(&j("{\"a\":1,\"b\":null,\"c\":null}"));
    assert_eq!(
        r.success(),
        Some(Rec {
            a: 1,
            b: 7,
            c: None,
            d: vec![9]
        })
    );
    // A null list element and a null map value are errors.
    assert!(list(IntCodec).decode(&j("[1,null]")).message().is_some());
    assert!(
        string_map(IntCodec)
            .decode(&j("{\"x\":null}"))
            .message()
            .is_some()
    );
}

#[test]
fn edge_06_strict_versus_lenient_optionals() {
    // Strict optional: present but invalid fails the record, partial has the default.
    let r = decode_rec(&j("{\"a\":1,\"b\":\"x\"}"));
    match r {
        DataResult::Error { message, partial } => {
            assert!(message.contains("b:"));
            assert_eq!(
                partial,
                Some(Rec {
                    a: 1,
                    b: 7,
                    c: None,
                    d: vec![9]
                })
            );
        }
        other => panic!("{other:?}"),
    }
    // Lenient: invalid silently becomes the default, even a partly valid list.
    let r = decode_rec(&j("{\"a\":1,\"d\":[1,\"x\",3]}"));
    assert_eq!(
        r.success(),
        Some(Rec {
            a: 1,
            b: 7,
            c: None,
            d: vec![9]
        })
    );
    // Optional without default: invalid is an error.
    assert!(decode_rec(&j("{\"a\":1,\"c\":5}")).message().is_some());
}

#[test]
fn records_accumulate_all_errors_joined_and_ignore_unknown_keys() {
    let r = decode_rec(&j("{\"b\":\"x\",\"c\":5,\"unknown\":[1,2]}"));
    let msg = r.message().unwrap().to_owned();
    assert!(msg.contains("No key a"), "{msg}");
    assert!(msg.contains("b:"), "{msg}");
    assert!(msg.contains("c:"), "{msg}");
    assert_eq!(msg.matches("; ").count(), 2, "{msg}");
    // Not an object.
    assert!(
        decode_rec(&j("[1]"))
            .message()
            .unwrap()
            .contains("Not a JSON object")
    );
    // Unknown keys alone are fine.
    assert!(decode_rec(&j("{\"a\":1,\"zzz\":{}}")).is_success());
}

#[test]
fn records_encode_omits_defaults_and_empty_optionals() {
    let mut w = RecordWriter::new();
    w.field("a", &IntCodec, &1)
        .field_default("b", &IntCodec, &7, &7)
        .field_default("b2", &IntCodec, &8, &7)
        .field_opt("c", &StringCodec, None::<&String>)
        .field_opt("c2", &StringCodec, Some(&"x".to_owned()));
    let json = w.finish().success().unwrap();
    assert_eq!(json.to_json_string(), "{\"a\":1,\"b2\":8,\"c2\":\"x\"}");
}

// --- composite shapes ---------------------------------------------------------------------------------------

#[test]
fn lists_keep_good_elements_as_partial_and_cut_when_too_long() {
    match list(IntCodec).decode(&j("[1,\"x\",3]")) {
        DataResult::Error { partial, .. } => assert_eq!(partial, Some(vec![1, 3])),
        other => panic!("{other:?}"),
    }
    assert!(
        list(IntCodec)
            .decode(&j("{}"))
            .message()
            .unwrap()
            .contains("Not a json array")
    );
    let c = sized_list(IntCodec, 2, 3);
    assert!(c.decode(&j("[1]")).message().unwrap().contains("too short"));
    match c.decode(&j("[1,2,3,4,5]")) {
        DataResult::Error { message, partial } => {
            assert!(message.contains("too long"));
            assert_eq!(partial, Some(vec![1, 2, 3]));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(c.decode(&j("[1,2]")).success(), Some(vec![1, 2]));
}

#[test]
fn string_maps_keep_good_entries_as_partial() {
    match string_map(IntCodec).decode(&j("{\"a\":1,\"b\":\"x\",\"c\":3}")) {
        DataResult::Error { message, partial } => {
            assert!(message.contains("b"), "{message}");
            assert_eq!(
                partial,
                Some(vec![("a".to_owned(), 1), ("c".to_owned(), 3)])
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        string_map(IntCodec)
            .decode(&j("[]"))
            .message()
            .unwrap()
            .contains("Not a JSON object")
    );
}

#[test]
fn either_first_success_wins_and_names_both_failures() {
    let c = either(IntCodec, StringCodec);
    assert_eq!(c.decode(&j("5")).success(), Some(Either::Left(5)));
    assert_eq!(
        c.decode(&j("\"x\"")).success(),
        Some(Either::Right("x".to_owned()))
    );
    let m = c.decode(&j("true")).message().unwrap().to_owned();
    assert!(m.contains("First:") && m.contains("Second:"), "{m}");
    // Input fitting both forms decodes as the first.
    let both = either(sized_list(IntCodec, 0, 5), list(IntCodec));
    assert_eq!(
        both.decode(&j("[1]")).success(),
        Some(Either::Left(vec![1]))
    );
}

#[test]
fn one_or_list_treats_a_lone_element_as_a_one_element_list() {
    let c = one_or_list(StringCodec);
    assert_eq!(c.decode(&j("\"a\"")).success(), Some(vec!["a".to_owned()]));
    assert_eq!(c.decode(&j("[\"a\",\"b\"]")).success().unwrap().len(), 2);
    assert!(
        c.decode(&j("5"))
            .message()
            .unwrap()
            .contains("Failed to parse either")
    );
    assert_eq!(
        c.encode(&vec!["a".to_owned()])
            .success()
            .unwrap()
            .to_json_string(),
        "\"a\""
    );
}

#[test]
fn type_dispatch_reads_the_type_and_ignores_extra_keys() {
    let decode = |text: &str| {
        decode_dispatch(&j(text), "type", |ty, input| match ty {
            "int" => {
                let mut r = RecordReader::new(input);
                let v = r.required("value", &IntCodec);
                Some(r.finish(|| v))
            }
            _ => None,
        })
    };
    assert_eq!(
        decode("{\"type\":\"int\",\"value\":3,\"junk\":true}").success(),
        Some(3)
    );
    assert!(
        decode("{\"value\":3}")
            .message()
            .unwrap()
            .contains("No key type")
    );
    assert!(
        decode("{\"type\":\"nope\"}")
            .message()
            .unwrap()
            .contains("Unknown type")
    );
    assert!(decode("{\"type\":5}").message().is_some());
}

#[test]
fn validated_values_check_after_conversion() {
    let c = non_negative_int();
    assert_eq!(c.decode(&j("-0.5")).success(), Some(0));
    assert!(c.decode(&j("-1")).message().is_some());
    assert!(c.decode(&j("4294967295")).message().is_some()); // wraps to -1
    let nonempty = validated(StringCodec, |s: &String| {
        if s.is_empty() {
            Err("empty".to_owned())
        } else {
            Ok(())
        }
    });
    assert!(nonempty.decode(&j("\"\"")).message().is_some());
    assert!(nonempty.decode(&j("\"x\"")).is_success());
}

#[test]
fn identifiers_decode_with_default_namespace() {
    let r = IdentifierCodec.decode(&j("\"stone\"")).success().unwrap();
    assert_eq!(r.to_string(), "minecraft:stone");
    assert!(IdentifierCodec.decode(&j("\"Bad:Id\"")).message().is_some());
    assert!(IdentifierCodec.decode(&j("5")).message().is_some());
}

// --- result consumption (spec 7.5/7.6) ------------------------------------------------------------------------

#[test]
fn consumers_demand_accept_partial_or_fall_back() {
    let partial = list(IntCodec).decode(&j("[1,\"x\"]"));
    // Demand success: a partial counts as failure.
    assert!(partial.clone().demand().is_err());
    // Accept partial: log and keep.
    let log = MemoryLog::shared();
    assert_eq!(partial.clone().accept_partial(&*log, "ctx"), Some(vec![1]));
    assert!(log.contains(Level::Error, "ctx"));
    // Fall back silently.
    assert_eq!(partial.or_default_silently(vec![0]), vec![0]);
    assert_eq!(
        Consumer::PackMetaSection.policy(),
        ErrorPolicy::DemandSuccess
    );
    assert_eq!(
        Consumer::WorldPackConfig.policy(),
        ErrorPolicy::LenientDefault
    );
    assert_eq!(Consumer::RegistryElement.policy(), ErrorPolicy::AbortBatch);
    assert_eq!(Consumer::TagFile.policy(), ErrorPolicy::SkipFile);
    assert_eq!(Consumer::FunctionFile.policy(), ErrorPolicy::SkipFile);
    assert_eq!(
        Consumer::LootValidation.policy(),
        ErrorPolicy::WarnThenAbortOnFatal
    );
}

// --- edge 7 -------------------------------------------------------------------------------------------------

#[test]
fn edge_07_one_bad_registry_file_records_errors_that_abort_the_whole_load() {
    struct Elem;
    impl Codec<(i32, i32)> for Elem {
        fn decode(&self, input: &Json) -> DataResult<(i32, i32)> {
            let mut r = RecordReader::new(input);
            let a = r.required("a", &IntCodec);
            let b = r.optional("b", &IntCodec, 0);
            r.finish(|| Some((a?, b)))
        }
        fn encode(&self, _v: &(i32, i32)) -> DataResult<Json> {
            DataResult::error("n/a")
        }
    }
    let recipe = Identifier::parse("recipe").unwrap();
    let advancement = Identifier::parse("advancement").unwrap();
    let id = |s: &str| Identifier::parse(s).unwrap();
    let mut errors = RegistryLoadErrors::new();
    let ok = errors.load_element(&recipe, &id("good"), "vanilla", br#"{"a":1}"#, &Elem);
    assert_eq!(ok, Some((1, 0)));
    assert!(errors.is_empty());
    // Malformed JSON, a schema error, and a partial result each record an error.
    assert!(
        errors
            .load_element(&recipe, &id("z_bad_json"), "file/p", b"{", &Elem)
            .is_none()
    );
    assert!(
        errors
            .load_element(&recipe, &id("a_missing"), "file/p", b"{}", &Elem)
            .is_none()
    );
    assert!(
        errors
            .load_element(
                &advancement,
                &id("partial"),
                "file/q",
                br#"{"a":1,"b":"x"}"#,
                &Elem
            )
            .is_none()
    );
    assert!(!errors.is_empty());
    assert_eq!(errors.len(), 3);
    // Grouped by registry (sorted), elements sorted by id; each names the pack.
    let grouped = errors.grouped();
    let keys: Vec<String> = grouped.keys().map(ToString::to_string).collect();
    assert_eq!(keys, ["minecraft:advancement", "minecraft:recipe"]);
    let recipe_errors: Vec<String> = grouped[&recipe]
        .iter()
        .map(|e| e.element.to_string())
        .collect();
    assert_eq!(
        recipe_errors,
        ["minecraft:a_missing", "minecraft:z_bad_json"]
    );
    let report = errors.log_report();
    assert!(
        report.contains("file/p") && report.contains("file/q"),
        "{report}"
    );
    let lines = errors.crash_report_lines();
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].starts_with("minecraft:advancement/minecraft:partial: "),
        "{lines:?}"
    );
}

// --- numbers on the JSON layer ---------------------------------------------------------------------------------

#[test]
fn number_text_is_preserved_exactly() {
    let v = j("[1.0, 1e2, -0, 12345678901234567890]");
    let texts: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_number().unwrap().text())
        .collect();
    assert_eq!(texts, ["1.0", "1e2", "-0", "12345678901234567890"]);
}
