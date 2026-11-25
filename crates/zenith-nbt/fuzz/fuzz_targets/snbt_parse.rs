#![no_main]

use libfuzzer_sys::fuzz_target;
use zenith_nbt::parse_snbt;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse_snbt(text);
    }
});
