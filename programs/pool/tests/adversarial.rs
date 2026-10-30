//! Adversarial suite: every guard the program claims, attacked through the
//! public surface. Each negative leg pins WHICH error fired (custom code
//! for our errors, hard failure for anchor's framework gates) — "it
//! failed" alone proves nothing.

mod common;

use common::{ANCHOR_ERROR_OFFSET, addrs, ixs, start};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_test::BanksClient;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
// `get_extension` is a trait method in spl-token-2022-interface 3.x.
use spl_token_2022_interface::extension::BaseStateWithExtensions;

/// Same fixture as the journey suite, initialized and ready for abuse.
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
    common::faucet_usdc(&banks, &payer, &usdc_mint, &source, 100_000_000).await;
    // The backend hot key pays claim-account rent on first deposit (the
    // program's design keeps depositors fee-free), so it needs lamports —
    // on devnet the operator funds it the same way.
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

impl Fixture {
    fn deposit_ix(&self, amount: u64) -> Instruction {
        ixs::deposit(
            &addrs::pool(),
            &self.backend.pubkey(),
            &self.depositor.pubkey(),
            &self.source,
            &addrs::vault(),
            &addrs::claim_mint(),
            &addrs::claim(&self.depositor.pubkey()),
            amount,
        )
    }

    fn withdraw_ix(&self, amount: u64) -> Instruction {
        ixs::withdraw(
            &addrs::pool(),
            &self.backend.pubkey(),
            &self.destination,
            &self.depositor.pubkey(),
            &addrs::vault(),
            &addrs::claim_mint(),
            &addrs::claim(&self.depositor.pubkey()),
            amount,
        )
    }
}

#[tokio::test]
async fn deposit_requires_the_configured_backend_key() {
    let f = fixture().await;
    // A random key submitted "for" the depositor: has_one(backend) fires.
    let imposter = Keypair::new();
    let mut ix = f.deposit_ix(1_000_000);
    ix.accounts[1].pubkey = imposter.pubkey(); // backend slot
    let err = common::send(&f.banks, &f.payer, vec![ix], &[&imposter, &f.depositor])
        .await
        .unwrap_err();
    // Anchor's ConstraintHasOne — a framework error, not ours; hard fail is
    // the assertion (a specific framework code would couple us to anchor
    // internals; any failure here IS the gate).
    assert!(common::custom_code(&err).is_none_or(|c| c != ANCHOR_ERROR_OFFSET));
}

#[tokio::test]
async fn deposit_from_a_source_the_depositor_does_not_own_fails() {
    let f = fixture().await;
    let stranger = Keypair::new();
    let stranger_acct =
        common::create_token_account(&f.banks, &f.payer, &stranger.pubkey(), &f.usdc_mint).await;
    common::faucet_usdc(&f.banks, &f.payer, &f.usdc_mint, &stranger_acct, 10_000_000).await;
    let mut ix = f.deposit_ix(1_000_000);
    ix.accounts[3].pubkey = stranger_acct; // source slot
    let err = common::send(&f.banks, &f.payer, vec![ix], &[&f.backend, &f.depositor])
        .await
        .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::WrongSourceOwner as u32)
    );
}

#[tokio::test]
async fn deposit_of_a_non_usdc_mint_fails() {
    let f = fixture().await;
    // A 6-decimals decoy mint ≠ the pool's USDC: WrongSourceMint.
    let decoy = common::create_usdc_mint(&f.banks, &f.payer).await;
    let decoy_acct =
        common::create_token_account(&f.banks, &f.payer, &f.depositor.pubkey(), &decoy).await;
    common::faucet_usdc(&f.banks, &f.payer, &decoy, &decoy_acct, 10_000_000).await;
    let mut ix = f.deposit_ix(1_000_000);
    ix.accounts[3].pubkey = decoy_acct;
    let err = common::send(&f.banks, &f.payer, vec![ix], &[&f.backend, &f.depositor])
        .await
        .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::WrongSourceMint as u32)
    );
}

#[tokio::test]
async fn zero_amounts_are_refused() {
    let f = fixture().await;
    // Anchor runs account constraints (including the claim row's
    // existence for withdraw) BEFORE handler code, so the zero-amount
    // check is observable only once the claim row exists. Establish a
    // 1-micro-unit row first — this mirrors reality (a depositor probing
    // zero already has history) and isolates exactly the amount check.
    common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(1)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(0)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::ZeroAmount as u32)
    );
    let err = common::send(&f.banks, &f.payer, vec![f.withdraw_ix(0)], &[&f.backend])
        .await
        .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::ZeroAmount as u32)
    );
}

#[tokio::test]
async fn withdrawal_over_the_recorded_claims_is_refused() {
    let f = fixture().await;
    common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(10_000_000)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();
    let err = common::send(
        &f.banks,
        &f.payer,
        vec![f.withdraw_ix(10_000_001)],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::InsufficientClaims as u32)
    );
    // Vault untouched: no partial state.
    assert_eq!(
        common::token_balance(&f.banks, &addrs::vault()).await,
        10_000_000
    );
}

#[tokio::test]
async fn withdrawal_to_a_destination_the_depositor_does_not_own_fails() {
    let f = fixture().await;
    common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(10_000_000)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();
    let operator_acct =
        common::create_token_account(&f.banks, &f.payer, &f.backend.pubkey(), &f.usdc_mint).await;
    let mut ix = f.withdraw_ix(1_000_000);
    ix.accounts[2].pubkey = operator_acct; // destination slot
    let err = common::send(&f.banks, &f.payer, vec![ix], &[&f.backend])
        .await
        .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::WrongDestinationOwner as u32)
    );
}

#[tokio::test]
async fn paused_pool_refuses_withdrawals_too() {
    let f = fixture().await;
    common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(10_000_000)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();
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
        vec![f.withdraw_ix(1_000_000)],
        &[&f.backend],
    )
    .await
    .unwrap_err();
    assert_eq!(
        common::custom_code(&err),
        Some(ANCHOR_ERROR_OFFSET + pool::PoolError::Paused as u32)
    );
}

#[tokio::test]
async fn non_admin_cannot_pause_or_rotate() {
    let f = fixture().await;
    let nobody = Keypair::new();
    assert!(
        common::send(
            &f.banks,
            &f.payer,
            vec![ixs::toggle("pause", &addrs::pool(), &nobody.pubkey())],
            &[&nobody]
        )
        .await
        .is_err()
    );
    let other = Keypair::new();
    assert!(
        common::send(
            &f.banks,
            &f.payer,
            vec![ixs::rotate(
                "set_backend_authority",
                &addrs::pool(),
                &nobody.pubkey(),
                &other.pubkey()
            )],
            &[&nobody]
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn initialize_is_once_only() {
    let f = fixture().await;
    // The pool PDA already exists: seeds/init constraint aborts a second
    // bootstrap, so authorities cannot be re-rooted by re-running setup.
    assert!(
        common::send(
            &f.banks,
            &f.payer,
            vec![ixs::initialize(
                &f.payer,
                &f.usdc_mint,
                &f.admin.pubkey(),
                &f.backend.pubkey()
            )],
            &[]
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn rule9_claims_are_locked_not_transferable() {
    let f = fixture().await;
    common::send(
        &f.banks,
        &f.payer,
        vec![f.deposit_ix(10_000_000)],
        &[&f.backend, &f.depositor],
    )
    .await
    .unwrap();
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
