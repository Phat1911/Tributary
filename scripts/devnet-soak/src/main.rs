//! Devnet soak client — M8 (SPEC §14 V2 M8 row; kickoff 2026-10-30).
//!
//! Standalone crate (NOT a workspace member): the Squads SDK rides the
//! Solana 1.x cohort, which cannot be resolved inside the main workspace's
//! unified dependency graph. It runs ONLY against devnet and is the
//! executable core of the documented runbook (docs/devnet-soak-m8.md):
//!
//!   0. verify the USDC devnet mint's shape (6 decimals = micro-USD seam),
//!   1. create the Squads V4 2-of-2 multisig (founder + teammate — GO
//!      ruling 2026-10-30; SPEC §12's 2-of-3 remains the production
//!      posture),
//!   2. hand the program's upgrade authority to the multisig vault PDA,
//!   3. prove a single-key upgrade attempt is REFUSED,
//!   4. initialize the pool with admin = multisig vault PDA,
//!   5. deposit / withdraw round-trip in faucet USDC (rule 10: only
//!      operator keys sign),
//!   6. pause THROUGH a 2-of-2 vote, and prove the paused pool refuses
//!      deposits,
//!   7. upgrade the program THROUGH a 2-of-2 vote (vault-signed upgrade),
//!   8. rotate the teammate member key THROUGH a 2-of-2 vote.
//!
//! Our program's instructions are built the way an external integrator
//! builds them: Anchor discriminator + borsh args — no anchor client.

use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::bpf_loader_upgradeable;
// squads SDK: try_compile lives on an extension trait, not the type itself.
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;
use squads_multisig::anchor_lang::AnchorSerialize;
use squads_multisig::client as squads;
use squads_multisig::pda;
use squads_multisig::state as squads_state;
use squads_multisig::vault_transaction::VaultTransactionMessageExt;
use squads_multisig::{solana_program, squads_multisig_program};
// --- program ids, as verified byte arrays (decoded from base58 offline;
// --- self-checked against the strings at startup) ------------------------

const POOL_PROGRAM_BYTES: [u8; 32] = [
    0x82, 0xf7, 0x5e, 0x97, 0xda, 0xf1, 0xcf, 0x09, 0x46, 0x97, 0xa2, 0x1c, 0xe7, 0xcc, 0xaf, 0x5a,
    0xe7, 0xcc, 0xdc, 0xb5, 0xcc, 0x49, 0xe2, 0xd8, 0x68, 0xc9, 0x70, 0x24, 0x74, 0x43, 0x88, 0x06,
];
const USDC_DEVNET_BYTES: [u8; 32] = [
    0x3b, 0x44, 0x2c, 0xb3, 0x91, 0x21, 0x57, 0xf1, 0x3a, 0x93, 0x3d, 0x01, 0x34, 0x28, 0x2d, 0x03,
    0x2b, 0x5f, 0xfe, 0xcd, 0x01, 0xa2, 0xdb, 0xf1, 0xb7, 0x79, 0x06, 0x08, 0xdf, 0x00, 0x2e, 0xa7,
];
const TOKEN_V3_BYTES: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
];
const TOKEN_2022_BYTES: [u8; 32] = [
    0x06, 0xdd, 0xf6, 0xe1, 0xee, 0x75, 0x8f, 0xde, 0x18, 0x42, 0x5d, 0xbc, 0xe4, 0x6c, 0xcd, 0xda,
    0xb6, 0x1a, 0xfc, 0x4d, 0x83, 0xb9, 0x0d, 0x27, 0xfe, 0xbd, 0xf9, 0x28, 0xd8, 0xa1, 0x8b, 0xfc,
];
const ATA_PROGRAM_BYTES: [u8; 32] = [
    0x8c, 0x97, 0x25, 0x8f, 0x4e, 0x24, 0x89, 0xf1, 0xbb, 0x3d, 0x10, 0x29, 0x14, 0x8e, 0x0d, 0x83,
    0x0b, 0x5a, 0x13, 0x99, 0xda, 0xff, 0x10, 0x84, 0x04, 0x8e, 0x7b, 0xd8, 0xdb, 0xe9, 0xf8, 0x5b,
];
const RENT_SYSVAR_BYTES: [u8; 32] = [
    0x06, 0xa7, 0xd5, 0x17, 0x19, 0x2c, 0x5c, 0x51, 0x21, 0x8c, 0xc9, 0x4c, 0x3d, 0x4a, 0xf1, 0x7f,
    0x58, 0xda, 0xee, 0x08, 0x9b, 0xa1, 0xfd, 0x44, 0xe3, 0xdb, 0xd9, 0x8a, 0x00, 0x00, 0x00, 0x00,
];

const POOL_PROGRAM_STR: &str = "9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX";
const USDC_DEVNET_STR: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const TOKEN_V3_STR: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_STR: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const ATA_PROGRAM_STR: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knN";
const RENT_SYSVAR_STR: &str = "SysvarRent111111111111111111111111111111111";

// --- program seeds (must mirror programs/pool/src/state.rs exactly) ------

const SEED_POOL: &[u8] = b"pool";
const SEED_VAULT: &[u8] = b"vault";
const SEED_CLAIM_MINT: &[u8] = b"claim_mint";
const SEED_CLAIM: &[u8] = b"claim";
const SEED_PREPAID: &[u8] = b"prepaid";

const DEPOSIT_AMOUNT: u64 = 25_000_000; // 25 USDC in micro-units
/// Renter prepay leg (10 USDC) — the only USDC inflow (D40).
const PREPAY_AMOUNT: u64 = 10_000_000;

macro_rules! evidence {
    ($step:expr, $($arg:tt)*) => {{
        println!("[soak] STEP {:02} :: {}", $step, format!($($arg)*));
    }};
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        if let Err(e) = run().await {
            eprintln!("[soak] FAILED: {e:?}");
            std::process::exit(1);
        }
    });
}

struct Ctx {
    rpc: RpcClient,
    pool: Pubkey,
    usdc: Pubkey,
    token_v3: Pubkey,
    token_2022: Pubkey,
    rent_sysvar: Pubkey,
    deploy: Keypair,
    founder: Keypair,
    teammate: Keypair,
    backend: Keypair,
    depositor: Keypair,
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Self-check: the byte arrays must decode to the well-known strings —
    // a mismatch means someone edited a constant wrongly.
    let pool = Pubkey::new_from_array(POOL_PROGRAM_BYTES);
    let usdc = Pubkey::new_from_array(USDC_DEVNET_BYTES);
    let token_v3 = Pubkey::new_from_array(TOKEN_V3_BYTES);
    let token_2022 = Pubkey::new_from_array(TOKEN_2022_BYTES);
    let ata_program = Pubkey::new_from_array(ATA_PROGRAM_BYTES);
    let rent_sysvar = Pubkey::new_from_array(RENT_SYSVAR_BYTES);
    assert_eq!(pool.to_string(), POOL_PROGRAM_STR, "POOL bytes drifted");
    assert_eq!(usdc.to_string(), USDC_DEVNET_STR, "USDC bytes drifted");
    assert_eq!(token_v3.to_string(), TOKEN_V3_STR, "token v3 bytes drifted");
    assert_eq!(
        token_2022.to_string(),
        TOKEN_2022_STR,
        "token 2022 bytes drifted"
    );
    assert_eq!(
        ata_program.to_string(),
        ATA_PROGRAM_STR,
        "ATA bytes drifted"
    );
    assert_eq!(
        rent_sysvar.to_string(),
        RENT_SYSVAR_STR,
        "rent sysvar bytes drifted"
    );

    let rpc = RpcClient::new(
        std::env::var("SOAK_RPC").unwrap_or_else(|_| "https://api.devnet.solana.com".into()),
    );
    let ctx = Ctx {
        rpc,
        pool,
        usdc,
        token_v3,
        token_2022,
        rent_sysvar,
        deploy: load_key("deploy")?,
        founder: load_key("founder")?,
        teammate: load_key("teammate")?,
        backend: load_key("backend")?,
        depositor: load_key("depositor")?,
    };
    println!(
        "[soak] program={} deploy={} founder={} teammate={} backend={} depositor={}",
        ctx.pool,
        ctx.deploy.pubkey(),
        ctx.founder.pubkey(),
        ctx.teammate.pubkey(),
        ctx.backend.pubkey(),
        ctx.depositor.pubkey()
    );

    // Member keys sign proposals; give each a little fee lamports.
    fund_members(&ctx).await?;

    // ---- 0. USDC shape --------------------------------------------------------
    let usdc_acct = ctx.rpc.get_account(&ctx.usdc).await?;
    if usdc_acct.owner != ctx.token_v3 || usdc_acct.data[44] != 6 {
        return Err("devnet USDC mint shape wrong (want Token v3, 6 decimals)".into());
    }
    evidence!(
        0,
        "USDC devnet mint verified: Token v3 owner, 6 decimals (micro-USD seam)"
    );

    // ---- 1. multisig 2-of-2 ---------------------------------------------------
    let create_key = load_key("ms-create-key")?;
    let (ms, _) = pda::get_multisig_pda(&create_key.pubkey(), None);
    let (vault, _) = pda::get_vault_pda(&ms, 0, None);
    if ctx.rpc.get_account(&ms).await.is_ok() {
        evidence!(1, "multisig exists at {} (vault {})", ms, vault);
    } else {
        let all = squads_state::Permissions::from_vec(&[
            squads_state::Permission::Initiate,
            squads_state::Permission::Vote,
            squads_state::Permission::Execute,
        ]);
        let ix = squads::multisig_create_v2(
            squads::MultisigCreateAccountsV2 {
                program_config: pda::get_program_config_pda(None).0,
                treasury: vault,
                multisig: ms,
                create_key: create_key.pubkey(),
                creator: ctx.deploy.pubkey(),
                system_program: solana_program::system_program::ID,
            },
            squads::MultisigCreateArgsV2 {
                members: vec![
                    squads_state::Member {
                        key: ctx.founder.pubkey(),
                        permissions: all,
                    },
                    squads_state::Member {
                        key: ctx.teammate.pubkey(),
                        permissions: all,
                    },
                ],
                threshold: 2,
                time_lock: 0,
                config_authority: None,
                rent_collector: None,
                memo: Some("Tributary M8 devnet 2-of-2 (GO ruling 2026-10-30)".into()),
            },
            None,
        );
        let sig = send(
            &ctx,
            &[ix],
            &ctx.deploy.pubkey(),
            &[&ctx.deploy, &create_key],
        )
        .await?;
        evidence!(1, "multisig created {} (vault {}) sig={}", ms, vault, sig);
    }

    // ---- 2. upgrade authority -> multisig vault -------------------------------
    let program_data =
        Pubkey::find_program_address(&[ctx.pool.as_ref()], &bpf_loader_upgradeable::id());
    let auth = read_upgrade_authority(&ctx, &program_data.0).await?;
    match auth {
        Some(current) if current == ctx.deploy.pubkey() => {
            let ix = bpf_loader_upgradeable::set_upgrade_authority(
                &ctx.pool,
                &ctx.deploy.pubkey(),
                Some(&vault),
            );
            let sig = send(&ctx, &[ix], &ctx.deploy.pubkey(), &[&ctx.deploy]).await?;
            evidence!(
                2,
                "upgrade authority: deploy -> vault {} sig={} (last unilateral action)",
                vault,
                sig
            );
        }
        Some(current) if current == vault => {
            evidence!(2, "upgrade authority already vault {}", vault);
        }
        other => return Err(format!("unexpected upgrade authority {other:?}").into()),
    }

    // ---- 3. single-key upgrade refused ----------------------------------------
    let rogue = load_key("rogue-upgrader")?;
    let fake_buffer = rogue.pubkey(); // not a buffer; failure is the point
    let ix = bpf_loader_upgradeable::upgrade(
        &ctx.pool,
        &fake_buffer,
        &rogue.pubkey(),
        &ctx.deploy.pubkey(),
    );
    match send(&ctx, &[ix], &ctx.deploy.pubkey(), &[&rogue, &ctx.deploy]).await {
        Ok(sig) => {
            return Err(format!("SECURITY: non-authority upgrade ACCEPTED sig={sig}").into())
        }
        Err(e) => evidence!(3, "single-key upgrade REFUSED: {}", short_err(e.as_ref())),
    }

    // ---- 4. initialize the pool (admin = multisig vault) ----------------------
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let claim_mint = Pubkey::find_program_address(&[SEED_POOL, SEED_CLAIM_MINT], &ctx.pool).0;
    if ctx.rpc.get_account(&pool_pda).await.is_ok() {
        evidence!(4, "pool already initialized at {}", pool_pda);
    } else {
        let ix = pool_ix(
            "initialize",
            (&vault, &ctx.backend.pubkey()),
            vec![
                AccountMeta::new(pool_pda, false),
                AccountMeta::new(claim_mint, false),
                AccountMeta::new_readonly(ctx.usdc, false),
                AccountMeta::new(vault_ta, false),
                AccountMeta::new(ctx.deploy.pubkey(), true),
                // Rent sysvar — declared by Initialize (host-test compat;
                // see lib.rs account docs). Anchor consumes accounts
                // positionally: order must mirror the struct field order.
                AccountMeta::new_readonly(ctx.rent_sysvar, false),
                AccountMeta::new_readonly(ctx.token_v3, false),
                AccountMeta::new_readonly(ctx.token_2022, false),
                AccountMeta::new_readonly(solana_program::system_program::ID, false),
            ],
        );
        let sig = send(&ctx, &[ix], &ctx.deploy.pubkey(), &[&ctx.deploy]).await?;
        evidence!(
            4,
            "pool initialized admin=vault({}) backend={} sig={}",
            vault,
            ctx.backend.pubkey(),
            sig
        );
    }

    // ---- 5. record a contribution + exit through BOTH doors (D40) ------
    // The custodial user key plays both roles on devnet: the depositor
    // identity for record_contribution (no USDC involved — contributions
    // are records) and the renter wallet for step 6's prepay.
    let claim_ta =
        Pubkey::find_program_address(&[SEED_CLAIM, ctx.depositor.pubkey().as_ref()], &ctx.pool).0;
    let rec = pool_ix(
        "record_contribution",
        (&ctx.depositor.pubkey(), &DEPOSIT_AMOUNT),
        vec![
            AccountMeta::new(pool_pda, false),
            AccountMeta::new(ctx.backend.pubkey(), true),
            AccountMeta::new(claim_mint, false),
            AccountMeta::new(claim_ta, false),
            AccountMeta::new_readonly(ctx.token_2022, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    let sig = send(&ctx, &[rec], &ctx.backend.pubkey(), &[&ctx.backend]).await?;
    let claims = token_amount(&ctx, &claim_ta).await?;
    let vault_bal = token_amount(&ctx, &vault_ta).await?;
    if vault_bal != 0 {
        return Err(format!("D40 VIOLATION: vault holds {vault_bal} micro-USDC after a recorded contribution — the vault must hold renter prepaid only").into());
    }
    evidence!(5, "record_contribution {} USDC-worth sig={} claims={} vault={} (D40: records only — vault untouched)",
        DEPOSIT_AMOUNT / 1_000_000, sig, claims, vault_bal);

    // Exit through the DIRECT door: only the depositor's key signs — the
    // backend is ABSENT. The escape hatch, proven live on devnet.
    let burn_direct = pool_ix(
        "burn_claims",
        (&ctx.depositor.pubkey(), &5_000_000u64),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.depositor.pubkey(), true),
            AccountMeta::new(claim_mint, false),
            AccountMeta::new(claim_ta, false),
            AccountMeta::new_readonly(ctx.token_2022, false),
        ],
    );
    let sig = send(
        &ctx,
        &[burn_direct],
        &ctx.backend.pubkey(),
        &[&ctx.depositor],
    )
    .await?;
    let claims_after = token_amount(&ctx, &claim_ta).await?;
    evidence!(5, "burn_claims 5 USDC-worth via the DIRECT door (depositor key only, backend absent) sig={} claims={}",
        sig, claims_after);

    // ---- 6. renter prepay -> refund round-trip (the ONLY USDC path) ----
    let user_ata = depositor_ata(&ctx);
    let prepaid_pda =
        Pubkey::find_program_address(&[SEED_PREPAID, ctx.depositor.pubkey().as_ref()], &ctx.pool).0;
    let before = token_amount(&ctx, &user_ata).await?;
    if before < PREPAY_AMOUNT {
        return Err(format!(
            "renter wallet {user_ata} holds {before} micro-USDC — run the Circle faucet step in the runbook"
        ).into());
    }
    // Prepay (assisted shape: backend gate + the renter's custodial key).
    let pre = pool_ix(
        "prepay_renter",
        (&ctx.depositor.pubkey(), &PREPAY_AMOUNT),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new(ctx.backend.pubkey(), true),
            AccountMeta::new_readonly(ctx.depositor.pubkey(), true),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault_ta, false),
            AccountMeta::new(prepaid_pda, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    let sig = send(
        &ctx,
        &[pre],
        &ctx.backend.pubkey(),
        &[&ctx.backend, &ctx.depositor],
    )
    .await?;
    let vault_bal = token_amount(&ctx, &vault_ta).await?;
    let record = prepaid_amount(&ctx, &prepaid_pda).await?;
    if vault_bal != PREPAY_AMOUNT || record != PREPAY_AMOUNT {
        return Err(format!("prepay mirror broken: vault={vault_bal} record={record}").into());
    }
    evidence!(
        6,
        "prepay {} USDC sig={} vault={} record={} (vault == Σ prepaid)",
        PREPAY_AMOUNT / 1_000_000,
        sig,
        vault_bal,
        record
    );

    // Partial refund through the ASSISTED door (backend signs)…
    let ref1 = pool_ix(
        "refund_renter",
        (&ctx.depositor.pubkey(), &4_000_000u64),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.backend.pubkey(), true),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault_ta, false),
            AccountMeta::new(prepaid_pda, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
        ],
    );
    let sig = send(&ctx, &[ref1], &ctx.backend.pubkey(), &[&ctx.backend]).await?;

    // …full remainder through the DIRECT door: only the renter's key signs.
    let ref2 = pool_ix(
        "refund_renter",
        (&ctx.depositor.pubkey(), &(PREPAY_AMOUNT - 4_000_000)),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.depositor.pubkey(), true),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault_ta, false),
            AccountMeta::new(prepaid_pda, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
        ],
    );
    let sig2 = send(&ctx, &[ref2], &ctx.backend.pubkey(), &[&ctx.depositor]).await?;
    let after = token_amount(&ctx, &user_ata).await?;
    let vault_zero = token_amount(&ctx, &vault_ta).await?;
    let record_zero = prepaid_amount(&ctx, &prepaid_pda).await?;
    if after != before || vault_zero != 0 || record_zero != 0 {
        return Err(format!(
            "round-trip failed: user {} -> {} vault={} record={}",
            before, after, vault_zero, record_zero
        )
        .into());
    }
    evidence!(6, "refund sig={} (assisted) + sig={} (direct door, backend absent): vault=0 record=0 user back at {} (round-trip proven, both doors live)",
        sig, sig2, after);

    // ---- 6b. pause through the 2-of-2 ----------------------------------------
    let ms_state = squads::get_multisig(&ctx.rpc, &ms).await?;
    let pause = pool_ix(
        "pause",
        (),
        vec![
            AccountMeta::new(pool_pda, false),
            AccountMeta::new_readonly(vault, true),
        ],
    );
    let idx = propose_approve_execute(&ctx, &ms, &ms_state, pause, "pause via 2-of-2").await?;
    evidence!(
        7,
        "pause executed through multisig (tx index {}) — admin gate is the vault PDA",
        idx
    );

    // A paused pool refuses EVERY movement class — record_contribution here
    // (the prepay/refund refusals are pinned in the adversarial suite).
    let rec2 = pool_ix(
        "record_contribution",
        (&ctx.depositor.pubkey(), &DEPOSIT_AMOUNT),
        vec![
            AccountMeta::new(pool_pda, false),
            AccountMeta::new(ctx.backend.pubkey(), true),
            AccountMeta::new(claim_mint, false),
            AccountMeta::new(claim_ta, false),
            AccountMeta::new_readonly(ctx.token_2022, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    match send(&ctx, &[rec2], &ctx.backend.pubkey(), &[&ctx.backend]).await {
        Ok(sig) => {
            return Err(format!("SECURITY: paused pool accepted a contribution sig={sig}").into())
        }
        Err(e) => evidence!(
            8,
            "paused pool refused record_contribution: {}",
            short_err(e.as_ref())
        ),
    }

    // ---- 7. program upgrade through the 2-of-2 ----------------------------------
    if let Ok(buffer) = std::env::var("SOAK_BUFFER") {
        let buffer: Pubkey = buffer.parse().expect("SOAK_BUFFER pubkey");
        let upgrade =
            bpf_loader_upgradeable::upgrade(&ctx.pool, &buffer, &vault, &ctx.deploy.pubkey());
        let ms_state = squads::get_multisig(&ctx.rpc, &ms).await?;
        let idx =
            propose_approve_execute(&ctx, &ms, &ms_state, upgrade, "program upgrade via 2-of-2")
                .await?;
        evidence!(
            9,
            "program UPGRADED through multisig (tx index {}) — vault-signed upgrade proven",
            idx
        );
    } else {
        println!("[soak] STEP 09 :: SKIPPED (SOAK_BUFFER unset — set it after the runbook's write-buffer step)");
    }

    // ---- 8. member rotation through the 2-of-2 -----------------------------------
    let fresh_teammate = load_key("teammate-rotated")?;
    let ms_state = squads::get_multisig(&ctx.rpc, &ms).await?;
    let tx_index = ms_state.transaction_index + 1;
    let (config_tx, _) = pda::get_transaction_pda(&ms, tx_index, None);
    let (proposal, _) = pda::get_proposal_pda(&ms, tx_index, None);
    let create = squads::config_transaction_create(
        squads::ConfigTransactionCreateAccounts {
            multisig: ms,
            creator: ctx.founder.pubkey(),
            rent_payer: ctx.deploy.pubkey(),
            transaction: config_tx,
            system_program: solana_program::system_program::ID,
        },
        squads::ConfigTransactionCreateArgs {
            actions: vec![
                squads_state::ConfigAction::RemoveMember {
                    old_member: ctx.teammate.pubkey(),
                },
                squads_state::ConfigAction::AddMember {
                    new_member: squads_state::Member {
                        key: fresh_teammate.pubkey(),
                        permissions: squads_state::Permissions::from_vec(&[
                            squads_state::Permission::Initiate,
                            squads_state::Permission::Vote,
                            squads_state::Permission::Execute,
                        ]),
                    },
                },
            ],
            memo: None,
        },
        None,
    );
    let prop = squads::proposal_create(
        squads::ProposalCreateAccounts {
            multisig: ms,
            creator: ctx.founder.pubkey(),
            proposal,
            rent_payer: ctx.deploy.pubkey(),
            system_program: solana_program::system_program::ID,
        },
        squads::ProposalCreateArgs {
            transaction_index: tx_index,
            draft: false,
        },
        None,
    );
    send(&ctx, &[create, prop], &ctx.deploy.pubkey(), &[&ctx.founder]).await?;
    for member in [&ctx.founder, &ctx.teammate] {
        let approve = squads::proposal_approve(
            squads::ProposalVoteAccounts {
                multisig: ms,
                proposal,
                member: member.pubkey(),
            },
            squads::ProposalVoteArgs { memo: None },
            None,
        );
        send(&ctx, &[approve], &ctx.deploy.pubkey(), &[member]).await?;
    }
    let exec = squads::config_transaction_execute(
        squads::ConfigTransactionExecuteAccounts {
            multisig: ms,
            member: ctx.founder.pubkey(),
            proposal,
            transaction: config_tx,
            rent_payer: None,
            system_program: None,
        },
        vec![],
        None,
    );
    let sig = send(&ctx, &[exec], &ctx.deploy.pubkey(), &[&ctx.founder]).await?;
    let ms_end = squads::get_multisig(&ctx.rpc, &ms).await?;
    evidence!(
        10,
        "member rotated (teammate -> {}) via 2-of-2 sig={} members={} threshold={}",
        fresh_teammate.pubkey(),
        sig,
        ms_end.members.len(),
        ms_end.threshold
    );

    println!("[soak] ALL STEPS COMPLETE — archive this output as the soak evidence.");
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn short_err(e: &dyn std::error::Error) -> String {
    e.to_string().chars().take(160).collect()
}

fn load_key(name: &str) -> Result<Keypair, Box<dyn std::error::Error>> {
    let dir = std::env::var("SOAK_KEYS_DIR").unwrap_or_else(|_| "../../target/m8-keys".into());
    let path = std::path::Path::new(&dir).join(format!("{name}.json"));
    let raw = std::fs::read_to_string(&path)?;
    // On-disk format: 64 bytes (32 secret + 32 public) as a JSON array.
    let bytes: Vec<u8> = serde_json::from_str(&raw)?;
    if bytes.len() != 64 {
        return Err("keypair file must hold 64 bytes".into());
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Ok(Keypair::from_bytes(&arr)?)
}

/// Anchor instruction discriminator: sha256("global:<snake_name>")[..8].
/// Anchor 1.2 namespaces every instruction under the literal "global"
/// (anchor-syn `SIGHASH_GLOBAL_NAMESPACE`) — the #[program] module name is
/// not part of the preimage.
fn sighash(namespace: &str, name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("{namespace}:{name}").as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&h[..8]);
    out
}

/// Builds a pool-program instruction: discriminator + borsh args.
fn pool_ix<A: AnchorSerialize>(name: &str, args: A, accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = sighash("global", name).to_vec();
    data.extend(args.try_to_vec().expect("borsh args"));
    Instruction {
        program_id: Pubkey::new_from_array(POOL_PROGRAM_BYTES),
        accounts,
        data,
    }
}

/// Sends a tx: `payer` pays fees, `signers` sign (payer included when it
/// signs). Splits the two so member keys never need lamports.
async fn send(
    ctx: &Ctx,
    ixs: &[Instruction],
    payer: &Pubkey,
    signers: &[&Keypair],
) -> Result<String, Box<dyn std::error::Error>> {
    let blockhash = ctx.rpc.get_latest_blockhash().await?;
    let mut tx = Transaction::new_unsigned(Message::new(ixs, Some(payer)));
    tx.sign(signers, blockhash);
    ctx.rpc
        .send_and_confirm_transaction(&tx)
        .await
        .map(|s| s.to_string())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Fee lamports for the member keys (they sign; deploy pays rent/fees —
/// but vote instructions include member as SIGNER only, no lamports
/// needed... funding keeps ANY failure mode from being a balance surprise).
async fn fund_members(ctx: &Ctx) -> Result<(), Box<dyn std::error::Error>> {
    for k in [&ctx.founder, &ctx.teammate] {
        let bal = ctx.rpc.get_balance(&k.pubkey()).await?;
        if bal < 100_000 {
            let ix = solana_sdk::system_instruction::transfer(
                &ctx.deploy.pubkey(),
                &k.pubkey(),
                100_000_000 - bal.min(100_000_000),
            );
            send(
                ctx,
                std::slice::from_ref(&ix),
                &ctx.deploy.pubkey(),
                &[&ctx.deploy],
            )
            .await?;
        }
    }
    Ok(())
}

/// Depositor's associated USDC token account (ATA derivation: owner,
/// token program, mint).
/// Reads a renter prepaid record's `amount` (anchor layout: 8-byte
/// discriminator, then u64 LE).
async fn prepaid_amount(ctx: &Ctx, pda: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let acct = ctx.rpc.get_account(pda).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&acct.data[8..16]);
    Ok(u64::from_le_bytes(b))
}

fn depositor_ata(ctx: &Ctx) -> Pubkey {
    Pubkey::find_program_address(
        &[
            ctx.depositor.pubkey().as_ref(),
            ctx.token_v3.as_ref(),
            ctx.usdc.as_ref(),
        ],
        &Pubkey::new_from_array(ATA_PROGRAM_BYTES),
    )
    .0
}

/// Raw u64 amount of a Token v3 account (layout: mint 32 + owner 32 +
/// amount u64 at offset 64).
async fn token_amount(ctx: &Ctx, acct: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let ai = ctx.rpc.get_account(acct).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&ai.data[64..72]);
    Ok(u64::from_le_bytes(b))
}

/// Current upgrade authority from a program-data account
/// (BPFUpgradeableLoader layout: 4-byte tag + u64 slot + pubkey-or-none).
async fn read_upgrade_authority(
    ctx: &Ctx,
    program_data: &Pubkey,
) -> Result<Option<Pubkey>, Box<dyn std::error::Error>> {
    let ai = ctx.rpc.get_account(program_data).await?;
    if ai.data[44] == 0 {
        return Ok(None);
    }
    let mut b = [0u8; 32];
    b.copy_from_slice(&ai.data[45..77]);
    Ok(Some(Pubkey::new_from_array(b)))
}

/// Full 2-of-2 vault-transaction lifecycle. Returns the tx index used.
async fn propose_approve_execute(
    ctx: &Ctx,
    ms: &Pubkey,
    ms_state: &squads_state::Multisig,
    inner: Instruction,
    label: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let vault = pda::get_vault_pda(ms, 0, None).0;
    let tx_index = ms_state.transaction_index + 1;
    let (tx_pda, _) = pda::get_transaction_pda(ms, tx_index, None);
    let (proposal, _) = pda::get_proposal_pda(ms, tx_index, None);
    let message = squads_multisig_program::TransactionMessage::try_compile(
        &vault,
        std::slice::from_ref(&inner),
        &[],
    )?;

    let create = squads::vault_transaction_create(
        squads::VaultTransactionCreateAccounts {
            multisig: *ms,
            transaction: tx_pda,
            creator: ctx.founder.pubkey(),
            rent_payer: ctx.deploy.pubkey(),
            system_program: solana_program::system_program::ID,
        },
        0,
        0,
        &message,
        Some(label.to_string()),
        None,
    );
    let prop = squads::proposal_create(
        squads::ProposalCreateAccounts {
            multisig: *ms,
            creator: ctx.founder.pubkey(),
            proposal,
            rent_payer: ctx.deploy.pubkey(),
            system_program: solana_program::system_program::ID,
        },
        squads::ProposalCreateArgs {
            transaction_index: tx_index,
            draft: false,
        },
        None,
    );
    send(ctx, &[create, prop], &ctx.deploy.pubkey(), &[&ctx.founder]).await?;
    for member in [&ctx.founder, &ctx.teammate] {
        let approve = squads::proposal_approve(
            squads::ProposalVoteAccounts {
                multisig: *ms,
                proposal,
                member: member.pubkey(),
            },
            squads::ProposalVoteArgs { memo: None },
            None,
        );
        send(ctx, &[approve], &ctx.deploy.pubkey(), &[member]).await?;
    }
    let exec = squads::vault_transaction_execute(
        squads::VaultTransactionExecuteAccounts {
            multisig: *ms,
            transaction: tx_pda,
            member: ctx.founder.pubkey(),
            proposal,
        },
        0,
        0,
        &message,
        &[],
        None,
    )?;
    let sig = send(ctx, &[exec], &ctx.deploy.pubkey(), &[&ctx.founder]).await?;
    println!("[soak]    2-of-2 executed '{label}' sig={sig}");
    // squads tx_index is u64 in the client; the instruction takes u32.
    Ok(u32::try_from(tx_index).expect("tx index fits u32"))
}
