//! Journey proof (workflow step 12): the SPEC §11 custody story, walked
//! through the program's PUBLIC instruction surface only — initialize,
//! deposit (×2, covering lazy claim-account creation AND reuse), withdraw
//! to zero, and the admin lifecycle. After every leg the mirror invariant
//! holds: vault USDC balance == total claim supply == Σ recorded principal
//! (the on-chain twin of `cash.escrow` backing `claims.contributed`).

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
/// check the Squads program satisfies on devnet), a depositor with a funded
/// custodial USDC account, and a payout destination.
struct Fixture {
    banks: BanksClient,
    payer: Keypair,
    admin: Keypair,
    backend: Keypair,
    depositor: Keypair,
    source: Pubkey,
    destination: Pubkey,
    usdc_mint: Pubkey,
}

async fn fixture() -> Fixture {
    let (banks, payer) = start().await;
    let usdc_mint = common::create_usdc_mint(&banks, &payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let depositor = Keypair::new();
    let source =
        common::create_token_account(&banks, &payer, &depositor.pubkey(), &usdc_mint).await;
    let destination =
        common::create_token_account(&banks, &payer, &depositor.pubkey(), &usdc_mint).await;
    // 100 USDC (devnet faucet stands in here).
    common::faucet_usdc(&banks, &payer, &usdc_mint, &source, 100_000_000).await;
    // The backend hot key pays claim-account rent on first deposit —
    // depositors stay fee-free by design, so the operator key is funded
    // (the soak client does the same on devnet).
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
        source,
        destination,
        usdc_mint,
    }
}

/// The mirror invariant: escrowed USDC == minted claims. If this ever
/// fails, the on-chain twin has diverged from its own registry — exactly
/// the drift M11's reconciler will alarm on internally.
async fn assert_mirror(f: &Fixture) {
    let vault = common::token_balance(&f.banks, &addrs::vault()).await;
    let mint_ai = f
        .banks
        .get_account(addrs::claim_mint())
        .await
        .unwrap()
        .unwrap();
    let supply = common::claim_supply(&mint_ai.data);
    assert_eq!(vault, supply, "vault USDC must equal total claim supply");
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

    // Empty vault to start.
    assert_eq!(common::token_balance(&f.banks, &addrs::vault()).await, 0);
    assert_mirror(&f).await;
}

#[tokio::test]
async fn journey_deposit_twice_withdraw_to_zero() {
    let f = fixture().await;
    let claim = addrs::claim(&f.depositor.pubkey());
    let one = &f.banks;

    // First deposit lazily creates the claim account; second reuses it.
    for amount in [40_000_000u64, 10_000_000] {
        common::send(
            one,
            &f.payer,
            vec![ixs::deposit(
                &addrs::pool(),
                &f.backend.pubkey(),
                &f.depositor.pubkey(),
                &f.source,
                &addrs::vault(),
                &addrs::claim_mint(),
                &claim,
                amount,
            )],
            &[&f.backend, &f.depositor],
        )
        .await
        .unwrap();
        assert_mirror(&f).await;
    }
    assert_eq!(
        common::token_balance(one, &addrs::vault()).await,
        50_000_000
    );
    assert_eq!(common::token_2022_balance(one, &claim).await, 50_000_000);
    assert_eq!(common::token_balance(one, &f.source).await, 50_000_000);

    // Two withdrawals, ending at zero — the full round-trip.
    for amount in [25_000_000u64, 25_000_000] {
        common::send(
            one,
            &f.payer,
            vec![ixs::withdraw(
                &addrs::pool(),
                &f.backend.pubkey(),
                &f.destination,
                &f.depositor.pubkey(),
                &addrs::vault(),
                &addrs::claim_mint(),
                &claim,
                amount,
            )],
            &[&f.backend], // rule 10: depositor signs nothing
        )
        .await
        .unwrap();
        assert_mirror(&f).await;
    }
    assert_eq!(common::token_balance(one, &addrs::vault()).await, 0);
    assert_eq!(common::token_2022_balance(one, &claim).await, 0);
    // 40M + 10M deposited, 25M + 25M withdrawn: the destination ends with
    // exactly the withdrawn principal (the remaining 50M stays in source).
    assert_eq!(common::token_balance(one, &f.destination).await, 50_000_000);
    assert_eq!(common::token_balance(one, &f.source).await, 50_000_000);
}

#[tokio::test]
async fn journey_admin_lifecycle_pause_and_key_rotations() {
    let f = fixture().await;
    let claim = addrs::claim(&f.depositor.pubkey());
    let dep = |f: &Fixture| {
        ixs::deposit(
            &addrs::pool(),
            &f.backend.pubkey(),
            &f.depositor.pubkey(),
            &f.source,
            &addrs::vault(),
            &addrs::claim_mint(),
            &claim,
            1_000_000,
        )
    };

    // Pause halts deposits; resume unhalts.
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
        vec![dep(&f)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(common::ANCHOR_ERROR_OFFSET + pool::PoolError::Paused as u32),
        "paused pool must refuse deposits with PoolError::Paused"
    );
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
        vec![dep(&f)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();

    // Backend rotation: the old hot key is dead, the new one works.
    let new_backend = Keypair::new();
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::rotate(
            "set_backend_authority",
            &addrs::pool(),
            &f.admin.pubkey(),
            &new_backend.pubkey(),
        )],
        &[&f.admin],
    )
    .await
    .unwrap();
    assert!(
        common::send(
            &f.banks,
            &f.payer,
            vec![dep(&f)],
            &[&f.backend, &f.depositor]
        )
        .await
        .is_err()
    );
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::withdraw(
            &addrs::pool(),
            &new_backend.pubkey(),
            &f.destination,
            &f.depositor.pubkey(),
            &addrs::vault(),
            &addrs::claim_mint(),
            &claim,
            1_000_000,
        )],
        &[&new_backend],
    )
    .await
    .unwrap();

    // Admin rotation: the old admin cannot even pause anymore.
    let new_admin = Keypair::new();
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::rotate(
            "set_admin_authority",
            &addrs::pool(),
            &f.admin.pubkey(),
            &new_admin.pubkey(),
        )],
        &[&f.admin],
    )
    .await
    .unwrap();
    assert!(
        common::send(
            &f.banks,
            &f.payer,
            vec![ixs::toggle("pause", &addrs::pool(), &f.admin.pubkey())],
            &[&f.admin]
        )
        .await
        .is_err()
    );
    common::send(
        &f.banks,
        &f.payer,
        vec![ixs::toggle("pause", &addrs::pool(), &new_admin.pubkey())],
        &[&new_admin],
    )
    .await
    .unwrap();
}
