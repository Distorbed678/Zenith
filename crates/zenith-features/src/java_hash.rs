//! Exact Java 21 `HashSet<BlockPos>` iteration order (`docs/specs/java/hash-collections.md`).
//!
//! Mirrors `java.util.HashMap`: spread hash (`h ^ (h >>> 16)`), `BlockPos.hashCode()`
//! (`((y + z * 31) * 31) + x`), load factor 0.75, lazy initial capacity 16, split-preserving power-of-two
//! resizing, and **tree bins** (a bucket that reaches 9 entries once the table has at least 64 buckets becomes a
//! red-black tree whose chain order differs from insertion order).
//!
//! Nodes live in an arena and are linked by index. A bucket is a chain (`next`); in a tree bin the same nodes
//! also carry `prev` / `parent` / `left` / `right` / colour. Iteration only follows the chains.
//!
//! One thing the JDK does not define: two distinct positions with an equal spread hash meeting in a tree bin are
//! ordered by `System.identityHashCode`. Here the later node goes right, and
//! [`JavaBlockPosSet::identity_tie_breaks`] counts how often that happened so callers can tell.

use crate::rng::BlockPos;

const DEFAULT_INITIAL_CAPACITY: usize = 16;
const MAXIMUM_CAPACITY: usize = 1 << 30;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

/// "No node".
const NIL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
struct Node {
    pos: BlockPos,
    /// Spread hash; compared as a signed `i32` where the tree orders nodes.
    hash: u32,
    next: u32,
    // Tree-bin links (meaningless for plain chain nodes).
    prev: u32,
    parent: u32,
    left: u32,
    right: u32,
    red: bool,
}

#[derive(Debug, Clone, Copy)]
struct Bin {
    /// First chain node, or `NIL`.
    head: u32,
    /// Whether the bucket is a tree bin (its first node is a tree node).
    tree: bool,
}

const EMPTY_BIN: Bin = Bin {
    head: NIL,
    tree: false,
};

/// A set of [`BlockPos`] values whose iteration order matches Java 21 `HashSet<BlockPos>`.
#[derive(Debug, Clone, Default)]
pub struct JavaBlockPosSet {
    nodes: Vec<Node>,
    free: Vec<u32>,
    bins: Vec<Bin>,
    len: usize,
    threshold: usize,
    /// Lower bound on the lowest non-empty bucket index (speeds up [`Self::pop_first`]).
    first_hint: usize,
    tie_breaks: u32,
}

#[inline]
const fn spread_hash(java_hash: i32) -> u32 {
    let h = java_hash as u32;
    h ^ (h >> 16)
}

impl JavaBlockPosSet {
    /// Creates an empty `JavaBlockPosSet` (equivalent to `new HashSet<>()`).
    #[inline]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            free: Vec::new(),
            bins: Vec::new(),
            len: 0,
            threshold: 0,
            first_hint: 0,
            tie_breaks: 0,
        }
    }

    /// Number of stored positions.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the set is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many times two distinct positions with an equal spread hash had to be ordered inside a tree bin.
    ///
    /// The JDK decides those by `System.identityHashCode`, which no emulation can reproduce, so the iteration
    /// order of a set with a non-zero count is only one of the orders the real game could produce.
    #[inline]
    pub const fn identity_tie_breaks(&self) -> u32 {
        self.tie_breaks
    }

    // ── arena helpers ────────────────────────────────────────────────────────────────────────────────────────

    #[inline]
    fn n(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    #[inline]
    fn nm(&mut self, i: u32) -> &mut Node {
        &mut self.nodes[i as usize]
    }

    fn alloc(&mut self, pos: BlockPos, hash: u32) -> u32 {
        let node = Node {
            pos,
            hash,
            next: NIL,
            prev: NIL,
            parent: NIL,
            left: NIL,
            right: NIL,
            red: false,
        };
        if let Some(i) = self.free.pop() {
            self.nodes[i as usize] = node;
            i
        } else {
            self.nodes.push(node);
            (self.nodes.len() - 1) as u32
        }
    }

    // ── red-black tree primitives (java.util.HashMap.TreeNode) ───────────────────────────────────────────────

    fn root_of(&self, mut x: u32) -> u32 {
        while self.n(x).parent != NIL {
            x = self.n(x).parent;
        }
        x
    }

    fn rotate_left(&mut self, mut root: u32, p: u32) -> u32 {
        if p == NIL {
            return root;
        }
        let r = self.n(p).right;
        if r == NIL {
            return root;
        }
        let rl = self.n(r).left;
        self.nm(p).right = rl;
        if rl != NIL {
            self.nm(rl).parent = p;
        }
        let pp = self.n(p).parent;
        self.nm(r).parent = pp;
        if pp == NIL {
            root = r;
            self.nm(r).red = false;
        } else if self.n(pp).left == p {
            self.nm(pp).left = r;
        } else {
            self.nm(pp).right = r;
        }
        self.nm(r).left = p;
        self.nm(p).parent = r;
        root
    }

    fn rotate_right(&mut self, mut root: u32, p: u32) -> u32 {
        if p == NIL {
            return root;
        }
        let l = self.n(p).left;
        if l == NIL {
            return root;
        }
        let lr = self.n(l).right;
        self.nm(p).left = lr;
        if lr != NIL {
            self.nm(lr).parent = p;
        }
        let pp = self.n(p).parent;
        self.nm(l).parent = pp;
        if pp == NIL {
            root = l;
            self.nm(l).red = false;
        } else if self.n(pp).right == p {
            self.nm(pp).right = l;
        } else {
            self.nm(pp).left = l;
        }
        self.nm(l).right = p;
        self.nm(p).parent = l;
        root
    }

    fn balance_insertion(&mut self, mut root: u32, mut x: u32) -> u32 {
        self.nm(x).red = true;
        loop {
            let mut xp = self.n(x).parent;
            if xp == NIL {
                self.nm(x).red = false;
                return x;
            }
            if !self.n(xp).red {
                return root;
            }
            let mut xpp = self.n(xp).parent;
            if xpp == NIL {
                return root;
            }
            if xp == self.n(xpp).left {
                let xppr = self.n(xpp).right;
                if xppr != NIL && self.n(xppr).red {
                    self.nm(xppr).red = false;
                    self.nm(xp).red = false;
                    self.nm(xpp).red = true;
                    x = xpp;
                } else {
                    if x == self.n(xp).right {
                        x = xp;
                        root = self.rotate_left(root, x);
                        xp = self.n(x).parent;
                        xpp = if xp == NIL { NIL } else { self.n(xp).parent };
                    }
                    if xp != NIL {
                        self.nm(xp).red = false;
                        if xpp != NIL {
                            self.nm(xpp).red = true;
                            root = self.rotate_right(root, xpp);
                        }
                    }
                }
            } else {
                let xppl = self.n(xpp).left;
                if xppl != NIL && self.n(xppl).red {
                    self.nm(xppl).red = false;
                    self.nm(xp).red = false;
                    self.nm(xpp).red = true;
                    x = xpp;
                } else {
                    if x == self.n(xp).left {
                        x = xp;
                        root = self.rotate_right(root, x);
                        xp = self.n(x).parent;
                        xpp = if xp == NIL { NIL } else { self.n(xp).parent };
                    }
                    if xp != NIL {
                        self.nm(xp).red = false;
                        if xpp != NIL {
                            self.nm(xpp).red = true;
                            root = self.rotate_left(root, xpp);
                        }
                    }
                }
            }
        }
    }

    fn balance_deletion(&mut self, mut root: u32, mut x: u32) -> u32 {
        loop {
            if x == NIL || x == root {
                return root;
            }
            let mut xp = self.n(x).parent;
            if xp == NIL {
                self.nm(x).red = false;
                return x;
            }
            if self.n(x).red {
                self.nm(x).red = false;
                return root;
            }
            let xpl = self.n(xp).left;
            if xpl == x {
                let mut xpr = self.n(xp).right;
                if xpr != NIL && self.n(xpr).red {
                    self.nm(xpr).red = false;
                    self.nm(xp).red = true;
                    root = self.rotate_left(root, xp);
                    xp = self.n(x).parent;
                    xpr = if xp == NIL { NIL } else { self.n(xp).right };
                }
                if xpr == NIL {
                    x = xp;
                } else {
                    let sl = self.n(xpr).left;
                    let mut sr = self.n(xpr).right;
                    let sr_red = sr != NIL && self.n(sr).red;
                    let sl_red = sl != NIL && self.n(sl).red;
                    if !sr_red && !sl_red {
                        self.nm(xpr).red = true;
                        x = xp;
                    } else {
                        if !sr_red {
                            if sl != NIL {
                                self.nm(sl).red = false;
                            }
                            self.nm(xpr).red = true;
                            root = self.rotate_right(root, xpr);
                            xp = self.n(x).parent;
                            xpr = if xp == NIL { NIL } else { self.n(xp).right };
                        }
                        if xpr != NIL {
                            self.nm(xpr).red = if xp == NIL { false } else { self.n(xp).red };
                            sr = self.n(xpr).right;
                            if sr != NIL {
                                self.nm(sr).red = false;
                            }
                        }
                        if xp != NIL {
                            self.nm(xp).red = false;
                            root = self.rotate_left(root, xp);
                        }
                        x = root;
                    }
                }
            } else {
                let mut xpl = xpl;
                if xpl != NIL && self.n(xpl).red {
                    self.nm(xpl).red = false;
                    self.nm(xp).red = true;
                    root = self.rotate_right(root, xp);
                    xp = self.n(x).parent;
                    xpl = if xp == NIL { NIL } else { self.n(xp).left };
                }
                if xpl == NIL {
                    x = xp;
                } else {
                    let mut sl = self.n(xpl).left;
                    let sr = self.n(xpl).right;
                    let sl_red = sl != NIL && self.n(sl).red;
                    let sr_red = sr != NIL && self.n(sr).red;
                    if !sl_red && !sr_red {
                        self.nm(xpl).red = true;
                        x = xp;
                    } else {
                        if !sl_red {
                            if sr != NIL {
                                self.nm(sr).red = false;
                            }
                            self.nm(xpl).red = true;
                            root = self.rotate_left(root, xpl);
                            xp = self.n(x).parent;
                            xpl = if xp == NIL { NIL } else { self.n(xp).left };
                        }
                        if xpl != NIL {
                            self.nm(xpl).red = if xp == NIL { false } else { self.n(xp).red };
                            sl = self.n(xpl).left;
                            if sl != NIL {
                                self.nm(sl).red = false;
                            }
                        }
                        if xp != NIL {
                            self.nm(xp).red = false;
                            root = self.rotate_right(root, xp);
                        }
                        x = root;
                    }
                }
            }
        }
    }

    /// Moves the tree root of bucket `idx` to the front of its chain.
    fn move_root_to_front(&mut self, idx: usize, root: u32) {
        if root == NIL {
            return;
        }
        let first = self.bins[idx].head;
        if root == first {
            return;
        }
        self.bins[idx].head = root;
        let rp = self.n(root).prev;
        let rn = self.n(root).next;
        if rn != NIL {
            self.nm(rn).prev = rp;
        }
        if rp != NIL {
            self.nm(rp).next = rn;
        }
        if first != NIL {
            self.nm(first).prev = root;
        }
        let r = self.nm(root);
        r.next = first;
        r.prev = NIL;
    }

    /// Builds a red-black tree from the chain of bucket `idx` (inserting in chain order), then moves the root to
    /// the front. The nodes keep their chain order.
    fn treeify(&mut self, idx: usize) {
        let mut root = NIL;
        let mut x = self.bins[idx].head;
        while x != NIL {
            let next = self.n(x).next;
            {
                let n = self.nm(x);
                n.left = NIL;
                n.right = NIL;
            }
            if root == NIL {
                let n = self.nm(x);
                n.parent = NIL;
                n.red = false;
                root = x;
            } else {
                let h = self.n(x).hash as i32;
                let mut p = root;
                loop {
                    let ph = self.n(p).hash as i32;
                    let dir = if ph > h {
                        -1
                    } else if ph < h {
                        1
                    } else {
                        // Distinct positions with an equal spread hash: the JDK uses identity hash codes here.
                        self.tie_breaks += 1;
                        1
                    };
                    let xp = p;
                    p = if dir <= 0 {
                        self.n(xp).left
                    } else {
                        self.n(xp).right
                    };
                    if p == NIL {
                        self.nm(x).parent = xp;
                        if dir <= 0 {
                            self.nm(xp).left = x;
                        } else {
                            self.nm(xp).right = x;
                        }
                        root = self.balance_insertion(root, x);
                        break;
                    }
                }
            }
            x = next;
        }
        self.move_root_to_front(idx, root);
    }

    /// Searches the subtree under `p` for `pos` (the JDK's `TreeNode.find` without a comparable key class).
    fn find_in_subtree(&self, mut p: u32, hash: u32, pos: BlockPos) -> u32 {
        let h = hash as i32;
        while p != NIL {
            let node = self.n(p);
            let ph = node.hash as i32;
            let (pl, pr) = (node.left, node.right);
            if ph > h {
                p = pl;
            } else if ph < h {
                p = pr;
            } else if node.pos == pos {
                return p;
            } else if pl == NIL {
                p = pr;
            } else if pr == NIL {
                p = pl;
            } else {
                let q = self.find_in_subtree(pr, hash, pos);
                if q != NIL {
                    return q;
                }
                p = pl;
            }
        }
        NIL
    }

    /// Inserts into tree bin `idx`; returns `false` if `pos` was already present.
    fn put_tree_val(&mut self, idx: usize, hash: u32, pos: BlockPos) -> bool {
        let root = self.root_of(self.bins[idx].head);
        let h = hash as i32;
        let mut searched = false;
        let mut p = root;
        loop {
            let ph = self.n(p).hash as i32;
            let dir = if ph > h {
                -1
            } else if ph < h {
                1
            } else if self.n(p).pos == pos {
                return false;
            } else {
                if !searched {
                    searched = true;
                    let (l, r) = (self.n(p).left, self.n(p).right);
                    if (l != NIL && self.find_in_subtree(l, hash, pos) != NIL)
                        || (r != NIL && self.find_in_subtree(r, hash, pos) != NIL)
                    {
                        return false;
                    }
                }
                self.tie_breaks += 1;
                1
            };
            let xp = p;
            p = if dir <= 0 {
                self.n(xp).left
            } else {
                self.n(xp).right
            };
            if p == NIL {
                let xpn = self.n(xp).next;
                let x = self.alloc(pos, hash);
                {
                    let n = self.nm(x);
                    n.next = xpn;
                    n.parent = xp;
                    n.prev = xp;
                }
                if dir <= 0 {
                    self.nm(xp).left = x;
                } else {
                    self.nm(xp).right = x;
                }
                self.nm(xp).next = x;
                if xpn != NIL {
                    self.nm(xpn).prev = x;
                }
                let new_root = self.balance_insertion(root, x);
                self.move_root_to_front(idx, new_root);
                return true;
            }
        }
    }

    /// Unlinks `p` from tree bin `idx` (chain and red-black tree) like `TreeNode.removeTreeNode`.
    /// `movable` is `false` for iterator removal (no untreeify, no root-to-front) and `true` for `Set.remove`.
    fn remove_tree_node(&mut self, idx: usize, p: u32, movable: bool) {
        let succ = self.n(p).next;
        let pred = self.n(p).prev;
        let first;
        if pred == NIL {
            first = succ;
            self.bins[idx].head = succ;
        } else {
            first = self.bins[idx].head;
            self.nm(pred).next = succ;
        }
        if succ != NIL {
            self.nm(succ).prev = pred;
        }
        if first == NIL {
            self.bins[idx] = EMPTY_BIN;
            self.free.push(p);
            return;
        }
        let mut root = first;
        if self.n(root).parent != NIL {
            root = self.root_of(root);
        }
        if movable {
            let r = self.n(root);
            let too_small = r.right == NIL || r.left == NIL || self.n(r.left).left == NIL;
            if too_small {
                // Untreeify: the chain stays as it is, the bucket becomes a plain list.
                self.bins[idx].tree = false;
                self.free.push(p);
                return;
            }
        }
        let (pl, pr) = (self.n(p).left, self.n(p).right);
        let replacement = if pl != NIL && pr != NIL {
            let mut s = pr;
            while self.n(s).left != NIL {
                s = self.n(s).left;
            }
            let (sc, pc) = (self.n(s).red, self.n(p).red);
            self.nm(s).red = pc;
            self.nm(p).red = sc;
            let sr = self.n(s).right;
            let pp = self.n(p).parent;
            if s == pr {
                self.nm(p).parent = s;
                self.nm(s).right = p;
            } else {
                let sp = self.n(s).parent;
                self.nm(p).parent = sp;
                if sp != NIL {
                    if s == self.n(sp).left {
                        self.nm(sp).left = p;
                    } else {
                        self.nm(sp).right = p;
                    }
                }
                self.nm(s).right = pr;
                self.nm(pr).parent = s;
            }
            self.nm(p).left = NIL;
            self.nm(p).right = sr;
            if sr != NIL {
                self.nm(sr).parent = p;
            }
            self.nm(s).left = pl;
            self.nm(pl).parent = s;
            self.nm(s).parent = pp;
            if pp == NIL {
                root = s;
            } else if p == self.n(pp).left {
                self.nm(pp).left = s;
            } else {
                self.nm(pp).right = s;
            }
            if sr != NIL { sr } else { p }
        } else if pl != NIL {
            pl
        } else if pr != NIL {
            pr
        } else {
            p
        };
        if replacement != p {
            let pp = self.n(p).parent;
            self.nm(replacement).parent = pp;
            if pp == NIL {
                root = replacement;
                self.nm(replacement).red = false;
            } else if p == self.n(pp).left {
                self.nm(pp).left = replacement;
            } else {
                self.nm(pp).right = replacement;
            }
            let n = self.nm(p);
            n.left = NIL;
            n.right = NIL;
            n.parent = NIL;
        }
        let r = if self.n(p).red {
            root
        } else {
            self.balance_deletion(root, replacement)
        };
        if replacement == p {
            let pp = self.n(p).parent;
            self.nm(p).parent = NIL;
            if pp != NIL {
                if p == self.n(pp).left {
                    self.nm(pp).left = NIL;
                } else if p == self.n(pp).right {
                    self.nm(pp).right = NIL;
                }
            }
        }
        if movable {
            self.move_root_to_front(idx, r);
        }
        self.free.push(p);
    }

    // ── table ────────────────────────────────────────────────────────────────────────────────────────────────

    /// Splits tree bin `old_j` (whose head is `head`) into the new table, like `TreeNode.split`.
    fn split_tree_bin(&mut self, old_j: usize, head: u32, bit: u32) {
        let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
        let (mut lc, mut hc) = (0usize, 0usize);
        let mut e = head;
        while e != NIL {
            let next = self.n(e).next;
            self.nm(e).next = NIL;
            if self.n(e).hash & bit == 0 {
                self.nm(e).prev = lo_tail;
                if lo_tail == NIL {
                    lo_head = e;
                } else {
                    self.nm(lo_tail).next = e;
                }
                lo_tail = e;
                lc += 1;
            } else {
                self.nm(e).prev = hi_tail;
                if hi_tail == NIL {
                    hi_head = e;
                } else {
                    self.nm(hi_tail).next = e;
                }
                hi_tail = e;
                hc += 1;
            }
            e = next;
        }
        if lo_head != NIL {
            if lc <= UNTREEIFY_THRESHOLD {
                self.bins[old_j] = Bin {
                    head: lo_head,
                    tree: false,
                };
            } else {
                self.bins[old_j] = Bin {
                    head: lo_head,
                    tree: true,
                };
                if hi_head != NIL {
                    self.treeify(old_j);
                }
            }
        }
        if hi_head != NIL {
            let j = old_j + bit as usize;
            if hc <= UNTREEIFY_THRESHOLD {
                self.bins[j] = Bin {
                    head: hi_head,
                    tree: false,
                };
            } else {
                self.bins[j] = Bin {
                    head: hi_head,
                    tree: true,
                };
                if lo_head != NIL {
                    self.treeify(j);
                }
            }
        }
    }

    /// `HashMap.resize()`: creates the table, or doubles it.
    fn resize(&mut self) {
        if self.bins.is_empty() {
            self.bins = vec![EMPTY_BIN; DEFAULT_INITIAL_CAPACITY];
            self.threshold = DEFAULT_INITIAL_CAPACITY * 3 / 4;
            return;
        }
        let old_cap = self.bins.len();
        if old_cap >= MAXIMUM_CAPACITY {
            self.threshold = usize::MAX;
            return;
        }
        let new_cap = old_cap << 1;
        let old_bins = std::mem::replace(&mut self.bins, vec![EMPTY_BIN; new_cap]);
        let bit = old_cap as u32;
        for (j, bin) in old_bins.into_iter().enumerate() {
            if bin.head == NIL {
                continue;
            }
            if bin.tree {
                if self.n(bin.head).next == NIL {
                    // A lone node is moved as it is (the JDK checks `next == null` before splitting a tree), so it
                    // stays a tree node: later inserts into this bucket still go through the tree rules.
                    let to = (self.n(bin.head).hash as usize) & (new_cap - 1);
                    self.bins[to] = bin;
                } else {
                    self.split_tree_bin(j, bin.head, bit);
                }
                continue;
            }
            let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
            let mut e = bin.head;
            while e != NIL {
                let next = self.n(e).next;
                if self.n(e).hash & bit == 0 {
                    if lo_tail == NIL {
                        lo_head = e;
                    } else {
                        self.nm(lo_tail).next = e;
                    }
                    lo_tail = e;
                } else {
                    if hi_tail == NIL {
                        hi_head = e;
                    } else {
                        self.nm(hi_tail).next = e;
                    }
                    hi_tail = e;
                }
                e = next;
            }
            if lo_tail != NIL {
                self.nm(lo_tail).next = NIL;
                self.bins[j] = Bin {
                    head: lo_head,
                    tree: false,
                };
            }
            if hi_tail != NIL {
                self.nm(hi_tail).next = NIL;
                self.bins[j + old_cap] = Bin {
                    head: hi_head,
                    tree: false,
                };
            }
        }
        self.threshold = self.threshold.saturating_mul(2);
        self.first_hint = 0;
    }

    /// `HashMap.treeifyBin`: grow the table while it is below 64 buckets, else turn the chain into a tree.
    fn treeify_bin(&mut self, idx: usize) {
        if self.bins.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
            return;
        }
        let mut e = self.bins[idx].head;
        if e == NIL {
            return;
        }
        let mut prev = NIL;
        while e != NIL {
            let n = self.nm(e);
            n.prev = prev;
            n.parent = NIL;
            n.left = NIL;
            n.right = NIL;
            prev = e;
            e = n.next;
        }
        self.bins[idx].tree = true;
        self.treeify(idx);
    }

    /// Inserts `pos` into the set. Returns `true` if newly inserted, `false` if already present.
    pub fn insert(&mut self, pos: BlockPos) -> bool {
        if self.bins.is_empty() {
            self.resize();
        }
        let hash = spread_hash(pos.java_hash_code());
        let idx = (hash as usize) & (self.bins.len() - 1);
        let bin = self.bins[idx];
        if bin.head == NIL {
            let x = self.alloc(pos, hash);
            self.bins[idx] = Bin {
                head: x,
                tree: false,
            };
        } else if bin.tree {
            if !self.put_tree_val(idx, hash, pos) {
                return false;
            }
        } else {
            let mut e = bin.head;
            let mut count = 1usize;
            loop {
                let n = self.n(e);
                if n.hash == hash && n.pos == pos {
                    return false;
                }
                if n.next == NIL {
                    break;
                }
                e = n.next;
                count += 1;
            }
            let x = self.alloc(pos, hash);
            self.nm(e).next = x;
            if count > TREEIFY_THRESHOLD - 1 {
                self.treeify_bin(idx);
            }
        }
        if idx < self.first_hint {
            self.first_hint = idx;
        }
        self.len += 1;
        if self.len > self.threshold {
            self.resize();
        }
        true
    }

    /// `set.iterator().next()` followed by `iterator.remove()`: removes and returns the first
    /// element in Java iteration order (the table never shrinks, as in `java.util.HashMap`).
    pub fn pop_first(&mut self) -> Option<BlockPos> {
        if self.len == 0 {
            return None;
        }
        let mut i = self.first_hint;
        while i < self.bins.len() {
            let bin = self.bins[i];
            if bin.head != NIL {
                self.first_hint = i;
                let pos = self.n(bin.head).pos;
                self.len -= 1;
                if bin.tree {
                    self.remove_tree_node(i, bin.head, false);
                } else {
                    self.bins[i].head = self.n(bin.head).next;
                    self.free.push(bin.head);
                }
                return Some(pos);
            }
            i += 1;
        }
        None
    }

    /// `set.remove(pos)`: returns `true` if it was present. Unlike the iterator removal of
    /// [`Self::pop_first`], a tree bin that gets too small turns back into a plain chain and the root moves to the
    /// front.
    pub fn remove(&mut self, pos: BlockPos) -> bool {
        if self.bins.is_empty() {
            return false;
        }
        let hash = spread_hash(pos.java_hash_code());
        let idx = (hash as usize) & (self.bins.len() - 1);
        let bin = self.bins[idx];
        if bin.head == NIL {
            return false;
        }
        if bin.tree {
            let root = self.root_of(bin.head);
            let node = self.find_in_subtree(root, hash, pos);
            if node == NIL {
                return false;
            }
            self.remove_tree_node(idx, node, true);
        } else {
            let (mut prev, mut e) = (NIL, bin.head);
            while e != NIL {
                let n = self.n(e);
                if n.hash == hash && n.pos == pos {
                    break;
                }
                prev = e;
                e = n.next;
            }
            if e == NIL {
                return false;
            }
            let next = self.n(e).next;
            if prev == NIL {
                self.bins[idx].head = next;
            } else {
                self.nm(prev).next = next;
            }
            self.free.push(e);
        }
        self.len -= 1;
        true
    }

    /// Returns `true` if `pos` is present.
    pub fn contains(&self, pos: BlockPos) -> bool {
        if self.bins.is_empty() {
            return false;
        }
        let hash = spread_hash(pos.java_hash_code());
        let bin = self.bins[(hash as usize) & (self.bins.len() - 1)];
        if bin.head == NIL {
            return false;
        }
        if bin.tree {
            return self.find_in_subtree(self.root_of(bin.head), hash, pos) != NIL;
        }
        let mut e = bin.head;
        while e != NIL {
            let n = self.n(e);
            if n.hash == hash && n.pos == pos {
                return true;
            }
            e = n.next;
        }
        false
    }

    /// Iterates stored positions in exact Java 21 `HashSet<BlockPos>` bucket order.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = BlockPos> + '_ {
        self.bins.iter().flat_map(move |bin| {
            let mut e = bin.head;
            std::iter::from_fn(move || {
                if e == NIL {
                    return None;
                }
                let n = &self.nodes[e as usize];
                e = n.next;
                Some(n.pos)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_code_matches_java_vec3i() {
        let p = BlockPos::new(10, 64, -5);
        // ((64 + (-5) * 31) * 31) + 10 = (-91 * 31) + 10 = -2821 + 10 = -2811
        assert_eq!(p.java_hash_code(), -2811);
    }

    #[test]
    fn dedup_and_resize() {
        let mut s = JavaBlockPosSet::new();
        for x in 0..20 {
            assert!(s.insert(BlockPos::new(x, 60, x * 2)));
        }
        assert!(!s.insert(BlockPos::new(5, 60, 10)));
        assert_eq!(s.len(), 20);
        assert_eq!(s.iter().count(), 20);
    }

    // Fixture generated with `java.util.HashSet` on a record whose `hashCode` is
    // `(y + z * 31) * 31 + x` (the `Vec3i` hash), 90 random inserts (with duplicates).
    const INSERTS: [(i32, i32, i32); 90] = [
        (-5244, 112, -2217),
        (-5246, 111, -2218),
        (-5242, 115, -2222),
        (-5246, 113, -2220),
        (-5242, 117, -2217),
        (-5248, 116, -2214),
        (-5243, 110, -2220),
        (-5246, 110, -2221),
        (-5242, 115, -2213),
        (-5250, 113, -2221),
        (-5247, 110, -2213),
        (-5249, 110, -2220),
        (-5242, 113, -2216),
        (-5246, 118, -2222),
        (-5247, 110, -2219),
        (-5245, 117, -2213),
        (-5244, 110, -2220),
        (-5245, 113, -2222),
        (-5246, 111, -2214),
        (-5245, 110, -2215),
        (-5246, 110, -2216),
        (-5241, 118, -2217),
        (-5250, 114, -2213),
        (-5243, 117, -2220),
        (-5243, 111, -2218),
        (-5244, 116, -2221),
        (-5249, 115, -2216),
        (-5246, 113, -2216),
        (-5243, 115, -2214),
        (-5249, 112, -2214),
        (-5244, 114, -2215),
        (-5243, 111, -2221),
        (-5250, 116, -2217),
        (-5245, 113, -2216),
        (-5249, 111, -2217),
        (-5243, 115, -2222),
        (-5246, 116, -2220),
        (-5249, 114, -2218),
        (-5242, 118, -2215),
        (-5248, 115, -2217),
        (-5249, 110, -2215),
        (-5243, 112, -2221),
        (-5242, 116, -2215),
        (-5244, 118, -2217),
        (-5249, 118, -2221),
        (-5246, 111, -2214),
        (-5249, 113, -2221),
        (-5246, 110, -2215),
        (-5245, 114, -2222),
        (-5246, 115, -2220),
        (-5249, 118, -2220),
        (-5241, 111, -2218),
        (-5243, 111, -2217),
        (-5249, 113, -2215),
        (-5246, 114, -2222),
        (-5249, 118, -2213),
        (-5244, 110, -2217),
        (-5241, 118, -2213),
        (-5241, 117, -2221),
        (-5247, 113, -2217),
        (-5242, 113, -2217),
        (-5242, 113, -2216),
        (-5242, 118, -2220),
        (-5243, 118, -2214),
        (-5246, 115, -2221),
        (-5245, 117, -2217),
        (-5243, 114, -2213),
        (-5250, 117, -2214),
        (-5250, 114, -2215),
        (-5242, 112, -2213),
        (-5247, 117, -2221),
        (-5244, 110, -2219),
        (-5247, 116, -2220),
        (-5247, 110, -2213),
        (-5247, 117, -2220),
        (-5249, 114, -2217),
        (-5250, 114, -2215),
        (-5245, 118, -2221),
        (-5241, 110, -2222),
        (-5242, 117, -2214),
        (-5241, 111, -2216),
        (-5243, 115, -2214),
        (-5250, 110, -2215),
        (-5248, 110, -2213),
        (-5250, 110, -2221),
        (-5250, 113, -2214),
        (-5250, 116, -2213),
        (-5246, 116, -2219),
        (-5244, 116, -2218),
        (-5243, 112, -2216),
    ];
    const ITERATION: [(i32, i32, i32); 85] = [
        (-5249, 118, -2221),
        (-5241, 111, -2216),
        (-5241, 111, -2218),
        (-5246, 111, -2214),
        (-5243, 111, -2218),
        (-5243, 115, -2214),
        (-5242, 113, -2217),
        (-5246, 111, -2218),
        (-5245, 117, -2213),
        (-5242, 117, -2217),
        (-5249, 113, -2215),
        (-5247, 113, -2217),
        (-5242, 115, -2222),
        (-5241, 117, -2221),
        (-5245, 117, -2217),
        (-5249, 115, -2216),
        (-5243, 115, -2222),
        (-5246, 115, -2220),
        (-5249, 113, -2221),
        (-5250, 113, -2221),
        (-5247, 110, -2213),
        (-5245, 110, -2215),
        (-5243, 114, -2213),
        (-5246, 110, -2215),
        (-5244, 110, -2217),
        (-5248, 110, -2213),
        (-5243, 112, -2216),
        (-5241, 118, -2213),
        (-5244, 114, -2215),
        (-5244, 110, -2219),
        (-5249, 110, -2215),
        (-5249, 112, -2214),
        (-5242, 118, -2215),
        (-5250, 110, -2215),
        (-5247, 110, -2219),
        (-5241, 118, -2217),
        (-5246, 110, -2221),
        (-5250, 114, -2213),
        (-5248, 116, -2214),
        (-5244, 116, -2218),
        (-5244, 118, -2217),
        (-5250, 114, -2215),
        (-5249, 118, -2213),
        (-5249, 114, -2217),
        (-5250, 110, -2221),
        (-5246, 116, -2220),
        (-5247, 116, -2220),
        (-5245, 118, -2221),
        (-5246, 118, -2222),
        (-5249, 118, -2220),
        (-5242, 115, -2213),
        (-5242, 113, -2216),
        (-5243, 111, -2217),
        (-5242, 117, -2214),
        (-5245, 113, -2216),
        (-5246, 113, -2216),
        (-5243, 111, -2221),
        (-5249, 111, -2217),
        (-5250, 113, -2214),
        (-5246, 113, -2220),
        (-5245, 113, -2222),
        (-5243, 117, -2220),
        (-5248, 115, -2217),
        (-5250, 117, -2214),
        (-5246, 115, -2221),
        (-5247, 117, -2220),
        (-5247, 117, -2221),
        (-5242, 112, -2213),
        (-5246, 110, -2216),
        (-5244, 112, -2217),
        (-5243, 110, -2220),
        (-5242, 116, -2215),
        (-5241, 110, -2222),
        (-5244, 110, -2220),
        (-5243, 118, -2214),
        (-5243, 112, -2221),
        (-5249, 110, -2220),
        (-5250, 116, -2213),
        (-5242, 118, -2220),
        (-5244, 116, -2221),
        (-5249, 114, -2218),
        (-5245, 114, -2222),
        (-5246, 116, -2219),
        (-5246, 114, -2222),
        (-5250, 116, -2217),
    ];
    const POP_FIRST: [(i32, i32, i32); 85] = [
        (-5249, 118, -2221),
        (-5241, 111, -2216),
        (-5241, 111, -2218),
        (-5246, 111, -2214),
        (-5243, 111, -2218),
        (-5243, 115, -2214),
        (-5242, 113, -2217),
        (-5246, 111, -2218),
        (-5245, 117, -2213),
        (-5242, 117, -2217),
        (-5249, 113, -2215),
        (-5247, 113, -2217),
        (-5242, 115, -2222),
        (-5241, 117, -2221),
        (-5245, 117, -2217),
        (-5249, 115, -2216),
        (-5243, 115, -2222),
        (-5246, 115, -2220),
        (-5249, 113, -2221),
        (-5250, 113, -2221),
        (-5247, 110, -2213),
        (-5245, 110, -2215),
        (-5243, 114, -2213),
        (-5246, 110, -2215),
        (-5244, 110, -2217),
        (-5248, 110, -2213),
        (-5243, 112, -2216),
        (-5241, 118, -2213),
        (-5244, 114, -2215),
        (-5244, 110, -2219),
        (-5249, 110, -2215),
        (-5249, 112, -2214),
        (-5242, 118, -2215),
        (-5250, 110, -2215),
        (-5247, 110, -2219),
        (-5241, 118, -2217),
        (-5246, 110, -2221),
        (-5250, 114, -2213),
        (-5248, 116, -2214),
        (-5244, 116, -2218),
        (-5244, 118, -2217),
        (-5250, 114, -2215),
        (-5249, 118, -2213),
        (-5249, 114, -2217),
        (-5250, 110, -2221),
        (-5246, 116, -2220),
        (-5247, 116, -2220),
        (-5245, 118, -2221),
        (-5246, 118, -2222),
        (-5249, 118, -2220),
        (-5242, 115, -2213),
        (-5242, 113, -2216),
        (-5243, 111, -2217),
        (-5242, 117, -2214),
        (-5245, 113, -2216),
        (-5246, 113, -2216),
        (-5243, 111, -2221),
        (-5249, 111, -2217),
        (-5250, 113, -2214),
        (-5246, 113, -2220),
        (-5245, 113, -2222),
        (-5243, 117, -2220),
        (-5248, 115, -2217),
        (-5250, 117, -2214),
        (-5246, 115, -2221),
        (-5247, 117, -2220),
        (-5247, 117, -2221),
        (-5242, 112, -2213),
        (-5246, 110, -2216),
        (-5244, 112, -2217),
        (-5243, 110, -2220),
        (-5242, 116, -2215),
        (-5241, 110, -2222),
        (-5244, 110, -2220),
        (-5243, 118, -2214),
        (-5243, 112, -2221),
        (-5249, 110, -2220),
        (-5250, 116, -2213),
        (-5242, 118, -2220),
        (-5244, 116, -2221),
        (-5249, 114, -2218),
        (-5245, 114, -2222),
        (-5246, 116, -2219),
        (-5246, 114, -2222),
        (-5250, 116, -2217),
    ];

    fn pos(t: (i32, i32, i32)) -> BlockPos {
        BlockPos::new(t.0, t.1, t.2)
    }

    #[test]
    fn iteration_and_pop_order_match_java_hash_set() {
        let mut s = JavaBlockPosSet::new();
        for t in INSERTS {
            s.insert(pos(t));
        }
        let got: Vec<BlockPos> = s.iter().collect();
        let expected: Vec<BlockPos> = ITERATION.iter().copied().map(pos).collect();
        assert_eq!(got, expected);
        let mut popped = Vec::new();
        while let Some(p) = s.pop_first() {
            popped.push(p);
        }
        let expected_pop: Vec<BlockPos> = POP_FIRST.iter().copied().map(pos).collect();
        assert_eq!(popped, expected_pop);
        assert!(s.is_empty());
    }

    // ── tree bins ────────────────────────────────────────────────────────────────────────────────────────────
    //
    // Positions are `(x, 0, 0)` with `0 <= x < 65536` (spread hash == x) unless noted, so `x` alone names them and
    // multiples of 64 share bucket 0 once the table has 64 buckets. Every expected order below was produced by a
    // real `java.util.HashSet` (record with the `Vec3i` hash) and matches the spec's rules.

    fn at(x: i32) -> BlockPos {
        BlockPos::new(x, 0, 0)
    }

    fn set_of(xs: impl IntoIterator<Item = i32>) -> JavaBlockPosSet {
        let mut s = JavaBlockPosSet::new();
        for x in xs {
            assert!(s.insert(at(x)));
        }
        s
    }

    fn order(s: &JavaBlockPosSet) -> Vec<i32> {
        s.iter().map(|p| p.x).collect()
    }

    fn multiples(step: i32, from: i32, to: i32) -> impl Iterator<Item = i32> {
        (from..=to).map(move |i| step * i)
    }

    #[test]
    fn tree_bin_creation_moves_the_root_to_the_front() {
        // The 9th and 10th entry of bucket 0 only grow the table (16 -> 32 -> 64); the 11th builds the tree.
        // Inserting ascending keys leaves key #3 as the root, which moves in front of the untouched chain.
        let mut s = set_of(multiples(64, 0, 8));
        assert_eq!(order(&s), [0, 64, 128, 192, 256, 320, 384, 448, 512]);
        assert!(s.insert(at(64 * 9)));
        assert_eq!(order(&s), [0, 64, 128, 192, 256, 320, 384, 448, 512, 576]);
        assert!(s.insert(at(64 * 10)));
        assert_eq!(
            order(&s),
            [192, 0, 64, 128, 256, 320, 384, 448, 512, 576, 640]
        );
        assert_eq!(s.identity_tie_breaks(), 0);
    }

    #[test]
    fn tree_insert_splices_after_the_tree_parent() {
        let mut s = set_of(multiples(128, 0, 10));
        assert_eq!(
            order(&s),
            [384, 0, 128, 256, 512, 640, 768, 896, 1024, 1152, 1280]
        );
        // 320 sorts between 256 and 512; 64 between 0 and 128; 960 between 896 and 1024 (not appended at the end).
        s.insert(at(320));
        assert_eq!(
            order(&s),
            [384, 0, 128, 256, 320, 512, 640, 768, 896, 1024, 1152, 1280]
        );
        s.insert(at(64));
        s.insert(at(960));
        assert_eq!(
            order(&s),
            [
                384, 0, 64, 128, 256, 320, 512, 640, 768, 896, 1024, 960, 1152, 1280
            ]
        );
        s.insert(at(1344));
        assert_eq!(
            order(&s),
            [
                384, 0, 64, 128, 256, 320, 512, 640, 768, 896, 1024, 960, 1152, 1280, 1344
            ]
        );
        assert!(!s.insert(at(960)));
        assert!(s.contains(at(960)) && !s.contains(at(961)));
    }

    #[test]
    fn new_root_after_rebalancing_moves_to_the_front() {
        // 11 ascending keys, pop the two first chain nodes (no root move on removal), then two more inserts
        // rebalance the tree so a different root has to come to the front.
        let mut s = set_of(multiples(64, 0, 10));
        assert_eq!(s.pop_first(), Some(at(192)));
        assert_eq!(order(&s), [0, 64, 128, 256, 320, 384, 448, 512, 576, 640]);
        assert_eq!(s.pop_first(), Some(at(0)));
        assert_eq!(order(&s), [64, 128, 256, 320, 384, 448, 512, 576, 640]);
        s.insert(at(64 * 11));
        s.insert(at(64 * 12));
        assert_eq!(
            order(&s),
            [448, 64, 128, 256, 320, 384, 512, 576, 640, 704, 768]
        );
        for x in [448, 64, 128] {
            assert_eq!(s.pop_first(), Some(at(x)));
        }
        assert_eq!(order(&s), [256, 320, 384, 512, 576, 640, 704, 768]);
        s.insert(at(5 * 64 + 32 * 64));
        assert_eq!(order(&s), [512, 256, 320, 384, 576, 640, 704, 768, 2368]);
    }

    #[test]
    fn signed_hash_ordering_in_a_tree_bin() {
        // Spread hashes 0x8000_8000 + 64 i are negative as i32 and share bucket 0 with the small keys.
        let mut s = JavaBlockPosSet::new();
        for i in 0..7 {
            s.insert(at(64 * i));
            s.insert(at(i32::MIN + 64 * i));
        }
        assert_eq!(
            order(&s),
            [
                0,
                i32::MIN,
                64,
                i32::MIN + 64,
                128,
                i32::MIN + 128,
                192,
                i32::MIN + 192,
                256,
                i32::MIN + 256,
                i32::MIN + 320,
                i32::MIN + 384,
                320,
                384
            ]
        );
        assert!(s.contains(at(i32::MIN + 320)) && s.contains(at(384)));
        assert_eq!(s.identity_tie_breaks(), 0);
    }

    fn with_fillers(
        bucket_zero: impl IntoIterator<Item = i32>,
        fillers: std::ops::RangeInclusive<i32>,
    ) -> JavaBlockPosSet {
        let mut s = set_of(bucket_zero);
        for x in fillers {
            s.insert(at(x));
        }
        s
    }

    #[test]
    fn resize_untreeifies_halves_with_at_most_six_nodes() {
        // 11 nodes in a 64-bucket tree; the 49th element doubles the table: 6 even and 5 odd multiples of 64.
        // Both halves become plain chains in the old chain order (the tree root 192 lands in the odd half).
        let s = with_fillers(multiples(64, 0, 10), 1..=38);
        let mut want = vec![0, 128, 256, 384, 512, 640];
        want.extend(1..=38);
        want.extend([192, 64, 320, 448, 576]);
        assert_eq!(order(&s), want);
    }

    #[test]
    fn resize_rebuilds_both_halves_when_both_stay_large() {
        // 15 nodes: 8 even + 7 odd multiples of 64, both above 6 -> both re-treeified, each root moves forward.
        let mut s = with_fillers(multiples(64, 0, 14), 1..=34);
        let mut want = vec![384, 0, 128, 256, 512, 640, 768, 896];
        want.extend(1..=34);
        want.extend([192, 64, 320, 448, 576, 704, 832]);
        assert_eq!(order(&s), want);
        // The rebuilt trees decide where the next inserts go.
        s.insert(at(64 * 15));
        s.insert(at(64 * 16));
        let mut want = vec![384, 0, 128, 256, 512, 640, 768, 896, 1024];
        want.extend(1..=34);
        want.extend([448, 192, 64, 320, 576, 704, 832, 960]);
        assert_eq!(order(&s), want);
    }

    #[test]
    fn resize_keeps_the_old_tree_when_the_other_half_is_empty() {
        // Twelve multiples of 128 all stay in the low half: no rebuild, so the root stays in front.
        let mut s = with_fillers(multiples(128, 0, 11), 1..=37);
        let mut want = vec![384, 0, 128, 256, 512, 640, 768, 896, 1024, 1152, 1280, 1408];
        want.extend(1..=37);
        assert_eq!(order(&s), want);
        s.insert(at(128 * 12));
        s.insert(at(128 * 13));
        let mut want = vec![
            384, 0, 128, 256, 512, 640, 768, 896, 1024, 1152, 1280, 1408, 1536, 1664,
        ];
        want.extend(1..=37);
        assert_eq!(order(&s), want);
    }

    #[test]
    fn resize_mixes_a_rebuilt_tree_and_an_untreeified_chain() {
        // 10 low-half nodes stay a tree (rebuilt, because the high half is not empty); 3 high-half nodes untreeify.
        let mut s = with_fillers(multiples(128, 0, 9).chain([64, 192, 320]), 1..=36);
        let mut want = vec![384, 0, 128, 256, 512, 640, 768, 896, 1024, 1152];
        want.extend(1..=36);
        want.extend([320, 192, 64]);
        assert_eq!(order(&s), want);
        s.insert(at(128 * 10));
        s.insert(at(128 * 11));
        s.insert(at(64 + 128 * 3));
        let mut want = vec![384, 0, 128, 256, 512, 640, 768, 896, 1024, 1152, 1280, 1408];
        want.extend(1..=36);
        want.extend([320, 192, 64, 448]);
        assert_eq!(order(&s), want);
    }

    #[test]
    fn a_lone_tree_node_stays_a_tree_node_through_a_resize() {
        // Pop a tree bin down to one node, then resize: the JDK moves that lone node untouched, so it is still a
        // tree node and the following inserts are spliced after their tree parent, not appended.
        let mut s = set_of(multiples(64, 0, 10));
        for _ in 0..10 {
            s.pop_first();
        }
        assert_eq!(order(&s), [640]);
        for x in 1..=48 {
            s.insert(at(x));
        }
        for x in [1280, 128, 256, 384] {
            s.insert(at(x));
        }
        let mut want = vec![640, 128, 256, 384, 1280];
        want.extend(1..=48);
        assert_eq!(order(&s), want);
    }

    #[test]
    fn set_remove_untreeifies_a_small_tree_and_moves_the_root() {
        let mut s = set_of(multiples(64, 0, 10));
        assert!(s.remove(at(64 * 4)));
        assert_eq!(order(&s), [192, 0, 64, 128, 320, 384, 448, 512, 576, 640]);
        assert!(s.remove(at(0)));
        assert_eq!(order(&s), [192, 64, 128, 320, 384, 448, 512, 576, 640]);
        assert!(!s.remove(at(0)) && !s.remove(at(1)));
        for x in [640, 576, 512] {
            assert!(s.remove(at(x)));
        }
        assert_eq!(order(&s), [192, 64, 128, 320, 384, 448]);
        assert!(s.remove(at(448)) && s.remove(at(384)));
        assert_eq!(order(&s), [192, 64, 128, 320]);
        // Now a plain chain again: the next insert appends.
        s.insert(at(64 * 20));
        assert_eq!(order(&s), [192, 64, 128, 320, 1280]);
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn equal_spread_hash_in_a_tree_bin_is_reported() {
        // (x, y, z) and (x + 31, y - 1, z) share a hash code. Nine of them in one bucket of a 64-bucket table are
        // fine; two with an equal hash inside a tree make the order depend on identity hash codes in the JDK.
        let mut s = set_of(multiples(64, 0, 10));
        assert_eq!(s.identity_tie_breaks(), 0);
        assert!(s.insert(BlockPos::new(64 * 3 + 31, -1, 0)));
        assert_eq!(s.identity_tie_breaks(), 1);
        assert!(!s.insert(BlockPos::new(64 * 3 + 31, -1, 0)));
        assert!(s.contains(at(192)) && s.contains(BlockPos::new(64 * 3 + 31, -1, 0)));
    }

    #[test]
    fn pop_everything_from_a_big_dense_set_keeps_len_and_membership() {
        let mut s = JavaBlockPosSet::new();
        let all: Vec<i32> = (0..300).map(|i| i * 64).collect();
        for &x in &all {
            s.insert(at(x));
        }
        assert_eq!(s.len(), 300);
        let mut seen = Vec::new();
        while let Some(p) = s.pop_first() {
            assert!(!s.contains(p));
            seen.push(p.x);
        }
        seen.sort_unstable();
        assert_eq!(seen, all);
        assert!(s.is_empty());
    }
}
