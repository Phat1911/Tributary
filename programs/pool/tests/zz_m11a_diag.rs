//! TEMPORARY Session-A diagnostic (2026-11-26) — NOT part of the doctrine
//! suite and removed in the very next commit. Purpose: on the stub branch
//! every suite test stops at `fund_bond`, so the other five account lists
//! are never EXECUTED. This seeds the bond record and one root (diagnostic
//! seeding only — the suite itself never seeds program state), drives all
//! six settlement instructions, and checks that each one VALIDATES its
//! accounts and reaches its `todo!()` (an abort, not a constraint error),
//! and that the constraint refusals fire where the contract says.
mod common;

use anchor_lang::{AccountSerialize, Discriminator};
use common::{addrs, ixs, merkle};
use solana_account::{Account, AccountSharedData};
use solana_keypair::Keypair;
use solana_program_test::ProgramTestContext;
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use solana_signer::Signer;

const USDC: u64 = 1_000_000;
const WINDOW: u64 = 1;

fn seeded<T: AccountSerialize + Discriminator>(value: &T) -> Account {
    let mut data = T::DISCRIMINATOR.to_vec();
    value.try_serialize(&mut data).expect("serializes");
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
) -> Result<(), solana_program_test::BanksClientError> {
    common::send(&ctx.banks_client, &ctx.payer, vec![ix], signers).await
}

/// The instruction validated every account and ABORTED in the handler (the
/// `todo!()`) — i.e. no constraint answered first.
fn reached_todo(err: &solana_program_test::BanksClientError, name: &str) {
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
async fn diag_every_settlement_instruction_validates_its_accounts_and_reaches_its_todo() {
    let depositor = Keypair::new();
    let tree = merkle::Tree::new(&[
        merkle::Leaf {
            depositor: depositor.pubkey(),
            amount: 3 * USDC,
        },
        merkle::Leaf {
            depositor: Pubkey::new_from_array([9; 32]),
            amount: 2 * USDC,
        },
    ]);

    // The harness's derivations agree with the raw seeds (bumps included).
    let (bond_addr, bond_bump) =
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_BOND], &common::program_id());
    assert_eq!(bond_addr, addrs::bond());
    let (root_addr, root_bump) = Pubkey::find_program_address(
        &[pool::SEED_ROOT, &WINDOW.to_le_bytes()],
        &common::program_id(),
    );
    assert_eq!(root_addr, addrs::root(WINDOW));

    let mut pt = common::boot();
    pt.add_account(
        addrs::bond(),
        seeded(&pool::OperatorBond {
            amount: 10 * USDC,
            bump: bond_bump,
        }),
    );
    pt.add_account(
        addrs::root(WINDOW),
        seeded(&pool::SettlementRoot {
            pool: ap(&addrs::pool()),
            window: WINDOW,
            merkle_root: tree.root(),
            total_payout: tree.total(),
            status: pool::RootStatus::Active,
            posted_at: 1,
            bond: 10 * USDC,
            challenger: Default::default(),
            evidence: [0; 32],
            ruled_slash: 0,
            victim: Default::default(),
            bump: root_bump,
        }),
    );
    let mut ctx = pt.start_with_context().await;
    let usdc_mint = common::create_usdc_mint(&ctx.banks_client, &ctx.payer).await;
    let admin = Keypair::new();
    let backend = Keypair::new();
    let rogue = Keypair::new();
    let backend_wallet =
        common::create_token_account(&ctx.banks_client, &ctx.payer, &backend.pubkey(), &usdc_mint)
            .await;
    let dep_wallet = common::create_token_account(
        &ctx.banks_client,
        &ctx.payer,
        &depositor.pubkey(),
        &usdc_mint,
    )
    .await;
    let victim_wallet =
        common::create_token_account(&ctx.banks_client, &ctx.payer, &rogue.pubkey(), &usdc_mint)
            .await;
    let renter = Keypair::new();
    let renter_source =
        common::create_token_account(&ctx.banks_client, &ctx.payer, &renter.pubkey(), &usdc_mint)
            .await;
    common::faucet_usdc(
        &ctx.banks_client,
        &ctx.payer,
        &usdc_mint,
        &renter_source,
        20 * USDC,
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
    common::send(
        &ctx.banks_client,
        &ctx.payer,
        vec![ixs::prepay_renter(
            &addrs::pool(),
            &backend.pubkey(),
            &renter.pubkey(),
            &renter_source,
            &addrs::vault(),
            &addrs::prepaid(&renter.pubkey()),
            &renter.pubkey(),
            10 * USDC,
        )],
        &[&backend, &renter],
    )
    .await
    .expect("prepay lands");

    // A second pool-owned USDC token account for the bond vault: a copy of
    // the (initialized) escrow vault's bytes at the bond-vault address.
    let vault = ctx
        .banks_client
        .get_account(addrs::vault())
        .await
        .unwrap()
        .expect("vault exists");
    ctx.set_account(
        &addrs::bond_vault(),
        &AccountSharedData::from(vault.clone()),
    );
    let banks = &ctx.banks_client;
    let payer = &ctx.payer;
    // The seeded records deserialize as what they are.
    let bond_ai = banks.get_account(addrs::bond()).await.unwrap().unwrap();
    let rec: pool::OperatorBond =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &bond_ai.data[..]).unwrap();
    assert_eq!(rec.amount, 10 * USDC);

    // ---- every instruction: accounts validate, then the todo!() aborts.
    let pool = addrs::pool();
    let root = addrs::root(WINDOW);
    reached_todo(
        &send(
            &ctx,
            ixs::fund_bond(
                &pool,
                &backend.pubkey(),
                &backend_wallet,
                &usdc_mint,
                &addrs::bond_vault(),
                &addrs::bond(),
                USDC,
            ),
            &[&backend],
        )
        .await
        .expect_err("fund_bond aborts"),
        "fund_bond",
    );
    let later = merkle::Tree::new(&[merkle::Leaf {
        depositor: depositor.pubkey(),
        amount: USDC,
    }]);
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
                later.root(),
                later.total(),
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
            ixs::claim(
                &pool,
                &depositor.pubkey(),
                &root,
                &dep_wallet,
                &addrs::vault(),
                &addrs::claim_marker(&root, 0),
                &payer.pubkey(),
                WINDOW,
                0,
                &depositor.pubkey(),
                3 * USDC,
                &tree.proof(0),
            ),
            &[&depositor],
        )
        .await
        .expect_err("claim aborts"),
        "claim",
    );
    reached_todo(
        &send(
            &ctx,
            ixs::challenge(&pool, &rogue.pubkey(), &root, WINDOW, [0xE7; 32]),
            &[&rogue],
        )
        .await
        .expect_err("challenge aborts"),
        "challenge",
    );
    reached_todo(
        &send(
            &ctx,
            ixs::resolve_challenge(
                &pool,
                &admin.pubkey(),
                &root,
                &addrs::bond(),
                WINDOW,
                ixs::Ruling::Stands,
            ),
            &[&admin],
        )
        .await
        .expect_err("resolve_challenge aborts"),
        "resolve_challenge",
    );
    reached_todo(
        &send(
            &ctx,
            ixs::slash_bond(
                &pool,
                &root,
                &addrs::bond_vault(),
                &addrs::bond(),
                &victim_wallet,
                WINDOW,
            ),
            &[],
        )
        .await
        .expect_err("slash_bond aborts"),
        "slash_bond",
    );

    // ---- the constraint refusals, exactly where the contract puts them.
    // A payout to a wallet the leaf's depositor does not own: 6014.
    let err = send(
        &ctx,
        ixs::claim(
            &pool,
            &backend.pubkey(),
            &root,
            &backend_wallet,
            &addrs::vault(),
            &addrs::claim_marker(&root, 0),
            &payer.pubkey(),
            WINDOW,
            0,
            &depositor.pubkey(),
            3 * USDC,
            &tree.proof(0),
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
    // A non-multisig ruling: Anchor's has_one (2001).
    let err = send(
        &ctx,
        ixs::resolve_challenge(
            &pool,
            &rogue.pubkey(),
            &root,
            &addrs::bond(),
            WINDOW,
            ixs::Ruling::Stands,
        ),
        &[&rogue],
    )
    .await
    .expect_err("non-admin ruling refused");
    assert_eq!(common::custom_code(&err), Some(2001), "ConstraintHasOne");
    // A root account at the wrong address: Anchor's seeds gate (2006).
    let err = send(
        &ctx,
        ixs::post_root(
            &pool,
            &backend.pubkey(),
            &addrs::root(6),
            &addrs::vault(),
            &addrs::bond(),
            5,
            later.root(),
            later.total(),
        ),
        &[&backend],
    )
    .await
    .expect_err("wrong root address refused");
    assert_eq!(common::custom_code(&err), Some(2006), "ConstraintSeeds");
    // A claim marker at the wrong address: the seeds gate again.
    let err = send(
        &ctx,
        ixs::claim(
            &pool,
            &depositor.pubkey(),
            &root,
            &dep_wallet,
            &addrs::vault(),
            &addrs::claim_marker(&root, 1),
            &payer.pubkey(),
            WINDOW,
            0,
            &depositor.pubkey(),
            3 * USDC,
            &tree.proof(0),
        ),
        &[&depositor],
    )
    .await
    .expect_err("wrong marker address refused");
    assert_eq!(
        common::custom_code(&err),
        Some(2006),
        "ConstraintSeeds (marker)"
    );
}
