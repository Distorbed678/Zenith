//! Java `Double.toString` / `Float.toString` parity against fixtures printed by a real JDK
//! (`tools/javamap-fixtures/JavaNumberFixtures.java`).

use zenith_nbt::java_number::{java_double_to_string, java_float_to_string};

#[test]
fn matches_jdk_output() {
    let text = include_str!("fixtures/java_numbers.jsonl");
    let (mut checked, mut failures) = (0usize, Vec::new());
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let expected = v["s"].as_str().unwrap();
        let bits = v["bits"].as_str().unwrap();
        let got = if v["t"] == "d" {
            java_double_to_string(f64::from_bits(u64::from_str_radix(bits, 16).unwrap()))
        } else {
            java_float_to_string(f32::from_bits(u32::from_str_radix(bits, 16).unwrap()))
        };
        checked += 1;
        if got != expected && failures.len() < 20 {
            failures.push(format!("{} {bits}: expected {expected}, got {got}", v["t"]));
        }
    }
    assert!(checked > 10_000, "fixture too small");
    assert!(
        failures.is_empty(),
        "{} mismatches, e.g.:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
