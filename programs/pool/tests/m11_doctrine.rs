//! M11 DOCTRINE SUITE — Session A (doctrine-suite mode, workflow step 14),
//! 2026-11-26; revised at the GO gate the same day (rulings R1–R4, which
//! added sentences 11 and 12). TWELVE tests, one per doctrine sentence, each
//! NAMED as its sentence: the test names ARE the spec the founder and director read at
//! the GO gate, and Session B implements the stubs to green WITHOUT editing
//! a line of this file (a test that must change is a doctrine question and
//! routes back through the user — rule 6, extended).
//!
//! # The doctrine, restated
//!
//! | # | The sentence | What it means operationally |
//! |---|---|---|
//! | 1 | A root whose total payout exceeds the vault's USDC fails closed and moves nothing. | `post_root` checks `total_payout` against the vault balance BEFORE writing anything; over by one micro-unit ⇒ `RootExceedsVault`, no root account, no balance changed. |
//! | 2 | A claim whose inclusion proof does not verify against the published root is rejected. | Another leaf's proof, a tampered amount, a proof from another tree, a flipped byte — all ⇒ `InvalidProof`, nothing paid. |
//! | 3 | One leaf funds exactly one claim; a replayed claim is refused. | The claim marker is a plain-`init` PDA keyed by root + leaf index: born once, EVER, whichever door retries; and the proof is bound to its index, so a leaf cannot return through a fresh marker. |
//! | 4 | A payout reaches only the leaf's named depositor, through either door. | Backend (assisted) or the depositor's own key (direct) may open the door; the destination's owner is an ACCOUNT CONSTRAINT; the leaf hash binds the depositor's pubkey. |
//! | 5 | A challenge inside the window freezes that root's payouts until the multisig rules. | Any key challenges inside the window ⇒ status `Challenged` ⇒ `RootFrozen` for claims; only the admin resolves; "stands" unfreezes; after the window a challenge is `ChallengeWindowClosed`. |
//! | 6 | The bond slashes by exactly the ruled amount for a root proven false, and by nothing otherwise. | A `RootFalse` ruling records amount + victim; `slash_bond` moves EXACTLY that; every other state ⇒ `RootNotProvenFalse`; an overstated ruling ⇒ `SlashExceedsBond`. |
//! | 7 | A second root for the same window is refused; roots are append-only. | The root is a plain-`init` PDA keyed by window: a repost dies at account creation and the first root stays byte-identical; the next window appends. |
//! | 8 | Every state change is reconstructible from emitted events alone. | A subscriber fed ONLY decoded `Program data:` events ends with a mirror equal to the chain: vault, bond, every root field, every marker. |
//! | 9 | Settlement mints no transferable receipt. | Through the whole lifecycle: registry supply, the depositor's claim row and the USDC supply do not move; the claim mint stays NonTransferable; the payout is USDC, exactly. |
//! | 10 | No single key and no upgrade authority can move vault funds; only a satisfied program instruction can. | Raw transfers signed by the backend, the admin and the deployment (upgrade) authority key all refuse; no sweep instruction exists; a VALID claim is what moves the vault. |
//! | 11 | No payout leaves the vault before the challenge window has passed. | A claim succeeds iff the root is `Active` AND (the window has passed OR the multisig ruled it stands); inside the window on an Active root ⇒ `ChallengeWindowOpen`, nothing moves. *(GO-gate ruling.)* |
//! | 12 | A root never pays out more than its declared total. | The root carries `paid_out`; a claim needs `paid_out + amount ≤ total_payout`, else `PayoutExceedsTotal`. *(GO-gate ruling.)* |
//!
//! # Sentence 11 shapes the other tests too
//!
//! Before its first successful claim, each of tests 2, 3, 4, 7, 8, 9 and 10
//! moves the clock past the challenge window (one Fixture helper). Tests 5
//! and 8 also use the other door through the window: the multisig's "stands"
//! ruling, which waives the rest of it. Test 5's opening control IS the
//! refusal. Tests 1 and 6 make no claims. A root posted AFTER a clock move
//! starts a window of its own, so it can still be challenged.
//!
//! # RED for the right reason
//!
//! Every settlement handler is `todo!()` on this branch, so a test that
//! merely expected "it fails" would pass by accident. Nothing here does.
//! Each test pins outcomes: a step that MUST LAND (a success, or state only
//! a real instruction can produce) or an EXACT error code. A refusal the
//! program itself never answers cannot produce `Some(6012..=6018)`, and an
//! aborted stub cannot land a success. The settlement dependency chain
//! starts at `fund_bond` (a root needs a bond), so on the stub branch each
//! test goes red at its first "must land" settlement step — and the panic
//! message names the sentence and the step.
//!
//! # Two refusals are STRUCTURAL, on purpose
//!
//! Invariants 3 and 7 are enforced by ADDRESSES: a marker (or root) PDA
//! created with plain `init` cannot be created twice, so the refusal comes
//! from account creation — before any handler logic runs — and its error
//! code is the runtime's, not a named `PoolError`. Those legs pin the STATE
//! (exactly one payout; the first root byte-identical) and require the
//! refusal NOT to be one of the program's handler codes: a handler check is
//! a weaker guarantee than the sentence states.
//!
//! # The contract in one table (what Session B implements)
//!
//! | Instruction | Signer | Refuses with |
//! |---|---|---|
//! | `fund_bond(amount)` | backend | — |
//! | `post_root(window, merkle_root, total_payout)` | backend | `RootExceedsVault` (6012) |
//! | `claim(window, leaf_index, depositor, amount, proof)` | backend OR `depositor` | `WrongPayoutDestination` (6014, account constraint), `ExitAuthority` (6009), `InvalidProof` (6013), `RootFrozen` (6015), `ChallengeWindowOpen` (6019), `PayoutExceedsTotal` (6020) |
//! | `challenge(window, evidence)` | ANY key | `ChallengeWindowClosed` (6016) |
//! | `resolve_challenge(window, ruling)` | admin (multisig) | `SlashExceedsBond` (6018) |
//! | `slash_bond(window)` | none (cranks a ruling) | `RootNotProvenFalse` (6017) |
//!
//! Merkle recipe, seeds, account layouts and events: `programs/pool/src/
//! settlement.rs` (module docs). Instruction wire shapes and event layouts
//! are pinned on the host by that module's unit tests.

mod common;

use common::{addrs, events, ixs, merkle, start_with_context};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::{BanksClient, BanksClientError, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use std::future::Future;

// ---------------------------------------------------------------------------
// Constants — micro-USDC and the error codes this suite pins. The codes are
// LITERALS on purpose (house style: the suites pin Anchor's 6000 + variant
// index), and the const block ties each literal to its variant, so a
// reordered enum is a compile error here — never a silent shift.
// ---------------------------------------------------------------------------

/// One USDC, in integer micro-units (1 minor unit == 1 micro-USDC).
const USDC: u64 = 1_000_000;

const EXIT_AUTHORITY: u32 = 6009;
const WRONG_DESTINATION_OWNER: u32 = 6005;
const ROOT_EXCEEDS_VAULT: u32 = 6012;
const INVALID_PROOF: u32 = 6013;
const WRONG_PAYOUT_DESTINATION: u32 = 6014;
const ROOT_FROZEN: u32 = 6015;
const CHALLENGE_WINDOW_CLOSED: u32 = 6016;
const ROOT_NOT_PROVEN_FALSE: u32 = 6017;
const SLASH_EXCEEDS_BOND: u32 = 6018;
const CHALLENGE_WINDOW_OPEN: u32 = 6019;
const PAYOUT_EXCEEDS_TOTAL: u32 = 6020;
/// The highest `PoolError` code this branch declares.
const LAST_POOL_ERROR: u32 = PAYOUT_EXCEEDS_TOTAL;

/// Anchor's framework code for an instruction name the dispatcher does not
/// know — a public constant of the `ErrorCode` enum.
const ANCHOR_INSTRUCTION_FALLBACK_NOT_FOUND: u32 = 101;
/// SPL Token's `TokenError::OwnerMismatch` — variant 4, a stable public
/// constant of the Token program (spl-token-interface 2.0.0 `error.rs`).
const TOKEN_OWNER_MISMATCH: u32 = 4;

const _: () = {
    use pool::PoolError as E;
    assert!(E::ExitAuthority as u32 + 6000 == EXIT_AUTHORITY);
    assert!(E::WrongDestinationOwner as u32 + 6000 == WRONG_DESTINATION_OWNER);
    assert!(E::RootExceedsVault as u32 + 6000 == ROOT_EXCEEDS_VAULT);
    assert!(E::InvalidProof as u32 + 6000 == INVALID_PROOF);
    assert!(E::WrongPayoutDestination as u32 + 6000 == WRONG_PAYOUT_DESTINATION);
    assert!(E::RootFrozen as u32 + 6000 == ROOT_FROZEN);
    assert!(E::ChallengeWindowClosed as u32 + 6000 == CHALLENGE_WINDOW_CLOSED);
    assert!(E::RootNotProvenFalse as u32 + 6000 == ROOT_NOT_PROVEN_FALSE);
    assert!(E::SlashExceedsBond as u32 + 6000 == SLASH_EXCEEDS_BOND);
    assert!(E::ChallengeWindowOpen as u32 + 6000 == CHALLENGE_WINDOW_OPEN);
    assert!(E::PayoutExceedsTotal as u32 + 6000 == PAYOUT_EXCEEDS_TOTAL);
};

// ---------------------------------------------------------------------------
// Outcome helpers — the two shapes every leg of every test takes.
// ---------------------------------------------------------------------------

/// A step that MUST LAND. On the stub branch this is where each test goes
/// red; the panic names the sentence and the step, so the CI log reads as
/// doctrine rather than as a stack trace.
async fn lands<T>(
    label: &str,
    step: &str,
    outcome: impl Future<Output = Result<T, BanksClientError>>,
) -> T {
    outcome
        .await
        .unwrap_or_else(|e| panic!("{label} — `{step}` must land: {e}"))
}

/// A step that MUST BE REFUSED; returns the bank's error so the caller can
/// pin WHICH refusal (an exact code, or — for the structural refusals —
/// "not one of the handler's").
async fn refused(
    label: &str,
    step: &str,
    outcome: impl Future<Output = Result<(), BanksClientError>>,
) -> BanksClientError {
    outcome
        .await
        .expect_err(&format!("{label} — `{step}` must be REFUSED"))
}

/// Pins a refusal to an EXACT custom error code.
fn assert_code(label: &str, step: &str, err: &BanksClientError, code: u32) {
    assert_eq!(
        common::custom_code(err),
        Some(code),
        "{label} — `{step}` must refuse with custom program error {code}"
    );
}

/// A refusal that must come from ACCOUNT CREATION — a plain-`init` PDA that
/// already exists. Its code is the runtime's, never one of the program's
/// handler checks: invariants 3 and 7 are guaranteed by an address, which
/// no handler logic can weaken.
fn assert_structural_refusal(label: &str, step: &str, err: &BanksClientError) {
    assert!(
        common::custom_code(err).is_none_or(|code| !(6000..=LAST_POOL_ERROR).contains(&code)),
        "{label} — `{step}` must be refused by ACCOUNT CREATION, not by a handler check (got {err})"
    );
}

// ---------------------------------------------------------------------------
// Fixture: one initialized pool and the identities the doctrine names.
// ---------------------------------------------------------------------------

/// An initialized pool with every identity the settlement doctrine needs.
/// The fixture uses ONLY M8 instructions, so building it never goes red on
/// the M11 stubs: each test's first settlement call is explicit.
struct Fixture {
    /// The validator context — kept whole for `warp_to_slot` (invariant 5).
    ctx: ProgramTestContext,
    /// The admin — the multisig's stand-in (a plain key in tests).
    admin: Keypair,
    /// The operator's hot key: bonds, posts roots, opens the assisted door.
    backend: Keypair,
    /// A key that never holds any role — the attacker, the challenger.
    rogue: Keypair,
    renter: Keypair,
    renter_source: Pubkey,
    /// Four depositor identities A..D. Leaf `i` of every tree below names
    /// `depositors[i]`.
    depositors: [Keypair; 4],
    /// A USDC wallet owned by each depositor — where payouts may land.
    dep_wallets: [Pubkey; 4],
    /// The operator's own USDC wallet (bond money comes from here).
    backend_wallet: Pubkey,
    /// A USDC wallet owned by the rogue — where payouts must never land.
    rogue_wallet: Pubkey,
    /// The identity a `RootFalse` ruling names as victim, and their wallet.
    victim: Keypair,
    victim_wallet: Pubkey,
    usdc_mint: Pubkey,
}

impl Fixture {
    fn banks(&self) -> &BanksClient {
        &self.ctx.banks_client
    }

    /// Sends ONE instruction as its own transaction (fee payer = the rig's
    /// funded key, which is also the claim-marker rent payer).
    async fn send(&self, ix: Instruction, signers: &[&Keypair]) -> Result<(), BanksClientError> {
        common::send(self.banks(), &self.ctx.payer, vec![ix], signers).await
    }

    /// Vault USDC.
    async fn vault(&self) -> u64 {
        common::token_balance(self.banks(), &addrs::vault()).await
    }

    /// USDC of any token account.
    async fn wallet(&self, account: &Pubkey) -> u64 {
        common::token_balance(self.banks(), account).await
    }

    /// The operator bond record.
    async fn bond(&self) -> u64 {
        common::bond_amount(self.banks()).await
    }

    /// A posted root, which must exist.
    async fn root(&self, window: u64) -> pool::SettlementRoot {
        common::settlement_root(self.banks(), window)
            .await
            .unwrap_or_else(|| panic!("root for window {window} must exist"))
    }

    /// Leaf `index` of a tree: it names depositor `index` and pays `amount`.
    fn leaf(&self, index: usize, amount: u64) -> merkle::Leaf {
        merkle::Leaf {
            depositor: self.depositors[index].pubkey(),
            amount,
        }
    }

    // ---- instruction builders (each mirrors its Accounts struct) --------

    /// `prepay_renter` — the ONLY USDC inflow (D40), an M8 instruction.
    fn prepay_ix(&self, amount: u64) -> Instruction {
        ixs::prepay_renter(
            &addrs::pool(),
            &self.backend.pubkey(),
            &self.renter.pubkey(),
            &self.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&self.renter.pubkey()),
            &self.renter.pubkey(),
            amount,
        )
    }

    fn fund_bond_ix(&self, amount: u64) -> Instruction {
        ixs::fund_bond(
            &addrs::pool(),
            &self.backend.pubkey(),
            &self.backend_wallet,
            &self.usdc_mint,
            &addrs::bond_vault(),
            &addrs::bond(),
            amount,
        )
    }

    /// `post_root` carrying `declared_total` — what the OPERATOR attests the
    /// root pays. The honest figure is `tree.total()` (invariant 12 makes
    /// the declared total, not the leaves, the ceiling).
    fn post_root_ix(&self, window: u64, tree: &merkle::Tree, declared_total: u64) -> Instruction {
        ixs::post_root(
            &addrs::pool(),
            &self.backend.pubkey(),
            &addrs::root(window),
            &addrs::vault(),
            &addrs::bond(),
            window,
            tree.root(),
            declared_total,
        )
    }

    /// The claim-marker rent payer is the rig's funded key, so the DIRECT
    /// door needs nothing from the depositor but a signature.
    #[allow(clippy::too_many_arguments)]
    fn claim_ix(
        &self,
        authority: &Pubkey,
        window: u64,
        leaf_index: u32,
        depositor: &Pubkey,
        destination: &Pubkey,
        amount: u64,
        proof: &[[u8; 32]],
    ) -> Instruction {
        ixs::claim(
            &addrs::pool(),
            authority,
            &addrs::root(window),
            destination,
            &addrs::vault(),
            &addrs::claim_marker(&addrs::root(window), leaf_index),
            &self.ctx.payer.pubkey(),
            window,
            leaf_index,
            depositor,
            amount,
            proof,
        )
    }

    fn challenge_ix(&self, challenger: &Pubkey, window: u64, evidence: [u8; 32]) -> Instruction {
        ixs::challenge(
            &addrs::pool(),
            challenger,
            &addrs::root(window),
            window,
            evidence,
        )
    }

    fn resolve_ix(&self, window: u64, ruling: ixs::Ruling<'_>) -> Instruction {
        ixs::resolve_challenge(
            &addrs::pool(),
            &self.admin.pubkey(),
            &addrs::root(window),
            &addrs::bond(),
            window,
            ruling,
        )
    }

    fn slash_ix(&self, window: u64, destination: &Pubkey) -> Instruction {
        ixs::slash_bond(
            &addrs::pool(),
            &addrs::root(window),
            &addrs::bond_vault(),
            &addrs::bond(),
            destination,
            window,
        )
    }

    // ---- the raw results tests wrap in `lands` / `refused` --------------

    /// An HONEST claim: `tree`'s leaf `leaf`, its own proof and amount, the
    /// leaf's named depositor, opened by `authority` (either door).
    async fn claim(
        &self,
        authority: &Keypair,
        window: u64,
        tree: &merkle::Tree,
        leaf: usize,
        destination: &Pubkey,
    ) -> Result<(), BanksClientError> {
        let named = tree.leaf(leaf);
        self.claim_forged(
            authority,
            window,
            leaf as u32,
            &named.depositor,
            destination,
            named.amount,
            &tree.proof(leaf),
        )
        .await
    }

    /// A claim with EVERY argument chosen by the caller — the shape a
    /// forger uses, and the shape the honest path reduces to.
    #[allow(clippy::too_many_arguments)]
    async fn claim_forged(
        &self,
        authority: &Keypair,
        window: u64,
        leaf_index: u32,
        depositor: &Pubkey,
        destination: &Pubkey,
        amount: u64,
        proof: &[[u8; 32]],
    ) -> Result<(), BanksClientError> {
        self.send(
            self.claim_ix(
                &authority.pubkey(),
                window,
                leaf_index,
                depositor,
                destination,
                amount,
                proof,
            ),
            &[authority],
        )
        .await
    }

    /// `slash_bond(window)` paying `destination` — no signer: it cranks a
    /// ruling that already exists.
    async fn slash(&self, window: u64, destination: &Pubkey) -> Result<(), BanksClientError> {
        self.send(self.slash_ix(window, destination), &[]).await
    }

    // ---- "must land" compound steps ----

    /// Renter money into escrow — funds the vault the root will pay from.
    async fn prepay(&self, label: &str, amount: u64) {
        lands(
            label,
            &format!(
                "fixture: renter prepay of {} USDC (the only inflow)",
                amount / USDC
            ),
            self.send(self.prepay_ix(amount), &[&self.backend, &self.renter]),
        )
        .await;
    }

    /// Bonds `amount` from the operator's own wallet.
    async fn fund_bond(&self, label: &str, amount: u64) {
        lands(
            label,
            &format!(
                "fund_bond {} USDC (a bonded operator backs its roots)",
                amount / USDC
            ),
            self.send(self.fund_bond_ix(amount), &[&self.backend]),
        )
        .await;
    }

    /// Builds the tree over `leaves` and posts its root for `window`,
    /// declaring the honest sum as the total; the returned tree is where
    /// proofs come from.
    async fn post(&self, label: &str, window: u64, leaves: &[merkle::Leaf]) -> merkle::Tree {
        let tree = merkle::Tree::new(leaves);
        self.post_declaring(label, window, &tree, tree.total())
            .await;
        tree
    }

    /// Posts `tree`'s root for `window` while DECLARING `declared_total` as
    /// what it pays — for a root whose leaves sum past its own total
    /// (invariant 12).
    async fn post_declaring(
        &self,
        label: &str,
        window: u64,
        tree: &merkle::Tree,
        declared_total: u64,
    ) {
        lands(
            label,
            &format!(
                "post_root for window {window} ({} leaves summing to {} micro-USDC, declaring {})",
                tree.leaf_count(),
                tree.total(),
                declared_total
            ),
            self.send(
                self.post_root_ix(window, tree, declared_total),
                &[&self.backend],
            ),
        )
        .await;
    }

    /// Moves the clock PAST the challenge window of `window`'s root
    /// (invariant 11): from here on a claim against a root posted BEFORE
    /// this call may pay. Not a program instruction — a clock move the rig
    /// performs between steps.
    async fn pass_challenge_window(&mut self, window: u64) {
        let posted_at = self.root(window).await.posted_at;
        common::warp_past_window(&mut self.ctx, posted_at);
    }

    /// `challenge(window)` by ANY key, with a commitment to a fraud proof.
    async fn challenge(&self, label: &str, who: &Keypair, window: u64) {
        lands(
            label,
            &format!("challenge window {window} (inside the window)"),
            self.send(self.challenge_ix(&who.pubkey(), window, EVIDENCE), &[who]),
        )
        .await;
    }

    /// `resolve_challenge(window, ruling)` by the multisig.
    async fn resolve(&self, label: &str, step: &str, window: u64, ruling: ixs::Ruling<'_>) {
        lands(
            label,
            step,
            self.send(self.resolve_ix(window, ruling), &[&self.admin]),
        )
        .await;
    }
}

/// A challenger's commitment to their off-chain fraud proof — recorded on
/// the root and in the event, never verified on-chain (the chain cannot
/// read the books; the multisig does).
const EVIDENCE: [u8; 32] = [0xE7; 32];

async fn fixture() -> Fixture {
    let ctx = start_with_context().await;
    let banks = &ctx.banks_client;
    let payer = &ctx.payer;
    let usdc_mint = common::create_usdc_mint(banks, payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let rogue = Keypair::new();
    let renter = Keypair::new();
    let renter_source =
        common::create_token_account(banks, payer, &renter.pubkey(), &usdc_mint).await;
    common::faucet_usdc(banks, payer, &usdc_mint, &renter_source, 500 * USDC).await;
    let depositors = [
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
    ];
    let mut dep_wallets = [Pubkey::default(); 4];
    for (index, depositor) in depositors.iter().enumerate() {
        dep_wallets[index] =
            common::create_token_account(banks, payer, &depositor.pubkey(), &usdc_mint).await;
    }
    let backend_wallet =
        common::create_token_account(banks, payer, &backend.pubkey(), &usdc_mint).await;
    common::faucet_usdc(banks, payer, &usdc_mint, &backend_wallet, 100 * USDC).await;
    let rogue_wallet =
        common::create_token_account(banks, payer, &rogue.pubkey(), &usdc_mint).await;
    let victim = Keypair::new();
    let victim_wallet =
        common::create_token_account(banks, payer, &victim.pubkey(), &usdc_mint).await;
    // The backend pays rent for the roots and the bond records.
    common::fund(banks, payer, &backend.pubkey(), 5_000_000_000).await;
    // A-041: initialize is co-signed by the rig's deployment authority.
    common::send(
        banks,
        payer,
        vec![ixs::initialize(
            payer,
            &usdc_mint,
            &admin.pubkey(),
            &backend.pubkey(),
        )],
        &[common::deploy_authority()],
    )
    .await
    .expect("fixture: initialize lands (M8, deployment-authority co-signed)");
    Fixture {
        ctx,
        admin,
        backend,
        rogue,
        renter,
        renter_source,
        depositors,
        dep_wallets,
        backend_wallet,
        rogue_wallet,
        victim,
        victim_wallet,
        usdc_mint,
    }
}

// ===========================================================================
// INVARIANT 1
// ===========================================================================

/// **A root whose total payout exceeds the vault's USDC fails closed and
/// moves nothing.**
///
/// The ceiling is the VAULT BALANCE — the only on-chain fact about what the
/// root can pay from. Over by ONE micro-unit: the program's own
/// `RootExceedsVault`, no root account born, no balance moved (vault, bond,
/// operator wallet). The boundary is strict — a root paying EXACTLY the
/// vault's USDC posts — and posting it moves nothing either.
#[tokio::test]
async fn root_whose_total_payout_exceeds_the_vaults_usdc_fails_closed_and_moves_nothing() {
    const LABEL: &str = "invariant 1";
    let f = fixture().await;
    f.prepay(LABEL, 5 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;

    // Over by one micro-unit.
    let over = merkle::Tree::new(&[f.leaf(0, 5 * USDC + 1)]);
    let step = "post_root whose total payout is 1 micro-USDC over the vault";
    let err = refused(
        LABEL,
        step,
        f.send(f.post_root_ix(1, &over, 5 * USDC + 1), &[&f.backend]),
    )
    .await;
    assert_code(LABEL, step, &err, ROOT_EXCEEDS_VAULT);

    // "fails closed and moves nothing".
    assert!(
        common::settlement_root(f.banks(), 1).await.is_none(),
        "{LABEL} — a refused root must not exist"
    );
    assert_eq!(
        f.vault().await,
        5 * USDC,
        "{LABEL} — the vault did not move"
    );
    assert_eq!(f.bond().await, 10 * USDC, "{LABEL} — the bond did not move");
    assert_eq!(
        f.wallet(&f.backend_wallet).await,
        90 * USDC,
        "{LABEL} — the operator paid nothing beyond its bond"
    );

    // The boundary: exactly the vault's USDC.
    let exact = merkle::Tree::new(&[f.leaf(0, 5 * USDC)]);
    let step = "control: post_root whose total payout is EXACTLY the vault's USDC";
    lands(
        LABEL,
        step,
        f.send(f.post_root_ix(1, &exact, 5 * USDC), &[&f.backend]),
    )
    .await;
    let root = f.root(1).await;
    assert_eq!(
        root.total_payout,
        5 * USDC,
        "{LABEL} — the boundary root pays the vault exactly"
    );
    assert_eq!(
        root.merkle_root,
        exact.root(),
        "{LABEL} — the posted root is the tree's root"
    );
    assert_eq!(
        root.status,
        pool::RootStatus::Active,
        "{LABEL} — a fresh root is Active"
    );
    assert_eq!(
        f.vault().await,
        5 * USDC,
        "{LABEL} — posting a root moves nothing either"
    );
}

// ===========================================================================
// INVARIANT 2
// ===========================================================================

/// **A claim whose inclusion proof does not verify against the published
/// root is rejected.**
///
/// A VALID proof pays first — the control that makes every later refusal
/// mean something. Then four forgeries, each `InvalidProof`, each leaving
/// the vault and every wallet untouched: another leaf's proof, a tampered
/// amount (the leaf hashes it), a proof cut from a different tree, and the
/// right proof with one byte flipped.
#[tokio::test]
async fn claim_whose_inclusion_proof_does_not_verify_against_the_published_root_is_rejected() {
    const LABEL: &str = "invariant 2";
    let mut f = fixture().await;
    f.prepay(LABEL, 10 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f
        .post(
            LABEL,
            1,
            &[
                f.leaf(0, 3 * USDC),
                f.leaf(1, 2 * USDC),
                f.leaf(2, USDC),
                f.leaf(3, 4 * USDC),
            ],
        )
        .await;
    // Invariant 11: nothing pays inside the window — the clock moves first.
    f.pass_challenge_window(1).await;
    let a = &f.depositors[0];
    let b = &f.depositors[1];
    let c = &f.depositors[2];

    // CONTROL: a valid proof pays.
    let step = "control: claim leaf 0 with ITS valid proof (direct door)";
    lands(LABEL, step, f.claim(a, 1, &tree, 0, &f.dep_wallets[0])).await;
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — the valid proof paid leaf 0"
    );

    // FORGERY 1 — another leaf's proof.
    let step = "claim leaf 1 with LEAF 0's proof";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            b,
            1,
            1,
            &b.pubkey(),
            &f.dep_wallets[1],
            2 * USDC,
            &tree.proof(0),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    // FORGERY 2 — the right proof, a tampered amount.
    let step = "claim leaf 1 with its own proof but 1 micro-USDC more than the leaf says";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            b,
            1,
            1,
            &b.pubkey(),
            &f.dep_wallets[1],
            2 * USDC + 1,
            &tree.proof(1),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    // FORGERY 3 — a proof cut from a DIFFERENT tree.
    let other = merkle::Tree::new(&[
        f.leaf(0, 9 * USDC),
        f.leaf(1, 8 * USDC),
        f.leaf(2, 7 * USDC),
        f.leaf(3, 6 * USDC),
    ]);
    let step = "claim leaf 2 with a proof cut from a different tree";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            c,
            1,
            2,
            &c.pubkey(),
            &f.dep_wallets[2],
            USDC,
            &other.proof(2),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    // FORGERY 4 — the right proof with ONE byte flipped.
    let mut flipped = tree.proof(2);
    flipped[0][31] ^= 0x01;
    let step = "claim leaf 2 with its own proof, one byte flipped";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(c, 1, 2, &c.pubkey(), &f.dep_wallets[2], USDC, &flipped),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    // Nothing paid but the control: the vault is down by leaf 0 alone, and
    // no refused claim left a marker behind.
    assert_eq!(
        f.vault().await,
        7 * USDC,
        "{LABEL} — only the valid claim was paid"
    );
    for index in 1..4 {
        assert_eq!(
            f.wallet(&f.dep_wallets[index]).await,
            0,
            "{LABEL} — depositor {index} was paid nothing"
        );
        assert!(
            common::claim_marker(f.banks(), &addrs::root(1), index as u32)
                .await
                .is_none(),
            "{LABEL} — a refused claim must leave no marker for leaf {index}"
        );
    }
}

// ===========================================================================
// INVARIANT 3
// ===========================================================================

/// **One leaf funds exactly one claim; a replayed claim is refused.**
///
/// The marker is an ADDRESS — `[claim_marker, root, leaf_index]`, plain
/// `init` — so it can be born once, ever. Leaf 0 is claimed through the
/// ASSISTED door and leaf 1 through the DIRECT door; every replay of
/// either leaf, through EITHER door, is refused by account creation,
/// before any handler logic runs. The address alone is not the whole
/// sentence, though: leaf 0's OWN valid proof, presented under ANOTHER
/// index whose marker does not exist yet, must fail the proof check
/// (`InvalidProof`) — the proof is bound to its position, or one leaf
/// would fund a second claim through a fresh marker. The proof of the
/// sentence is the state: each depositor holds exactly one payout, the
/// vault is down by exactly the two leaves, each leaf has exactly one
/// marker.
#[tokio::test]
async fn one_leaf_funds_exactly_one_claim_a_replayed_claim_is_refused() {
    const LABEL: &str = "invariant 3";
    let mut f = fixture().await;
    f.prepay(LABEL, 7 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f
        .post(LABEL, 1, &[f.leaf(0, 3 * USDC), f.leaf(1, 2 * USDC)])
        .await;
    // Invariant 11: nothing pays inside the window — the clock moves first.
    f.pass_challenge_window(1).await;
    let a = &f.depositors[0];
    let b = &f.depositors[1];

    // Leaf 0, ASSISTED door: only the backend signs.
    lands(
        LABEL,
        "claim leaf 0 through the ASSISTED door (backend signs)",
        f.claim(&f.backend, 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — leaf 0 paid its depositor"
    );

    // Replays of leaf 0 — through BOTH doors.
    for (door, authority) in [("assisted", &f.backend), ("direct", a)] {
        let step = format!("REPLAY leaf 0 through the {door} door");
        let err = refused(
            LABEL,
            &step,
            f.claim(authority, 1, &tree, 0, &f.dep_wallets[0]),
        )
        .await;
        assert_structural_refusal(LABEL, &step, &err);
    }
    // THE POSITION-BINDING REPLAY: leaf 0's own leaf and its own VALID
    // proof, presented under ANOTHER index whose marker does not exist
    // yet. The proof must not verify there — otherwise one leaf would fund
    // a second claim through a fresh marker.
    let step = "REPLAY leaf 0's content under ANOTHER index (leaf 1's slot) with its own proof";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            &f.backend,
            1,
            1,
            &a.pubkey(),
            &f.dep_wallets[0],
            3 * USDC,
            &tree.proof(0),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — leaf 0 paid EXACTLY once"
    );
    assert_eq!(
        f.vault().await,
        4 * USDC,
        "{LABEL} — the vault paid leaf 0 once"
    );

    // Leaf 1, DIRECT door: only B's key is in the transaction.
    lands(
        LABEL,
        "claim leaf 1 through the DIRECT door (backend absent)",
        f.claim(b, 1, &tree, 1, &f.dep_wallets[1]),
    )
    .await;

    // Replays of leaf 1 — through BOTH doors, the other order.
    for (door, authority) in [("direct", b), ("assisted", &f.backend)] {
        let step = format!("REPLAY leaf 1 through the {door} door");
        let err = refused(
            LABEL,
            &step,
            f.claim(authority, 1, &tree, 1, &f.dep_wallets[1]),
        )
        .await;
        assert_structural_refusal(LABEL, &step, &err);
    }
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        2 * USDC,
        "{LABEL} — leaf 1 paid EXACTLY once"
    );

    // The markers: one per leaf, naming the leaf's depositor and amount.
    let root = addrs::root(1);
    let marker = common::claim_marker(f.banks(), &root, 0)
        .await
        .expect("leaf 0 has its marker");
    assert_eq!(
        (marker.depositor, marker.amount),
        (a.pubkey(), 3 * USDC),
        "{LABEL} — leaf 0's marker names A and 3 USDC"
    );
    let marker = common::claim_marker(f.banks(), &root, 1)
        .await
        .expect("leaf 1 has its marker");
    assert_eq!(
        (marker.depositor, marker.amount),
        (b.pubkey(), 2 * USDC),
        "{LABEL} — leaf 1's marker names B and 2 USDC"
    );
    assert_eq!(
        f.vault().await,
        2 * USDC,
        "{LABEL} — the vault paid exactly the two leaves, once each"
    );
}

// ===========================================================================
// INVARIANT 4
// ===========================================================================

/// **A payout reaches only the leaf's named depositor, through either
/// door.**
///
/// The two doors pay their named depositors first — the direct door with
/// the backend absent, the assisted door with the depositor absent. Then
/// four attacks on leaf 2, each with its exact refusal: the backend routes
/// to a wallet IT owns (`WrongPayoutDestination` — an account constraint),
/// the backend names itself the depositor (`InvalidProof` — the leaf
/// hashes the depositor's pubkey), a third key opens neither door
/// (`ExitAuthority`), and C's own key aims at a wallet C does not own.
#[tokio::test]
async fn payout_reaches_only_the_leafs_named_depositor_through_either_door() {
    const LABEL: &str = "invariant 4";
    let mut f = fixture().await;
    f.prepay(LABEL, 10 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f
        .post(
            LABEL,
            1,
            &[f.leaf(0, 3 * USDC), f.leaf(1, 2 * USDC), f.leaf(2, USDC)],
        )
        .await;
    // Invariant 11: nothing pays inside the window — the clock moves first.
    f.pass_challenge_window(1).await;
    let a = &f.depositors[0];
    let c = &f.depositors[2];
    let operator_before = f.wallet(&f.backend_wallet).await;

    // DIRECT door: only A's key is in the transaction.
    lands(
        LABEL,
        "DIRECT door: A claims leaf 0 with A's own key (backend absent)",
        f.claim(a, 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — the direct door paid A"
    );

    // ASSISTED door: only the backend is in the transaction.
    lands(
        LABEL,
        "ASSISTED door: the backend claims leaf 1 for B (B absent)",
        f.claim(&f.backend, 1, &tree, 1, &f.dep_wallets[1]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        2 * USDC,
        "{LABEL} — the assisted door paid B, not the operator"
    );

    // ATTACK 1 — the backend routes C's leaf to a wallet the BACKEND owns.
    let step = "backend presents leaf 2 but pays a wallet the BACKEND owns";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            &f.backend,
            1,
            2,
            &c.pubkey(),
            &f.backend_wallet,
            USDC,
            &tree.proof(2),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, WRONG_PAYOUT_DESTINATION);

    // ATTACK 2 — the backend names ITSELF the depositor of C's leaf.
    let step = "backend presents leaf 2 naming ITSELF the depositor";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            &f.backend,
            1,
            2,
            &f.backend.pubkey(),
            &f.backend_wallet,
            USDC,
            &tree.proof(2),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, INVALID_PROOF);

    // ATTACK 3 — neither door.
    let step = "a third key claims leaf 2 for C";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(
            &f.rogue,
            1,
            2,
            &c.pubkey(),
            &f.dep_wallets[2],
            USDC,
            &tree.proof(2),
        ),
    )
    .await;
    assert_code(LABEL, step, &err, EXIT_AUTHORITY);

    // ATTACK 4 — C's own key, a wallet C does not own.
    let step = "C's own key, but a wallet C does not own";
    let err = refused(
        LABEL,
        step,
        f.claim_forged(c, 1, 2, &c.pubkey(), &f.rogue_wallet, USDC, &tree.proof(2)),
    )
    .await;
    assert_code(LABEL, step, &err, WRONG_PAYOUT_DESTINATION);

    // Nothing reached anyone but the two named depositors.
    assert_eq!(
        f.wallet(&f.dep_wallets[2]).await,
        0,
        "{LABEL} — C was paid nothing"
    );
    assert_eq!(
        f.wallet(&f.rogue_wallet).await,
        0,
        "{LABEL} — the rogue was paid nothing"
    );
    assert_eq!(
        f.wallet(&f.backend_wallet).await,
        operator_before,
        "{LABEL} — the operator's wallet did not grow"
    );
    assert_eq!(
        f.vault().await,
        5 * USDC,
        "{LABEL} — the vault paid the two named leaves only"
    );
    assert!(
        common::claim_marker(f.banks(), &addrs::root(1), 2)
            .await
            .is_none(),
        "{LABEL} — a refused payout leaves no marker"
    );
}

// ===========================================================================
// INVARIANT 5
// ===========================================================================

/// **A challenge inside the window freezes that root's payouts until the
/// multisig rules.**
///
/// The opening control is now invariant 11's refusal: inside the window
/// nothing pays at all. Then ANY key challenges and the root turns
/// `Challenged` — B's claim is `RootFrozen` through both doors (a freeze,
/// not merely the window's delay), and neither the challenger nor the
/// operator can lift it. The admin's "stands" ruling returns the root to
/// `Active` AND puts the ruling on record — and that ruling waives the rest
/// of the window, so B is paid while the clock has not moved. The
/// complement closes it: on a second root the clock is warped PAST the
/// window, a challenge is `ChallengeWindowClosed`, and that root pays.
#[tokio::test]
async fn challenge_inside_the_window_freezes_that_roots_payouts_until_the_multisig_rules() {
    const LABEL: &str = "invariant 5";
    let mut f = fixture().await;
    f.prepay(LABEL, 20 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f
        .post(LABEL, 1, &[f.leaf(0, 3 * USDC), f.leaf(1, 2 * USDC)])
        .await;
    let a = &f.depositors[0];
    let b = &f.depositors[1];

    // The opening control (invariant 11): inside the window nothing pays —
    // so every payment below is the freeze and the ruling at work, not time.
    let step = "control: A claims leaf 0 INSIDE the challenge window";
    let err = refused(LABEL, step, f.claim(a, 1, &tree, 0, &f.dep_wallets[0])).await;
    assert_code(LABEL, step, &err, CHALLENGE_WINDOW_OPEN);
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        0,
        "{LABEL} — nothing was paid inside the window"
    );
    assert_eq!(
        f.vault().await,
        20 * USDC,
        "{LABEL} — nothing left the vault inside the window"
    );

    // Any key challenges, inside the window.
    f.challenge(LABEL, &f.rogue, 1).await;
    assert_eq!(
        f.root(1).await.status,
        pool::RootStatus::Challenged,
        "{LABEL} — a challenge freezes the root"
    );

    // Frozen through BOTH doors — a freeze, not merely the window's delay.
    for (door, authority) in [("direct", b), ("assisted", &f.backend)] {
        let step = format!("B claims leaf 1 through the {door} door while the root is challenged");
        let err = refused(
            LABEL,
            &step,
            f.claim(authority, 1, &tree, 1, &f.dep_wallets[1]),
        )
        .await;
        assert_code(LABEL, &step, &err, ROOT_FROZEN);
    }
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        0,
        "{LABEL} — a frozen root pays nobody"
    );
    assert_eq!(
        f.vault().await,
        20 * USDC,
        "{LABEL} — the vault did not move while frozen"
    );

    // Nobody but the multisig rules — not the challenger, not the operator.
    // (No code is pinned here: the refusal is Anchor's `has_one` gate on the
    // admin — a framework constant — and the state proof below is the pin.)
    //
    // The instruction names the ATTEMPTING key as `admin_authority` and that
    // key signs — otherwise the transaction needs the real admin's signature
    // and dies host-side at `Transaction::sign` before `has_one` can fire
    // (founder ruling 2026-11-27, AUDIT A-065).
    for who in [&f.rogue, &f.backend] {
        let step = "a non-multisig key tries to resolve the challenge";
        let ix = ixs::resolve_challenge(
            &addrs::pool(),
            &who.pubkey(), // not the admin: `has_one = admin_authority` must refuse this
            &addrs::root(1),
            &addrs::bond(),
            1,
            ixs::Ruling::Stands,
        );
        refused(LABEL, step, f.send(ix, &[who])).await;
        assert_eq!(
            f.root(1).await.status,
            pool::RootStatus::Challenged,
            "{LABEL} — the freeze holds until the multisig rules"
        );
    }

    // The multisig rules: the root stands. The ruling is ON RECORD, and it
    // waives the rest of the window (invariant 11) — the clock has not moved.
    f.resolve(
        LABEL,
        "the multisig rules the root STANDS",
        1,
        ixs::Ruling::Stands,
    )
    .await;
    let root = f.root(1).await;
    assert_eq!(
        root.status,
        pool::RootStatus::Active,
        "{LABEL} — the ruling unfroze the root"
    );
    assert!(
        root.stands_ruled,
        "{LABEL} — the ruling is on record: it waives the rest of the window"
    );
    lands(
        LABEL,
        "B claims leaf 1 after the ruling, still INSIDE the window (payouts resumed)",
        f.claim(b, 1, &tree, 1, &f.dep_wallets[1]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        2 * USDC,
        "{LABEL} — the unfrozen root paid B"
    );

    // The complement: AFTER the window, a challenge is refused.
    let later = f.post(LABEL, 2, &[f.leaf(3, 4 * USDC)]).await;
    f.pass_challenge_window(2).await;
    let step = "challenge window 2 AFTER its challenge window closed";
    let err = refused(
        LABEL,
        step,
        f.send(f.challenge_ix(&f.rogue.pubkey(), 2, EVIDENCE), &[&f.rogue]),
    )
    .await;
    assert_code(LABEL, step, &err, CHALLENGE_WINDOW_CLOSED);
    assert_eq!(
        f.root(2).await.status,
        pool::RootStatus::Active,
        "{LABEL} — a late challenge freezes nothing"
    );
    let d = &f.depositors[3];
    lands(
        LABEL,
        "D claims leaf 0 of window 2 after its window closed",
        f.claim(d, 2, &later, 0, &f.dep_wallets[3]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[3]).await,
        4 * USDC,
        "{LABEL} — the root past its window still pays"
    );
}

// ===========================================================================
// INVARIANT 6
// ===========================================================================

/// **The bond slashes by exactly the ruled amount for a root proven false,
/// and by nothing otherwise.**
///
/// EXACTLY: a ruling of 2 USDC moves 2 USDC — bond record, bond vault and
/// the victim's wallet agree to the micro-unit — and a second slash of the
/// same root is refused. OTHERWISE: a root never challenged, a root ruled
/// to stand, and a root still awaiting its ruling all refuse
/// (`RootNotProvenFalse`); a ruling larger than the bond refuses
/// (`SlashExceedsBond`) and changes nothing; and a crank aiming the slash
/// at anyone but the recorded victim is refused.
#[tokio::test]
async fn bond_slashes_by_exactly_the_ruled_amount_for_a_root_proven_false_and_by_nothing_otherwise()
{
    const LABEL: &str = "invariant 6";
    let f = fixture().await;
    f.prepay(LABEL, 10 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;

    // ---- A root proven false: EXACTLY the ruled amount.
    f.post(LABEL, 1, &[f.leaf(0, 3 * USDC)]).await;
    f.challenge(LABEL, &f.rogue, 1).await;
    f.resolve(
        LABEL,
        "the multisig rules the root FALSE: 2 USDC to the victim",
        1,
        ixs::Ruling::False {
            slash_amount: 2 * USDC,
            victim: &f.victim.pubkey(),
        },
    )
    .await;
    let root = f.root(1).await;
    assert_eq!(
        root.status,
        pool::RootStatus::ProvenFalse,
        "{LABEL} — a false ruling marks the root"
    );
    assert_eq!(
        root.ruled_slash,
        2 * USDC,
        "{LABEL} — the ruling records its exact amount"
    );
    assert_eq!(
        root.victim,
        f.victim.pubkey(),
        "{LABEL} — the ruling records its victim"
    );
    assert_eq!(
        f.bond().await,
        10 * USDC,
        "{LABEL} — a ruling alone slashes nothing"
    );

    lands(
        LABEL,
        "slash_bond executes the ruled 2 USDC (anyone may crank it)",
        f.slash(1, &f.victim_wallet),
    )
    .await;
    assert_eq!(
        f.bond().await,
        8 * USDC,
        "{LABEL} — the bond fell by EXACTLY the ruled amount"
    );
    assert_eq!(
        f.wallet(&addrs::bond_vault()).await,
        8 * USDC,
        "{LABEL} — the bond vault agrees with the record"
    );
    assert_eq!(
        f.wallet(&f.victim_wallet).await,
        2 * USDC,
        "{LABEL} — the victim received EXACTLY the ruled amount"
    );
    assert_eq!(
        f.root(1).await.status,
        pool::RootStatus::Slashed,
        "{LABEL} — the slash executed once"
    );

    let step = "slash_bond a SECOND time on the same root";
    let err = refused(LABEL, step, f.slash(1, &f.victim_wallet)).await;
    assert_code(LABEL, step, &err, ROOT_NOT_PROVEN_FALSE);

    // ---- By nothing otherwise.
    let one = [f.leaf(0, USDC)];

    // (a) never challenged.
    f.post(LABEL, 2, &one).await;
    let step = "slash_bond a root that was never challenged";
    let err = refused(LABEL, step, f.slash(2, &f.victim_wallet)).await;
    assert_code(LABEL, step, &err, ROOT_NOT_PROVEN_FALSE);

    // (b) challenged, then ruled to stand.
    f.post(LABEL, 3, &one).await;
    f.challenge(LABEL, &f.rogue, 3).await;
    f.resolve(
        LABEL,
        "the multisig rules window 3 STANDS",
        3,
        ixs::Ruling::Stands,
    )
    .await;
    let step = "slash_bond a root the multisig ruled to stand";
    let err = refused(LABEL, step, f.slash(3, &f.victim_wallet)).await;
    assert_code(LABEL, step, &err, ROOT_NOT_PROVEN_FALSE);

    // (c) still challenged — no ruling yet.
    f.post(LABEL, 4, &one).await;
    f.challenge(LABEL, &f.rogue, 4).await;
    let step = "slash_bond a root still awaiting its ruling";
    let err = refused(LABEL, step, f.slash(4, &f.victim_wallet)).await;
    assert_code(LABEL, step, &err, ROOT_NOT_PROVEN_FALSE);
    assert_eq!(
        f.bond().await,
        8 * USDC,
        "{LABEL} — none of the three slashed anything"
    );

    // (d) a ruling may not exceed the bond backing the root.
    let step = "ruling a slash of 1 micro-USDC MORE than the bond";
    let err = refused(
        LABEL,
        step,
        f.send(
            f.resolve_ix(
                4,
                ixs::Ruling::False {
                    slash_amount: 8 * USDC + 1,
                    victim: &f.victim.pubkey(),
                },
            ),
            &[&f.admin],
        ),
    )
    .await;
    assert_code(LABEL, step, &err, SLASH_EXCEEDS_BOND);
    let root = f.root(4).await;
    assert_eq!(
        root.status,
        pool::RootStatus::Challenged,
        "{LABEL} — a refused ruling changes nothing"
    );
    assert_eq!(root.ruled_slash, 0, "{LABEL} — no amount was recorded");

    // (e) The victim is RECORDED, never passed: a crank paying anyone else
    //     is refused, and the same crank then pays the victim.
    f.resolve(
        LABEL,
        "the multisig rules window 4 FALSE: 1 USDC to the victim",
        4,
        ixs::Ruling::False {
            slash_amount: USDC,
            victim: &f.victim.pubkey(),
        },
    )
    .await;
    let step = "slash_bond window 4 paying the ROGUE's wallet";
    let err = refused(LABEL, step, f.slash(4, &f.rogue_wallet)).await;
    assert_code(LABEL, step, &err, WRONG_DESTINATION_OWNER);
    assert_eq!(
        f.bond().await,
        8 * USDC,
        "{LABEL} — a misdirected crank slashed nothing"
    );
    lands(
        LABEL,
        "slash_bond window 4 paying the recorded victim",
        f.slash(4, &f.victim_wallet),
    )
    .await;
    assert_eq!(
        f.wallet(&f.rogue_wallet).await,
        0,
        "{LABEL} — the rogue received nothing"
    );

    // "exactly": the bond paid out precisely the two ruled amounts, and the
    // victim holds precisely them.
    assert_eq!(
        10 * USDC - f.bond().await,
        3 * USDC,
        "{LABEL} — the bond paid exactly 2 + 1 USDC"
    );
    assert_eq!(
        f.wallet(&f.victim_wallet).await,
        3 * USDC,
        "{LABEL} — the victim holds exactly 2 + 1 USDC"
    );
}

// ===========================================================================
// INVARIANT 7
// ===========================================================================

/// Every field of one root account, as one value — "append-only" means
/// every field, so every field is compared.
#[derive(Debug, PartialEq)]
struct RootSnapshot {
    pool: Pubkey,
    window: u64,
    merkle_root: [u8; 32],
    total_payout: u64,
    status: pool::RootStatus,
    posted_at: i64,
    bond: u64,
    challenger: Pubkey,
    evidence: [u8; 32],
    ruled_slash: u64,
    victim: Pubkey,
    stands_ruled: bool,
    paid_out: u64,
    bump: u8,
}

/// Reads a root, every field.
fn snapshot(root: &pool::SettlementRoot) -> RootSnapshot {
    RootSnapshot {
        pool: root.pool,
        window: root.window,
        merkle_root: root.merkle_root,
        total_payout: root.total_payout,
        status: root.status.clone(),
        posted_at: root.posted_at,
        bond: root.bond,
        challenger: root.challenger,
        evidence: root.evidence,
        ruled_slash: root.ruled_slash,
        victim: root.victim,
        stands_ruled: root.stands_ruled,
        paid_out: root.paid_out,
        bump: root.bump,
    }
}

/// **A second root for the same window is refused; roots are append-only.**
///
/// The root is an ADDRESS — `[settlement_root, window]`, plain `init`. A
/// different root for the same window, and the identical root again, both
/// die at account creation and the first root stays identical in every
/// field. The NEXT window appends, leaves the first untouched, and the
/// first root's proofs still pay — history is live, not merely stored.
#[tokio::test]
async fn second_root_for_the_same_window_is_refused_roots_are_append_only() {
    const LABEL: &str = "invariant 7";
    let mut f = fixture().await;
    f.prepay(LABEL, 20 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;

    let first = f.post(LABEL, 7, &[f.leaf(0, 3 * USDC)]).await;
    let posted = f.root(7).await;

    // A DIFFERENT root for the same window.
    let replacement = merkle::Tree::new(&[f.leaf(1, 4 * USDC)]);
    let step = "post a DIFFERENT root for window 7";
    let err = refused(
        LABEL,
        step,
        f.send(f.post_root_ix(7, &replacement, 4 * USDC), &[&f.backend]),
    )
    .await;
    assert_structural_refusal(LABEL, step, &err);
    assert_eq!(
        snapshot(&f.root(7).await),
        snapshot(&posted),
        "{LABEL} — the refused replacement left window 7 identical"
    );

    // The IDENTICAL root again — a second post is a second post.
    let step = "post the IDENTICAL root for window 7 again";
    let err = refused(
        LABEL,
        step,
        f.send(f.post_root_ix(7, &first, 3 * USDC), &[&f.backend]),
    )
    .await;
    assert_structural_refusal(LABEL, step, &err);
    assert_eq!(
        snapshot(&f.root(7).await),
        snapshot(&posted),
        "{LABEL} — the refused replay left window 7 identical"
    );

    // Append-only: the NEXT window posts, and history is untouched.
    let next = f.post(LABEL, 8, &[f.leaf(2, 2 * USDC)]).await;
    let root = f.root(8).await;
    assert_eq!(
        root.merkle_root,
        next.root(),
        "{LABEL} — window 8 carries its own root"
    );
    assert_eq!(
        root.total_payout,
        2 * USDC,
        "{LABEL} — window 8 carries its own total"
    );
    assert_eq!(
        root.status,
        pool::RootStatus::Active,
        "{LABEL} — window 8 is live"
    );
    assert_eq!(
        snapshot(&f.root(7).await),
        snapshot(&posted),
        "{LABEL} — posting window 8 left window 7 identical"
    );

    // The first root is still LIVE: its proofs still pay — once its window
    // has passed (invariant 11). Window 8 was posted in the same slot, so
    // one clock move passes both.
    f.pass_challenge_window(7).await;
    let a = &f.depositors[0];
    lands(
        LABEL,
        "a claim against the FIRST root still pays after window 8 was posted",
        f.claim(a, 7, &first, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — the first root paid its leaf"
    );
}

// ===========================================================================
// INVARIANT 8
// ===========================================================================

/// One instruction, observed twice: SIMULATED first (the only path that
/// exposes logs), then EXECUTED. The subscriber sees the simulated events;
/// the chain keeps the executed state; invariant 8 is the claim that the
/// two end equal.
#[allow(clippy::too_many_arguments)]
async fn observed(
    f: &Fixture,
    label: &str,
    step: &str,
    ix: Instruction,
    signers: &[&Keypair],
    subscriber: &mut events::Subscriber,
    seen: &mut Vec<&'static str>,
) {
    let logs = common::simulate_logs(f.banks(), &f.ctx.payer, vec![ix.clone()], signers).await;
    let decoded = events::decode(&logs);
    assert!(
        !decoded.is_empty(),
        "{label} — `{step}` emitted NO event: every state change needs one"
    );
    for event in &decoded {
        subscriber.apply(event);
    }
    seen.extend(decoded.iter().map(|event| event.name));
    lands(label, step, f.send(ix, signers)).await;
}

/// **Every state change is reconstructible from emitted events alone.**
///
/// The whole settlement lifecycle runs one state change per step: bond
/// funded twice, vault funded, a root posted, challenged and ruled to
/// stand, the clock past its window, two leaves paid (one per door), a
/// second root posted, challenged, ruled false and slashed. A
/// subscriber that reads ONLY decoded `Program data:` events — never an
/// account — ends with a mirror that must equal the chain: the vault, the
/// bond (record and vault), every field of both roots, and every marker,
/// present and absent. An event census closes the loop: nothing emitted
/// twice, nothing emitted unnamed.
#[tokio::test]
async fn every_state_change_is_reconstructible_from_emitted_events_alone() {
    const LABEL: &str = "invariant 8";
    let mut f = fixture().await; // nothing has happened yet
    let mut subscriber = events::Subscriber::default();
    let mut seen: Vec<&'static str> = Vec::new();

    observed(
        &f,
        LABEL,
        "fund_bond 4 USDC",
        f.fund_bond_ix(4 * USDC),
        &[&f.backend],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "fund_bond 6 USDC more",
        f.fund_bond_ix(6 * USDC),
        &[&f.backend],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "renter prepay 10 USDC",
        f.prepay_ix(10 * USDC),
        &[&f.backend, &f.renter],
        &mut subscriber,
        &mut seen,
    )
    .await;

    let tree = merkle::Tree::new(&[f.leaf(0, 3 * USDC), f.leaf(1, 2 * USDC), f.leaf(2, USDC)]);
    observed(
        &f,
        LABEL,
        "post_root window 1",
        f.post_root_ix(1, &tree, tree.total()),
        &[&f.backend],
        &mut subscriber,
        &mut seen,
    )
    .await;

    // Invariant 11: nothing pays inside the window. The lifecycle takes the
    // challenge-and-ruling path FIRST, then moves the clock.
    observed(
        &f,
        LABEL,
        "challenge window 1",
        f.challenge_ix(&f.rogue.pubkey(), 1, EVIDENCE),
        &[&f.rogue],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "resolve window 1: the root stands",
        f.resolve_ix(1, ixs::Ruling::Stands),
        &[&f.admin],
        &mut subscriber,
        &mut seen,
    )
    .await;

    // The clock passes the window. Not a program instruction: no event.
    f.pass_challenge_window(1).await;

    let a = &f.depositors[0];
    let b = &f.depositors[1];
    observed(
        &f,
        LABEL,
        "claim leaf 0 of window 1 (assisted door)",
        f.claim_ix(
            &f.backend.pubkey(),
            1,
            0,
            &a.pubkey(),
            &f.dep_wallets[0],
            3 * USDC,
            &tree.proof(0),
        ),
        &[&f.backend],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "claim leaf 1 of window 1 (direct door)",
        f.claim_ix(
            &b.pubkey(),
            1,
            1,
            &b.pubkey(),
            &f.dep_wallets[1],
            2 * USDC,
            &tree.proof(1),
        ),
        &[b],
        &mut subscriber,
        &mut seen,
    )
    .await;

    // Window 2 is posted AT the warped slot: a challenge window of its own.
    let later = merkle::Tree::new(&[f.leaf(3, USDC)]);
    observed(
        &f,
        LABEL,
        "post_root window 2",
        f.post_root_ix(2, &later, later.total()),
        &[&f.backend],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "challenge window 2",
        f.challenge_ix(&f.victim.pubkey(), 2, EVIDENCE),
        &[&f.victim],
        &mut subscriber,
        &mut seen,
    )
    .await;
    let victim = f.victim.pubkey();
    observed(
        &f,
        LABEL,
        "resolve window 2: false, 2 USDC to the victim",
        f.resolve_ix(
            2,
            ixs::Ruling::False {
                slash_amount: 2 * USDC,
                victim: &victim,
            },
        ),
        &[&f.admin],
        &mut subscriber,
        &mut seen,
    )
    .await;
    observed(
        &f,
        LABEL,
        "slash_bond window 2",
        f.slash_ix(2, &f.victim_wallet),
        &[],
        &mut subscriber,
        &mut seen,
    )
    .await;

    // ---- The mirror built from events ALONE, against the chain.
    assert_eq!(
        subscriber.vault,
        f.vault().await,
        "{LABEL} — the vault rebuilt from events equals the chain"
    );
    assert_eq!(
        subscriber.bond,
        f.bond().await,
        "{LABEL} — the bond record rebuilt from events equals the chain"
    );
    assert_eq!(
        subscriber.bond,
        f.wallet(&addrs::bond_vault()).await,
        "{LABEL} — the bond vault rebuilt from events equals the chain"
    );

    for window in [1u64, 2u64] {
        let chain = f.root(window).await;
        let mirror = subscriber
            .roots
            .get(&window)
            .unwrap_or_else(|| panic!("{LABEL} — the events never described window {window}"));
        assert_eq!(
            mirror.merkle_root, chain.merkle_root,
            "{LABEL} — window {window}: merkle_root"
        );
        assert_eq!(
            mirror.total_payout, chain.total_payout,
            "{LABEL} — window {window}: total_payout"
        );
        assert_eq!(
            mirror.posted_at, chain.posted_at,
            "{LABEL} — window {window}: posted_at"
        );
        assert_eq!(mirror.bond, chain.bond, "{LABEL} — window {window}: bond");
        assert_eq!(
            mirror.challenger, chain.challenger,
            "{LABEL} — window {window}: challenger"
        );
        assert_eq!(
            mirror.evidence, chain.evidence,
            "{LABEL} — window {window}: evidence"
        );
        assert_eq!(
            mirror.ruled_slash, chain.ruled_slash,
            "{LABEL} — window {window}: ruled_slash"
        );
        assert_eq!(
            mirror.victim, chain.victim,
            "{LABEL} — window {window}: victim"
        );
        assert_eq!(
            mirror.stands_ruled, chain.stands_ruled,
            "{LABEL} — window {window}: stands_ruled"
        );
        assert_eq!(
            mirror.paid_out, chain.paid_out,
            "{LABEL} — window {window}: paid_out"
        );
        assert_eq!(
            subscriber.status[&window], chain.status,
            "{LABEL} — window {window}: status"
        );
    }

    // Markers: every paid leaf, with its depositor and amount.
    for (window, leaf, depositor, amount) in [
        (1u64, 0u32, &f.depositors[0], 3 * USDC),
        (1, 1, &f.depositors[1], 2 * USDC),
    ] {
        let chain = common::claim_marker(f.banks(), &addrs::root(window), leaf)
            .await
            .unwrap_or_else(|| panic!("{LABEL} — leaf {leaf} of window {window} has a marker"));
        let mirror = subscriber.markers.get(&(window, leaf)).unwrap_or_else(|| {
            panic!("{LABEL} — the events never described leaf {leaf} of window {window}")
        });
        assert_eq!(
            (mirror.depositor, mirror.amount),
            (chain.depositor, chain.amount),
            "{LABEL} — marker for leaf {leaf} of window {window} rebuilt from events equals the chain"
        );
        assert_eq!(
            (chain.depositor, chain.amount),
            (depositor.pubkey(), amount),
            "{LABEL} — marker for leaf {leaf} of window {window} names the leaf"
        );
    }
    // Unpaid leaves: no marker on chain, none in the mirror.
    for (window, leaf) in [(1u64, 2u32), (2, 0)] {
        assert!(
            !subscriber.markers.contains_key(&(window, leaf)),
            "{LABEL} — no event paid leaf {leaf} of window {window}"
        );
        assert!(
            common::claim_marker(f.banks(), &addrs::root(window), leaf)
                .await
                .is_none(),
            "{LABEL} — leaf {leaf} of window {window} has no marker"
        );
    }
    assert_eq!(
        subscriber.markers.len(),
        2,
        "{LABEL} — exactly two leaves were paid"
    );

    // Event census: each state change emitted exactly its event.
    for (name, count) in [
        ("BondFunded", 2),
        ("PrepayRecorded", 1),
        ("RootPosted", 2),
        ("ClaimPaid", 2),
        ("RootChallenged", 2),
        ("ChallengeResolved", 2),
        ("BondSlashed", 1),
    ] {
        assert_eq!(
            seen.iter().filter(|seen_name| **seen_name == name).count(),
            count,
            "{LABEL} — `{name}` emitted exactly {count} time(s)"
        );
    }
    assert_eq!(
        seen.len(),
        12,
        "{LABEL} — twelve steps, twelve events, none unaccounted"
    );
}

// ===========================================================================
// INVARIANT 9
// ===========================================================================

/// What the TOKEN LAYER says about the registry and the money — the value
/// invariant 9 compares before and after the whole settlement lifecycle.
#[derive(Debug, PartialEq, Eq)]
struct Registry {
    /// Total supply of the claim mint (the registry's record).
    supply: u64,
    /// Depositor A's claim row.
    claim_row_a: u64,
    /// Rule 9 still ON: the Token-2022 NonTransferable extension.
    non_transferable: bool,
    /// The one mint authority the pool PDA holds still points at the pool.
    authority_is_pool: bool,
    /// Total supply of the USDC mint — nobody minted receipts as USDC.
    usdc_supply: u64,
}

async fn registry(f: &Fixture) -> Registry {
    use spl_token_2022_interface::extension::non_transferable::NonTransferable;
    use spl_token_2022_interface::extension::{BaseStateWithExtensions, StateWithExtensions};
    use spl_token_2022_interface::state as token22_state;

    let banks = f.banks();
    let mint_ai = f
        .banks()
        .get_account(addrs::claim_mint())
        .await
        .expect("bank answers")
        .expect("claim mint exists");
    let mint = StateWithExtensions::<token22_state::Mint>::unpack(&mint_ai.data)
        .expect("claim mint parses");
    Registry {
        supply: common::claim_supply(&mint_ai.data),
        claim_row_a: common::token_2022_balance(banks, &addrs::claim(&f.depositors[0].pubkey()))
            .await,
        non_transferable: mint.get_extension::<NonTransferable>().is_ok(),
        authority_is_pool: !mint.base.mint_authority.is_none()
            && mint.base.mint_authority.unwrap().as_ref() == addrs::pool().as_ref(),
        usdc_supply: common::v3_supply(
            &banks
                .get_account(f.usdc_mint)
                .await
                .expect("bank answers")
                .expect("usdc mint exists")
                .data,
        ),
    }
}

/// **Settlement mints no transferable receipt.**
///
/// A payout is MONEY, not a token: through post, claim, challenge, ruling
/// and slash, the registry's supply, the depositor's claim row and the
/// USDC supply do not move by a micro-unit, and the claim mint — the one
/// mint the pool controls — keeps its NonTransferable extension and its
/// authority. What the depositor receives is exactly the leaf's USDC. Two
/// roots carry the two paths: window 1 is claimed past its challenge window
/// (invariant 11); window 2 is posted at the warped slot, so it has a
/// window of its own to be challenged in, then ruled false and slashed.
#[tokio::test]
async fn settlement_mints_no_transferable_receipt() {
    const LABEL: &str = "invariant 9";
    let mut f = fixture().await;
    let a = &f.depositors[0];

    // A's registry row: a non-zero record whose survival is the point.
    lands(
        LABEL,
        "fixture: record A's 10 USDC contribution (M8 — a record, not money)",
        f.send(
            ixs::record_contribution(
                &addrs::pool(),
                &f.backend.pubkey(),
                &a.pubkey(),
                &addrs::claim_mint(),
                &addrs::claim(&a.pubkey()),
                10 * USDC,
            ),
            &[&f.backend],
        ),
    )
    .await;
    f.prepay(LABEL, 10 * USDC).await;
    let before = registry(&f).await;
    assert_eq!(
        before.supply,
        10 * USDC,
        "{LABEL} — the registry holds A's record"
    );
    assert!(
        before.non_transferable,
        "{LABEL} — the claim mint starts NonTransferable (rule 9)"
    );
    assert!(
        before.authority_is_pool,
        "{LABEL} — the pool PDA is the mint authority"
    );

    // The lifecycle, with the token layer compared after every instruction.
    f.fund_bond(LABEL, 10 * USDC).await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — bonding minted nothing"
    );

    // Window 1 — the CLAIM path: nothing pays before its window passes.
    let tree = f.post(LABEL, 1, &[f.leaf(0, 3 * USDC)]).await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — posting a root minted nothing"
    );
    f.pass_challenge_window(1).await;
    lands(
        LABEL,
        "A claims leaf 0",
        f.claim(&f.depositors[0], 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — a payout minted nothing"
    );
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — the payout is USDC, exactly the leaf's amount"
    );

    // Window 2 — the CHALLENGE path: posted at the warped slot, so it sits
    // inside a challenge window of its own.
    f.post(LABEL, 2, &[f.leaf(1, USDC)]).await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — posting a second root minted nothing"
    );
    f.challenge(LABEL, &f.rogue, 2).await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — a challenge minted nothing"
    );
    f.resolve(
        LABEL,
        "the multisig rules window 2 FALSE: 1 USDC to the victim",
        2,
        ixs::Ruling::False {
            slash_amount: USDC,
            victim: &f.victim.pubkey(),
        },
    )
    .await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — a ruling minted nothing"
    );
    lands(LABEL, "slash_bond window 2", f.slash(2, &f.victim_wallet)).await;
    assert_eq!(
        registry(&f).await,
        before,
        "{LABEL} — a slash minted nothing"
    );
}

// ===========================================================================
// INVARIANT 10
// ===========================================================================

/// **No single key and no upgrade authority can move vault funds; only a
/// satisfied program instruction can.**
///
/// The structural fact first: the vault's token authority is the POOL PDA.
/// Then three single keys each try a RAW transfer out of the vault — the
/// backend, the admin, and the deployment (upgrade) authority itself —
/// and the Token program refuses each with `OwnerMismatch`. No backdoor
/// instruction answers either. The control is the other half of the
/// sentence: a VALID claim — a satisfied program instruction — is what
/// moves the vault, and it moves it exactly once, to the named depositor.
#[tokio::test]
async fn no_single_key_and_no_upgrade_authority_can_move_vault_funds_only_a_satisfied_program_instruction_can()
 {
    const LABEL: &str = "invariant 10";
    let mut f = fixture().await;
    f.prepay(LABEL, 10 * USDC).await;

    // The structural fact: no key signs for the vault.
    let vault_ai = f
        .banks()
        .get_account(addrs::vault())
        .await
        .expect("bank answers")
        .expect("vault exists");
    assert_eq!(
        common::v3_owner(&vault_ai.data),
        addrs::pool(),
        "{LABEL} — the vault's authority is the pool PDA, not any key"
    );

    // Three single keys, three raw transfers — all bypassing the program.
    for (who, authority) in [
        ("the backend key", &f.backend),
        ("the admin (multisig stand-in) key", &f.admin),
        (
            "the DEPLOYMENT (upgrade) authority key",
            common::deploy_authority(),
        ),
    ] {
        let step = format!("a raw Token transfer out of the vault signed by {who}");
        let err = refused(
            LABEL,
            &step,
            f.send(
                common::ixs::raw_v3_transfer(
                    &addrs::vault(),
                    &f.rogue_wallet,
                    &authority.pubkey(),
                    USDC,
                ),
                &[authority],
            ),
        )
        .await;
        assert_code(LABEL, &step, &err, TOKEN_OWNER_MISMATCH);
    }

    // No backdoor: the dispatcher does not know a `sweep_vault`.
    let hopeful = common::ix(
        "sweep_vault",
        (USDC,),
        vec![
            AccountMeta::new(addrs::pool(), false),
            AccountMeta::new_readonly(f.admin.pubkey(), true),
            AccountMeta::new(addrs::vault(), false),
            AccountMeta::new(f.rogue_wallet, false),
            AccountMeta::new_readonly(common::TOKEN_V3, false),
        ],
    );
    let step = "the admin calls a hopeful `sweep_vault` instruction";
    let err = refused(LABEL, step, f.send(hopeful, &[&f.admin])).await;
    assert_code(LABEL, step, &err, ANCHOR_INSTRUCTION_FALLBACK_NOT_FOUND);
    assert_eq!(
        f.vault().await,
        10 * USDC,
        "{LABEL} — no key and no backdoor moved the vault"
    );

    // The other half: ONLY a satisfied program instruction moves it.
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f.post(LABEL, 1, &[f.leaf(0, 3 * USDC)]).await;
    assert_eq!(
        f.vault().await,
        10 * USDC,
        "{LABEL} — bonding and posting moved nothing"
    );
    // Invariant 11: nothing pays inside the window — the clock moves first.
    f.pass_challenge_window(1).await;
    let a = &f.depositors[0];
    lands(
        LABEL,
        "a SATISFIED instruction — a valid claim — moves vault funds",
        f.claim(a, 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.vault().await,
        7 * USDC,
        "{LABEL} — the claim moved exactly the leaf's amount"
    );
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — and it reached the named depositor"
    );
    assert_eq!(
        f.wallet(&f.rogue_wallet).await,
        0,
        "{LABEL} — nothing reached anyone else"
    );
}

// ===========================================================================
// INVARIANT 11  (GO-gate ruling, 2026-11-26)
// ===========================================================================

/// **No payout leaves the vault before the challenge window has passed.**
///
/// A root's payouts used to be live from posting. The GO gate closed that
/// door, and the reason is the rest of the design: the backend key posts
/// roots AND opens the assisted door, a leaf may name any pubkey, the only
/// ceiling is the vault, and the bond is unrelated to the vault — a
/// compromised backend could drain the vault before anyone could challenge.
/// So inside the window the claim refuses through BOTH doors with
/// `ChallengeWindowOpen` and moves nothing: no wallet, not the vault, no
/// marker, `paid_out` still zero. After the clock passes the window, the
/// SAME claim — same leaf, same proof, same door — pays. (The other way
/// through the window, the multisig ruling the root stands, is test 5.)
#[tokio::test]
async fn no_payout_leaves_the_vault_before_the_challenge_window_has_passed() {
    const LABEL: &str = "invariant 11";
    let mut f = fixture().await;
    f.prepay(LABEL, 5 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;
    let tree = f.post(LABEL, 1, &[f.leaf(0, 3 * USDC)]).await;

    // Inside the window: both doors refuse, and nothing moves.
    for (door, authority) in [("direct", &f.depositors[0]), ("assisted", &f.backend)] {
        let step = format!("A claims leaf 0 through the {door} door INSIDE the challenge window");
        let err = refused(
            LABEL,
            &step,
            f.claim(authority, 1, &tree, 0, &f.dep_wallets[0]),
        )
        .await;
        assert_code(LABEL, &step, &err, CHALLENGE_WINDOW_OPEN);
    }
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        0,
        "{LABEL} — nothing reached the depositor inside the window"
    );
    assert_eq!(
        f.vault().await,
        5 * USDC,
        "{LABEL} — nothing left the vault inside the window"
    );
    assert!(
        common::claim_marker(f.banks(), &addrs::root(1), 0)
            .await
            .is_none(),
        "{LABEL} — a refused claim writes no marker"
    );
    let root = f.root(1).await;
    assert_eq!(
        (root.status, root.paid_out),
        (pool::RootStatus::Active, 0),
        "{LABEL} — the root is merely waiting; nothing was paid out"
    );

    // The clock passes the window: the SAME claim pays.
    f.pass_challenge_window(1).await;
    lands(
        LABEL,
        "the SAME claim after the challenge window has passed",
        f.claim(&f.depositors[0], 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.wallet(&f.dep_wallets[0]).await,
        3 * USDC,
        "{LABEL} — once the window has passed, the claim pays"
    );
    assert_eq!(
        f.vault().await,
        2 * USDC,
        "{LABEL} — the vault paid exactly the leaf, and only now"
    );
    assert_eq!(
        f.root(1).await.paid_out,
        3 * USDC,
        "{LABEL} — the payout is on the root's record (invariant 12)"
    );
}

// ===========================================================================
// INVARIANT 12  (GO-gate ruling, 2026-11-26)
// ===========================================================================

/// **A root never pays out more than its declared total.**
///
/// The declared total is the operator's ATTESTATION; the leaves are what
/// claimants present, and only claims reveal any gap between them. A tree
/// whose leaves sum to 6 USDC is posted DECLARING 4 USDC: claims are valid
/// (right proof, right door, window passed) one by one until the root has
/// paid exactly its declared 4 — the leaf that would overshoot refuses with
/// `PayoutExceedsTotal` and moves nothing, a smaller leaf that fits still
/// pays, and once the total is reached every leaf refuses. The vault gave 4,
/// never 6.
#[tokio::test]
async fn a_root_never_pays_out_more_than_its_declared_total() {
    const LABEL: &str = "invariant 12";
    let mut f = fixture().await;
    f.prepay(LABEL, 10 * USDC).await;
    f.fund_bond(LABEL, 10 * USDC).await;

    // Leaves sum to 6 USDC; the root DECLARES 4 USDC.
    let tree = merkle::Tree::new(&[f.leaf(0, 3 * USDC), f.leaf(1, 2 * USDC), f.leaf(2, USDC)]);
    f.post_declaring(LABEL, 1, &tree, 4 * USDC).await;
    let root = f.root(1).await;
    assert_eq!(
        (root.total_payout, root.paid_out),
        (4 * USDC, 0),
        "{LABEL} — the root declares 4 USDC and has paid nothing"
    );
    f.pass_challenge_window(1).await;

    // 3 USDC fits (3 ≤ 4).
    lands(
        LABEL,
        "A claims leaf 0: 3 USDC of a declared 4",
        f.claim(&f.depositors[0], 1, &tree, 0, &f.dep_wallets[0]),
    )
    .await;
    assert_eq!(
        f.root(1).await.paid_out,
        3 * USDC,
        "{LABEL} — the payout is counted on the root"
    );

    // B's 2 USDC does NOT fit (3 + 2 > 4) — although B's leaf is in the
    // tree and B's proof is valid.
    let step = "B claims leaf 1: 2 USDC on top of 3 would pass the declared 4";
    let err = refused(
        LABEL,
        step,
        f.claim(&f.depositors[1], 1, &tree, 1, &f.dep_wallets[1]),
    )
    .await;
    assert_code(LABEL, step, &err, PAYOUT_EXCEEDS_TOTAL);
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        0,
        "{LABEL} — the overshooting claim paid nothing"
    );
    assert!(
        common::claim_marker(f.banks(), &addrs::root(1), 1)
            .await
            .is_none(),
        "{LABEL} — the overshooting claim wrote no marker"
    );
    assert_eq!(
        f.vault().await,
        7 * USDC,
        "{LABEL} — the refused claim moved nothing"
    );

    // C's 1 USDC fits exactly (3 + 1 = 4): the root has now paid EXACTLY
    // its declared total.
    lands(
        LABEL,
        "C claims leaf 2: 1 USDC completes the declared 4 exactly",
        f.claim(&f.depositors[2], 1, &tree, 2, &f.dep_wallets[2]),
    )
    .await;
    let root = f.root(1).await;
    assert_eq!(
        root.paid_out, root.total_payout,
        "{LABEL} — the root paid out EXACTLY its declared total"
    );
    assert_eq!(
        f.vault().await,
        6 * USDC,
        "{LABEL} — the vault gave exactly the declared total, although the leaves sum to 6"
    );

    // Past the total, every remaining leaf refuses.
    let step = "B claims leaf 1 again, on a root that has paid its declared total";
    let err = refused(
        LABEL,
        step,
        f.claim(&f.depositors[1], 1, &tree, 1, &f.dep_wallets[1]),
    )
    .await;
    assert_code(LABEL, step, &err, PAYOUT_EXCEEDS_TOTAL);
    assert_eq!(
        f.wallet(&f.dep_wallets[1]).await,
        0,
        "{LABEL} — B was never paid"
    );
    assert_eq!(
        f.wallet(&f.dep_wallets[2]).await,
        USDC,
        "{LABEL} — C was paid once"
    );
}
