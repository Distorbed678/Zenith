//! SNBT printer and 1.21.5+ parser tests.

use zenith_nbt::{Compound, List, NbtString, Tag, parse_snbt, parse_snbt_compound, snbt, to_snbt};

#[test]
fn printer_key_quoting_rules() {
    // Bare keys: start with letter, `.`, `_`; continue with letter/digit/`.`/`_`/`+`/`-`; not true/false
    assert!(snbt::is_bare_key(&NbtString::from("foo")));
    assert!(snbt::is_bare_key(&NbtString::from("_bar")));
    assert!(snbt::is_bare_key(&NbtString::from(".dot")));
    assert!(snbt::is_bare_key(&NbtString::from("a0._+-Z")));

    // Must be quoted
    assert!(!snbt::is_bare_key(&NbtString::from("")));
    assert!(!snbt::is_bare_key(&NbtString::from("0abc")));
    assert!(!snbt::is_bare_key(&NbtString::from("+abc")));
    assert!(!snbt::is_bare_key(&NbtString::from("-abc")));
    assert!(!snbt::is_bare_key(&NbtString::from("true")));
    assert!(!snbt::is_bare_key(&NbtString::from("TRUE")));
    assert!(!snbt::is_bare_key(&NbtString::from("False")));
    assert!(!snbt::is_bare_key(&NbtString::from("minecraft:stone")));
    assert!(!snbt::is_bare_key(&NbtString::from("has space")));
}

#[test]
fn printer_string_quote_selection_and_control_escapes() {
    // No quotes -> double quotes
    assert_eq!(to_snbt(&Tag::from("hello")).unwrap(), "\"hello\"");
    // First quote is `"` -> uses single quotes, leaves `"` unescaped
    assert_eq!(to_snbt(&Tag::from("say \"hi\"")).unwrap(), "'say \"hi\"'");
    // First quote is `'` -> uses double quotes, leaves `'` unescaped
    assert_eq!(
        to_snbt(&Tag::from("it's \"fine\"")).unwrap(),
        "\"it's \\\"fine\\\"\""
    );
    // First quote is `"` then `'` -> single quotes, escapes `'`, leaves `"` alone
    assert_eq!(
        to_snbt(&Tag::from("\"a\" and 'b'")).unwrap(),
        "'\"a\" and \\'b\\''"
    );
    // Backslash always escaped
    assert_eq!(to_snbt(&Tag::from("a\\b")).unwrap(), "\"a\\\\b\"");

    // Control characters: \b \t \n \f \r and \xXX for other < U+0020
    let controls = "\u{0000}\u{0007}\u{0008}\t\n\u{000B}\u{000C}\r\u{001F}";
    assert_eq!(
        to_snbt(&Tag::from(controls)).unwrap(),
        "\"\\x00\\x07\\b\\t\\n\\x0b\\f\\r\\x1f\""
    );
}

#[test]
fn printer_typed_arrays_and_primitives() {
    assert_eq!(to_snbt(&Tag::Byte(-5)).unwrap(), "-5b");
    assert_eq!(to_snbt(&Tag::Short(320)).unwrap(), "320s");
    assert_eq!(to_snbt(&Tag::Int(42)).unwrap(), "42");
    assert_eq!(to_snbt(&Tag::Long(999)).unwrap(), "999L");
    assert_eq!(to_snbt(&Tag::Float(1.0)).unwrap(), "1.0f");
    assert_eq!(to_snbt(&Tag::Double(2.5)).unwrap(), "2.5d");
    assert_eq!(to_snbt(&Tag::ByteArray(vec![1, -2])).unwrap(), "[B;1B,-2B]");
    assert_eq!(to_snbt(&Tag::IntArray(vec![10, 20])).unwrap(), "[I;10,20]");
    assert_eq!(
        to_snbt(&Tag::LongArray(vec![100, -200])).unwrap(),
        "[L;100L,-200L]"
    );
}

#[test]
fn parser_numbers_hex_binary_underscores_signedness_and_floats() {
    // Standard suffixes (including 0b / 0B byte zero vs binary prefix!)
    assert_eq!(parse_snbt("0b").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("0B").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("-0b").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("+0B").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("0b0").unwrap(), Tag::Int(0));
    assert_eq!(parse_snbt("0b0b").unwrap(), Tag::Byte(0));
    assert!(parse_snbt("0x").is_err());
    assert_eq!(parse_snbt("127b").unwrap(), Tag::Byte(127));
    assert_eq!(parse_snbt("-128B").unwrap(), Tag::Byte(-128));
    assert_eq!(parse_snbt("32767s").unwrap(), Tag::Short(32767));
    assert_eq!(parse_snbt("-32768S").unwrap(), Tag::Short(-32768));
    assert_eq!(parse_snbt("42").unwrap(), Tag::Int(42));
    assert_eq!(parse_snbt("42i").unwrap(), Tag::Int(42));
    assert_eq!(parse_snbt("-42si").unwrap(), Tag::Int(-42));
    assert_eq!(
        parse_snbt("1_000_000_000L").unwrap(),
        Tag::Long(1_000_000_000)
    );

    // Signed/unsigned suffixes
    assert_eq!(parse_snbt("240ub").unwrap(), Tag::Byte(-16));
    assert_eq!(parse_snbt("-16sb").unwrap(), Tag::Byte(-16));
    assert_eq!(parse_snbt("65535us").unwrap(), Tag::Short(-1));
    assert_eq!(parse_snbt("4_294_967_295ui").unwrap(), Tag::Int(-1));
    assert_eq!(parse_snbt("4_294_967_295u").unwrap(), Tag::Int(-1));
    assert_eq!(
        parse_snbt("18_446_744_073_709_551_615ul").unwrap(),
        Tag::Long(-1)
    );

    // Hexadecimal (default unsigned; trailing `b` is a hex digit unless `sb`/`ub`!)
    assert_eq!(parse_snbt("0xFF").unwrap(), Tag::Int(255));
    assert_eq!(parse_snbt("0xFFb").unwrap(), Tag::Int(0xFFB));
    assert_eq!(parse_snbt("0xFFub").unwrap(), Tag::Byte(-1));
    assert_eq!(
        parse_snbt("0x7F_sb".replace('_', "").as_str()).unwrap(),
        Tag::Byte(127)
    );
    assert_eq!(parse_snbt("-0x80sb").unwrap(), Tag::Byte(-128));
    assert_eq!(parse_snbt("0xFFFF_FFFF").unwrap(), Tag::Int(-1));
    assert_eq!(parse_snbt("0xCAFE_BABEL").unwrap(), Tag::Long(0xCAFE_BABE));

    // Binary (default unsigned)
    assert_eq!(
        parse_snbt("0b1010_0101b").unwrap(),
        Tag::Byte(0b1010_0101u8 as i8)
    );
    assert_eq!(parse_snbt("0B1111_0000").unwrap(), Tag::Int(0xF0));
    assert_eq!(parse_snbt("-0b101si").unwrap(), Tag::Int(-5));

    // Floats & doubles (partial decimals, underscores, scientific notation)
    assert_eq!(parse_snbt(".5").unwrap(), Tag::Double(0.5));
    assert_eq!(parse_snbt("5.").unwrap(), Tag::Double(5.0));
    assert_eq!(parse_snbt("1_000.25_5f").unwrap(), Tag::Float(1000.255));
    assert_eq!(parse_snbt("1.2e3").unwrap(), Tag::Double(1200.0));
    assert_eq!(parse_snbt("-2.5E-2f").unwrap(), Tag::Float(-0.025));
    assert_eq!(parse_snbt("10f").unwrap(), Tag::Float(10.0));
    assert_eq!(parse_snbt("10d").unwrap(), Tag::Double(10.0));

    // Rejected invalid numbers
    assert!(parse_snbt("256ub").is_err());
    assert!(parse_snbt("128sb").is_err());
    assert!(parse_snbt("-1ub").is_err());
    assert!(parse_snbt("-0x1").is_err());
    assert!(parse_snbt("1_").is_err());
    assert!(parse_snbt("0x_1").is_err());
    assert!(parse_snbt("1e999d").is_err());
    assert!(parse_snbt("1e99f").is_err());
}

#[test]
fn parser_string_escapes_including_unicode_names() {
    assert_eq!(
        parse_snbt(r#""a\sb\tc\nd\re\bf\fg\\h\"i'j""#).unwrap(),
        Tag::from("a b\tc\nd\re\u{0008}f\u{000C}g\\h\"i'j")
    );
    assert_eq!(parse_snbt(r"'it\'s'").unwrap(), Tag::from("it's"));
    assert_eq!(
        parse_snbt(r#""\x41\u00e9\U0001F600""#).unwrap(),
        Tag::from("A\u{00e9}\u{1F600}")
    );
    assert_eq!(
        parse_snbt(r#""\N{Snowman}""#).unwrap(),
        Tag::from("\u{2603}")
    );
    assert_eq!(
        parse_snbt(r#""\N{LATIN SMALL LETTER A}""#).unwrap(),
        Tag::from("a")
    );

    // Invalid escapes
    assert!(parse_snbt(r#""\q""#).is_err());
    assert!(parse_snbt(r#""\uD800""#).is_err());
    assert!(parse_snbt(r#""\U00110000""#).is_err());
    assert!(parse_snbt(r#""\N{NOT A REAL UNICODE NAME}""#).is_err());
    assert!(parse_snbt(r#""\x4""#).is_err());
}

#[test]
fn parser_booleans_arrays_heterogeneous_lists_and_operations() {
    // Booleans
    assert_eq!(parse_snbt("true").unwrap(), Tag::Byte(1));
    assert_eq!(parse_snbt("false").unwrap(), Tag::Byte(0));

    // Operations: bool() and uuid()
    assert_eq!(parse_snbt("bool(true)").unwrap(), Tag::Byte(1));
    assert_eq!(parse_snbt("bool(false)").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("bool(0)").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("bool(42)").unwrap(), Tag::Byte(1));
    assert_eq!(parse_snbt("bool(0.0)").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("bool(-0.0)").unwrap(), Tag::Byte(0));
    assert_eq!(parse_snbt("bool(1.5f)").unwrap(), Tag::Byte(1));
    assert!(parse_snbt("bool(\"yes\")").is_err());
    assert!(parse_snbt("bool()").is_err());

    assert_eq!(
        parse_snbt(r#"uuid("12345678-abcd-1234-abcd-123456789abc")"#).unwrap(),
        Tag::IntArray(vec![
            0x1234_5678u32 as i32,
            0xABCD_1234u32 as i32,
            0xABCD_1234u32 as i32,
            0x5678_9ABCu32 as i32,
        ])
    );
    assert!(parse_snbt(r#"uuid("not-a-uuid")"#).is_err());
    assert!(parse_snbt("unknown_op(1)").is_err());

    // Typed arrays (including trailing comma, empty, and booleans/ops inside byte array)
    assert_eq!(parse_snbt("[B;]").unwrap(), Tag::ByteArray(vec![]));
    assert_eq!(
        parse_snbt("[B; 1b, true, false, bool(5), 255ub,]").unwrap(),
        Tag::ByteArray(vec![1, 1, 0, 1, -1])
    );
    assert_eq!(
        parse_snbt("[I; 1, 0x10, -3,]").unwrap(),
        Tag::IntArray(vec![1, 16, -3])
    );
    assert_eq!(
        parse_snbt("[L; 1L, 2l,]").unwrap(),
        Tag::LongArray(vec![1, 2])
    );
    // Type mismatch in typed array fails
    assert!(parse_snbt("[B; 1]").is_err());
    assert!(parse_snbt("[I; 1b]").is_err());
    assert!(parse_snbt("[L; 1]").is_err());

    // Heterogeneous list with trailing comma
    let list = parse_snbt(r#"[1b, "hello", {a: 2,}, [I; 3, 4],]"#).unwrap();
    let mut expected_c = Compound::new();
    expected_c.insert("a", Tag::Int(2));
    assert_eq!(
        list,
        Tag::List(List::from(vec![
            Tag::Byte(1),
            Tag::from("hello"),
            Tag::Compound(expected_c),
            Tag::IntArray(vec![3, 4]),
        ]))
    );

    // Compound with trailing comma and quoted/unquoted keys
    let c = parse_snbt_compound(r#"{foo: bar, "true": 1b, 'minecraft:id': "stone",}"#).unwrap();
    assert_eq!(c.get("foo"), Some(&Tag::from("bar")));
    assert_eq!(c.get("true"), Some(&Tag::Byte(1)));
    assert_eq!(c.get("minecraft:id"), Some(&Tag::from("stone")));

    // Unquoted boolean as key rejected
    assert!(parse_snbt("{true: 1}").is_err());
    // Trailing garbage rejected
    assert!(parse_snbt("{a: 1} extra").is_err());
}

#[test]
fn snbt_printer_parser_roundtrip() {
    let mut root = Compound::new();
    root.insert("b", Tag::Byte(-12));
    root.insert("s", Tag::Short(300));
    root.insert("i", Tag::Int(-123_456));
    root.insert("l", Tag::Long(9_876_543_210));
    root.insert("f", Tag::Float(1.5));
    root.insert("d", Tag::Double(-0.25));
    root.insert(
        "minecraft:name",
        Tag::from("line1\nline2\t\"quoted\" 'single' \\slash"),
    );
    root.insert("true", Tag::from("keyword key"));
    root.insert("ba", Tag::ByteArray(vec![-1, 0, 127]));
    root.insert("ia", Tag::IntArray(vec![1, 2, 3]));
    root.insert("la", Tag::LongArray(vec![-1, 1]));
    root.insert(
        "mixed",
        Tag::List(List::from(vec![
            Tag::Int(1),
            Tag::from("x"),
            Tag::Double(2.0),
        ])),
    );

    let tag = Tag::Compound(root);
    let printed = to_snbt(&tag).unwrap();
    let parsed = parse_snbt(&printed).unwrap();
    assert_eq!(parsed, tag);
}

/// Reader answer Q1: the text form sorts keys by UTF-16 code units, not hash-map order and not `str` order.
#[test]
fn printer_sorts_keys_by_utf16_units() {
    let mut c = Compound::new();
    for k in ["zeta", "alpha", "Mid", "_x"] {
        c.insert(k, Tag::Byte(1));
    }
    assert_eq!(
        to_snbt(&Tag::Compound(c.clone())).unwrap(),
        to_snbt(&Tag::Compound(c)).unwrap()
    );

    // U+1F600 (surrogates D83D DE00) must sort BEFORE U+E000 in Java order, although it sorts after it as a code point.
    let mut c = Compound::new();
    c.insert("\u{E000}", Tag::Byte(1));
    c.insert("\u{1F600}", Tag::Byte(2));
    let out = to_snbt(&Tag::Compound(c)).unwrap();
    let smile = out.find('\u{1F600}').expect("astral key printed");
    let private = out.find('\u{E000}').expect("private-use key printed");
    assert!(
        smile < private,
        "UTF-16 order puts the astral key first: {out}"
    );

    let mut c = Compound::new();
    for k in ["b", "a", "C", "B"] {
        c.insert(k, Tag::Byte(0));
    }
    let out = to_snbt(&Tag::Compound(c)).unwrap();
    let order: Vec<usize> = ["B", "C", "a", "b"]
        .iter()
        .map(|k| out.find(&format!("{k}:")).unwrap())
        .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "uppercase sorts before lowercase: {out}"
    );
}

/// Reader answer Q4: boolean words parse case-insensitively.
#[test]
fn parser_booleans_case_insensitive() {
    for (src, v) in [
        ("true", 1),
        ("TRUE", 1),
        ("True", 1),
        ("false", 0),
        ("FALSE", 0),
        ("fAlSe", 0),
    ] {
        assert_eq!(parse_snbt(src).unwrap(), Tag::Byte(v), "{src}");
    }
    assert_eq!(
        parse_snbt("truely").unwrap(),
        Tag::String(NbtString::from("truely"))
    );
}
