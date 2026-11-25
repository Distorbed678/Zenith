//! Binary I/O, modified UTF-8, list wrapper, accounting, and float bit-pattern tests.

use zenith_nbt::{
    Accounter, AccountingError, Compound, DEFAULT_MAX_DEPTH, DEFAULT_QUOTA, Error, FormatError,
    List, MAX_ENCODED_LEN, NbtString, Tag, TagType, UNCOMPRESSED_QUOTA, from_bytes_named,
    from_bytes_network, mutf8, read_gzip_named, read_named, read_network, read_zlib_named,
    to_bytes_named, to_bytes_network, write_gzip_named, write_named, write_zlib_named,
};

#[test]
fn roundtrip_all_tag_types_named_and_network() {
    let mut root = Compound::new();
    root.insert("byte", Tag::Byte(-42));
    root.insert("short", Tag::Short(-12_345));
    root.insert("int", Tag::Int(0x1234_5678));
    root.insert("long", Tag::Long(-9_000_000_000_000_000_000));
    root.insert("float", Tag::Float(3.25));
    root.insert("double", Tag::Double(-2.5e100));
    root.insert("bytes", Tag::ByteArray(vec![-128, -1, 0, 1, 127]));
    root.insert("string", Tag::from("hello, Zenith!"));
    root.insert(
        "int_list",
        Tag::List(List::from(vec![Tag::Int(1), Tag::Int(2), Tag::Int(3)])),
    );
    let mut nested = Compound::new();
    nested.insert("flag", Tag::Byte(1));
    root.insert("nested", Tag::Compound(nested));
    root.insert("ints", Tag::IntArray(vec![i32::MIN, 0, i32::MAX]));
    root.insert("longs", Tag::LongArray(vec![i64::MIN, 0, i64::MAX]));

    let tag = Tag::Compound(root);

    // Named root round-trip
    let bytes = to_bytes_named("root_name", &tag).unwrap();
    let mut acc = Accounter::default_quota();
    let (name, decoded) = from_bytes_named(&bytes, &mut acc).unwrap();
    assert_eq!(name, "root_name");
    assert_eq!(decoded, tag);
    // Byte-identical re-serialization
    let bytes2 = to_bytes_named(&name, &decoded).unwrap();
    assert_eq!(bytes, bytes2);

    // Unnamed (network) root round-trip
    let net_bytes = to_bytes_network(Some(&tag)).unwrap();
    let mut net_acc = Accounter::default_quota();
    let net_decoded = from_bytes_network(&net_bytes, &mut net_acc).unwrap();
    assert_eq!(net_decoded, Some(tag.clone()));
    assert_eq!(to_bytes_network(net_decoded.as_ref()).unwrap(), net_bytes);

    // Gzip round-trip
    let mut gz_buf = Vec::new();
    write_gzip_named(&mut gz_buf, "", &tag).unwrap();
    let (gz_name, gz_decoded) =
        read_gzip_named(gz_buf.as_slice(), &mut Accounter::default_quota()).unwrap();
    assert_eq!(gz_name, "");
    assert_eq!(gz_decoded, tag);

    // Zlib round-trip
    let mut zl_buf = Vec::new();
    write_zlib_named(&mut zl_buf, "chunk", &tag).unwrap();
    let (zl_name, zl_decoded) =
        read_zlib_named(zl_buf.as_slice(), &mut Accounter::default_quota()).unwrap();
    assert_eq!(zl_name, "chunk");
    assert_eq!(zl_decoded, tag);
}

#[test]
fn network_end_means_none_and_charges_8() {
    let bytes = to_bytes_network(None).unwrap();
    assert_eq!(bytes, vec![0]);

    let mut acc = Accounter::new(8, DEFAULT_MAX_DEPTH);
    let val = from_bytes_network(&bytes, &mut acc).unwrap();
    assert_eq!(val, None);
    assert_eq!(acc.used(), 8);

    // Quota 7 fails on End (needs 8)
    let mut tight = Accounter::new(7, DEFAULT_MAX_DEPTH);
    let err = from_bytes_network(&bytes, &mut tight).unwrap_err();
    assert!(matches!(
        err,
        Error::Accounting(AccountingError::QuotaExceeded { .. })
    ));

    // Named End also writes 1 byte and charges 8
    let named_end = to_bytes_named("ignored", &Tag::End).unwrap();
    assert_eq!(named_end, vec![0]);
    let mut named_acc = Accounter::new(8, DEFAULT_MAX_DEPTH);
    let (name, tag) = from_bytes_named(&named_end, &mut named_acc).unwrap();
    assert_eq!(name, "");
    assert_eq!(tag, Tag::End);
    assert_eq!(named_acc.used(), 8);
}

#[test]
fn modified_utf8_vectors_nul_surrogates_astral_and_limit() {
    // U+0000 must encode as 0xC0 0x80 (never 0x00)
    let nul_enc = mutf8::encode_mutf8("\0").unwrap();
    assert_eq!(nul_enc, vec![0xC0, 0x80]);
    assert_eq!(mutf8::decode_mutf8(&nul_enc).unwrap(), "\0");

    // Raw 0x00 byte is still accepted on decode per Java DataInput.readUTF spec
    assert_eq!(mutf8::decode_mutf8(&[0x00]).unwrap(), "\0");

    // Astral character U+1F600 (UTF-16: 0xD83D 0xDE00) -> two 3-byte surrogate encodings
    let emoji = "\u{1F600}";
    let emoji_enc = mutf8::encode_mutf8(emoji).unwrap();
    assert_eq!(emoji_enc, vec![0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
    assert_eq!(mutf8::decode_mutf8(&emoji_enc).unwrap(), emoji);

    // Standard 4-byte UTF-8 (0xF0 0x9F 0x98 0x80) must be REJECTED by modified UTF-8 decoder
    let err = mutf8::decode_mutf8(emoji.as_bytes()).unwrap_err();
    assert!(matches!(
        err,
        Error::Format(FormatError::InvalidModifiedUtf8(_))
    ));

    // Lone surrogates survive binary NBT round-trip losslessly (both as value and as key)
    let lone_high = NbtString::from_utf16(vec![0x0041, 0xD800, 0x0042]);
    let lone_low = NbtString::from_utf16(vec![0xDC00]);
    let reversed_pair = NbtString::from_utf16(vec![0xDC00, 0xD800]);
    assert_eq!(lone_high.as_str(), None);
    assert_eq!(lone_high.utf16_len(), 3);

    let mut c = Compound::new();
    c.insert(lone_high.clone(), Tag::String(lone_low.clone()));
    c.insert("rev", Tag::String(reversed_pair.clone()));
    let tag = Tag::Compound(c);
    let wire = to_bytes_named(&lone_high, &tag).unwrap();
    let (decoded_name, decoded_tag) =
        from_bytes_named(&wire, &mut Accounter::default_quota()).unwrap();
    assert_eq!(decoded_name, lone_high);
    assert_eq!(decoded_tag, tag);

    // Exact 65,535-byte boundary succeeds; 65,536 bytes fails with StringTooLong (never truncates)
    let exact_65535 = "a".repeat(MAX_ENCODED_LEN);
    let enc_ok = mutf8::encode_mutf8(&exact_65535).unwrap();
    assert_eq!(enc_ok.len(), 65_535);
    let wire_ok = to_bytes_named("", &Tag::from(exact_65535.clone())).unwrap();
    let (_, decoded_ok) = from_bytes_named(&wire_ok, &mut Accounter::default_quota()).unwrap();
    assert_eq!(decoded_ok, Tag::from(exact_65535));

    let too_long_ascii = "a".repeat(MAX_ENCODED_LEN + 1);
    let err = mutf8::encode_mutf8(&too_long_ascii).unwrap_err();
    assert!(matches!(
        err,
        Error::StringTooLong {
            encoded_len: 65_536
        }
    ));

    // Multi-byte limit test: 32,767 NUL chars = 65,534 bytes (ok) + 'a' = 65,535 (ok),
    // whereas 32,768 NUL chars = 65,536 bytes (fails!)
    let mut boundary_nul = "\0".repeat(32_767);
    boundary_nul.push('a');
    assert_eq!(mutf8::encode_mutf8(&boundary_nul).unwrap().len(), 65_535);
    let over_nul = "\0".repeat(32_768);
    assert!(matches!(
        to_bytes_named("", &Tag::from(over_nul)),
        Err(Error::StringTooLong {
            encoded_len: 65_536
        })
    ));
}

#[test]
fn list_wrapper_rules_mixed_empty_and_wrapper_shaped_compounds() {
    // 1. Empty list writes element type End (0) and count 0
    let empty_list = Tag::List(List::new());
    let empty_wire = to_bytes_network(Some(&empty_list)).unwrap();
    assert_eq!(
        empty_wire,
        vec![TagType::List.id(), TagType::End.id(), 0, 0, 0, 0]
    );
    let decoded_empty = from_bytes_network(&empty_wire, &mut Accounter::default_quota())
        .unwrap()
        .unwrap();
    assert_eq!(decoded_empty, empty_list);

    // 2. Mixed-type list wraps non-compounds and preserves non-wrapper compounds
    let mut normal_compound = Compound::new();
    normal_compound.insert("x", Tag::Int(10));

    let mut wrapper_shaped = Compound::new();
    wrapper_shaped.insert("", Tag::Int(99));

    let mut double_wrapper = Compound::new();
    double_wrapper.insert("", Tag::Compound(wrapper_shaped.clone()));

    let mixed = Tag::List(List::from(vec![
        Tag::Int(1),
        Tag::from("two"),
        Tag::Compound(normal_compound.clone()),
        Tag::Compound(wrapper_shaped.clone()),
        Tag::Compound(double_wrapper.clone()),
        Tag::List(List::from(vec![Tag::Byte(7)])),
    ]));

    let wire = to_bytes_network(Some(&mixed)).unwrap();
    // Wire element type byte (index 1) must be Compound (10)
    assert_eq!(wire[0], TagType::List.id());
    assert_eq!(wire[1], TagType::Compound.id());

    let decoded = from_bytes_network(&wire, &mut Accounter::default_quota())
        .unwrap()
        .unwrap();
    assert_eq!(decoded, mixed);
    // Re-serialization must be byte-identical
    assert_eq!(to_bytes_network(Some(&decoded)).unwrap(), wire);

    // 3. All-compound list containing a wrapper-shaped compound must ALSO survive round-trip
    let all_compounds = Tag::List(List::from(vec![
        Tag::Compound(wrapper_shaped),
        Tag::Compound(normal_compound),
    ]));
    let wire_all = to_bytes_network(Some(&all_compounds)).unwrap();
    let decoded_all = from_bytes_network(&wire_all, &mut Accounter::default_quota())
        .unwrap()
        .unwrap();
    assert_eq!(decoded_all, all_compounds);
    assert_eq!(to_bytes_network(Some(&decoded_all)).unwrap(), wire_all);
}

#[test]
fn float_and_double_bit_patterns_nan_and_neg_zero() {
    // Verified against vanilla with the oracle (2026-11-25): reading turns -0.0 into +0.0 (the game's tag factory
    // shares one zero instance); every NaN is written canonically.
    let mut c = Compound::new();
    c.insert("fnz", Tag::Float(-0.0));
    c.insert("dnz", Tag::Double(-0.0));
    c.insert("fnan", Tag::Float(f32::from_bits(0xFFC1_ABCD)));
    c.insert("fsnan", Tag::Float(f32::from_bits(0x7F80_0001)));
    c.insert("dnan", Tag::Double(f64::from_bits(0xFFF8_DEAD_BEEF_CAFE)));
    c.insert("dsnan", Tag::Double(f64::from_bits(0x7FF0_0000_0000_0001)));
    let wire = to_bytes_named("", &Tag::Compound(c)).unwrap();
    let (_, decoded) = from_bytes_named(&wire, &mut Accounter::default_quota()).unwrap();
    let dc = decoded.as_compound().unwrap();
    let f = |k: &str| dc.get(k).unwrap().as_float().unwrap().to_bits();
    let d = |k: &str| dc.get(k).unwrap().as_double().unwrap().to_bits();
    assert_eq!(f("fnz"), 0.0f32.to_bits());
    assert_eq!(d("dnz"), 0.0f64.to_bits());
    assert_eq!(f("fnan"), 0x7FC0_0000);
    assert_eq!(f("fsnan"), 0x7FC0_0000);
    assert_eq!(d("dnan"), 0x7FF8_0000_0000_0000);
    assert_eq!(d("dsnan"), 0x7FF8_0000_0000_0000);

    // Reading keeps the exact bits in memory (only writing canonicalizes).
    let mut raw = vec![0x0A, 0, 0, 0x05, 0, 1, b'f'];
    raw.extend_from_slice(&0xFFC1_ABCDu32.to_be_bytes());
    raw.push(0);
    let (_, t) = from_bytes_named(&raw, &mut Accounter::default_quota()).unwrap();
    let got = t
        .as_compound()
        .unwrap()
        .get("f")
        .unwrap()
        .as_float()
        .unwrap()
        .to_bits();
    assert_eq!(got, 0xFFC1_ABCD);
}

#[test]
fn depth_limit_512_succeeds_and_513_fails() {
    fn build_nested_compounds(depth: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(depth * 5);
        // Unnamed network root compound
        buf.push(TagType::Compound.id());
        for _ in 1..depth {
            // Entry: type Compound, key "a" (len 1), ...
            buf.push(TagType::Compound.id());
            buf.extend_from_slice(&1u16.to_be_bytes());
            buf.push(b'a');
        }
        // Close all `depth` compounds
        buf.resize(buf.len() + depth, TagType::End.id());
        buf
    }

    fn build_nested_lists(depth: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(TagType::List.id());
        for _ in 1..depth {
            buf.push(TagType::List.id());
            buf.extend_from_slice(&1i32.to_be_bytes());
        }
        // Innermost empty list
        buf.push(TagType::End.id());
        buf.extend_from_slice(&0i32.to_be_bytes());
        buf
    }

    // 512 nested compounds succeeds
    let wire_512 = build_nested_compounds(512);
    let mut acc = Accounter::unlimited();
    assert!(from_bytes_network(&wire_512, &mut acc).is_ok());
    assert_eq!(acc.depth(), 0);

    // 513 nested compounds fails with MaxDepthExceeded(512)
    let wire_513 = build_nested_compounds(513);
    let err = from_bytes_network(&wire_513, &mut Accounter::unlimited()).unwrap_err();
    assert!(matches!(
        err,
        Error::Accounting(AccountingError::MaxDepthExceeded { max_depth: 512 })
    ));

    // 512 nested lists succeeds; 513 fails
    let list_512 = build_nested_lists(512);
    assert!(from_bytes_network(&list_512, &mut Accounter::unlimited()).is_ok());

    let list_513 = build_nested_lists(513);
    let err = from_bytes_network(&list_513, &mut Accounter::unlimited()).unwrap_err();
    assert!(matches!(
        err,
        Error::Accounting(AccountingError::MaxDepthExceeded { max_depth: 512 })
    ));
}

#[test]
fn quota_constants_and_exact_charge_boundaries() {
    assert_eq!(DEFAULT_QUOTA, 2_097_152);
    assert_eq!(UNCOMPRESSED_QUOTA, 104_857_600);
    assert_eq!(DEFAULT_MAX_DEPTH, 512);

    // Verify exact charges for every primitive & array type:
    // Byte=9, Short=10, Int=12, Float=12, Long=16, Double=16
    let cases: Vec<(Tag, u64)> = vec![
        (Tag::Byte(1), 9),
        (Tag::Short(1), 10),
        (Tag::Int(1), 12),
        (Tag::Float(1.0), 12),
        (Tag::Long(1), 16),
        (Tag::Double(1.0), 16),
        // String "ab\u{1F600}": UTF-16 len = 1 + 1 + 2 = 4 -> 36 + 2*4 = 44
        (Tag::from("ab\u{1F600}"), 36 + 2 * 4),
        // ByteArray len 5 -> 24 + 5 = 29
        (Tag::ByteArray(vec![0; 5]), 24 + 5),
        // IntArray len 3 -> 24 + 4*3 = 36
        (Tag::IntArray(vec![0; 3]), 24 + 12),
        // LongArray len 2 -> 24 + 8*2 = 40
        (Tag::LongArray(vec![0; 2]), 24 + 16),
        // List of 3 Ints -> 36 + 4*3 + 3*12 = 84
        (
            Tag::List(List::from(vec![Tag::Int(1), Tag::Int(2), Tag::Int(3)])),
            36 + 12 + 36,
        ),
    ];

    for (tag, expected_charge) in cases {
        let wire = to_bytes_network(Some(&tag)).unwrap();

        // Exactly at quota succeeds
        let mut exact = Accounter::new(expected_charge, DEFAULT_MAX_DEPTH);
        let decoded = from_bytes_network(&wire, &mut exact).unwrap();
        assert_eq!(decoded, Some(tag.clone()));
        assert_eq!(exact.used(), expected_charge);

        // One under quota fails
        let mut under = Accounter::new(expected_charge - 1, DEFAULT_MAX_DEPTH);
        let err = from_bytes_network(&wire, &mut under).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Accounting(AccountingError::QuotaExceeded { .. })
            ),
            "expected QuotaExceeded for {tag:?}, got {err:?}"
        );
    }
}

#[test]
fn duplicate_compound_key_does_not_charge_new_entry_36() {
    // Hand-craft a network compound with duplicate key "k" (UTF-16 len 1):
    // Compound base: 48
    // First entry "k" -> Byte(1): key (28 + 2*1 = 30) + value (9) + new-key (36) = 75
    // Second entry "k" -> Byte(2): key (28 + 2*1 = 30) + value (9) + NO new-key (0) = 39
    // Total expected charge: 48 + 75 + 39 = 162
    let mut wire = Vec::new();
    wire.push(TagType::Compound.id());
    // Entry 1: Byte "k" = 1
    wire.push(TagType::Byte.id());
    wire.extend_from_slice(&1u16.to_be_bytes());
    wire.push(b'k');
    wire.push(1);
    // Entry 2: Byte "k" = 2 (duplicate key)
    wire.push(TagType::Byte.id());
    wire.extend_from_slice(&1u16.to_be_bytes());
    wire.push(b'k');
    wire.push(2);
    // End
    wire.push(TagType::End.id());

    let expected_charge = 48 + (28 + 2 + 9 + 36) + (28 + 2 + 9);
    assert_eq!(expected_charge, 162);

    let mut exact = Accounter::new(expected_charge, DEFAULT_MAX_DEPTH);
    let tag = from_bytes_network(&wire, &mut exact).unwrap().unwrap();
    assert_eq!(exact.used(), expected_charge);
    assert_eq!(tag.as_compound().unwrap().get("k"), Some(&Tag::Byte(2)));

    // One byte under must fail
    let mut under = Accounter::new(expected_charge - 1, DEFAULT_MAX_DEPTH);
    assert!(matches!(
        from_bytes_network(&wire, &mut under),
        Err(Error::Accounting(AccountingError::QuotaExceeded { .. }))
    ));
}

#[test]
fn quota_hit_by_single_large_array_and_many_small_tags() {
    // Single large IntArray claiming 1,000,000 ints -> 24 + 4_000_000 > DEFAULT_QUOTA (2 MiB)
    // Fails immediately without needing 4 MB of input bytes!
    let mut header_only = vec![TagType::IntArray.id()];
    header_only.extend_from_slice(&1_000_000i32.to_be_bytes());
    let err = from_bytes_network(&header_only, &mut Accounter::default_quota()).unwrap_err();
    assert!(matches!(
        err,
        Error::Accounting(AccountingError::QuotaExceeded { .. })
    ));

    // Many small tags hitting a small quota (300 bytes)
    let mut list = List::new();
    for i in 0..30 {
        list.push(Tag::Int(i));
    }
    let wire = to_bytes_network(Some(&Tag::List(list))).unwrap();
    // Total needed: 36 + 30*4 + 30*12 = 516
    assert!(matches!(
        from_bytes_network(&wire, &mut Accounter::new(300, DEFAULT_MAX_DEPTH)),
        Err(Error::Accounting(AccountingError::QuotaExceeded { .. }))
    ));
    assert!(from_bytes_network(&wire, &mut Accounter::new(516, DEFAULT_MAX_DEPTH)).is_ok());
}

#[test]
fn invalid_format_inputs_rejected() {
    // Unknown tag type ID 99
    let mut slice: &[u8] = &[99];
    assert!(matches!(
        read_network(&mut slice, &mut Accounter::default_quota()),
        Err(Error::Format(FormatError::UnknownTagType(99)))
    ));

    // Negative array length
    let mut neg_arr = vec![TagType::ByteArray.id()];
    neg_arr.extend_from_slice(&(-5i32).to_be_bytes());
    assert!(matches!(
        from_bytes_network(&neg_arr, &mut Accounter::default_quota()),
        Err(Error::Format(FormatError::NegativeLength(-5)))
    ));

    // Negative list length
    let mut neg_list = vec![TagType::List.id(), TagType::Int.id()];
    neg_list.extend_from_slice(&(-1i32).to_be_bytes());
    assert!(matches!(
        from_bytes_network(&neg_list, &mut Accounter::default_quota()),
        Err(Error::Format(FormatError::NegativeLength(-1)))
    ));

    // Non-empty list of End tags
    let mut end_list = vec![TagType::List.id(), TagType::End.id()];
    end_list.extend_from_slice(&3i32.to_be_bytes());
    assert!(matches!(
        from_bytes_network(&end_list, &mut Accounter::default_quota()),
        Err(Error::Format(FormatError::NonEmptyEndList(3)))
    ));

    // Attempting to write Tag::End inside a compound or list fails
    let mut bad_c = Compound::new();
    bad_c.insert("end", Tag::End);
    let mut out = Vec::new();
    assert!(matches!(
        write_named(&mut out, "", &Tag::Compound(bad_c)),
        Err(Error::Format(FormatError::InvalidTagPlacement(
            TagType::End
        )))
    ));

    let bad_l = Tag::List(List::from(vec![Tag::End]));
    assert!(matches!(
        to_bytes_network(Some(&bad_l)),
        Err(Error::Format(FormatError::InvalidTagPlacement(
            TagType::End
        )))
    ));

    // Truncated stream returns Io error
    let mut truncated: &[u8] = &[TagType::Int.id(), 0, 0];
    assert!(matches!(
        read_named(&mut truncated, &mut Accounter::default_quota()),
        Err(Error::Io(_))
    ));
}

#[test]
fn treeified_bin_is_detected_and_reported() {
    // Generate 10 distinct strings with identical Java hashCode (using "Aa"/"BB" pairs:
    // "Aa" and "BB" both have hashCode 2112, so 4-pair combinations give 2^4 = 16 strings
    // with the exact same Java hashCode!).
    let mut colliding = Vec::new();
    for mask in 0u8..16 {
        let mut s = String::new();
        for bit in 0..4 {
            if (mask & (1 << bit)) == 0 {
                s.push_str("Aa");
            } else {
                s.push_str("BB");
            }
        }
        colliding.push(s);
    }

    // Verify all 16 have the exact same Java hashCode
    let h0 = mutf8::java_hash_str(&colliding[0]);
    assert!(colliding.iter().all(|s| mutf8::java_hash_str(s) == h0));

    // Start at capacity 64 so the 9th colliding entry triggers treeification detection
    let mut c = Compound::with_capacity(64);
    for (i, key) in colliding.iter().take(8).enumerate() {
        c.insert(key.as_str(), Tag::Int(i as i32));
        assert!(!c.has_treeified_bin());
    }
    assert!(c.try_iter().is_ok());

    // 9th colliding entry at capacity >= 64 triggers treeification detection!
    c.insert(colliding[8].as_str(), Tag::Int(8));
    assert!(c.has_treeified_bin());
    assert!(matches!(
        c.try_iter(),
        Err(Error::UnsupportedTreeifiedBin {
            bucket_len: 9,
            capacity: 64
        })
    ));
    assert!(matches!(
        to_bytes_network(Some(&Tag::Compound(c))),
        Err(Error::UnsupportedTreeifiedBin { .. })
    ));
}
