# Java: `HashMap` / `HashSet` iteration order (OpenJDK 21+, as used by 26.3)

- Verified by suites: `java-hashset` (fixtures from the user's own JDK, our generator `tools/javamap-fixtures/`)
- Sources: the public, open-source OpenJDK `java.util.HashMap` behaviour (JDK 8 through 25 share it), described in
  our own words; black-box fixtures produced by a JDK-only generator. No game code is involved.
- Related specs: `nbt/binary-format.md` (compound key order also follows `HashMap`), `worldgen/features.md` §4.24,
  §4.55, §4.56 and `worldgen/features-trees.md` (feature code that iterates or pops `HashSet<BlockPos>`).

## Summary

Several game algorithms iterate a `HashSet` (a `HashMap` with dummy values) and draw random numbers per element, so the
**exact iteration order** decides the output. The order is the order of the table's buckets (index 0 upward), and
inside a bucket the order of its linked chain. Plain buckets keep insertion order, but a bucket can turn into a
**tree bin**, whose chain order differs from insertion order. An emulation that only appends to buckets is wrong in
about 0.02% of patch-sized sets (found 2026-11-25: 4 of 19,600 random patch shapes; one golden-features chunk).

## Inputs and state

- `hashCode()` of the key: for `BlockPos` / `Vec3i` it is `((y + z * 31) * 31) + x` in wrapping 32-bit arithmetic.
- **Spread hash** `h = hashCode ^ (hashCode >>> 16)` (unsigned shift). All bucket and tree decisions use `h`.
  Tree ordering compares `h` as a **signed** 32-bit integer.
- Table: power-of-two length `n` (created lazily at 16 on first insert), bucket index `(n - 1) & h`, `size`,
  `threshold = n * 3 / 4` (load factor 0.75; doubles together with `n`).
- Constants: tree threshold 8, untree threshold 6, minimum tree capacity 64, maximum capacity 2^30.
- Every node has `next` (the chain order used by iteration) and, in tree bins, `prev`, `parent`, `left`, `right`,
  `red`.

## Behavior (ordered rules)

### 1. Insert (`add` / `put`)
1. Create the table at length 16 if absent.
2. Empty bucket: the node becomes the bucket's only node.
3. If the bucket's first node is a tree node, insert into the tree (rule 3). Otherwise walk the chain; if an equal key
   exists, stop (no change). Otherwise append the node at the chain's end; if the chain had **at least 8 nodes before
   the append** (so it now has 9 or more), call *treeify-bin* (rule 2) for that bucket.
4. Then `size += 1`; if `size > threshold`, resize (rule 4). (Treeify-bin may itself resize first; the size check still
   follows and, in practice, never triggers a second resize.)

### 2. Treeify-bin
- If the table length is below 64: resize instead (rule 4); the bucket stays a plain chain.
- Otherwise convert the bucket's chain to tree nodes **keeping the chain order** and build a red-black tree by
  inserting the nodes one by one in chain order with the tree-insert walk (rule 3, steps 1–3 without the chain
  splice), each followed by insert-rebalancing (rule 5). When done, **move the root to the front** of the chain
  (rule 6).

### 3. Insert into a tree bin
1. Start at the root. Compare the new key's `h` with the node's `h` (signed): smaller → go left, larger → go right.
2. Equal `h`: if the keys are equal, stop (already present). Otherwise the keys are compared by their natural order
   only if the key's *runtime class itself* declares `Comparable<ThatClass>`; `BlockPos` does not (its comparison is
   declared on `Vec3i`), so for `BlockPos` the game first searches both subtrees for an equal key, then breaks the tie
   by class name (equal) and finally by `System.identityHashCode` — **not reproducible** (see Edge cases 1).
3. Descend until the chosen child is empty; attach the new node there as that child of the last node `p`.
4. **Chain splice:** the new node is linked into the chain **directly after `p`** (between `p` and `p`'s old `next`),
   not at the end.
5. Rebalance (rule 5) and move the (possibly new) root to the front of the chain (rule 6).

### 4. Resize (double)
- New length `2n`, new threshold `2 * threshold` (from 16: 12 → 24 → 48 …).
- For each old bucket `j` in index order, its nodes go to `j` (bit `n` of `h` clear) or `j + n` (set), **keeping their
  relative chain order** in each half.
- A **tree bin** splits the same way into a low and a high chain (order kept). For each half that is non-empty: if it
  has 6 or fewer nodes it becomes a plain chain (same order); otherwise it stays a tree bin, and it is **re-built as a
  tree (rule 2's build, then root to front) only if the other half is also non-empty**. If one half is empty, the other
  half keeps the old tree unchanged.

### 5. Insert rebalancing (red-black)
Standard red-black insert fix-up on the new node `x` (coloured red): while `x`'s parent is red, with grandparent `g`:
if the uncle is red, recolour parent and uncle black and `g` red, and continue from `g`; otherwise, if `x` is the
"inner" child, rotate at the parent to make it outer; then colour the parent black, `g` red, and rotate at `g` the
other way. The root is always black. Rotations update the root when they rotate at it.

### 6. Move root to front
If the bucket's first chain node is not the tree root: unlink the root from its chain position and put it first; the
rest keeps its order.

### 7. Remove through an iterator (`iterator.remove()`, e.g. "pop the first element")
- Plain chain: unlink the node (if it was first, the next node becomes first).
- Tree bin: unlink the node from the chain (its `next` / `prev` neighbours join), then delete it from the red-black
  tree with the standard delete (swap with the in-order successor when it has two children — exchanging their tree
  positions and colours, not their chain positions — then delete fix-up). Iterator removal does **not** move the root
  to the front and does **not** turn a small tree back into a plain chain.
- The table never shrinks.

### 8. Iteration
Buckets `0 .. n-1` in order; within a bucket, follow `next` from the first node.

## Edge cases and known quirks

1. **Equal spread hash, different `BlockPos` in one tree bin:** the order depends on identity hash codes and is not
   deterministic between runs of the game. Raw `BlockPos` hashes collide only for positions 31 apart in `x` with `y`
   one lower (or 961 apart in `x` with `z` one lower), which feature-sized sets almost never contain. Zenith uses a
   deterministic tie-break (insertion order: the new node goes right) and parity suites must report such cases as
   "vanilla-nondeterministic" instead of failures.
2. Tree bins appear only when a bucket collects 9 entries while the table has at least 64 buckets.
3. The chain order of a tree bin is what iteration uses; the tree shape only matters for where the next insert is
   spliced and which node is the root.

## Constants and IDs

Initial capacity 16; load factor 0.75; tree threshold 8; untree threshold 6; minimum tree capacity 64.

## Open questions

None yet; the `java-hashset` fixtures settle every rule above except the non-reproducible identity tie-break.

## Parity status

`java-hashset`: not yet run.
