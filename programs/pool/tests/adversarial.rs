//! Adversarial suite: every guard the program claims, attacked through the
//! public surface. Each negative leg pins WHICH error fired (custom code
//! for our errors, hard failure for anchor's framework gates) — "it
//! failed" alone proves nothing.
//!
//! Error codes (Anchor offset 6000 + enum index): Paused=6000,
//! ZeroAmount=6001, WrongSourceOwner=6003, WrongSourceMint=6004,
//! WrongDestinationOwner=6005, InsufficientClaims=6007,
//! PrepaidInsufficient=6008, ExitAuthority=6009.

mod common;

use common::{addrs, ixs, start};
use solana_keypair::Keypair;
use solana_program_test::BanksClient;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
// `get_extension` is a trait method in spl-token-2022-interface 3.x.
use spl_token_2022_interface::extension::BaseStateWithExtensions;

/// Same fixture as the journey suite, initialized and ready for abuse:
/// a depositor (records need no money — D40) and a renter with 20 USDC.
struct Fixture {
    banks: BanksClient,
    payer: Keypair,
    admin: Keypair,
    backend: Keypair,
    depositor: Keypair,
    renter: Keypair,
    renter_source: Pubkey,
    renter_dest: Pubkey,
    rogue: Keypair,
    usdc_mint: Pubkey,
}

impl Fixture {
    /// A recorded contribution for `depositor` (claim row lazily created).
    async fn record(&self, depositor: &Keypair, amount: u64) {
        common::send(
            &self.banks,
            &self.payer,
            vec![ixs::record_contribution(
                &addrs::pool(),
                &self.backend.pubkey(),
                &depositor.pubkey(),
                &addrs::claim_mint(),
                &addrs::claim(&depositor.pubkey()),
                amount,
            )],
            &[&self.backend],
        )
        .await
        .expect("setup record lands");
    }

    /// A renter prepay from the fixture's funded wallet.
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
    let rogue = Keypair::new();
    let renter_source =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    let renter_dest =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    common::faucet_usdc(&banks, &payer, &usdc_mint, &renter_source, 20_000_000).await;
    // The backend hot key pays claim-row and prepaid-record rent on first
    // use (the program keeps depositors and renters fee-free), so it needs
    // lamports — on devnet the operator funds it the same way.
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
        rogue,
        usdc_mint,
    }
}

// ---------------------------------------------------------------------------
// THE TWO DOORS (director-ruled first tests, 2026-10-31): assisted must
// never mean imprisoned. Proven for burn_claims, then for refund_renter.
// ---------------------------------------------------------------------------

/// Door 1 of the triad: with the backend key ABSENT from the transaction,
/// the depositor's OWN key still exits their record (direct door).
#[tokio::test]
async fn burn_direct_door_works_with_backend_absent() {
    let f = fixture().await;
    f.record(&f.depositor, 10_000_000).await;
    let claim = addrs::claim(&f.depositor.pubkey());

    // Signers: ONLY the depositor. The backend key is not merely unused —
    // it is not in the transaction at all (an offline/evil operator).
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.depositor.pubkey(), // authority = depositor's own key
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &claim,
            4_000_000,
        )],
        &[&f.depositor],
    )
    .await
    .expect("direct door must work with backend absent");
    assert_eq!(
        common::token_2022_balance(&f.banks, &claim).await,
        6_000_000
    );
    // And the exit moved no money (D40): vault untouched.
    assert_eq!(common::token_balance(&f.banks, &addrs::vault()).await, 0);
}

/// Door 2 of the triad: with the depositor absent (custodial posture —
/// rule 10), the backend still exits on their behalf (assisted door).
#[tokio::test]
async fn burn_assisted_door_works_with_depositor_absent() {
    let f = fixture().await;
    f.record(&f.depositor, 10_000_000).await;
    let claim = addrs::claim(&f.depositor.pubkey());

    // Signers: ONLY the backend. The depositor key does not exist in this
    // transaction — they are walletless by design.
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &claim,
            10_000_000,
        )],
        &[&f.backend],
    )
    .await
    .expect("assisted door must work with depositor absent");
    assert_eq!(common::token_2022_balance(&f.banks, &claim).await, 0);
}

/// Door 3 of the triad: NEITHER door — a third party's key cannot burn
/// someone else's record. Pinned to the exact custom error.
#[tokio::test]
async fn burn_refused_with_neither_door() {
    let f = fixture().await;
    f.record(&f.depositor, 10_000_000).await;

    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.rogue.pubkey(), // neither backend nor depositor
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            1_000_000,
        )],
        &[&f.rogue],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6009), "ExitAuthority");
}

// ---------------------------------------------------------------------------
// record_contribution guards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn record_requires_the_configured_backend_key() {
    let f = fixture().await;
    // A rogue key sits in the backend slot: anchor's has_one gate refuses
    // (framework error, not a custom one — a hard failure is the proof).
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.rogue.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            1_000_000,
        )],
        &[&f.rogue],
    )
    .await
    .unwrap_err();
    // Anchor's framework gate (has_one / init ordering): any failure IS
    // the refusal — the strong half of the proof is that NOTHING WAS
    // MINTED, asserted below.
    assert!(common::custom_code(&err).is_none_or(|c| c != 6000));
    // And nothing was minted.
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(common::claim_supply(&mint_ai.data), 0);
}

#[tokio::test]
async fn record_refuses_zero_amount() {
    let f = fixture().await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            0,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6001), "ZeroAmount");
}

#[tokio::test]
async fn record_refused_while_paused() {
    let f = fixture().await;
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
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "Paused");
}

// ---------------------------------------------------------------------------
// burn_claims guards beyond the doors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn burn_overdraw_refused() {
    let f = fixture().await;
    f.record(&f.depositor, 3_000_000).await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor.pubkey()),
            3_000_001,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6007), "InsufficientClaims");
}

#[tokio::test]
async fn burn_of_an_unrecorded_identity_refused() {
    let f = fixture().await;
    // A rogue names THEMSELVES — their claim row was never created (no
    // record ever landed for this identity), so the account constraint
    // refuses before the handler runs.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.rogue.pubkey(),
            &f.rogue.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.rogue.pubkey()),
            1_000_000,
        )],
        &[&f.rogue],
    )
    .await
    .unwrap_err();
    assert!(common::custom_code(&err).is_none_or(|c| c != 6000));
    // And their claim row was never born.
    assert!(
        f.banks
            .get_account(addrs::claim(&f.rogue.pubkey()))
            .await
            .unwrap()
            .is_none(),
        "no claim account may exist for an unrecorded identity"
    );
}

// ---------------------------------------------------------------------------
// prepay_renter guards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prepay_wrong_source_owner_refused() {
    let f = fixture().await;
    // A wallet owned by the rogue, not by the declared renter key.
    let rogue_wallet =
        common::create_token_account(&f.banks, &f.payer, &f.rogue.pubkey(), &f.usdc_mint).await;
    common::faucet_usdc(&f.banks, &f.payer, &f.usdc_mint, &rogue_wallet, 1_000_000).await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(), // renter_key whose wallet this is NOT
            &rogue_wallet,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6003), "WrongSourceOwner");
}

#[tokio::test]
async fn prepay_wrong_source_mint_refused() {
    let f = fixture().await;
    // A second, wrong mint ("USDT-shaped"): same decimals, different mint.
    let wrong_mint = common::create_usdc_mint(&f.banks, &f.payer).await;
    let wrong_wallet =
        common::create_token_account(&f.banks, &f.payer, &f.renter.pubkey(), &wrong_mint).await;
    common::faucet_usdc(&f.banks, &f.payer, &wrong_mint, &wrong_wallet, 1_000_000).await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &wrong_wallet,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6004), "WrongSourceMint");
}

#[tokio::test]
async fn prepay_refuses_zero_and_refuses_when_paused() {
    let f = fixture().await;
    let err = common::send(
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
            0,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6001), "ZeroAmount");

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
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "Paused");
}

#[tokio::test]
async fn prepay_without_backend_gate_refused() {
    let f = fixture().await;
    // The renter alone cannot self-serve money into the pool: prepaid is a
    // liability, and uninvited money must not create uninvited debts.
    // (The rogue key sits in the backend slot; has_one refuses.)
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.rogue.pubkey(), // not the configured backend
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.rogue, &f.renter],
    )
    .await
    .unwrap_err();
    assert!(common::custom_code(&err).is_none_or(|c| c != 6000));
    // And no liability was born: vault and record untouched.
    assert_eq!(common::token_balance(&f.banks, &addrs::vault()).await, 0);
}

// ---------------------------------------------------------------------------
// refund_renter guards — separability is an on-chain fact
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refund_exceeding_own_record_refused() {
    let f = fixture().await;
    f.prepay(5_000_000).await;
    // 1 micro-unit over the renter's OWN on-chain record: refused, even
    // by the backend (assisted door). The ceiling is the record, never an
    // operator attestation — the M10 refund rail's separability proof.
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
            5_000_001,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6008), "PrepaidInsufficient");
    // State unchanged: vault and record both still 5M.
    assert_eq!(
        common::token_balance(&f.banks, &addrs::vault()).await,
        5_000_000
    );
    assert_eq!(
        common::prepaid_amount(&f.banks, &f.renter.pubkey()).await,
        5_000_000
    );
}

#[tokio::test]
async fn refund_cannot_draw_another_renters_record() {
    let f = fixture().await;
    // Renter prepays 5M; a DIFFERENT identity tries to refund against
    // that record — by naming the renter but destining a rogue wallet
    // (destination-owner constraint), or by naming themselves (their
    // record PDA was never initialized).
    f.prepay(5_000_000).await;
    let rogue_dest =
        common::create_token_account(&f.banks, &f.payer, &f.rogue.pubkey(), &f.usdc_mint).await;

    // Shape 1: name the renter, but destination owned by the rogue.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &rogue_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(6005),
        "WrongDestinationOwner"
    );

    // Shape 2: the rogue names THEMSELVES (no record PDA ever born).
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &rogue_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.rogue.pubkey()),
            &f.rogue.pubkey(),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert!(common::custom_code(&err).is_none_or(|c| c != 6000));
    // And the renter's record is untouched.
    assert_eq!(
        common::prepaid_amount(&f.banks, &f.renter.pubkey()).await,
        5_000_000
    );
}

#[tokio::test]
async fn refund_refused_with_neither_door() {
    let f = fixture().await;
    f.prepay(2_000_000).await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.rogue.pubkey(), // neither backend nor renter
            &f.renter_dest,
            &addrs::vault(),
            &addrs::prepaid(&f.renter.pubkey()),
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.rogue],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6009), "ExitAuthority");
}

#[tokio::test]
async fn refund_direct_door_works_with_backend_absent() {
    let f = fixture().await;
    f.prepay(2_000_000).await;
    // Signers: ONLY the renter — the escape hatch for money, not just
    // records.
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
            2_000_000,
        )],
        &[&f.renter],
    )
    .await
    .expect("renter direct refund works with backend absent");
    assert_eq!(
        common::token_balance(&f.banks, &f.renter_dest).await,
        2_000_000
    );
    assert_eq!(common::token_balance(&f.banks, &addrs::vault()).await, 0);
}

// ---------------------------------------------------------------------------
// D42: no cross-pool drawing — a forged mint/account from another
// deployment can never satisfy this program's seeds.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forged_claim_mint_from_foreign_deployment_refused() {
    let f = fixture().await;
    f.record(&f.depositor, 10_000_000).await;

    // Shape 1: a foreign account in the claim_mint slot. The seeds
    // constraint fails: this program's claim mint PDA derives from ITS
    // program id — another deployment's records cannot interoperate.
    let foreign_mint = common::create_token_account(
        // (ab)using an account creation as a stand-in: any non-PDA address
        // fails the seeds check identically.
        &f.banks,
        &f.payer,
        &f.rogue.pubkey(),
        &f.usdc_mint,
    )
    .await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &foreign_mint, // NOT [pool, claim_mint]
            &addrs::claim(&f.depositor.pubkey()),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    // Refusal shape: either the seeds gate or mint-shape deserialization —
    // a foreign account can never satisfy this program's PDAs.
    assert!(
        err.to_string().contains("invalid account data")
            || err.to_string().contains("SeedsViolation")
            || err.to_string().contains("ConstraintSeeds"),
        "foreign mint must be refused: {err}"
    );

    // Shape 2: the REAL mint, but a foreign account in the claim slot —
    // its address is not [claim, <the named identity>], so the seeds gate
    // refuses this shape too.
    let foreign_claim =
        common::create_token_account(&f.banks, &f.payer, &f.rogue.pubkey(), &f.usdc_mint).await;
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.rogue.pubkey(), // seeds would demand [claim, rogue]
            &addrs::claim_mint(),
            &foreign_claim, // ...and this is not that PDA
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    // Refused (anchor's seeds gate surfaces as a raw hex code in the
    // transport error). The strong half of the proof is state: the REAL
    // depositor's row is untouched by either forgery shape.
    assert!(common::custom_code(&err).is_none_or(|c| c != 6000));
    assert_eq!(
        common::token_2022_balance(&f.banks, &addrs::claim(&f.depositor.pubkey())).await,
        10_000_000,
        "no forgery may touch the real registry rows"
    );
}

// ---------------------------------------------------------------------------
// Rule 9: the registry is locked at the token level
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rule9_claims_are_locked_not_transferable() {
    let f = fixture().await;
    f.record(&f.depositor, 10_000_000).await;
    let claim = addrs::claim(&f.depositor.pubkey());

    // (a) The mint carries the NonTransferable extension — the token
    //     program itself rejects transfers of this mint.
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    let mint = spl_token_2022_interface::extension::StateWithExtensions::<
        spl_token_2022_interface::state::Mint,
    >::unpack(&mint_ai.data)
    .unwrap();
    assert!(
        mint.get_extension::<spl_token_2022_interface::extension::non_transferable::NonTransferable>()
            .is_ok(),
        "claim mint must be NonTransferable (rule 9)"
    );

    // (b) The claim account is owned by the POOL PDA, not the depositor —
    //     there is no holder key that could burn, close, or delegate it.
    //     (Byte-compare across cohort types — see harness header note.)
    let claim_ai = f.banks.get_account(claim).await.unwrap().unwrap();
    let acct = spl_token_2022_interface::extension::StateWithExtensions::<
        spl_token_2022_interface::state::Account,
    >::unpack(&claim_ai.data)
    .unwrap();
    assert_eq!(acct.base.owner.as_ref(), addrs::pool().as_ref());

    // (c) follows structurally from (a): even if this program (or a future
    //     upgraded version) contained an instruction that CPI'd a transfer
    //     of claim tokens, the Token-2022 program rejects it — the
    //     NonTransferable extension is initialized at mint creation and
    //     can never be removed. The only sanctioned change path for this
    //     program is a multisig-voted upgrade, which is the designed
    //     governance lever, not a bypass.
}

// ---------------------------------------------------------------------------
// Admin gates
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_admin_cannot_pause_or_rotate() {
    let f = fixture().await;
    // The BACKEND itself (hot key) tries the admin levers: refused — the
    // hot key operates flows, it never governs.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.backend.pubkey())],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "pause by backend must be refused"
    );
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::rotate(
            "set_backend_authority",
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.rogue.pubkey(),
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "rotation by backend must be refused"
    );
    // A random key likewise.
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.rogue.pubkey())],
        &[&f.rogue],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "pause by rogue must be refused"
    );
    // State proof: the pool never paused.
    let pool_ai = f.banks.get_account(addrs::pool()).await.unwrap().unwrap();
    let st: pool::Pool =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &pool_ai.data[..]).unwrap();
    assert!(!st.paused, "non-admin attempts must never pause the pool");
    assert_eq!(
        st.backend_authority,
        f.backend.pubkey(),
        "backend key unchanged"
    );
}

#[tokio::test]
async fn initialize_is_once_only() {
    let f = fixture().await;
    // A second initialize on the already-born config PDA: anchor's init
    // gate refuses (the account exists).
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![ixs::initialize(
            &f.payer,
            &f.usdc_mint,
            &f.rogue.pubkey(), // would-be usurper admin
            &f.rogue.pubkey(),
        )],
        &[],
    )
    .await
    .unwrap_err();
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "second initialize must be refused"
    );
    // State proof: the usurper never became anything.
    let pool_ai = f.banks.get_account(addrs::pool()).await.unwrap().unwrap();
    let st: pool::Pool =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &pool_ai.data[..]).unwrap();
    assert_eq!(
        st.admin_authority,
        f.admin.pubkey(),
        "admin must be unchanged"
    );
    assert_eq!(st.backend_authority, f.backend.pubkey());
}
