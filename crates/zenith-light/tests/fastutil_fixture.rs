//! Replays the iteration order of `LongOpenHashSet(512, 0.5)` against the real fastutil jar.
//!
//! The jar is looked up in `ZENITH_FASTUTIL_JAR`, else in the Prism Launcher libraries of the current user. The test
//! compiles a throw-away Java driver into the cargo target tmp dir (nothing is committed) and **skips cleanly** when
//! the jar, `java` or `javac` is missing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use zenith_light::LongOpenHashSet;
use zenith_light::pos::block_node;

const DRIVER: &str = r#"
import it.unimi.dsi.fastutil.longs.*;
import java.io.*;
public class FastutilOrder {
    public static void main(String[] args) throws Exception {
        BufferedReader r = new BufferedReader(new FileReader(args[0]));
        PrintWriter w = new PrintWriter(new FileWriter(args[1]));
        String line;
        while ((line = r.readLine()) != null) {
            LongOpenHashSet s = new LongOpenHashSet(512, 0.5f);
            line = line.trim();
            if (!line.isEmpty()) for (String t : line.split(" ")) s.add(Long.parseLong(t));
            StringBuilder sb = new StringBuilder();
            LongIterator it = s.iterator();
            boolean first = true;
            while (it.hasNext()) { if (!first) sb.append(' '); first = false; sb.append(it.nextLong()); }
            w.println(sb);
        }
        w.close();
    }
}
"#;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn find_jar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ZENITH_FASTUTIL_JAR") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let p = home.join(
        ".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/libraries/it/unimi/dsi/fastutil/8.5.18/fastutil-8.5.18.jar",
    );
    p.is_file().then_some(p)
}

fn tool_works(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn make_sets() -> Vec<Vec<i64>> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut sets: Vec<Vec<i64>> = vec![vec![], vec![0], vec![1, 0, 2]];
    for &n in &[5usize, 30, 200, 511, 512, 513, 514, 1000, 1500, 2100, 4200] {
        // clustered packed block nodes around one chunk
        let keys = (0..n)
            .map(|_| {
                block_node(
                    (rng.below(48) as i32) - 16,
                    (rng.below(200) as i32) - 64,
                    (rng.below(48) as i32) - 16,
                )
            })
            .collect();
        sets.push(keys);
        // uniformly random 64-bit keys, with a zero in the middle and duplicates
        let mut keys: Vec<i64> = (0..n).map(|_| rng.next() as i64).collect();
        keys.insert(n / 2, 0);
        keys.push(keys[0]);
        sets.push(keys);
    }
    // a vertical stack of blocks (consecutive y)
    sets.push((0..300).map(|y| block_node(7, y - 64, 9)).collect());
    sets
}

#[test]
fn iteration_order_matches_fastutil_8_5_18() {
    let Some(jar) = find_jar() else {
        eprintln!("skipped: no fastutil jar (set ZENITH_FASTUTIL_JAR)");
        return;
    };
    if !tool_works("java") || !tool_works("javac") {
        eprintln!("skipped: java / javac not available");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fastutil-fixture");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("FastutilOrder.java");
    fs::write(&src, DRIVER).unwrap();
    let st = Command::new("javac")
        .args(["-cp"])
        .arg(&jar)
        .arg("-d")
        .arg(&dir)
        .arg(&src)
        .status()
        .unwrap();
    assert!(st.success(), "javac failed");

    let sets = make_sets();
    let input: String = sets
        .iter()
        .map(|s| s.iter().map(i64::to_string).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let (inp, out) = (dir.join("in.txt"), dir.join("out.txt"));
    fs::write(&inp, input).unwrap();
    let cp = format!("{}:{}", jar.display(), dir.display());
    let st = Command::new("java")
        .args(["-cp", &cp, "FastutilOrder"])
        .arg(&inp)
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success(), "java failed");
    let got = fs::read_to_string(&out).unwrap();
    let lines: Vec<&str> = got.lines().collect();
    assert_eq!(lines.len(), sets.len());
    for (i, (keys, line)) in sets.iter().zip(lines).enumerate() {
        let mut set = LongOpenHashSet::for_checks();
        for &k in keys {
            set.insert(k);
        }
        let want: Vec<i64> = line
            .split_whitespace()
            .map(|t| t.parse().unwrap())
            .collect();
        assert_eq!(
            set.to_vec(),
            want,
            "set #{i} ({} keys) order differs",
            keys.len()
        );
    }
}
