// Zenith fixture generator — original source, MIT OR Apache-2.0. JDK only (no game code).
//
// Produces java.util.HashSet iteration-order fixtures for a 3-int key whose hashCode is the well-known
// "Vec3i" shape ((y + z * 31) * 31) + x. Every sequence is a list of operations executed on a real
// java.util.HashSet; the JDK's resulting order is recorded after every pop/remove (as a 64-bit digest of the
// whole iteration order) and in full at the end of the sequence.
//
// Usage: java tools/javamap-fixtures/HashSetOrder.java <seed> <count> [outfile]
//        (no outfile: the fixtures go to stdout)
//
// Format (all integers big-endian):
//   header    : i32 magic 0x48534F31 ("HSO1"), i64 seed, i32 count
//   sequence  : i8 kind, i32 opCount, opCount * op, i32 finalSize, finalSize * (i32 x, i32 y, i32 z)
//   op        : i8 type, then
//     type 0 insert (set.add)        : i32 x, i32 y, i32 z
//     type 1 pop-first               : i32 x, i32 y, i32 z (the element the JDK returned), i64 digest, i32 size
//                                      (it.next(); it.remove() on a fresh iterator; digest/size are taken after it)
//     type 2 remove (set.remove)     : i32 x, i32 y, i32 z, i8 removed, i64 digest, i32 size (after the call)
//   digest    : d = 0xcbf29ce484222325; for every element in iteration order, for v in (x, y, z):
//               d = (d ^ (v & 0xffffffff)) * 0x100000001b3 (wrapping 64-bit)
//   kind      : 0 patch (grid with random subset and y jitter), 1 dense low-bit collision clusters (force tree bins),
//               2 growth (random boxes and lines, past 64/128/256/... buckets), 3 leaf-distance BFS (insert + pop-first),
//               4 random interleaved insert / pop-first / remove, with box or strided positions.
import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.SplittableRandom;

public final class HashSetOrder {
    record Pos(int x, int y, int z) {
        @Override
        public int hashCode() {
            return ((y + z * 31) * 31) + x;
        }
    }

    static long mix(long d, int v) {
        d ^= (v & 0xffffffffL);
        return d * 0x100000001b3L;
    }

    /** One sequence: runs every operation on a real HashSet and records the results. */
    static final class Run {
        final HashSet<Pos> set = new HashSet<>();
        final ByteArrayOutputStream buf = new ByteArrayOutputStream();
        final DataOutputStream out = new DataOutputStream(buf);
        int ops = 0;

        long digest() {
            long d = 0xcbf29ce484222325L;
            for (Pos p : set) {
                d = mix(d, p.x());
                d = mix(d, p.y());
                d = mix(d, p.z());
            }
            return d;
        }

        void insert(int x, int y, int z) throws IOException {
            set.add(new Pos(x, y, z));
            out.writeByte(0);
            out.writeInt(x);
            out.writeInt(y);
            out.writeInt(z);
            ops++;
        }

        Pos pop() throws IOException {
            Iterator<Pos> it = set.iterator();
            Pos p = it.next();
            it.remove();
            out.writeByte(1);
            out.writeInt(p.x());
            out.writeInt(p.y());
            out.writeInt(p.z());
            out.writeLong(digest());
            out.writeInt(set.size());
            ops++;
            return p;
        }

        void remove(Pos p) throws IOException {
            boolean removed = set.remove(p);
            out.writeByte(2);
            out.writeInt(p.x());
            out.writeInt(p.y());
            out.writeInt(p.z());
            out.writeByte(removed ? 1 : 0);
            out.writeLong(digest());
            out.writeInt(set.size());
            ops++;
        }

        void write(DataOutputStream to, int kind) throws IOException {
            to.writeByte(kind);
            to.writeInt(ops);
            buf.writeTo(to);
            to.writeInt(set.size());
            for (Pos p : set) {
                to.writeInt(p.x());
                to.writeInt(p.y());
                to.writeInt(p.z());
            }
        }
    }

    static int baseCoord(SplittableRandom r) {
        return switch (r.nextInt(4)) {
            case 0 -> r.nextInt(-64, 64);
            case 1 -> r.nextInt(-2000, 2000);
            case 2 -> r.nextInt(-30_000_000, 30_000_000);
            default -> r.nextInt(-20000, 20000);
        };
    }

    static void shuffle(List<int[]> l, SplittableRandom r) {
        for (int i = l.size() - 1; i > 0; i--) {
            int j = r.nextInt(i + 1);
            int[] t = l.get(i);
            l.set(i, l.get(j));
            l.set(j, t);
        }
    }

    // ── kind 0: a patch like a vegetation/feature footprint ──────────────────────────────────────────────────
    static void patch(SplittableRandom r, Run run) throws IOException {
        int radius = r.nextInt(2, 21);
        int shape = r.nextInt(3);
        double keep = r.nextInt(3) == 0 ? 1.0 : 0.2 + 0.75 * r.nextDouble();
        int x0 = baseCoord(r), z0 = baseCoord(r), y0 = r.nextInt(-64, 320);
        int jitter = r.nextInt(3) == 0 ? 0 : r.nextInt(1, 5);
        int layers = radius > 12 ? r.nextInt(1, 3) : r.nextInt(1, 4);
        List<int[]> cells = new ArrayList<>();
        for (int dx = -radius; dx <= radius; dx++) {
            for (int dz = -radius; dz <= radius; dz++) {
                boolean in = switch (shape) {
                    case 0 -> true;
                    case 1 -> dx * dx + dz * dz <= radius * radius;
                    default -> dx * dx * 4 + dz * dz <= radius * radius * 2;
                };
                if (!in) continue;
                for (int l = 0; l < layers; l++) {
                    if (r.nextDouble() > keep) continue;
                    int y = y0 + l + (jitter == 0 ? 0 : r.nextInt(jitter));
                    cells.add(new int[] {x0 + dx, y, z0 + dz});
                }
            }
        }
        if (r.nextInt(3) != 0) shuffle(cells, r);
        for (int[] c : cells) {
            run.insert(c[0], c[1], c[2]);
            if (r.nextInt(20) == 0) run.insert(c[0], c[1], c[2]);
        }
    }

    // ── kind 1: dense low-bit collision clusters (tree bins) ─────────────────────────────────────────────────
    static void dense(SplittableRandom r, Run run) throws IOException {
        int m = 64 << r.nextInt(5);
        int x0 = r.nextInt(3) == 0 ? baseCoord(r) : r.nextInt(-200, 200);
        int y = r.nextInt(-64, 320), z = r.nextInt(3) == 0 ? baseCoord(r) : r.nextInt(-50, 50);
        int residues = r.nextInt(1, 5);
        List<int[]> cells = new ArrayList<>();
        for (int res = 0; res < residues; res++) {
            int k = r.nextInt(9, 70);
            int axis = r.nextInt(4);
            for (int i = 0; i < k; i++) {
                int x = x0 + res + m * i, yy = y, zz = z;
                if (axis == 1) { // stride along z instead (hash steps by 961 per z)
                    x = x0 + res;
                    zz = z + m * i;
                } else if (axis == 2) { // stride along y (hash steps by 31 per y)
                    x = x0 + res;
                    yy = y + m * i; // 31 is odd, so a stride of m in y keeps the low bits of the hash apart by 31*m
                } else if (axis == 3) { // x and y both move so the raw hash keeps the same low bits
                    x = x0 + res + m * i;
                    yy = y + r.nextInt(2);
                }
                cells.add(new int[] {x, yy, zz});
            }
        }
        // Raw-hash collisions (equal spread hash, different position) appear in a few sequences only.
        if (r.nextInt(20) == 0) {
            int n = cells.size();
            for (int i = 0; i < n && i < 4; i++) {
                int[] c = cells.get(r.nextInt(n));
                cells.add(new int[] {c[0] + 31, c[1] - 1, c[2]});
                if (r.nextBoolean()) cells.add(new int[] {c[0] + 961, c[1], c[2] - 1});
            }
        }
        int fillers = r.nextInt(3) == 0 ? 0 : r.nextInt(0, 140);
        for (int i = 0; i < fillers; i++) {
            cells.add(new int[] {x0 + r.nextInt(-12, 12), y + r.nextInt(-3, 3), z + r.nextInt(-12, 12)});
        }
        if (r.nextInt(4) != 0) shuffle(cells, r);
        for (int[] c : cells) run.insert(c[0], c[1], c[2]);
    }

    // ── kind 2: growth past 64/128/256/... buckets ───────────────────────────────────────────────────────────
    static void growth(SplittableRandom r, Run run) throws IOException {
        int n = (int) Math.exp(r.nextDouble() * Math.log(3500)) + 5;
        int x0 = baseCoord(r), y0 = r.nextInt(-64, 320), z0 = baseCoord(r);
        int shape = r.nextInt(4);
        int wx = r.nextInt(2, 64), wy = r.nextInt(1, 40), wz = r.nextInt(2, 64);
        for (int i = 0; i < n; i++) {
            int x, y, z;
            switch (shape) {
                case 0 -> { // box
                    x = x0 + r.nextInt(wx);
                    y = y0 + r.nextInt(wy);
                    z = z0 + r.nextInt(wz);
                }
                case 1 -> { // line along x with noise
                    x = x0 + i + r.nextInt(3);
                    y = y0 + r.nextInt(2);
                    z = z0 + r.nextInt(2);
                }
                case 2 -> { // line along z
                    x = x0 + r.nextInt(2);
                    y = y0 + r.nextInt(2);
                    z = z0 + i;
                }
                default -> { // wide sparse
                    x = x0 + r.nextInt(-3000, 3000);
                    y = y0 + r.nextInt(-20, 20);
                    z = z0 + r.nextInt(-3000, 3000);
                }
            }
            run.insert(x, y, z);
        }
    }

    // ── kind 3: leaf-distance style BFS: pop the first element, push unseen neighbours ───────────────────────
    static final int[][] DIRS = {{0, -1, 0}, {0, 1, 0}, {0, 0, -1}, {0, 0, 1}, {-1, 0, 0}, {1, 0, 0}};

    static void bfs(SplittableRandom r, Run run) throws IOException {
        int radius = r.nextInt(3, 13);
        double q = 0.5 + 0.5 * r.nextDouble();
        boolean dedupe = r.nextInt(4) != 0;
        int x0 = baseCoord(r), y0 = r.nextInt(-64, 320), z0 = baseCoord(r);
        HashSet<Pos> seen = new HashSet<>();
        int starts = r.nextInt(1, 6);
        for (int i = 0; i < starts; i++) {
            Pos s = new Pos(x0 + r.nextInt(-2, 3), y0 + r.nextInt(-1, 2), z0 + r.nextInt(-2, 3));
            run.insert(s.x(), s.y(), s.z());
            seen.add(s);
        }
        int pops = 0;
        while (!run.set.isEmpty() && pops < 2500) {
            Pos p = run.pop();
            pops++;
            for (int[] d : DIRS) {
                if (r.nextDouble() > q) continue;
                Pos n = new Pos(p.x() + d[0], p.y() + d[1], p.z() + d[2]);
                int dist = Math.abs(n.x() - x0) + Math.abs(n.y() - y0) + Math.abs(n.z() - z0);
                if (dist > radius) continue;
                if (dedupe && !seen.add(n)) continue;
                run.insert(n.x(), n.y(), n.z());
            }
        }
    }

    // ── kind 4: random interleaving of insert / pop-first / remove ───────────────────────────────────────────
    static void interleave(SplittableRandom r, Run run) throws IOException {
        int n = r.nextInt(100, 2500);
        int mode = r.nextInt(3);
        int x0 = baseCoord(r), y0 = r.nextInt(-64, 320), z0 = baseCoord(r);
        int w = r.nextInt(3, 30), wy = r.nextInt(1, 6);
        int stride = 64 << r.nextInt(3);
        int pPop = r.nextInt(10, 60), pRemove = r.nextInt(0, 20);
        List<Pos> history = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            int roll = r.nextInt(100);
            if (!run.set.isEmpty() && roll < pPop) {
                run.pop();
            } else if (roll < pPop + pRemove) {
                Pos t = history.isEmpty() || r.nextInt(5) == 0
                        ? new Pos(x0 + r.nextInt(w), y0 + r.nextInt(wy), z0 + r.nextInt(w))
                        : history.get(r.nextInt(history.size()));
                run.remove(t);
            } else {
                int x, y, z;
                if (mode == 0) { // box
                    x = x0 + r.nextInt(w);
                    y = y0 + r.nextInt(wy);
                    z = z0 + r.nextInt(w);
                } else if (mode == 1) { // strided along x: heavy low-bit clustering
                    x = x0 + stride * r.nextInt(30) + r.nextInt(2);
                    y = y0 + r.nextInt(2);
                    z = z0;
                } else { // mix
                    x = r.nextBoolean() ? x0 + stride * r.nextInt(24) : x0 + r.nextInt(w);
                    y = y0 + r.nextInt(wy);
                    z = z0 + r.nextInt(3);
                }
                run.insert(x, y, z);
                history.add(new Pos(x, y, z));
            }
        }
    }

    public static void main(String[] args) throws Exception {
        long seed = Long.decode(args[0]);
        int count = Integer.parseInt(args[1]);
        OutputStream raw = args.length > 2
                ? new FileOutputStream(args[2])
                : new FileOutputStream(FileDescriptor.out);
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(raw, 1 << 16))) {
            out.writeInt(0x48534F31);
            out.writeLong(seed);
            out.writeInt(count);
            SplittableRandom master = new SplittableRandom(seed);
            for (int i = 0; i < count; i++) {
                SplittableRandom r = master.split();
                int kind = switch (i % 10) {
                    case 0, 1, 2 -> 0;
                    case 3, 4 -> 1;
                    case 5 -> 2;
                    case 6, 7 -> 3;
                    default -> 4;
                };
                Run run = new Run();
                switch (kind) {
                    case 0 -> patch(r, run);
                    case 1 -> dense(r, run);
                    case 2 -> growth(r, run);
                    case 3 -> bfs(r, run);
                    default -> interleave(r, run);
                }
                run.write(out, kind);
            }
        }
    }
}
