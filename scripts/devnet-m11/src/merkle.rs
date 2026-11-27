//! The payout Merkle tree — the OFF-CHAIN operator's view of a settlement
//! window, built with the EXACT recipe the pool program verifies on-chain
//! (`programs/pool/src/settlement.rs`, module docs) and the pool test
//! harness mirrors (`programs/pool/tests/common/mod.rs`).
//!
//! If this tree ever diverges from the program's verifier, every claim the
//! GLUE submits is refused with `InvalidProof`. So the recipe is copied
//! verbatim and pinned by a known-answer self-check against the same `sha2`
//! the harness uses — two implementations of one published recipe that must
//! agree.
//!
//! * `leaf_hash = SHA-256(depositor_pubkey[32] ‖ amount_le_u64[8])` — 40 bytes.
//! * `node_hash = SHA-256(left[32] ‖ right[32])` — ORDER-SENSITIVE, 64 bytes.
//! * Leaves sit at level 0 in INDEX ORDER; each level pairs left to right;
//!   an odd level pairs its LAST node with itself.
//! * A proof is the sibling hashes from the leaf's level upward, verified by
//!   walking the leaf's index bits (even index → running hash on the LEFT).

use sha2::{Digest, Sha256};

/// One payout leaf: WHO gets paid (a devnet depositor pubkey), and how much
/// USDC in integer micro-units (1 minor unit == 1 micro-USDC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub depositor: [u8; 32],
    pub amount: u64,
}

/// SHA-256 of a byte string, with the harness's own `sha2`.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(data));
    out
}

/// `SHA-256(depositor ‖ amount_le_u64)` — a leaf of the payout tree.
fn leaf_hash(leaf: &Leaf) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(40);
    preimage.extend_from_slice(&leaf.depositor);
    preimage.extend_from_slice(&leaf.amount.to_le_bytes());
    sha256(&preimage)
}

/// `SHA-256(left ‖ right)` — an interior node. Order matters: this is NOT a
/// sorted-pair tree, so the proof is bound to the leaf's INDEX.
fn node_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(64);
    preimage.extend_from_slice(&left);
    preimage.extend_from_slice(&right);
    sha256(&preimage)
}

/// A payout tree: `levels[0]` is the leaf hashes, the last level is the
/// single root. The leaves are kept so the GLUE reads each claim's amount
/// and depositor off the tree it posted — never off a second copy.
pub struct Tree {
    leaves: Vec<Leaf>,
    levels: Vec<Vec<[u8; 32]>>,
}

impl Tree {
    /// Builds every level above the leaves, in index order.
    pub fn new(leaves: &[Leaf]) -> Self {
        assert!(!leaves.is_empty(), "a tree needs at least one leaf");
        let mut levels = vec![leaves.iter().map(leaf_hash).collect::<Vec<_>>()];
        while levels.last().expect("at least the leaf level").len() > 1 {
            let prev = levels.last().expect("at least the leaf level");
            let mut next = Vec::with_capacity(prev.len().div_ceil(2));
            for pair in prev.chunks(2) {
                // An unpaired last node is its own sibling.
                next.push(node_hash(pair[0], *pair.last().expect("non-empty chunk")));
            }
            levels.push(next);
        }
        Tree {
            leaves: leaves.to_vec(),
            levels,
        }
    }

    /// Leaf `index`, as it was built.
    pub fn leaf(&self, index: usize) -> Leaf {
        self.leaves[index]
    }

    /// The sum of the leaf amounts — the `total_payout` the root attests to.
    pub fn total(&self) -> u64 {
        self.leaves.iter().map(|leaf| leaf.amount).sum()
    }

    /// The root to hand to `post_root`.
    pub fn root(&self) -> [u8; 32] {
        self.levels.last().expect("at least the leaf level")[0]
    }

    /// The inclusion proof for the leaf at `index`: one sibling per level,
    /// from the leaf's level upward.
    pub fn proof(&self, index: usize) -> Vec<[u8; 32]> {
        let top = self.levels.len() - 1;
        let mut proof = Vec::with_capacity(top);
        let mut at = index;
        for level in &self.levels[..top] {
            // The pair partner: 2k pairs with 2k+1 and vice versa.
            let partner = at ^ 1;
            // An odd level's last node pairs with itself.
            proof.push(level[partner.min(level.len() - 1)]);
            at /= 2;
        }
        proof
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The proof must walk the index bits exactly like the on-chain
    /// verifier: an even index puts the running hash on the LEFT of its
    /// sibling, an odd one on the right. Checked by re-deriving the root.
    fn apply_proof(leaf_index: u32, mut h: [u8; 32], proof: &[[u8; 32]]) -> [u8; 32] {
        let mut at = leaf_index;
        for sibling in proof {
            h = if at.is_multiple_of(2) {
                node_hash(h, *sibling)
            } else {
                node_hash(*sibling, h)
            };
            at /= 2;
        }
        h
    }

    #[test]
    fn every_leafs_proof_reaches_the_root_for_all_tree_shapes() {
        // Trees of size 1..=9: even, odd, and the odd-pairs-itself edges.
        for n in 1..=9usize {
            let leaves: Vec<Leaf> = (0..n)
                .map(|i| Leaf {
                    depositor: [i as u8 + 1; 32],
                    amount: (i as u64 + 1) * 1_000_000,
                })
                .collect();
            let tree = Tree::new(&leaves);
            for (i, leaf) in leaves.iter().enumerate() {
                let proof = tree.proof(i);
                assert_eq!(
                    apply_proof(i as u32, leaf_hash(leaf), &proof),
                    tree.root(),
                    "leaf {i} of a {n}-leaf tree must verify against the root"
                );
            }
        }
    }

    #[test]
    fn a_wrong_index_or_amount_fails_verification() {
        let leaves: Vec<Leaf> = (0..4)
            .map(|i| Leaf {
                depositor: [i as u8 + 1; 32],
                amount: (i as u64 + 1) * 1_000_000,
            })
            .collect();
        let tree = Tree::new(&leaves);
        // Leaf 0's content under leaf 1's slot must NOT verify (index-bound).
        assert_ne!(
            apply_proof(1, leaf_hash(&leaves[0]), &tree.proof(0)),
            tree.root()
        );
        // A tampered amount must NOT verify (the leaf hashes the amount).
        let mut tampered = leaves[1];
        tampered.amount += 1;
        assert_ne!(
            apply_proof(1, leaf_hash(&tampered), &tree.proof(1)),
            tree.root()
        );
    }
}
