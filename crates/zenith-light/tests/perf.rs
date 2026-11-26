//! A rough timing of lighting a hilly 5x5 neighbourhood (run with `--ignored --release -- --nocapture`).

mod common;

use common::{HEIGHT, LEVEL, MIN_Y, scene};
use zenith_light::{ChunkBlocks, DenseBlocks, LightWorld, LitChunk};

#[test]
#[ignore = "timing only"]
fn light_a_hilly_neighbourhood() {
    let s = scene();
    let mut chunks = Vec::new();
    for cz in -2..=2 {
        for cx in -2..=2 {
            let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
            for z in 0..16usize {
                for x in 0..16usize {
                    let (wx, wz) = (cx * 16 + x as i32, cz * 16 + z as i32);
                    let h = 70
                        + ((wx as f64 * 0.21).sin() * 9.0 + (wz as f64 * 0.17).cos() * 7.0) as i32;
                    for y in MIN_Y..=h {
                        b.set_state(x, y, z, s.stone);
                    }
                    // caves with torches
                    if (wx + wz) % 11 == 0 {
                        for y in 20..30 {
                            b.set_state(x, y, z, s.air);
                        }
                        b.set_state(x, 25, z, s.torch);
                    }
                    if (wx * 3 + wz) % 17 == 0 {
                        b.set_state(x, h + 1, z, s.leaves);
                    }
                }
            }
            chunks.push(LitChunk::new(cx, cz, b));
        }
    }
    let mut w = LightWorld::new(s.facts.clone(), LEVEL, true);
    let t = std::time::Instant::now();
    w.light_chunks(chunks);
    eprintln!(
        "lit 25 chunks in {:?} ({} batches)",
        t.elapsed(),
        w.batches_run()
    );
}
