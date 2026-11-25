#![no_main]

use libfuzzer_sys::fuzz_target;
use zenith_nbt::{Accounter, from_bytes_network};

fuzz_target!(|data: &[u8]| {
    let mut acc = Accounter::default_quota();
    let _ = from_bytes_network(data, &mut acc);
});
