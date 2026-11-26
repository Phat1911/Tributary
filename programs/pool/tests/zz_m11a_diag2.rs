//! TEMPORARY Session-A diagnostic #2 (2026-11-26, GO-gate revision) — NOT
//! part of the doctrine suite; removed in a later commit. Invariant 11
//! makes the clock warp load-bearing in nine of the twelve tests, and the
//! warp has never executed in this rig. This checks: the warp moves the
//! bank; ordinary (M8) transactions still land afterwards; the revised
//! `claim` account list (it gained the clock) and the other clock-bearing
//! instructions still validate their accounts and reach their `todo!()`;
//! a second warp works (roots posted at a warped slot get a window of
//! their own). It seeds one root — diagnostic seeding only; the suite
//! never seeds program state.
mod common;

use anchor_lang::Discriminator;
use borsh::BorshSerialize;
use common::{addrs, ixs};
use solana_account::Account;
use solana_keypair::Keypair;
use solana_program_test::{BanksClientError, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use solana_signer::Signer;

const USDC: u64 = 1_000_000;

fn seeded<T: BorshSerialize + Discriminator>(value: &T) -> Account {
    let mut data = T::DISCRIMINATOR.to_vec();
    // Borsh fields only: `#[account]`'s own `try_serialize` would prepend
    // the discriminator again.
    value.serialize(&mut data).expect("serializes");
    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: common::program_id(),
        executable: false,
        rent_epoch: u64::MAX,
    }
}

fn ap(p: &Pubkey) -> anchor_lang::prelude::Pubkey {
    anchor_lang::prelude::Pubkey::new_from_array(p.to_bytes())
}

async fn send(
    ctx: &ProgramTestContext,
    ix: solana_instruction::Instruction,
    signers: &[&Keypair],
) -> Result<(), BanksClientError> {
    common::send(&ctx.banks_client, &ctx.payer, vec![ix], signers).await
}

/// The instruction validated every account and ABORTED in the handler.
fn reached_todo(err: &BanksClientError, name: &str) {
    assert!(
        common::custom_code(err).is_none(),
        "{name}: a CONSTRAINT answered before the handler — {err}"
    );
    assert!(
        err.to_string().contains("Program failed to complete"),
        "{name}: expected the handler's todo!() abort, got — {err}"
    );
}

#[tokio::test]
async fn diag_clock_warp_and_the_revised_account_lists() {
    // One root, posted in slot 1 (the rig's first working slot).
    let (root_addr, root_bump) = Pubkey::find_program_address(
        &[pool::SEED_ROOT, &1u64.to_le_bytes()],
        &common::program_id(),
    );
    assert_eq!(root_addr, addrs::root(1));
    let (bond_addr, bond_bump) =
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_BOND], &common::program_id());
    assert_eq!(bond_addr, addrs::bond());
    let mut pt = common::boot();
    // `post_root` needs an existing bond record (the suite gets it from
    // `fund_bond`; the first run of this diagnostic forgot to seed it and
    // stopped at 3012 AccountNotInitialized).
    pt.add_account(
        addrs::bond(),
        seeded(&pool::OperatorBond {
            amount: 10 * USDC,
            bump: bond_bump,
        }),
    );
    pt.add_account(
        addrs::root(1),
        seeded(&pool::SettlementRoot {
            pool: ap(&addrs::pool()),
            window: 1,
            merkle_root: [3; 32],
            total_payout: 9 * USDC,
            status: pool::RootStatus::Active,
            posted_at: 1,
            bond: 0,
            challenger: Default::default(),
            evidence: [0; 32],
            ruled_slash: 0,
            victim: Default::default(),
            stands_ruled: false,
            paid_out: 0,
            bump: root_bump,
        }),
    );
    let mut ctx = pt.start_with_context().await;
    let usdc_mint = common::create_usdc_mint(&ctx.banks_client, &ctx.payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let renter = Keypair::new();
    let renter_source =
        common::create_token_account(&ctx.banks_client, &ctx.payer, &renter.pubkey(), &usdc_mint)
            .await;
    common::faucet_usdc(
        &ctx.banks_client,
        &ctx.payer,
        &usdc_mint,
        &renter_source,
        5 * USDC,
    )
    .await;
    let backend_wallet =
        common::create_token_account(&ctx.banks_client, &ctx.payer, &backend.pubkey(), &usdc_mint)
            .await;
    let depositor = Keypair::new();
    let dep_wallet = common::create_token_account(
        &ctx.banks_client,
        &ctx.payer,
        &depositor.pubkey(),
        &usdc_mint,
    )
    .await;
    common::fund(
        &ctx.banks_client,
        &ctx.payer,
        &backend.pubkey(),
        5_000_000_000,
    )
    .await;
    common::send(
        &ctx.banks_client,
        &ctx.payer,
        vec![ixs::initialize(
            &ctx.payer,
            &usdc_mint,
            &admin.pubkey(),
            &backend.pubkey(),
        )],
        &[common::deploy_authority()],
    )
    .await
    .expect("initialize lands");
    let prepay_ix = |amount: u64| {
        ixs::prepay_renter(
            &addrs::pool(),
            &backend.pubkey(),
            &renter.pubkey(),
            &renter_source,
            &addrs::vault(),
            &addrs::prepaid(&renter.pubkey()),
            &renter.pubkey(),
            amount,
        )
    };
    send(&ctx, prepay_ix(USDC), &[&backend, &renter])
        .await
        .expect("prepay before the warp lands");

    // ---- the warp moves the bank's clock.
    let before = ctx.banks_client.get_root_slot().await.unwrap();
    let target = common::warp_past_window(&mut ctx, 1);
    let after = ctx.banks_client.get_root_slot().await.unwrap();
    println!("diag: root slot {before} before the warp, target {target}, {after} after");
    assert_eq!(
        target,
        u64::try_from(1 + pool::CHALLENGE_WINDOW_SLOTS + 1).unwrap()
    );
    assert!(
        after >= target - 1,
        "the clock must move past the window: root slot {after}, target {target}"
    );

    // ---- ordinary transactions still land after the warp.
    send(&ctx, prepay_ix(USDC), &[&backend, &renter])
        .await
        .expect("an M8 transaction lands after the warp");
    assert_eq!(
        common::prepaid_amount(&ctx.banks_client, &renter.pubkey()).await,
        2 * USDC
    );

    // ---- the revised `claim` list (it gained the clock) validates and
    // reaches its todo!() — with a valid door and with a misdirected payout.
    let pool = addrs::pool();
    let root = addrs::root(1);
    let marker = addrs::claim_marker(&root, 0);
    reached_todo(
        &send(
            &ctx,
            ixs::claim(
                &pool,
                &depositor.pubkey(),
                &root,
                &dep_wallet,
                &addrs::vault(),
                &marker,
                &ctx.payer.pubkey(),
                1,
                0,
                &depositor.pubkey(),
                USDC,
                &[],
            ),
            &[&depositor],
        )
        .await
        .expect_err("claim aborts"),
        "claim (with the clock account)",
    );
    let err = send(
        &ctx,
        ixs::claim(
            &pool,
            &backend.pubkey(),
            &root,
            &backend_wallet,
            &addrs::vault(),
            &marker,
            &ctx.payer.pubkey(),
            1,
            0,
            &depositor.pubkey(),
            USDC,
            &[],
        ),
        &[&backend],
    )
    .await
    .expect_err("misdirected payout refused");
    assert_eq!(
        common::custom_code(&err),
        Some(6014),
        "WrongPayoutDestination"
    );

    // ---- the other clock-bearing instructions too.
    reached_todo(
        &send(
            &ctx,
            ixs::post_root(
                &pool,
                &backend.pubkey(),
                &addrs::root(5),
                &addrs::vault(),
                &addrs::bond(),
                5,
                [4; 32],
                USDC,
            ),
            &[&backend],
        )
        .await
        .expect_err("post_root aborts"),
        "post_root",
    );
    reached_todo(
        &send(
            &ctx,
            ixs::challenge(&pool, &depositor.pubkey(), &root, 1, [0xE7; 32]),
            &[&depositor],
        )
        .await
        .expect_err("challenge aborts"),
        "challenge",
    );

    // ---- a second clock move: a root posted AT the warped slot gets a
    // window of its own, and transactions still land after it.
    let second = common::warp_past_window(&mut ctx, i64::try_from(target).unwrap());
    assert!(second > target, "a second warp must move forward");
    let after2 = ctx.banks_client.get_root_slot().await.unwrap();
    println!("diag: second target {second}, root slot {after2} after");
    assert!(after2 >= second - 1);
    send(&ctx, prepay_ix(USDC), &[&backend, &renter])
        .await
        .expect("an M8 transaction lands after the second warp");
    assert_eq!(
        common::prepaid_amount(&ctx.banks_client, &renter.pubkey()).await,
        3 * USDC
    );
}
