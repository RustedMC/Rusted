//! Java collection behaviour PNX's mangrove relies on: `HashSet<BlockVector3>` iteration order
//! (including bin treeification) and `java.util.Random` for `Collections.shuffle`.

use std::collections::HashSet;

pub(crate) type P = (i32, i32, i32);

const NIL: usize = usize::MAX;

struct Node {
    p: P,
    h: i32,
    left: usize,
    right: usize,
    parent: usize,
    red: bool,
}

#[derive(Default)]
struct Bin {
    /// Nodes in `next` order.
    list: Vec<usize>,
    tree: bool,
}

/// `java.util.HashSet<BlockVector3>` as far as insertion and iteration order go.
#[derive(Default)]
pub(crate) struct JavaHashSet {
    bins: Vec<Bin>,
    nodes: Vec<Node>,
    seen: HashSet<P>,
}

/// BlockVector3.hashCode, spread like HashMap.hash.
fn hash(p: P) -> i32 {
    let h = p.0 ^ (p.2 << 12) ^ (p.1 << 24);
    h ^ ((h as u32) >> 16) as i32
}

impl JavaHashSet {
    pub(crate) fn contains(&self, p: P) -> bool {
        self.seen.contains(&p)
    }

    pub(crate) fn insert(&mut self, p: P) {
        if !self.seen.insert(p) {
            return;
        }
        if self.bins.is_empty() {
            self.resize();
        }
        let h = hash(p);
        let x = self.nodes.len();
        self.nodes.push(Node { p, h, left: NIL, right: NIL, parent: NIL, red: false });
        let i = (h as usize) & (self.bins.len() - 1);
        if self.bins[i].list.is_empty() {
            self.bins[i].list.push(x);
        } else if self.bins[i].tree {
            self.put_tree_val(i, x);
        } else {
            let n = self.bins[i].list.len();
            self.bins[i].list.push(x);
            if n >= 8 {
                if self.bins.len() < 64 {
                    self.resize();
                } else {
                    self.treeify(i);
                }
            }
        }
        if self.seen.len() > self.bins.len() * 3 / 4 {
            self.resize();
        }
    }

    /// Elements in Java iteration order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = P> + '_ {
        self.bins.iter().flat_map(|b| b.list.iter().map(|&n| self.nodes[n].p))
    }

    fn resize(&mut self) {
        let old = self.bins.len();
        if old == 0 {
            self.bins = (0..16).map(|_| Bin::default()).collect();
            return;
        }
        let mut bins: Vec<Bin> = (0..old * 2).map(|_| Bin::default()).collect();
        let old_bins = std::mem::take(&mut self.bins);
        for (j, b) in old_bins.into_iter().enumerate() {
            let (lo, hi): (Vec<usize>, Vec<usize>) = b.list.iter().partition(|&&n| self.nodes[n].h as usize & old == 0);
            let (lo_empty, hi_empty) = (lo.is_empty(), hi.is_empty());
            bins[j].list = lo;
            bins[j + old].list = hi;
            if b.tree {
                for (k, other_empty) in [(j, hi_empty), (j + old, lo_empty)] {
                    if bins[k].list.is_empty() {
                        continue;
                    }
                    if bins[k].list.len() <= 6 {
                        bins[k].tree = false;
                    } else {
                        bins[k].tree = true;
                        if !other_empty {
                            self.bins = std::mem::take(&mut bins);
                            self.treeify(k);
                            bins = std::mem::take(&mut self.bins);
                        }
                    }
                }
            }
        }
        self.bins = bins;
    }

    fn treeify(&mut self, i: usize) {
        let list = self.bins[i].list.clone();
        for &n in &list {
            let nd = &mut self.nodes[n];
            (nd.left, nd.right, nd.parent) = (NIL, NIL, NIL);
        }
        let mut root = NIL;
        for &x in &list {
            if root == NIL {
                self.nodes[x].red = false;
                root = x;
                continue;
            }
            self.attach(root, x);
            root = self.balance_insertion(root, x);
        }
        self.bins[i].tree = true;
        self.move_root_to_front(i, root);
    }

    /// Walks from `root` by hash and hangs `x` under the leaf it reaches; returns the parent.
    fn attach(&mut self, root: usize, x: usize) -> usize {
        let h = self.nodes[x].h;
        let mut p = root;
        loop {
            // TODO: equal hashes use Java's identity tie-break (random there too), we just go left.
            let left = self.nodes[p].h >= h;
            let next = if left { self.nodes[p].left } else { self.nodes[p].right };
            if next == NIL {
                self.nodes[x].parent = p;
                if left {
                    self.nodes[p].left = x;
                } else {
                    self.nodes[p].right = x;
                }
                return p;
            }
            p = next;
        }
    }

    fn put_tree_val(&mut self, i: usize, x: usize) {
        let root = self.bins[i].list[0];
        let xp = self.attach(root, x);
        let pos = self.bins[i].list.iter().position(|&n| n == xp).unwrap();
        self.bins[i].list.insert(pos + 1, x);
        let root = self.balance_insertion(root, x);
        self.move_root_to_front(i, root);
    }

    fn move_root_to_front(&mut self, i: usize, root: usize) {
        let l = &mut self.bins[i].list;
        if l[0] != root {
            let pos = l.iter().position(|&n| n == root).unwrap();
            l.remove(pos);
            l.insert(0, root);
        }
    }

    fn red(&self, n: usize) -> bool {
        n != NIL && self.nodes[n].red
    }

    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let r = if p == NIL { NIL } else { self.nodes[p].right };
        if r != NIL {
            let rl = self.nodes[r].left;
            self.nodes[p].right = rl;
            if rl != NIL {
                self.nodes[rl].parent = p;
            }
            let pp = self.nodes[p].parent;
            self.nodes[r].parent = pp;
            if pp == NIL {
                root = r;
                self.nodes[r].red = false;
            } else if self.nodes[pp].left == p {
                self.nodes[pp].left = r;
            } else {
                self.nodes[pp].right = r;
            }
            self.nodes[r].left = p;
            self.nodes[p].parent = r;
        }
        root
    }

    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let l = if p == NIL { NIL } else { self.nodes[p].left };
        if l != NIL {
            let lr = self.nodes[l].right;
            self.nodes[p].left = lr;
            if lr != NIL {
                self.nodes[lr].parent = p;
            }
            let pp = self.nodes[p].parent;
            self.nodes[l].parent = pp;
            if pp == NIL {
                root = l;
                self.nodes[l].red = false;
            } else if self.nodes[pp].right == p {
                self.nodes[pp].right = l;
            } else {
                self.nodes[pp].left = l;
            }
            self.nodes[l].right = p;
            self.nodes[p].parent = l;
        }
        root
    }

    /// HashMap.TreeNode.balanceInsertion.
    fn balance_insertion(&mut self, mut root: usize, mut x: usize) -> usize {
        self.nodes[x].red = true;
        loop {
            let mut xp = self.nodes[x].parent;
            if xp == NIL {
                self.nodes[x].red = false;
                return x;
            }
            let mut xpp = self.nodes[xp].parent;
            if !self.nodes[xp].red || xpp == NIL {
                return root;
            }
            let xppl = self.nodes[xpp].left;
            if xp == xppl {
                let xppr = self.nodes[xpp].right;
                if self.red(xppr) {
                    self.nodes[xppr].red = false;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp].right {
                        x = xp;
                        root = self.rotate_left(root, x);
                        xp = self.nodes[x].parent;
                        xpp = if xp == NIL { NIL } else { self.nodes[xp].parent };
                    }
                    if xp != NIL {
                        self.nodes[xp].red = false;
                        if xpp != NIL {
                            self.nodes[xpp].red = true;
                            root = self.rotate_right(root, xpp);
                        }
                    }
                }
            } else if self.red(xppl) {
                self.nodes[xppl].red = false;
                self.nodes[xp].red = false;
                self.nodes[xpp].red = true;
                x = xpp;
            } else {
                if x == self.nodes[xp].left {
                    x = xp;
                    root = self.rotate_right(root, x);
                    xp = self.nodes[x].parent;
                    xpp = if xp == NIL { NIL } else { self.nodes[xp].parent };
                }
                if xp != NIL {
                    self.nodes[xp].red = false;
                    if xpp != NIL {
                        self.nodes[xpp].red = true;
                        root = self.rotate_left(root, xpp);
                    }
                }
            }
        }
    }
}

/// `java.util.Random` (only what `Collections.shuffle` uses).
pub(crate) struct JavaRandom(i64);

const MULT: i64 = 0x5DEECE66D;
const MASK: i64 = (1 << 48) - 1;

impl JavaRandom {
    pub(crate) fn new(seed: i64) -> Self {
        Self((seed ^ MULT) & MASK)
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(MULT).wrapping_add(0xB)) & MASK;
        (self.0 >> (48 - bits)) as i32
    }
    pub(crate) fn next_int(&mut self, bound: i32) -> i32 {
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            return ((bound as i64 * r as i64) >> 31) as i32;
        }
        let mut u = r;
        loop {
            r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) < 0 {
                u = self.next(31);
            } else {
                return r;
            }
        }
    }
    /// Collections.shuffle(list, this).
    pub(crate) fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (2..=v.len()).rev() {
            let j = self.next_int(i as i32) as usize;
            v.swap(i - 1, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_jdk() {
        // new Random(42).nextInt(10) x3 = 0, 3, 8 in the JDK.
        let mut r = JavaRandom::new(42);
        assert_eq!([r.next_int(10), r.next_int(10), r.next_int(10)], [0, 3, 8]);
    }

    #[test]
    fn hashset_small_order_is_bucket_order() {
        let mut s = JavaHashSet::default();
        for p in [(3, 0, 0), (1, 0, 0), (2, 0, 0)] {
            s.insert(p);
        }
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![(1, 0, 0), (2, 0, 0), (3, 0, 0)]);
        // Many colliding keys force resizes and treeified bins; every element must survive once.
        let mut s = JavaHashSet::default();
        for x in 0..5 {
            for y in 60..63 {
                for z in 0..5 {
                    s.insert((x, y, z));
                }
            }
        }
        let mut v: Vec<_> = s.iter().collect();
        assert_eq!(v.len(), 75);
        v.sort();
        v.dedup();
        assert_eq!(v.len(), 75);
    }
}
