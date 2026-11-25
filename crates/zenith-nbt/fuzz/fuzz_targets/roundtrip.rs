#![no_main]

use libfuzzer_sys::fuzz_target;
use zenith_nbt::{
    Accounter, Error, from_bytes_network, parse_snbt, to_bytes_network, to_snbt,
};

fuzz_target!(|data: &[u8]| {
    // Binary round-trip invariant: any network NBT value that decodes cleanly under the default
    // quota must re-encode identically (unless it hit an unsupported treeified bin) and decode
    // back to an equal Tag tree.
    let mut acc = Accounter::default_quota();
    if let Ok(Some(tag)) = from_bytes_network(data, &mut acc) {
        match to_bytes_network(Some(&tag)) {
            Ok(encoded1) => {
                let mut acc2 = Accounter::unlimited();
                let decoded2 = from_bytes_network(&encoded1, &mut acc2)
                    .expect("re-encoded NBT must decode")
                    .expect("non-End tag must decode as Some");
                assert_eq!(decoded2, tag, "binary round-trip changed Tag value");

                let encoded2 = to_bytes_network(Some(&decoded2))
                    .expect("second encode must succeed");
                assert_eq!(encoded2, encoded1, "binary re-encoding must be idempotent");
            }
            Err(Error::UnsupportedTreeifiedBin { .. }) => {}
            Err(e) => panic!("unexpected encode error on decoded tag: {e}"),
        }
    }

    // SNBT round-trip invariant: any valid SNBT input must print and re-parse to an equal Tag,
    // and survive a binary network round-trip.
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(tag) = parse_snbt(text)
    {
        if let Ok(printed) = to_snbt(&tag) {
            let reparsed = parse_snbt(&printed).expect("printed SNBT must re-parse");
            assert_eq!(reparsed, tag, "SNBT round-trip changed Tag value");
        }
        if let Ok(wire) = to_bytes_network(Some(&tag)) {
            let mut acc2 = Accounter::unlimited();
            let decoded = from_bytes_network(&wire, &mut acc2)
                .expect("binary wire from SNBT must decode")
                .expect("SNBT tag is never End");
            assert_eq!(decoded, tag, "SNBT -> binary -> Tag round-trip mismatch");
        }
    }
});
