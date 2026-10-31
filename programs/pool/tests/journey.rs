//! Journey proof (workflow step 12): the SPEC §12 custody story, walked
//! through the program's PUBLIC instruction surface only — initialize,
//! record_contribution (×2 depositors, lazy claim-row creation AND reuse),
//! burn_claims through BOTH doors, the renter prepay→refund round-trip,
//! and the admin lifecycle. Per D40 the two mirror invariants are
//! asserted after every leg, and they are SEPARATE: claim supply ==
//! Σ recorded contributions (a RECORD — no money moved), vault USDC ==
//! Σ renter prepaid (the only USDC the vault ever holds).

mod common;

use common::{addrs, ixs, start};
use solana_keypair::Keypair;
use solana_program_test::BanksClient;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
// Extension-aware Mint parsing for the Token-2022 claim registry mint.
// (Parsing lives on raw bytes; the crate's own Pubkey type never crosses
// into assertions — we compare bytes.)
use spl_token_2022_interface::extension::non_transferable::NonTransferable;
use spl_token_2022_interface::extension::{BaseStateWithExtensions, StateWithExtensions};
use spl_token_2022_interface::state as token22_state;

/// A pool fixture: payer, admin/backend keypairs (devnet postures: admin is
/// the multisig vault PDA — a plain key here, which is the same signature
/// check the Squads program satisfies on devnet), two depositor identities
/// (no USDC needed — contributions are RECORDS per D40), and a renter with
/// a funded custodial USDC wallet plus a refund destination.
struct Fixture {
    banks: BanksClient,
    payer: Keypair,
    admin: Keypair,
    backend: Keypair,
    depositor_a: Keypair,
    depositor_b: Keypair,
    renter: Keypair,
    renter_source: Pubkey,
    renter_dest: Pubkey,
    usdc_mint: Pubkey,
}

async fn fixture() -> Fixture {
    let (banks, payer) = start().await;
    let usdc_mint = common::create_usdc_mint(&banks, &payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let depositor_a = Keypair::new();
    let depositor_b = Keypair::new();
    let renter = Keypair::new();
    let renter_source =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    let renter_dest =
        common::create_token_account(&banks, &payer, &renter.pubkey(), &usdc_mint).await;
    // 30 USDC for the renter (the devnet Circle faucet stands in here) —
    // renters are the ONLY source of vault USDC (D40).
    common::faucet_usdc(&banks, &payer, &usdc_mint, &renter_source, 30_000_000).await;
    // The backend hot key pays claim-row and prepaid-record rent on first
    // use — depositors and renters stay fee-free by design, so the operator
    // key is funded (the soak client does the same on devnet).
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
        depositor_a,
        depositor_b,
        renter,
        renter_source,
        renter_dest,
        usdc_mint,
    }
}

/// Claim-side mirror: total claim supply == Σ recorded contributions.
/// If this drifts, the on-chain twin has diverged from its own registry —
/// exactly the drift M11's reconciler will alarm on internally.
async fn assert_claim_supply(f: &Fixture, expected: u64) {
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    let supply = common::claim_supply(&mint_ai.data);
    assert_eq!(supply, expected, "claim supply == Σ recorded contributions");
}

/// USDC-side mirror: vault balance == Σ renter prepaid records. Before any
/// renter arrives this is ZERO — the D40 proof that depositor principal
/// never sits in the vault.
async fn assert_vault(f: &Fixture, expected: u64) {
    let vault = common::token_balance(&f.banks, &addrs::vault()).await;
    assert_eq!(vault, expected, "vault USDC == Σ renter prepaid");
}

#[tokio::test]
async fn journey_initialize_sets_up_the_registry_locked() {
    let f = fixture().await;

    // Config account answers every mirror query in one read.
    let pool_ai = f.banks.get_account(addrs::pool()).await.unwrap().unwrap();
    let pool_state: pool::Pool =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &pool_ai.data[..]).unwrap();
    assert_eq!(pool_state.admin_authority, f.admin.pubkey());
    assert_eq!(pool_state.backend_authority, f.backend.pubkey());
    assert_eq!(pool_state.usdc_mint, f.usdc_mint);
    assert_eq!(pool_state.claim_mint, addrs::claim_mint());
    assert_eq!(pool_state.vault, addrs::vault());
    assert!(!pool_state.paused);
    assert_eq!(pool_state.reserved, [0u8; 64]);

    // Registry mint: 6 decimals (micro-USD), authority = pool PDA, and the
    // NonTransferable flag ON — rule 9 enforced by the token program.
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    let mint = StateWithExtensions::<token22_state::Mint>::unpack(&mint_ai.data).unwrap();
    assert_eq!(mint.base.decimals, pool::DECIMALS);
    // Byte-compare across cohort types (see harness header note).
    assert_eq!(
        mint.base.mint_authority.unwrap().as_ref(),
        addrs::pool().as_ref(),
        "claim mint authority must be the pool PDA"
    );
    assert!(mint.get_extension::<NonTransferable>().is_ok());

    // Fresh vault and registry: both ZERO — nothing has moved.
    assert_vault(&f, 0).await;
    assert_claim_supply(&f, 0).await;
}

#[tokio::test]
async fn journey_contributions_are_records_not_money() {
    let f = fixture().await;
    let claim_a = addrs::claim(&f.depositor_a.pubkey());
    let claim_b = addrs::claim(&f.depositor_b.pubkey());
    let one = &f.banks;

    // Contributions recorded for two depositors (25 USDC-worth and 10),
    // then a second record for depositor A (5): the first record lazily
    // creates the claim row, later records reuse it.
    for (depositor, claim, amount) in [
        (&f.depositor_a, &claim_a, 25_000_000u64),
        (&f.depositor_b, &claim_b, 10_000_000),
        (&f.depositor_a, &claim_a, 5_000_000),
    ] {
        common::send(
            one,
            &f.payer,
            vec![ixs::record_contribution(
                &addrs::pool(),
                &f.backend.pubkey(),
                &depositor.pubkey(),
                &addrs::claim_mint(),
                claim,
                amount,
            )],
            &[&f.backend],
        )
        .await
        .unwrap();
        // THE D40 INVARIANT: the vault stays at zero while claims grow —
        // contributions are records, never escrowed money.
        assert_vault(&f, 0).await;
    }
    assert_eq!(common::token_2022_balance(one, &claim_a).await, 30_000_000);
    assert_eq!(common::token_2022_balance(one, &claim_b).await, 10_000_000);
    assert_claim_supply(&f, 40_000_000).await;

    // THE DIRECT DOOR: depositor_a exits 5M of their record with their OWN
    // key — the backend key is deliberately ABSENT from the transaction.
    // (Exit-without-cooperation: an evil or offline operator cannot
    // imprison records.)
    common::send(
        one,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.depositor_a.pubkey(), // authority = the depositor themselves
            &f.depositor_a.pubkey(),
            &addrs::claim_mint(),
            &claim_a,
            5_000_000,
        )],
        &[&f.depositor_a],
    )
    .await
    .unwrap();
    assert_eq!(common::token_2022_balance(one, &claim_a).await, 25_000_000);
    assert_vault(&f, 0).await; // burns move no money either

    // THE ASSISTED DOOR: the backend exits 10M on depositor_a's behalf —
    // the custodial UX (rule 10), depositor signs nothing.
    common::send(
        one,
        &f.payer,
        vec![ixs::burn_claims(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor_a.pubkey(),
            &addrs::claim_mint(),
            &claim_a,
            10_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap();
    assert_eq!(common::token_2022_balance(one, &claim_a).await, 15_000_000);
    assert_claim_supply(&f, 25_000_000).await;
    assert_vault(&f, 0).await;
}

#[tokio::test]
async fn journey_renter_prepay_refund_round_trip() {
    let f = fixture().await;
    let one = &f.banks;
    let prepaid = addrs::prepaid(&f.renter.pubkey());

    // Prepay 1: 25 USDC enters escrow (the ONLY USDC inflow — D40) and
    // the renter's own record is born with the same number.
    common::send(
        one,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            25_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap();
    assert_vault(&f, 25_000_000).await;
    assert_eq!(
        common::prepaid_amount(one, &f.renter.pubkey()).await,
        25_000_000
    );
    assert_eq!(
        common::token_balance(one, &f.renter_source).await,
        5_000_000
    );

    // Prepay 2: another 5 — records ACCUMULATE per renter (the record is
    // the renter's ledger row, not a one-shot receipt).
    common::send(
        one,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            5_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap();
    assert_vault(&f, 30_000_000).await;
    assert_eq!(
        common::prepaid_amount(one, &f.renter.pubkey()).await,
        30_000_000
    );

    // Partial refund via THE DIRECT DOOR: the renter's own key signs; the
    // backend is deliberately ABSENT. Money out, bounded by the record.
    common::send(
        one,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.renter.pubkey(), // authority = the renter themselves
            &f.renter_dest,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            12_345_678,
        )],
        &[&f.renter],
    )
    .await
    .unwrap();
    assert_vault(&f, 17_654_322).await;
    assert_eq!(
        common::prepaid_amount(one, &f.renter.pubkey()).await,
        17_654_322
    );
    assert_eq!(common::token_balance(one, &f.renter_dest).await, 12_345_678);

    // Full remainder via THE ASSISTED DOOR: backend signs for the renter.
    // After this the vault is empty and the record is exactly zero — the
    // renter-shaped custody round-trip the devnet soak replays.
    common::send(
        one,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter_dest,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            17_654_322,
        )],
        &[&f.backend],
    )
    .await
    .unwrap();
    assert_vault(&f, 0).await;
    assert_eq!(common::prepaid_amount(one, &f.renter.pubkey()).await, 0);
    assert_eq!(common::token_balance(one, &f.renter_dest).await, 30_000_000);
}

#[tokio::test]
async fn journey_admin_lifecycle_pauses_every_movement() {
    let f = fixture().await;
    let one = &f.banks;
    let claim_a = addrs::claim(&f.depositor_a.pubkey());
    let prepaid = addrs::prepaid(&f.renter.pubkey());

    // The renter prepays 1 USDC first so their record PDA exists — the
    // paused-refund leg below must reach the HANDLER's paused check (the
    // account-constraint phase would otherwise refuse first with a
    // less-legible 3012).
    common::send(
        one,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap();

    // Pause via the admin gate (the multisig vote on devnet).
    common::send(
        one,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();

    // EVERY movement class refuses while paused: recording a contribution…
    let err = common::send(
        one,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor_a.pubkey(),
            &addrs::claim_mint(),
            &claim_a,
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "Paused");

    // …renter prepay…
    let err = common::send(
        one,
        &f.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.renter.pubkey(),
            &f.renter_source,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.backend, &f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "Paused");

    // …and renter refund, even through the direct door — a paused pool is
    // a full stop for everyone, which is what an incident response needs.
    let err = common::send(
        one,
        &f.payer,
        vec![ixs::refund_renter(
            &addrs::pool(),
            &f.renter.pubkey(),
            &f.renter_dest,
            &addrs::vault(),
            &prepaid,
            &f.renter.pubkey(),
            1_000_000,
        )],
        &[&f.renter],
    )
    .await
    .unwrap_err();
    assert_eq!(common::custom_code(&err), Some(6000), "Paused");

    // Resume (second admin vote) — movements work again.
    common::send(
        one,
        &f.payer,
        vec![ixs::toggle("resume", &addrs::pool(), &f.admin.pubkey())],
        &[&f.admin],
    )
    .await
    .unwrap();
    common::send(
        one,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor_a.pubkey(),
            &addrs::claim_mint(),
            &claim_a,
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap();
    assert_claim_supply(&f, 1_000_000).await;

    // Rotate the backend hot key (multisig vote): the OLD backend is
    // locked out of attestations, the new one in — the compromise story.
    let backend2 = Keypair::new();
    common::fund(one, &f.payer, &backend2.pubkey(), 5_000_000_000).await;
    common::send(
        one,
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
        one,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &f.backend.pubkey(), // the RETIRED key
            &f.depositor_b.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor_b.pubkey()),
            1_000_000,
        )],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    // Anchor's framework gate (not our custom code): any failure IS the
    // refusal — pinning anchor's internal code would couple us to its
    // internals (same idiom as the adversarial suite).
    assert!(
        common::custom_code(&err).is_none_or(|c| c != 6000),
        "retired backend must be refused"
    );
    common::send(
        one,
        &f.payer,
        vec![ixs::record_contribution(
            &addrs::pool(),
            &backend2.pubkey(),
            &f.depositor_b.pubkey(),
            &addrs::claim_mint(),
            &addrs::claim(&f.depositor_b.pubkey()),
            1_000_000,
        )],
        &[&backend2],
    )
    .await
    .unwrap();
    assert_claim_supply(&f, 2_000_000).await;
}
