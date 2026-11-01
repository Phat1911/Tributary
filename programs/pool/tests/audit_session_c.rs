//! M8 SESSION C — adversarial audit PoCs (fresh-context audit, 2026-11-01;
//! workflow step 14). Attack rig for BOTH the implementation and the suite:
//! every test here is either a **finding PoC** (it asserts that a weakness
//! EXISTS — it goes red when the weakness is fixed, which is the handoff
//! contract for the fix session) or **attempted-clean evidence** (a guard
//! the doctrine suite never pinned, attacked through the public surface and
//! proving the code holds).
//!
//! Reading order (finding ids refer to AUDIT.md rows):
//! - `c1_*` — A-041: `initialize` is permissionless; the FIRST caller
//!   captures the singleton (front-run window between deploy and the
//!   operator's initialize; lib.rs claims "no window where a single dev
//!   key administers the pool").
//! - `c2_*` — A-042: emitted mirror events report PRE-CPI values (Anchor
//!   deserializes accounts once at entry and never reloads after the CPI):
//!   `vault_balance_after` / `supply_after` are stale by one instruction.
//! - `c3_*` — A-043: `prepay_renter`'s `renter` ARG is not bound to the
//!   paying `renter_key` — money can be prepaid INTO a third identity's
//!   record (misattribution; conservation holds, so low severity).
//! - `c4_*` — attempted-clean: rule 9 at the token layer (a real transfer
//!   attempt, which the doctrine suite never makes — it only checks the
//!   extension flag).
//! - `c5_*` — attempted-clean: guards the suite never pinned (burn while
//!   paused, direct-door refund overdraw, zero refund, admin rotation
//!   through the public surface).
//! - `c6_*` — directed composition chains (record → pause → burn → resume
//!   → refund; rotation mid-sequence; rotation cannot imprison the direct
//!   door) with both D40 mirrors asserted after every leg.
//! - `c7_*` — deterministic sequence fuzz: random legal-and-illegal
//!   interleavings of the whole instruction alphabet; after EVERY
//!   transaction, custody conservation (`vault == Σ prepaid`) and registry
//!   conservation (`supply == Σ claims`) must hold exactly.
//!
//! Devnet is untouched (local rig only). This file never edits product
//! code or the doctrine suites; findings route back through the user.

mod common;

use common::{addrs, ixs, start};
use solana_instruction::AccountMeta;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program_test::BanksClient;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
// VersionedTransaction: simulate_transaction takes the versioned shape;
// `From<Transaction>` converts our signed legacy tx into it.
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Small local helpers (no new dependencies — everything hand-rolled).
// ---------------------------------------------------------------------------

/// Deterministic xorshift64* PRNG so every fuzz run is reproducible from
/// its seed (a finding must replay bit-for-bit).
struct Rng(u64);

impl Rng {
    /// Next raw u64 (xorshift64* — one multiply avoids short-cycle low bits).
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish pick in 0..n.
    fn range(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Random element of a slice.
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.range(xs.len() as u64) as usize]
    }

    /// Random bool.
    fn flip(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

/// Standard-alphabet base64 decode (padding-aware) — needed because
/// Anchor's `emit!` lands in the log stream as `Program data: <base64>`
/// via `sol_log_data`, and the harness deliberately carries no base64 dep.
fn b64_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0, // '=' padding and anything else contribute nothing
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let b0 = val(chunk[0]) as u8;
        let b1 = val(chunk[1]) as u8;
        out.push((b0 << 2) | (b1 >> 4));
        if chunk.len() > 2 {
            let b2 = val(chunk[2]) as u8;
            out.push((b1 << 4) | (b2 >> 2));
            if chunk.len() > 3 {
                let b3 = val(chunk[3]) as u8;
                out.push((b2 << 6) | b3);
            }
        }
    }
    out
}

/// Little-endian u64 at `off` (event field reads).
fn le_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

/// Runs the instructions in SIMULATION (no state commit) and returns the
/// program log lines — BanksClient's simulate path is the one surface that
/// exposes logs; `emit!` events appear as `Program data: <base64>`.
async fn simulate_logs(
    banks: &BanksClient,
    payer: &Keypair,
    instructions: Vec<solana_instruction::Instruction>,
    signers: &[&Keypair],
) -> Vec<String> {
    let blockhash = banks.get_latest_blockhash().await.unwrap();
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend(signers.iter());
    let mut tx = Transaction::new_unsigned(Message::new(&instructions, Some(&payer.pubkey())));
    tx.sign(&all, blockhash);
    let res = banks
        .simulate_transaction(VersionedTransaction::from(tx))
        .await
        .expect("simulation runs");
    res.simulation_details
        .expect("simulation details (logs) present")
        .logs
}

/// Finds the first `Program data:` event whose 32-byte identity field at
/// offset 8 (after the 8-byte event discriminator) equals `identity` —
/// every event in this program leads with the acted-on pubkey, so this
/// names the event without trusting log text.
fn find_event(logs: &[String], identity: &Pubkey) -> Option<Vec<u8>> {
    for line in logs {
        if let Some(payload) = line.strip_prefix("Program data: ") {
            let bytes = b64_decode(payload);
            if bytes.len() >= 40 && &bytes[8..40] == identity.as_ref() {
                return Some(bytes);
            }
        }
    }
    None
}

// Raw Token-2022 wire builders (tags verified against
// spl-token-2022-interface 3.1.2 `instruction.rs`; same shapes as the
// harness's Token-v3 builders — Token-2022 numbers only the extensions
// differently, and InitializeNonTransferableMint is tag 32, NOT 36).

/// Token-2022 `InitializeNonTransferableMint` (tag 32): accounts = [mint].
fn t22_init_non_transferable(mint: &Pubkey) -> solana_instruction::Instruction {
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![AccountMeta::new(*mint, false)],
        data: vec![32u8],
    }
}

/// Token-2022 `InitializeImmutableOwner` (tag 22): accounts = [account].
/// NonTransferable mints refuse to mint into accounts whose ownership is
/// not immutable (NonTransferableNeedsImmutableOwnership) — the pool's
/// claim rows carry this via anchor's token-account init; the control
/// mint below must mirror it.
fn t22_init_immutable_owner(acct: &Pubkey) -> solana_instruction::Instruction {
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![AccountMeta::new(*acct, false)],
        data: vec![22u8],
    }
}

/// Token-2022 `InitializeMint2` (tag 20): data = decimals, mint authority,
/// freeze-authority COption (None here).
fn t22_init_mint2(mint: &Pubkey, authority: &Pubkey) -> solana_instruction::Instruction {
    let mut data = vec![20u8, pool::DECIMALS];
    data.extend_from_slice(&authority.to_bytes());
    data.extend_from_slice(&[0u8; 4]);
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![AccountMeta::new(*mint, false)],
        data,
    }
}

/// Token-2022 `InitializeAccount3` (tag 18): accounts = [account, mint].
fn t22_init_account3(
    acct: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
) -> solana_instruction::Instruction {
    let mut data = vec![18u8];
    data.extend_from_slice(&owner.to_bytes());
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![
            AccountMeta::new(*acct, false),
            AccountMeta::new_readonly(*mint, false),
        ],
        data,
    }
}

/// Token-2022 `MintTo` (tag 7): accounts = [mint, dest, authority].
fn t22_mint_to(
    mint: &Pubkey,
    dest: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> solana_instruction::Instruction {
    let mut data = vec![7u8];
    data.extend_from_slice(&amount.to_le_bytes());
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![
            AccountMeta::new(*mint, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// Token-2022 `Transfer` (tag 3): accounts = [source, dest, authority].
/// `auth_signer` marks the authority meta: true when a real key will sign
/// (control experiments), false when the "authority" is a PDA nobody can
/// sign as (the pool claim-account attempt — the runtime refusal, not a
/// client-side signing panic, is the evidence).
fn t22_transfer(
    source: &Pubkey,
    dest: &Pubkey,
    authority: &Pubkey,
    auth_signer: bool,
    amount: u64,
) -> solana_instruction::Instruction {
    let mut data = vec![3u8];
    data.extend_from_slice(&amount.to_le_bytes());
    solana_instruction::Instruction {
        program_id: common::TOKEN_2022,
        accounts: vec![
            AccountMeta::new(*source, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*authority, auth_signer),
        ],
        data,
    }
}

/// Creates a Token-2022 mint, optionally with the NonTransferable
/// extension initialized BEFORE base state (the program's own order), and
/// returns its address. Space: 82 base + 4 TLV when locked.
async fn t22_mint(banks: &BanksClient, payer: &Keypair, non_transferable: bool) -> Pubkey {
    let mint = Keypair::new();
    // Extension-aware mint size. Empirical anchor: the pool's OWN
    // NonTransferable claim mint lands at 170 bytes on-chain (asserted by
    // the println in c4) — the runtime builtin accepts init at that size
    // and refuses the minimal 82+1+4 shape. Match the program's shape.
    let len = if non_transferable { 170 } else { 82 };
    let rent = banks.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(len);
    let create = solana_system_interface::instruction::create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        lamports,
        len as u64,
        &common::TOKEN_2022,
    );
    let mut ixs = vec![create];
    if non_transferable {
        ixs.push(t22_init_non_transferable(&mint.pubkey()));
    }
    ixs.push(t22_init_mint2(&mint.pubkey(), &payer.pubkey()));
    common::send(banks, payer, ixs, &[&mint]).await.unwrap();
    mint.pubkey()
}

/// Creates a Token-2022 token account (owner = `owner`). `immutable` adds
/// the ImmutableOwner extension first (required by NonTransferable mints).
async fn t22_account(
    banks: &BanksClient,
    payer: &Keypair,
    owner: &Pubkey,
    mint: &Pubkey,
    immutable: bool,
) -> Pubkey {
    let acct = Keypair::new();
    let rent = banks.get_rent().await.unwrap();
    // Empirical anchor: the pool's own claim ROWS (Token-2022 accounts
    // with ImmutableOwner, anchor-created) land at 174 bytes on-chain
    // (asserted by the println in c4); the minimal 165+1+4 shape is
    // refused. Mirror the program's shape.
    let len = if immutable { 174 } else { 165 };
    let lamports = rent.minimum_balance(len);
    let create = solana_system_interface::instruction::create_account(
        &payer.pubkey(),
        &acct.pubkey(),
        lamports,
        len as u64,
        &common::TOKEN_2022,
    );
    let mut ixs = vec![create];
    if immutable {
        ixs.push(t22_init_immutable_owner(&acct.pubkey()));
    }
    ixs.push(t22_init_account3(&acct.pubkey(), owner, mint));
    common::send(banks, payer, ixs, &[&acct]).await.unwrap();
    acct.pubkey()
}

// ---------------------------------------------------------------------------
// Fixture: one initialized pool, one depositor identity, one funded renter.
// ---------------------------------------------------------------------------

struct Fixture {
    banks: BanksClient,
    payer: Keypair,
    admin: Keypair,
    backend: Keypair,
    depositor: Keypair,
    renter: Keypair,
    renter_source: Pubkey,
    renter_dest: Pubkey,
    usdc_mint: Pubkey,
}

impl Fixture {
    /// record_contribution via the assisted signer (backend).
    async fn record(&self, depositor: &Pubkey, amount: u64) {
        common::send(
            &self.banks,
            &self.payer,
            vec![ixs::record_contribution(
                &addrs::pool(),
                &self.backend.pubkey(),
                depositor,
                &addrs::claim_mint(),
                &addrs::claim(depositor),
                amount,
            )],
            &[&self.backend],
        )
        .await
        .expect("setup record lands");
    }

    /// prepay_renter from the fixture wallet, honestly labeled.
    async fn prepay(&self, amount: u64) {
        common::send(
            &self.banks,
            &self.payer,
            vec![ixs::prepay_renter(
                &addrs::pool(),
                &self.backend.pubkey(),
                &self.renter.pubkey(),
                &self.renter_source,
                &addrs::vault(),
                &addrs::prepaid(&self.renter.pubkey()),
                &self.renter.pubkey(),
                amount,
            )],
            &[&self.backend, &self.renter],
        )
        .await
        .expect("setup prepay lands");
    }
}

async fn fixture() -> Fixture {
    let (banks, payer) = start().await;
    let usdc_mint = common::create_usdc_mint(&banks, &payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let depositor = Keypair::new();
    let renter = Keypair::new();
    let renter_source =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    let renter_dest =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    common::faucet_usdc(&banks, &payer, &usdc_mint, &renter_source, 20_000_000).await;
    common::fund(&banks, &payer, &backend.pubkey(), 5_000_000_000).await;
    common::send(
        &banks,
        &payer,
        vec![ixs::initialize(
            &payer,
            &usdc_mint,
            &admin.pubkey(),
            &backend.pubkey(),
        )],
        &[],
    )
    .await
    .unwrap();
    Fixture {
        banks,
        payer,
        admin,
        backend,
        depositor,
        renter,
        renter_source,
        renter_dest,
        usdc_mint,
    }
}

// ---------------------------------------------------------------------------
// A-041 — permissionless initialize (deployment front-run)
// ---------------------------------------------------------------------------

/// FINDING PoC (A-041, medium): ANY funded key can call `initialize`
/// before the operator does — the first caller chooses admin AND backend,
/// so a single attacker key captures the singleton for this deployment.
/// The operator's own initialize is then refused forever (the pool PDA is
/// occupied), and the captured backend can attest claims for arbitrary
/// depositor identities. Every step is individually legal; the governance
/// invariant ("admin-governed by 2-of-2 from its first block — there is
/// no window") breaks at the end of the chain.
#[tokio::test]
async fn c1_initialize_front_run_captures_the_singleton() {
    let (banks, payer) = start().await;
    let usdc_mint = common::create_usdc_mint(&banks, &payer).await;

    // The attacker: any key with rent money. They initialize FIRST —
    // nothing gates who may call initialize (the payer receives no
    // authority, which is exactly what makes the first caller free).
    let attacker = Keypair::new();
    common::fund(&banks, &payer, &attacker.pubkey(), 5_000_000_000).await;
    common::send(
        &banks,
        &attacker,
        vec![ixs::initialize(
            &attacker,
            &usdc_mint,
            &attacker.pubkey(), // attacker admin (a single key!)
            &attacker.pubkey(), // attacker backend (the attester)
        )],
        &[],
    )
    .await
    .expect("attacker initialize must land — nothing gates the caller");

    // The singleton is captured: admin AND backend are the attacker's key.
    let pool_ai = banks.get_account(addrs::pool()).await.unwrap().unwrap();
    let st: pool::Pool =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &pool_ai.data[..]).unwrap();
    assert_eq!(st.admin_authority, attacker.pubkey(), "attacker is admin");
    assert_eq!(
        st.backend_authority,
        attacker.pubkey(),
        "attacker is the attester"
    );

    // The operator's initialize is now permanently refused.
    let err = common::send(
        &banks,
        &payer,
        vec![ixs::initialize(
            &payer,
            &usdc_mint,
            &payer.pubkey(),
            &payer.pubkey(),
        )],
        &[],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "operator initialize must be refused (singleton occupied)"
    );

    // Blast radius demo: the captured backend attests unbacked claims for
    // a depositor identity that never paid anything (A-039's rogue-backend
    // shape — but reached without ANY operator key compromise).
    let victim = Keypair::new();
    common::send(
        &banks,
        &attacker,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &attacker.pubkey(),
            &victim.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&victim.pubkey()),
            1_000_000,
        )],
        &[&attacker],
    )
    .await
    .expect("captured backend attests");
    assert_eq!(
        common::token_2022_balance(&banks, &addrs::claim(&victim.pubkey())).await,
        1_000_000,
        "unbacked claims minted under attacker governance"
    );
}

// ---------------------------------------------------------------------------
// A-042 — emitted mirror events report PRE-CPI (stale) values
// ---------------------------------------------------------------------------

/// FINDING PoC (A-042, part 1 of 3): `PrepayRecorded.vault_balance_after`
/// is read from Anchor's deserialized-at-entry copy of the vault account —
/// the CPI transfer mutates on-chain state but the in-memory copy is
/// never reloaded, so the event emits the PRE-transfer balance while the
/// chain state moves. This test asserts the BUG: emitted == 0 (stale)
/// while the executed truth is 5_000_000. Fix session: flip to 5_000_000.
#[tokio::test]
async fn c2a_prepay_event_emits_stale_vault_balance() {
    let f = fixture().await;

    // Simulate first (identical code path; logs are only observable there).
    let logs = simulate_logs(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            5_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await;
    // Event body after the 8-byte discriminator: renter(32) amount(8)
    // vault_balance_after(8) prepaid_after(8).
    let ev = find_event(&logs, &f.renter.pubkey()).expect("PrepayRecorded event emitted");
    assert_eq!(
        le_u64(&ev, 48),
        0,
        "A-042: emitted vault_balance_after is the PRE-CPI balance (stale)"
    );
    assert_eq!(
        le_u64(&ev, 56),
        5_000_000,
        "program-owned state (prepaid) is fresh — only CPI-written state is stale"
    );

    // Execute the same instruction for real: the chain truth the event
    // claimed to mirror is 5_000_000 — the emitted mirror is wrong.
    f.prepay(5_000_000).await;
    assert_eq!(
        common::token_balance(&f.banks, &addrs::vault()).await,
        5_000_000,
        "on-chain truth diverges from the emitted mirror"
    );
}

/// FINDING PoC (A-042, part 2 of 3): `PrepayRefunded.vault_balance_after`
/// emits the PRE-refund vault balance (stale copy) — here 5_000_000 while
/// the refund that just executed leaves 2_000_000.
#[tokio::test]
async fn c2b_refund_event_emits_stale_vault_balance() {
    let f = fixture().await;
    f.prepay(5_000_000).await;

    let logs = simulate_logs(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.renter.pubkey(), // direct door — the renter themselves
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            3_000_000,
        )],
        &[&f.renter],
    )
    .await;
    let ev = find_event(&logs, &f.renter.pubkey()).expect("PrepayRefunded event emitted");
    assert_eq!(
        le_u64(&ev, 48),
        5_000_000,
        "A-042: emitted vault_balance_after is the PRE-refund balance (stale)"
    );

    // Executed truth: vault 5_000_000 - 3_000_000 = 2_000_000.
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.renter.pubkey(),
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            3_000_000,
        )],
        &[&f.renter],
    )
    .await
    .unwrap();
    assert_eq!(
        common::token_balance(&f.banks, &addrs::vault()).await,
        2_000_000,
        "on-chain truth diverges from the emitted mirror"
    );
}

/// FINDING PoC (A-042, part 3 of 3): `ContributionRecorded.supply_after`
/// emits the PRE-mint supply — 0 on the registry's very first record,
/// while the mint that just executed holds 10_000_000. A reconciler
/// subscribing to events (lib.rs's stated M11 design) rebuilds a mirror
/// that is wrong by exactly one instruction on every event.
#[tokio::test]
async fn c2c_record_event_emits_stale_supply() {
    let f = fixture().await;

    let logs = simulate_logs(
        &f.banks,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            10_000_000,
        )],
        &[&f.backend],
    )
    .await;
    // Event body after the discriminator: depositor(32) amount(8)
    // supply_after(8).
    let ev = find_event(&logs, &f.depositor.pubkey()).expect("ContributionRecorded emitted");
    assert_eq!(
        le_u64(&ev, 48),
        0,
        "A-042: emitted supply_after is the PRE-mint supply (stale)"
    );

    // Executed truth: the supply is 10_000_000.
    f.record(&f.depositor.pubkey(), 10_000_000).await;
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        common::claim_supply(&mint_ai.data),
        10_000_000,
        "on-chain truth diverges from the emitted mirror"
    );
}

// ---------------------------------------------------------------------------
// A-043 — prepay identity not bound to the paying key
// ---------------------------------------------------------------------------

/// FINDING PoC (A-043, low): the `renter` ARG names whose record grows;
/// the money's owner is checked only against `renter_key`. With the
/// backend co-signing, wallet-owner A prepays money that lands as a
/// refundable balance for identity B — who never signed anything and can
/// then exit with it through the direct door. Conservation holds (no
/// solvency break — vault == Σ prepaid throughout), so this is
/// misattribution, not loss; the drift risk lands in M10/M11 attribution.
#[tokio::test]
async fn c3_prepay_can_credit_a_third_identitys_record() {
    let f = fixture().await;
    let bystander = Keypair::new();

    // renter_key = the fixture renter (owns the 20 USDC wallet); the
    // `renter` ARG = the bystander. Both required signers sign; the
    // instruction has all it asks for — and credits the wrong identity.
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(), // renter_key: source owner
            &f.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&bystander.pubkey()), // record PDA of B
            &bystander.pubkey(),                  // ...labeled as B
            5_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .expect("mislabeled prepay lands (renter arg is unbound)");

    // B's record now holds A's money — B never signed anything.
    assert_eq!(
        common::prepaid_amount(&f.banks, &bystander.pubkey()).await,
        5_000_000,
        "third identity's record grew from A's wallet"
    );

    // B exits with it through the DIRECT door (B's own key).
    let bystander_dest =
        common::create_token_account(&f.banks, &f.payer, &bystander.pubkey(), &f.usdc_mint).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &bystander.pubkey(),
            &bystander_dest,
            &addrs::vault(),
            &addrs::prepaid(&bystander.pubkey()),
            &bystander.pubkey(),
            5_000_000,
        )],
        &[&bystander],
    )
    .await
    .expect("bystander direct-door refund");
    assert_eq!(
        common::token_balance(&f.banks, &bystander_dest).await,
        5_000_000,
        "money left the vault to a non-payer"
    );

    // Conservation holds (why this is LOW): vault empty, A's wallet down.
    assert_eq!(common::token_balance(&f.banks, &addrs::vault()).await, 0);
    assert_eq!(
        common::token_balance(&f.banks, &f.renter_source).await,
        15_000_000
    );
}

// ---------------------------------------------------------------------------
// Attempted-clean — rule 9 at the token layer (the suite checks only the
// extension FLAG; this rig makes a real transfer attempt)
// ---------------------------------------------------------------------------

/// ATTEMPTED-CLEAN (rule 9): (a) a raw Token-2022 transfer between the
/// pool's claim accounts is refused (they are pool-PDA-owned, so no key
/// can even authorize it — the deeper lock); (b) CONTROL experiment on
/// fresh mints with a SIGNABLE authority: the same wire transfer succeeds
/// on a normal mint and is refused on a NonTransferable mint — isolating
/// the extension itself as the refusing layer, not signature mechanics.
#[tokio::test]
async fn c4_rule9_transfer_attempted_at_token_layer() {
    let f = fixture().await;
    f.record(&f.depositor.pubkey(), 10_000_000).await;
    // Evidence: the program-created NonTransferable mint's on-chain size
    // (the control mints below must match this extension-aware shape).
    let claim_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    println!("claim mint account len = {}", claim_ai.data.len());
    let claim_row_ai = f
        .banks
        .get_account(addrs::claim(&f.depositor.pubkey()))
        .await
        .unwrap()
        .unwrap();
    println!("claim row account len = {}", claim_row_ai.data.len());
    let other = Keypair::new();
    f.record(&other.pubkey(), 10_000_000).await;
    let claim_a = addrs::claim(&f.depositor.pubkey());
    let claim_b = addrs::claim(&other.pubkey());

    // (a) Raw transfer between claim accounts; authority = pool PDA (the
    // accounts' owner — nobody outside the program can sign it; the meta
    // is non-signer so the RUNTIME refuses, which is the observation).
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![t22_transfer(
            &claim_a,
            &claim_b,
            &addrs::pool(),
            false,
            1_000_000,
        )],
        &[],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "raw claim transfer must be refused"
    );
    assert_eq!(
        common::token_2022_balance(&f.banks, &claim_a).await,
        10_000_000,
        "registry rows untouched"
    );

    // (b) Control pair on signable mints.
    let normal = t22_mint(&f.banks, &f.payer, false).await;
    let locked = t22_mint(&f.banks, &f.payer, true).await;
    for (mint, expect_ok) in [(normal, true), (locked, false)] {
        let immutable = !expect_ok; // locked mint ⇒ immutable-owner accounts
        let src = t22_account(&f.banks, &f.payer, &f.payer.pubkey(), &mint, immutable).await;
        let dst = t22_account(&f.banks, &f.payer, &f.payer.pubkey(), &mint, immutable).await;
        common::send(
            &f.banks,
            &f.payer,
            vec![t22_mint_to(&mint, &src, &f.payer.pubkey(), 1_000_000)],
            &[],
        )
        .await
        .unwrap();
        let res = common::send(
            &f.banks,
            &f.payer,
            vec![t22_transfer(&src, &dst, &f.payer.pubkey(), true, 500_000)],
            &[],
        )
        .await;
        if expect_ok {
            res.expect("control transfer on a normal mint succeeds");
            assert_eq!(common::token_2022_balance(&f.banks, &dst).await, 500_000);
        } else {
            res.expect_err("NonTransferable mint must refuse even a signed transfer");
            assert_eq!(
                common::token_2022_balance(&f.banks, &src).await,
                1_000_000,
                "locked-mint balance must not move"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Attempted-clean — guards the doctrine suite never pinned
// ---------------------------------------------------------------------------

/// ATTEMPTED-CLEAN (four suite gaps, code proven correct here):
/// (a) `burn_claims` while paused refuses (the journey pauses
/// record/prepay/refund but never burn); (b) refund overdraw through the
/// DIRECT door refuses (the suite pins the assisted door only — doctrine
/// demands identical ceilings on both doors); (c) refund of zero refuses;
/// (d) `set_admin_authority` rotates through the public surface and the
/// RETIRED admin immediately loses every lever (never exercised in any
/// suite; only the devnet soak touched it).
#[tokio::test]
async fn c5_untested_guards_hold() {
    let f = fixture().await;
    f.record(&f.depositor.pubkey(), 10_000_000).await;
    f.prepay(5_000_000).await;

    // (a) pause, then burn through BOTH doors must refuse.
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();
    for signer in [&f.backend, &f.depositor] {
        let err = common::send(
            &f.banks,
            &f.payer,
            vec![ixs::burn_claims(
                &addrs::pool(),
                &signer.pubkey(),
                &f.depositor.pubkey(),
                &addrs::claim_mint(),
                &addrs::claim(&f.depositor.pubkey()),
                1_000_000,
            )],
            &[signer],
        )
        .await
        .unwrap_err();
        assert_eq!(common::custom_code(&err), Some(6000), "Paused");
    }
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("resume", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();

    // (b) DIRECT-door refund overdraw: 1 micro-unit over the record.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.renter.pubkey(), // direct door — the renter's own key
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            5_000_001,
        )],
        &[&f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6008), "PrepaidInsufficient");
    assert_eq!(
        common::token_balance(&f.banks, &addrs::vault()).await,
        5_000_000,
        "vault untouched by the refused overdraw"
    );

    // (c) zero-amount refund refuses.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            0,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6001), "ZeroAmount");

    // (d) admin rotation: old admin loses pause; new admin gains it.
    let admin2 = Keypair::new();
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::rotate(
            "set_admin_authority",
            &addrs::pool(),
            &f.admin.pubkey(),
            &admin2.pubkey(),
        )],
        &[&f.admin],
    )
    .await
    .unwrap();
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "retired admin must not pause"
    );
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &admin2.pubkey())],
        &[&admin2],
    )
    .await
    .expect("new admin pauses");
}

// ---------------------------------------------------------------------------
// Directed composition chains (kickoff's named shapes)
// ---------------------------------------------------------------------------

/// D40 mirrors, asserted from any state: vault USDC == Σ prepaid records
/// and claim supply == Σ claim balances. Called after EVERY instruction
/// (or refused attempt) in the chains and the fuzzer.
async fn assert_mirrors(
    banks: &BanksClient,
    prepaid_ids: &HashSet<Pubkey>,
    claim_ids: &HashSet<Pubkey>,
) {
    let vault = common::token_balance(banks, &addrs::vault()).await;
    let mut sum_prepaid = 0u64;
    for id in prepaid_ids {
        if let Some(ai) = banks.get_account(addrs::prepaid(id)).await.unwrap() {
            let rec: pool::RenterPrepaid =
                anchor_lang::AccountDeserialize::try_deserialize(&mut &ai.data[..]).unwrap();
            sum_prepaid = sum_prepaid.saturating_add(rec.amount);
        }
    }
    assert_eq!(
        vault, sum_prepaid,
        "custody conservation: vault == Σ prepaid records"
    );
    let mut sum_claims = 0u64;
    for id in claim_ids {
        if let Some(ai) = banks.get_account(addrs::claim(id)).await.unwrap() {
            sum_claims = sum_claims.saturating_add(common::claim_amount(&ai.data));
        }
    }
    let mint_ai = banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        common::claim_supply(&mint_ai.data),
        sum_claims,
        "registry conservation: supply == Σ claim balances"
    );
}

/// DIRECTED CHAINS (attempted-clean, mirrors after every leg):
/// (1) record → pause → burn REFUSED → resume → burn → prepay → refund —
///     pause defers both exit doors but loses nothing, and the resumed
///     state accepts the full exit;
/// (2) rotation mid-sequence: records under the OLD backend land, the
///     vote rotates, and the old key's very next attestation refuses —
///     no post-rotation window;
/// (3) rotation cannot imprison: after rotating the backend, the renter's
///     DIRECT door still refunds with the operator entirely absent.
#[tokio::test]
async fn c6_directed_chains_hold_the_invariants() {
    let f = fixture().await;
    let mut prepaid_ids = HashSet::new();
    let mut claim_ids = HashSet::new();
    let dep = f.depositor.pubkey();
    let renter = f.renter.pubkey();
    prepaid_ids.insert(renter);
    claim_ids.insert(dep);

    // (1) the kickoff's named chain.
    f.record(&dep, 10_000_000).await;
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &dep,
            &dep,
            &addrs::claim_mint(),
            &addrs::claim(&dep),
            2_000_000,
        )],
        &[&f.depositor],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "burn paused");
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("resume", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &dep,
            &dep,
            &addrs::claim_mint(),
            &addrs::claim(&dep),
            2_000_000,
        )],
        &[&f.depositor],
    )
    .await
    .expect("resumed burn lands");
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    f.prepay(5_000_000).await;
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &renter,
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&renter),
            &renter,
            5_000_000,
        )],
        &[&f.renter],
    )
    .await
    .expect("refund after resume");
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;

    // (2) rotation mid-sequence: old backend still valid in tx N, retired
    // in tx N+1 by the vote, refused in tx N+2.
    let backend2 = Keypair::new();
    common::fund(&f.banks, &f.payer, &backend2.pubkey(), 5_000_000_000).await;
    f.record(&dep, 1_000_000).await; // old key still valid
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::rotate(
            "set_backend_authority",
            &addrs::pool(),
            &f.admin.pubkey(),
            &backend2.pubkey(),
        )],
        &[&f.admin],
    )
    .await
    .unwrap();
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &dep,
            &addrs::claim_mint(),
            &addrs::claim(&dep),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "retired backend refused"
    );
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;

    // (3) rotation cannot imprison: direct refund under the NEW backend
    // regime, backend key absent from the transaction entirely. The prepay
    // here MUST be signed by backend2 (the rotation retired f.backend —
    // that retirement is itself part of the chain being proven).
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &backend2.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&renter),
            &renter,
            2_000_000,
        )],
        &[&backend2, &f.renter],
    )
    .await
    .expect("prepay under the rotated backend");
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &renter,
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&renter),
            &renter,
            2_000_000,
        )],
        &[&f.renter],
    )
    .await
    .expect("direct door immune to backend rotation");
    assert_mirrors(&f.banks, &prepaid_ids, &claim_ids).await;
}

// ---------------------------------------------------------------------------
// Sequence fuzz — composition is the primary method
// ---------------------------------------------------------------------------

/// Multi-identity world for the fuzzer: three renters (funded wallets),
/// three depositor identities, the two authorities, and a rogue key that
/// never holds any legitimate role.
struct World {
    banks: BanksClient,
    payer: Keypair,
    admin: Keypair,
    backend: Keypair,
    rogue: Keypair,
    renters: Vec<Keypair>,
    sources: Vec<Pubkey>,
    dests: Vec<Pubkey>,
    depositors: Vec<Keypair>,
}

async fn world() -> World {
    let (banks, payer) = start().await;
    let usdc_mint = common::create_usdc_mint(&banks, &payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let rogue = Keypair::new();
    let renters: Vec<Keypair> = (0..3).map(|_| Keypair::new()).collect();
    let mut sources = Vec::new();
    let mut dests = Vec::new();
    for r in &renters {
        sources.push(common::create_token_account(&banks, &payer, &r.pubkey(), &usdc_mint).await);
        dests.push(common::create_token_account(&banks, &payer, &r.pubkey(), &usdc_mint).await);
        common::faucet_usdc(
            &banks,
            &payer,
            &usdc_mint,
            sources.last().unwrap(),
            20_000_000,
        )
        .await;
    }
    let depositors: Vec<Keypair> = (0..3).map(|_| Keypair::new()).collect();
    common::fund(&banks, &payer, &backend.pubkey(), 5_000_000_000).await;
    common::send(
        &banks,
        &payer,
        vec![ixs::initialize(
            &payer,
            &usdc_mint,
            &admin.pubkey(),
            &backend.pubkey(),
        )],
        &[],
    )
    .await
    .unwrap();
    World {
        banks,
        payer,
        admin,
        backend,
        rogue,
        renters,
        sources,
        dests,
        depositors,
    }
}

/// One random operation, shaped from `rng` — a mix of legal moves and
/// illegal shapes (wrong door, overdraw, foreign destination, missing
/// gate signer, paused-mid-flight, mislabeled prepay). Returns
/// Ok(())/Err(..) from the bank; both outcomes are acceptable — what must
/// NEVER change is the post-state mirrors.
async fn random_op(w: &World, rng: &mut Rng, ids: &mut HashSet<Pubkey>) -> Result<(), String> {
    let choice = rng.range(10);
    let banks = &w.banks;
    let payer = &w.payer;
    let pool = addrs::pool();
    let ri = rng.range(w.renters.len() as u64) as usize;
    let di = rng.range(w.depositors.len() as u64) as usize;
    let amounts = [1u64, 500_000, 2_000_000, 5_000_000, 8_000_001, u64::MAX];
    let amount = *rng.pick(&amounts);
    let res = match choice {
        0..=1 => {
            // record_contribution: honest signer, or the rogue forging.
            let (sig, signer) = if rng.flip() {
                (w.backend.pubkey(), &w.backend)
            } else {
                (w.rogue.pubkey(), &w.rogue)
            };
            common::send(
                banks,
                payer,
                vec![ixs::record_contribution(
                    &pool,
                    &sig,
                    &w.depositors[di].pubkey(),
                    &addrs::claim_mint(),
                    &addrs::claim(&w.depositors[di].pubkey()),
                    amount,
                )],
                &[signer],
            )
            .await
        }
        2..=3 => {
            // burn_claims through a random door: depositor / backend / rogue.
            let dep = &w.depositors[di];
            let (sig, signer) = match rng.range(3) {
                0 => (dep.pubkey(), dep as &Keypair),
                1 => (w.backend.pubkey(), &w.backend),
                _ => (w.rogue.pubkey(), &w.rogue),
            };
            common::send(
                banks,
                payer,
                vec![ixs::burn_claims(
                    &pool,
                    &sig,
                    &dep.pubkey(),
                    &addrs::claim_mint(),
                    &addrs::claim(&dep.pubkey()),
                    amount,
                )],
                &[signer],
            )
            .await
        }
        4..=5 => {
            // prepay: honest, mislabeled (A-043 shape — `renter` arg names
            // a different renter), or missing the backend gate signer.
            let label = if rng.flip() {
                w.renters[rng.range(w.renters.len() as u64) as usize].pubkey()
            } else {
                w.renters[ri].pubkey()
            };
            ids.insert(label);
            if rng.flip() {
                common::send(
                    banks,
                    payer,
                    vec![ixs::prepay_renter(
                        &pool,
                        &w.backend.pubkey(),
                        &w.renters[ri].pubkey(),
                        &w.sources[ri],
                        &addrs::vault(),
                        &addrs::prepaid(&label),
                        &label,
                        amount,
                    )],
                    &[&w.backend, &w.renters[ri]],
                )
                .await
            } else {
                // rogue sits in the backend slot (has_one must refuse).
                common::send(
                    banks,
                    payer,
                    vec![ixs::prepay_renter(
                        &pool,
                        &w.rogue.pubkey(),
                        &w.renters[ri].pubkey(),
                        &w.sources[ri],
                        &addrs::vault(),
                        &addrs::prepaid(&label),
                        &label,
                        amount,
                    )],
                    &[&w.rogue, &w.renters[ri]],
                )
                .await
            }
        }
        6..=7 => {
            // refund: random door, sometimes a destination owned by a
            // DIFFERENT renter (must refuse), sometimes overdraw.
            let renter = &w.renters[ri];
            let (sig, signer) = match rng.range(3) {
                0 => (renter.pubkey(), renter as &Keypair),
                1 => (w.backend.pubkey(), &w.backend),
                _ => (w.rogue.pubkey(), &w.rogue),
            };
            let dest = if rng.flip() {
                w.dests[rng.range(w.dests.len() as u64) as usize]
            } else {
                w.dests[ri]
            };
            common::send(
                banks,
                payer,
                vec![ixs::refund_renter(
                    &pool,
                    &sig,
                    &dest,
                    &addrs::vault(),
                    &addrs::prepaid(&renter.pubkey()),
                    &renter.pubkey(),
                    amount,
                )],
                &[signer],
            )
            .await
        }
        8 => {
            // pause/resume through admin or rogue.
            let name = if rng.flip() { "pause" } else { "resume" };
            let (sig, signer) = if rng.flip() {
                (w.admin.pubkey(), &w.admin)
            } else {
                (w.rogue.pubkey(), &w.rogue)
            };
            common::send(
                banks,
                payer,
                vec![ixs::toggle(name, &pool, &sig)],
                &[signer],
            )
            .await
        }
        _ => {
            // backend rotation through admin or rogue.
            let (sig, signer) = if rng.flip() {
                (w.admin.pubkey(), &w.admin)
            } else {
                (w.rogue.pubkey(), &w.rogue)
            };
            common::send(
                banks,
                payer,
                vec![ixs::rotate(
                    "set_backend_authority",
                    &pool,
                    &sig,
                    &w.rogue.pubkey(), // rotating TO the rogue: legal by vote
                )],
                &[signer],
            )
            .await
        }
    };
    // NOTE: a successful admin rotation to the rogue key would break
    // later honest legs' signers, so the fuzzer treats that op as
    // state-only for mirrors; the mirrors never depend on authorities.
    res.map_err(|e| e.to_string())
}

/// SEQUENCE FUZZ (attempted-clean core): three fixed seeds × 90 random
/// operations across the whole instruction alphabet — every op checked
/// against BOTH conservation mirrors immediately after it lands (or is
/// refused). Composition is the attack: the invariant must survive every
/// interleaving, not just the suite's scripted journeys. Seeds are fixed:
/// a failure replays bit-for-bit.
#[tokio::test]
async fn c7_sequence_fuzz_custody_conservation() {
    for seed in [0x5EED_0001u64, 0x00C0_FFFE, 0x1A2B_3C4D] {
        let w = world().await;
        let mut rng = Rng(seed | 1); // |1: xorshift64* must not start at 0
        let mut prepaid_ids: HashSet<Pubkey> = w.renters.iter().map(|k| k.pubkey()).collect();
        let claim_ids: HashSet<Pubkey> = w.depositors.iter().map(|k| k.pubkey()).collect();
        let mut landed = 0usize;
        let mut refused = 0usize;
        for _step in 0..90 {
            match random_op(&w, &mut rng, &mut prepaid_ids).await {
                Ok(()) => landed += 1,
                Err(_) => refused += 1,
            }
            assert_mirrors(&w.banks, &prepaid_ids, &claim_ids).await;
        }
        println!(
            "seed {seed:#x}: {landed} landed / {refused} refused — both mirrors held after every op"
        );
    }
}
