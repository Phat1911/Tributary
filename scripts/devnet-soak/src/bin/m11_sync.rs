//! M11 DEVNET SYNC — upgrade the M8 pool program to M11 through the
//! EXISTING Squads 2-of-2, then prove a real settle run (SAT-2 kickoff,
//! 2026-11-28; evidence in docs/devnet-m11.md).
//!
//! This bin EXTENDS scripts/devnet-soak minimally (workflow: "reuse what
//! devnet-soak already does for proposals"): it reuses the crate's squads
//! SDK + the `propose_approve_execute` lifecycle, but with TODAY'S members
//! (founder + teammate-rotated — the original teammate was rotated out on
//! 2026-11-01, so the old soak client cannot approve anymore).
//!
//! The three 2026-11-02 ops lessons are applied here:
//!   (1) a GROWN ELF needs `solana program extend` FIRST — top-level, paid
//!       by the deploy key (the orchestrator did this; the runtime rejects
//!       ExtendProgram from inner instructions). This bin only CHECKS the
//!       ProgramData fit before proposing the upgrade.
//!   (2) the buffer-authority layout is `[u32 tag=1][u8 option][32 B key]`
//!       — the key sits at bytes 5..37 (the soak client's 4..36 parse is
//!       off by one; do NOT copy it).
//!   (3) stale approved-but-unexecuted proposals (7, 8) are skipped by
//!       reading the multisig's LIVE `transaction_index` and proposing at
//!       index + 1 — never a hardcoded index.
//!
//! Subcommands (devnet only; every keypair path is a 64-byte JSON array,
//! never logged):
//!   preflight                read-only: members, threshold, pool state,
//!                            program authority, ProgramData fit, slot.
//!   upgrade <buffer_pubkey>  vault-signed `Upgrade` through a fresh 2-of-2
//!                            vault transaction: create + proposal, approve
//!                            (founder), approve (teammate-rotated), execute.
//!   resume                   if the pool is paused, `resume` via 2-of-2.
//!   prepay <micro_usdc>      renter prepay into the escrow vault so a claim
//!                            can pay (backend gate + renter key sign).
//!   post_root / challenge    SAT-2b's second root + permissionless freeze.
//!   claim_frozen             simulate RootFrozen, then land that refusal.
//!   resolve_stands           RootStands through the live next 2-of-2 index.
//!   direct_claim             depositor signs and pays the marker rent.
//!   resolve_false <window> <slash_micro_usdc> <victim_key>
//!                            SAT-2c's RootFalse ruling through the 2-of-2.
//!   slash_bond <window> <victim_usdc_token_account>
//!                            the permissionless crank; the deploy key pays.
//!   claim_slashed            simulate RootFrozen on a Slashed root, land it.
//!   tx_readback <signature>  re-read one landed transaction (signers, token
//!                            deltas, CPI, decoded events) from the chain.
//!   verify                   re-read authority / pool / slot after the run.
//!
//! Env: M11_KEYS_DIR (default ../../target/m8-keys), M11_RPC (default
//! https://api.devnet.solana.com), M11_RENTER_WALLET (default the
//! provisioned custodial renter USDC account from the M8 soak).

use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_sdk::bpf_loader_upgradeable;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::Transaction;
use squads_multisig::anchor_lang::AnchorSerialize;
use squads_multisig::client as squads;
use squads_multisig::pda;
use squads_multisig::solana_program;
use squads_multisig::state as squads_state;
use squads_multisig::{squads_multisig_program, vault_transaction::VaultTransactionMessageExt};
// `FromStr` so the hardcoded address constants parse into Pubkeys.
use std::str::FromStr;

// --- verified id bytes (self-checked against the strings at startup) -------

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
const POOL_PROGRAM_STR: &str = "9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX";
const USDC_DEVNET_STR: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const TOKEN_V3_STR: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// The provisioned custodial renter USDC account (M8 soak step 6; NOT an
/// ATA — owner/mint/program verified on-chain before any money moves).
const RENTER_USDC_ATA_STR: &str = "3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs";
/// The multisig configuration account (members + threshold + live
/// transaction_index); the vault is its index-0 vault PDA.
const MULTISIG_STR: &str = "GuSeFfFkdKtjkubeLUEeBHW5pfHSJctGnz8AFk7xrCk5";
/// SAT-2b fixes the second root's one leaf to this depositor; loading the
/// wrong keypair must stop the run before any transaction is signed.
const EXPECTED_DEPOSITOR_STR: &str = "Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ";
/// The program stores a 32-byte fraud-proof commitment, while this exact
/// reviewer-supplied preimage is 40 bytes. Store SHA-256(preimage), and keep
/// the preimage itself in the public evidence document for verification.
const CHALLENGE_EVIDENCE_PREIMAGE: &[u8] = b"reviewer-requested challenge, 2026-11-28";
const CHALLENGE_WINDOW_SLOTS: u64 = 100;
const WINDOW_MARGIN_SLOTS: u64 = 5;

const SEED_POOL: &[u8] = b"pool";
const SEED_VAULT: &[u8] = b"vault";
const SEED_PREPAID: &[u8] = b"prepaid";
const SEED_ROOT: &[u8] = b"settlement_root";
const SEED_CLAIM_MARKER: &[u8] = b"claim_marker";
const SEED_BOND: &[u8] = b"bond";
const SEED_BOND_VAULT: &[u8] = b"bond_vault";

/// ProgramData layout: `[u32 tag=3][u64 deployed_slot][u8 option][32 B
/// authority]` — option at 12, authority at 13..45 (verified live 2026-11-01).
const PD_HEADER_LEN: usize = 45;
/// Pool account layout: `[8 disc][admin 32][backend 32][usdc 32][claim mint
/// 32][vault 32][paused u8]` — the paused flag sits at offset 168.
const POOL_PAUSED_OFFSET: usize = 168;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        if let Err(e) = run().await {
            eprintln!("[m11-sync] FAILED: {e:?}");
            std::process::exit(1);
        }
    });
}

struct Ctx {
    rpc: RpcClient,
    pool: Pubkey,
    usdc: Pubkey,
    token_v3: Pubkey,
    ms: Pubkey,
    vault: Pubkey,
    deploy: Keypair,
    founder: Keypair,
    teammate_rotated: Keypair,
    backend: Keypair,
    depositor: Keypair,
    rogue_upgrader: Keypair,
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(bs58_encode(&POOL_PROGRAM_BYTES), POOL_PROGRAM_STR);
    assert_eq!(bs58_encode(&USDC_DEVNET_BYTES), USDC_DEVNET_STR);
    assert_eq!(bs58_encode(&TOKEN_V3_BYTES), TOKEN_V3_STR);

    let rpc = RpcClient::new(
        std::env::var("M11_RPC").unwrap_or_else(|_| "https://api.devnet.solana.com".into()),
    );
    let pool = Pubkey::new_from_array(POOL_PROGRAM_BYTES);
    let ms = Pubkey::from_str(MULTISIG_STR)?;
    let (vault, _) = pda::get_vault_pda(&ms, 0, None);
    let ctx = Ctx {
        rpc,
        pool,
        usdc: Pubkey::new_from_array(USDC_DEVNET_BYTES),
        token_v3: Pubkey::new_from_array(TOKEN_V3_BYTES),
        ms,
        vault,
        deploy: load_key("deploy")?,
        founder: load_key("founder")?,
        teammate_rotated: load_key("teammate-rotated")?,
        backend: load_key("backend")?,
        depositor: load_key("depositor")?,
        rogue_upgrader: load_key("rogue-upgrader")?,
    };
    assert_eq!(
        ctx.depositor.pubkey().to_string(),
        EXPECTED_DEPOSITOR_STR,
        "depositor keypair does not match SAT-2b's leaf"
    );
    let cmd = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "preflight".into());
    match cmd.as_str() {
        "preflight" | "verify" => preflight(&ctx).await?,
        "upgrade" => {
            let arg = std::env::args()
                .nth(2)
                .ok_or("usage: m11_sync upgrade <buffer_pubkey>")?;
            upgrade(&ctx, Pubkey::from_str(&arg)?).await?;
        }
        "resume" => resume(&ctx).await?,
        "settle_verify" => {
            let arg = std::env::args()
                .nth(2)
                .ok_or("usage: m11_sync settle_verify <window_u64>")?;
            settle_verify(&ctx, arg.parse()?).await?;
        }
        "prepay" => {
            let arg = std::env::args()
                .nth(2)
                .ok_or("usage: m11_sync prepay <micro_usdc>")?;
            prepay(&ctx, arg.parse()?).await?;
        }
        "post_root" => {
            let window = required_arg(
                2,
                "usage: m11_sync post_root <window_u64> <amount_micro_usdc>",
            )?;
            let amount = required_arg(
                3,
                "usage: m11_sync post_root <window_u64> <amount_micro_usdc>",
            )?;
            post_root_single(&ctx, window.parse()?, amount.parse()?).await?;
        }
        "challenge" => {
            let window = required_arg(
                2,
                "usage: m11_sync challenge <window_u64> [evidence_preimage]",
            )?;
            // The evidence commitment is SHA-256 of a caller-supplied string.
            // Omitted, it stays Part 2's reviewer text (that command is
            // unchanged); SAT-2c names its own run.
            let evidence_preimage = match std::env::args().nth(3) {
                Some(text) => text.into_bytes(),
                None => CHALLENGE_EVIDENCE_PREIMAGE.to_vec(),
            };
            challenge_root(&ctx, window.parse()?, &evidence_preimage).await?;
        }
        "claim_frozen" => {
            let window = required_arg(
                2,
                "usage: m11_sync claim_frozen <window_u64> <amount_micro_usdc>",
            )?;
            let amount = required_arg(
                3,
                "usage: m11_sync claim_frozen <window_u64> <amount_micro_usdc>",
            )?;
            claim_frozen(&ctx, window.parse()?, amount.parse()?).await?;
        }
        "resolve_stands" => {
            let window = required_arg(2, "usage: m11_sync resolve_stands <window_u64>")?;
            resolve_stands(&ctx, window.parse()?).await?;
        }
        "direct_claim" => {
            let window = required_arg(
                2,
                "usage: m11_sync direct_claim <window_u64> <amount_micro_usdc>",
            )?;
            let amount = required_arg(
                3,
                "usage: m11_sync direct_claim <window_u64> <amount_micro_usdc>",
            )?;
            direct_claim(&ctx, window.parse()?, amount.parse()?).await?;
        }
        "resolve_false" => {
            let window = required_arg(
                2,
                "usage: m11_sync resolve_false <window_u64> <slash_micro_usdc> <victim_key>",
            )?;
            let slash = required_arg(
                3,
                "usage: m11_sync resolve_false <window_u64> <slash_micro_usdc> <victim_key>",
            )?;
            let victim = required_arg(
                4,
                "usage: m11_sync resolve_false <window_u64> <slash_micro_usdc> <victim_key>",
            )?;
            resolve_false(
                &ctx,
                window.parse()?,
                slash.parse()?,
                Pubkey::from_str(&victim)?,
            )
            .await?;
        }
        "slash_bond" => {
            let window = required_arg(
                2,
                "usage: m11_sync slash_bond <window_u64> <victim_usdc_token_account>",
            )?;
            let destination = required_arg(
                3,
                "usage: m11_sync slash_bond <window_u64> <victim_usdc_token_account>",
            )?;
            slash_bond(&ctx, window.parse()?, Pubkey::from_str(&destination)?).await?;
        }
        "claim_slashed" => {
            let window = required_arg(
                2,
                "usage: m11_sync claim_slashed <window_u64> <amount_micro_usdc>",
            )?;
            let amount = required_arg(
                3,
                "usage: m11_sync claim_slashed <window_u64> <amount_micro_usdc>",
            )?;
            claim_slashed(&ctx, window.parse()?, amount.parse()?).await?;
        }
        "tx_readback" => {
            let sig = required_arg(2, "usage: m11_sync tx_readback <signature>")?;
            tx_readback(&ctx, &sig).await?;
        }
        "ata_probe" => ata_probe(&ctx).await?,
        other => return Err(format!("unknown subcommand {other}").into()),
    }
    Ok(())
}

/// Read-only preflight/post-verify: everything the evidence doc needs, all
/// of it read back from the chain (never from memory).
async fn preflight(ctx: &Ctx) -> Result<(), Box<dyn std::error::Error>> {
    let slot = ctx.rpc.get_slot().await?;
    println!("[m11-sync] slot: {slot}");
    let ms_state = squads::get_multisig(&ctx.rpc, &ctx.ms).await?;
    println!(
        "[m11-sync] multisig {} threshold={} transaction_index={} members:",
        ctx.ms, ms_state.threshold, ms_state.transaction_index
    );
    for m in &ms_state.members {
        println!("[m11-sync]   member {}", m.key);
    }
    // TODAY'S signers must be members — approving with a rotated-out key
    // would burn a proposal index on an unexecutable proposal.
    for (role, key) in [
        ("founder", ctx.founder.pubkey()),
        ("teammate-rotated", ctx.teammate_rotated.pubkey()),
    ] {
        if !ms_state.members.iter().any(|m| m.key == key) {
            return Err(format!("{role} key {key} is NOT an on-chain member — STOP").into());
        }
    }
    let (pd, _) = Pubkey::find_program_address(&[ctx.pool.as_ref()], &bpf_loader_upgradeable::id());
    let pd_acct = ctx.rpc.get_account(&pd).await?;
    let authority = if pd_acct.data[12] == 0 {
        None
    } else {
        let mut b = [0u8; 32];
        b.copy_from_slice(&pd_acct.data[13..45]);
        Some(Pubkey::new_from_array(b))
    };
    println!(
        "[m11-sync] ProgramData {} space={} (program area {} B) authority={authority:?} (vault={})",
        pd,
        pd_acct.data.len(),
        pd_acct.data.len().saturating_sub(PD_HEADER_LEN),
        ctx.vault
    );
    let deployed_slot = u64::from_le_bytes(pd_acct.data[4..12].try_into().expect("slot slice"));
    println!("[m11-sync] last deployed slot: {deployed_slot}");
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    print_pool(ctx, &pool_pda).await?;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    println!(
        "[m11-sync] escrow vault ATA {vault_ta} balance={} micro-USDC",
        token_amount(ctx, &vault_ta).await?
    );
    for (role, k) in [
        ("deploy", ctx.deploy.pubkey()),
        ("backend", ctx.backend.pubkey()),
        ("depositor", ctx.depositor.pubkey()),
    ] {
        println!(
            "[m11-sync] {role} {k} SOL={}",
            ctx.rpc.get_balance(&k).await.unwrap_or(0)
        );
    }
    Ok(())
}

/// Prints the pool's admin, backend, and paused flag — the three fields that
/// MUST be byte-identical across the upgrade (state-layout safety).
async fn print_pool(ctx: &Ctx, pool_pda: &Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    let acct = ctx.rpc.get_account(pool_pda).await?;
    let admin = Pubkey::new_from_array(acct.data[8..40].try_into().expect("admin"));
    let backend = Pubkey::new_from_array(acct.data[40..72].try_into().expect("backend"));
    let paused = acct.data.get(POOL_PAUSED_OFFSET).copied().unwrap_or(0);
    println!(
        "[m11-sync] pool {} admin={} backend={} paused={} (data len {})",
        pool_pda,
        admin,
        backend,
        paused,
        acct.data.len()
    );
    Ok(())
}

/// The upgrade, end to end, through the 2-of-2. Prints the proposal index,
/// both approval signatures, the execute signature, and the slots before and
/// after — the evidence the kickoff requires.
async fn upgrade(ctx: &Ctx, buffer: Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    let slot_before = ctx.rpc.get_slot().await?;
    println!("[m11-sync] UPGRADE slot_before={slot_before}");
    // Lesson (2): buffer authority key lives at bytes 5..37 — parse it there.
    let buf = ctx.rpc.get_account(&buffer).await?;
    if buf.owner != bpf_loader_upgradeable::id() {
        return Err("buffer is not owned by the upgradeable loader".into());
    }
    let mut auth = [0u8; 32];
    auth.copy_from_slice(&buf.data[5..37]);
    let buf_authority = Pubkey::new_from_array(auth);
    println!(
        "[m11-sync] buffer {buffer} authority={buf_authority} (bytes 5..37), data len {}",
        buf.data.len()
    );
    if buf_authority != ctx.vault {
        return Err("buffer authority != vault — hand it over first (set-buffer-authority)".into());
    }
    // Lesson (1) guard: the extend must already have happened — the loader
    // refuses an upgrade that does not fit, and ExtendProgram cannot ride
    // inside the multisig (CPI allowlist).
    let (pd, _) = Pubkey::find_program_address(&[ctx.pool.as_ref()], &bpf_loader_upgradeable::id());
    let pd_len = ctx.rpc.get_account(&pd).await?.data.len();
    let program_area = pd_len - PD_HEADER_LEN;
    let needed = buf.data.len() - 37; // buffer = 37-byte header + ELF
    if needed > program_area {
        return Err(format!(
            "M11 ELF ({needed} B) does NOT fit ProgramData ({program_area} B) — run `solana program extend` top-level FIRST (lesson 1)"
        )
        .into());
    }
    println!("[m11-sync] fit OK: ELF {needed} B <= program area {program_area} B");
    let upgrade_ix = bpf_loader_upgradeable::upgrade(
        &ctx.pool,
        &buffer,
        &ctx.vault,           // upgrade authority: the vault PDA signs via Squads
        &ctx.deploy.pubkey(), // spill: leftover buffer lamports return to the deploy key
    );
    let ms_state = squads::get_multisig(&ctx.rpc, &ctx.ms).await?;
    let idx = propose_approve_execute(ctx, &ms_state, upgrade_ix, "M11 program upgrade via 2-of-2")
        .await?;
    let slot_after = ctx.rpc.get_slot().await?;
    println!("[m11-sync] UPGRADE executed at proposal index {idx}; slot_before={slot_before} slot_after={slot_after}");
    preflight(ctx).await?;
    Ok(())
}

/// Resume a paused pool through the 2-of-2 (a pool can ARRIVE paused after
/// an upgrade only if it was paused before — but check, never assume).
async fn resume(ctx: &Ctx) -> Result<(), Box<dyn std::error::Error>> {
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let acct = ctx.rpc.get_account(&pool_pda).await?;
    if acct.data.get(POOL_PAUSED_OFFSET) != Some(&1) {
        println!("[m11-sync] pool is ACTIVE — no resume needed");
        return Ok(());
    }
    let ix = pool_ix(
        "resume",
        (),
        vec![
            AccountMeta::new(pool_pda, false),
            AccountMeta::new_readonly(ctx.vault, true),
        ],
    );
    let ms_state = squads::get_multisig(&ctx.rpc, &ctx.ms).await?;
    let idx = propose_approve_execute(ctx, &ms_state, ix, "M11 resume via 2-of-2").await?;
    println!("[m11-sync] pool RESUMED via 2-of-2 (index {idx})");
    print_pool(ctx, &pool_pda).await?;
    Ok(())
}

/// `prepay_renter(renter, amount)`: USDC from the custodial renter wallet
/// into the escrow vault (D40's only USDC inflow). Backend gates + the
/// renter key signs; the vault and the renter's prepaid record mirror.
async fn prepay(ctx: &Ctx, amount: u64) -> Result<(), Box<dyn std::error::Error>> {
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let renter_wallet = Pubkey::from_str(
        &std::env::var("M11_RENTER_WALLET").unwrap_or_else(|_| RENTER_USDC_ATA_STR.into()),
    )?;
    // Verify the wallet's shape on-chain before moving money (owner/mint/
    // token program — a provisioned account, not a derived one).
    let wa = ctx.rpc.get_account(&renter_wallet).await?;
    if wa.owner != ctx.token_v3
        || &wa.data[0..32] != ctx.usdc.as_ref()
        || &wa.data[32..64] != ctx.depositor.pubkey().as_ref()
    {
        return Err("renter wallet shape wrong (program/mint/owner mismatch)".into());
    }
    let prepaid =
        Pubkey::find_program_address(&[SEED_PREPAID, ctx.depositor.pubkey().as_ref()], &ctx.pool).0;
    let ix = pool_ix(
        "prepay_renter",
        (&ctx.depositor.pubkey(), &amount),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new(ctx.backend.pubkey(), true),
            AccountMeta::new_readonly(ctx.depositor.pubkey(), true),
            AccountMeta::new(renter_wallet, false),
            AccountMeta::new(vault_ta, false),
            AccountMeta::new(prepaid, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    let before = token_amount(ctx, &vault_ta).await?;
    let sig = send(
        ctx,
        &[ix],
        &ctx.backend.pubkey(),
        &[&ctx.backend, &ctx.depositor],
    )
    .await?;
    let after = token_amount(ctx, &vault_ta).await?;
    let record = prepaid_amount(ctx, &prepaid).await?;
    let slot = signature_slot(ctx, &sig).await?;
    println!("[m11-sync] prepay_renter {amount} micro-USDC sig={sig} slot={slot}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    println!("[m11-sync]   vault {before} -> {after}; renter prepaid record={record}");
    if after != before + amount {
        return Err(format!("vault mirror broken: {before} + {amount} != {after}").into());
    }
    Ok(())
}

/// Reads the settlement run's on-chain state back for the evidence doc:
/// the SettlementRoot (total_payout / paid_out / status / posted_at / bond),
/// the leaf-0 ClaimMarker, the bond record + bond vault, the escrow vault,
/// and the depositor's USDC ATA. Layouts mirror settlement.rs exactly.
async fn settle_verify(ctx: &Ctx, window: u64) -> Result<(), Box<dyn std::error::Error>> {
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let (root, _) =
        Pubkey::find_program_address(&[SEED_ROOT, window.to_le_bytes().as_ref()], &ctx.pool);
    println!("[m11-sync] settle_verify window={window}");
    println!("[m11-sync]   pool {pool_pda}");
    println!("[m11-sync]   root account {root}");
    let r = ctx.rpc.get_account(&root).await?;
    let r_pool = Pubkey::new_from_array(r.data[8..40].try_into().expect("pool"));
    let r_window = u64::from_le_bytes(r.data[40..48].try_into().expect("window"));
    let mut merkle = [0u8; 32];
    merkle.copy_from_slice(&r.data[48..80]);
    let total = u64::from_le_bytes(r.data[80..88].try_into().expect("total"));
    let status = r.data[88];
    let posted_at = i64::from_le_bytes(r.data[89..97].try_into().expect("posted_at"));
    let bond = u64::from_le_bytes(r.data[97..105].try_into().expect("bond"));
    let challenger = Pubkey::new_from_array(r.data[105..137].try_into().expect("challenger"));
    let mut evidence = [0u8; 32];
    evidence.copy_from_slice(&r.data[137..169]);
    let ruled_slash = u64::from_le_bytes(r.data[169..177].try_into().expect("ruled_slash"));
    let victim = Pubkey::new_from_array(r.data[177..209].try_into().expect("victim"));
    let stands_ruled = r.data[209] != 0;
    let paid_out = u64::from_le_bytes(r.data[210..218].try_into().expect("paid_out"));
    let status_name = match status {
        0 => "Active",
        1 => "Challenged",
        2 => "ProvenFalse",
        3 => "Slashed",
        _ => "UNKNOWN",
    };
    println!(
        "[m11-sync]   root.pool={r_pool} window={r_window} status={status_name}({status}) posted_at={posted_at}"
    );
    println!("[m11-sync]   merkle_root=0x{}", hex32(&merkle));
    println!("[m11-sync]   total_payout={total} paid_out={paid_out} bond_snapshot={bond}");
    println!(
        "[m11-sync]   stands_ruled={stands_ruled} challenger={challenger} evidence=0x{}",
        hex32(&evidence)
    );
    println!("[m11-sync]   ruled_slash={ruled_slash} victim={victim}");
    let (marker, _) = Pubkey::find_program_address(
        &[SEED_CLAIM_MARKER, root.as_ref(), &0u32.to_le_bytes()],
        &ctx.pool,
    );
    println!("[m11-sync]   claim marker (leaf 0) {marker}");
    match ctx.rpc.get_account(&marker).await {
        Ok(m) => {
            let m_depositor = Pubkey::new_from_array(m.data[44..76].try_into().expect("depositor"));
            let m_amount = u64::from_le_bytes(m.data[76..84].try_into().expect("amount"));
            println!("[m11-sync]   marker.depositor={m_depositor} amount={m_amount}");
        }
        Err(_) => println!("[m11-sync]   marker does not exist yet (no leaf paid)"),
    }
    let (bond_rec, _) = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool);
    let (bond_vault, _) = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND_VAULT], &ctx.pool);
    let bond_amt = prepaid_amount(ctx, &bond_rec).await?; // same layout: disc + u64
    let bond_vault_amt = token_amount(ctx, &bond_vault).await?;
    let vault_amt = token_amount(ctx, &vault_ta).await?;
    println!("[m11-sync]   bond record {bond_rec} amount={bond_amt}; bond vault {bond_vault} balance={bond_vault_amt}");
    println!("[m11-sync]   escrow vault {vault_ta} balance={vault_amt}");
    let dep_ata = ata(&ctx.depositor.pubkey(), &ctx.usdc, &ctx.token_v3)?;
    let dep_amt = token_amount(ctx, &dep_ata).await?;
    println!("[m11-sync]   depositor USDC ATA {dep_ata} balance={dep_amt}");
    Ok(())
}

/// SAT-2b step 2: post the second settlement root without touching the bond.
/// The root pays one leaf (the loaded depositor key) and snapshots the bond
/// already standing from the SAT-2 run.
async fn post_root_single(
    ctx: &Ctx,
    window: u64,
    amount: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let bond = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool).0;
    let root = root_address(ctx, window);
    if ctx.rpc.get_account(&root).await.is_ok() {
        return Err(format!("root {root} already exists — roots are append-only").into());
    }
    let merkle_root = single_leaf_root(&ctx.depositor.pubkey(), amount);
    let ix = pool_ix(
        "post_root",
        (window, merkle_root, amount),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new(ctx.backend.pubkey(), true),
            AccountMeta::new(root, false),
            AccountMeta::new_readonly(vault_ta, false),
            AccountMeta::new_readonly(bond, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
            AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    let sig = send(ctx, &[ix], &ctx.backend.pubkey(), &[&ctx.backend]).await?;
    print_confirmed_signature(ctx, "post_root", &sig).await?;
    println!("[m11-sync]   root account {root} window={window} amount={amount}");
    println!(
        "[m11-sync]   leaf depositor {} amount={amount}",
        ctx.depositor.pubkey()
    );
    println!("[m11-sync]   merkle_root=0x{}", hex32(&merkle_root));
    settle_verify(ctx, window).await?;
    Ok(())
}

/// SAT-2b step 2 (challenge): the rogue key is not the operator and not a
/// multisig member; it signs the permissionless challenge. The deploy key
/// pays only the fee, so no faucet or key funding is needed. The evidence
/// commitment is SHA-256 of `evidence_preimage` (any length).
async fn challenge_root(
    ctx: &Ctx,
    window: u64,
    evidence_preimage: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (posted_at, status) = root_posted_at_status(ctx, &root).await?;
    if status != 0 {
        return Err(format!("root {root} is not Active (status={status})").into());
    }
    let slot_now = ctx.rpc.get_slot().await?;
    if slot_now >= posted_at + CHALLENGE_WINDOW_SLOTS {
        return Err(format!(
            "challenge window already closed: slot {slot_now} >= posted_at {posted_at} + {CHALLENGE_WINDOW_SLOTS}"
        )
        .into());
    }
    let evidence = sha256(evidence_preimage);
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let ix = pool_ix(
        "challenge",
        (window, evidence),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.rogue_upgrader.pubkey(), true),
            AccountMeta::new(root, false),
            AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        ],
    );
    println!(
        "[m11-sync] challenge evidence preimage: {:?} ({} bytes)",
        String::from_utf8_lossy(evidence_preimage),
        evidence_preimage.len()
    );
    println!(
        "[m11-sync] challenge evidence sha256=0x{}",
        hex32(&evidence)
    );
    println!(
        "[m11-sync] challenger {} is neither backend {} nor a multisig member",
        ctx.rogue_upgrader.pubkey(),
        ctx.backend.pubkey()
    );
    let sig = send(
        ctx,
        &[ix],
        &ctx.deploy.pubkey(),
        &[&ctx.deploy, &ctx.rogue_upgrader],
    )
    .await?;
    print_confirmed_signature(ctx, "challenge", &sig).await?;
    settle_verify(ctx, window).await?;
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 1 {
        return Err(format!("challenge did not freeze root: status={status}").into());
    }
    Ok(())
}

/// SAT-2b step 5: try the assisted claim against the frozen root. First the
/// exact transaction is simulated and must read `RootFrozen`; then it is
/// broadcast with skip_preflight so the refusal itself lands on devnet with
/// a signature and slot. A successful claim here would be a doctrine bug and
/// stops the run.
async fn claim_frozen(
    ctx: &Ctx,
    window: u64,
    amount: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 1 {
        return Err(format!("root {root} is not Challenged (status={status})").into());
    }
    refuse_claim_on_chain(ctx, window, amount, "frozen-claim").await
}

/// SAT-2c step 5: a claim against the SLASHED root. A slashed root is frozen
/// for good, so the exact claim must read `RootFrozen` — the status gate
/// fires before the window check, however long ago the window closed. A
/// successful claim here would be a doctrine bug and stops the run.
async fn claim_slashed(
    ctx: &Ctx,
    window: u64,
    amount: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 3 {
        return Err(format!("root {root} is not Slashed (status={status})").into());
    }
    refuse_claim_on_chain(ctx, window, amount, "slashed-claim").await
}

/// The refusal procedure shared by `claim_frozen` (Part 2) and
/// `claim_slashed` (Part 3): the exact claim — the ASSISTED door, the
/// backend signing — is simulated and MUST fail with `RootFrozen`/6015; it
/// is then broadcast with skip_preflight so the refusal itself lands with a
/// devnet signature and slot; the failed transaction is read back from the
/// chain; and the balances, the claim marker and `paid_out` are proven
/// unchanged.
async fn refuse_claim_on_chain(
    ctx: &Ctx,
    window: u64,
    amount: u64,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let destination = ata(&ctx.depositor.pubkey(), &ctx.usdc, &ctx.token_v3)?;
    let before_vault = token_amount(ctx, &vault_ta).await?;
    let before_destination = token_amount(ctx, &destination).await?;
    let ixs = single_leaf_claim_ixs(ctx, window, amount, &ctx.backend.pubkey())?;
    let blockhash = ctx.rpc.get_latest_blockhash().await?;
    let mut tx = Transaction::new_unsigned(Message::new(&ixs, Some(&ctx.backend.pubkey())));
    tx.sign(&[&ctx.backend], blockhash);
    let sig = tx.signatures[0].to_string();
    let sim = ctx.rpc.simulate_transaction(&tx).await?;
    let sim_logs = sim.value.logs.clone().unwrap_or_default();
    let sim_err = sim
        .value
        .err
        .ok_or("claim SIMULATION succeeded — STOP: a root that is not Active paid out")?;
    println!("[m11-sync] {label} simulation error: {sim_err:?}");
    for line in &sim_logs {
        println!("[m11-sync]   sim log: {line}");
    }
    if !root_frozen_error(&format!("{sim_err:?}"), &sim_logs) {
        return Err("simulation failed, but not with RootFrozen/6015 — STOP".into());
    }
    let send_result = ctx
        .rpc
        .send_and_confirm_transaction_with_spinner_and_config(
            &tx,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig {
                skip_preflight: true,
                ..RpcSendTransactionConfig::default()
            },
        )
        .await;
    match send_result {
        Ok(landed) => {
            return Err(
                format!("{label} unexpectedly PAID at {landed} — STOP: doctrine violation").into(),
            )
        }
        Err(e) => {
            let exact = format!("{e:?}");
            println!("[m11-sync] {label} on-chain refusal error: {exact}");
            println!("[m11-sync] {label} refusal signature {sig}");
            if !exact.contains("6015") && !exact.contains("0x177f") {
                return Err("landed refusal did not report RootFrozen/6015 — STOP".into());
            }
        }
    }
    let slot = failed_signature_slot(ctx, &sig, label).await?;
    println!("[m11-sync] {label} refusal landed in slot {slot}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    tx_readback(ctx, &sig).await?;
    let after_vault = token_amount(ctx, &vault_ta).await?;
    let after_destination = token_amount(ctx, &destination).await?;
    if (before_vault, before_destination) != (after_vault, after_destination) {
        return Err("failed claim moved balances — STOP: transaction was not atomic".into());
    }
    settle_verify(ctx, window).await?;
    // A refusal must leave no trace of a payout: no marker, `paid_out` 0.
    let marker = Pubkey::find_program_address(
        &[SEED_CLAIM_MARKER, root.as_ref(), &0u32.to_le_bytes()],
        &ctx.pool,
    )
    .0;
    if ctx.rpc.get_account(&marker).await.is_ok() {
        return Err("failed claim created a claim marker — STOP".into());
    }
    let acct = ctx.rpc.get_account(&root).await?;
    let paid_out = u64::from_le_bytes(acct.data[210..218].try_into().expect("paid_out"));
    if paid_out != 0 {
        return Err(format!("failed claim changed paid_out to {paid_out} — STOP").into());
    }
    println!(
        "[m11-sync] {label} post-refusal: marker {marker} absent, paid_out=0, escrow {after_vault} and depositor ATA {after_destination} unchanged"
    );
    Ok(())
}

/// SAT-2b step 6: a Squads 2-of-2 vault transaction carrying
/// `resolve_challenge(window, RootStands)`.
async fn resolve_stands(ctx: &Ctx, window: u64) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 1 {
        return Err(format!("root {root} is not Challenged (status={status})").into());
    }
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let bond = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool).0;
    // Borsh enum Ruling::RootStands is the single variant byte 0.
    let ix = pool_ix(
        "resolve_challenge",
        (window, 0u8),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.vault, true),
            AccountMeta::new(root, false),
            AccountMeta::new_readonly(bond, false),
        ],
    );
    let ms_state = squads::get_multisig(&ctx.rpc, &ctx.ms).await?;
    let idx =
        propose_approve_execute(ctx, &ms_state, ix, "M11 RootStands ruling via 2-of-2").await?;
    println!("[m11-sync] RootStands ruling executed via proposal index {idx}");
    settle_verify(ctx, window).await?;
    let acct = ctx.rpc.get_account(&root).await?;
    if acct.data[88] != 0 || acct.data[209] == 0 {
        return Err("ruling did not restore Active with stands_ruled=true".into());
    }
    Ok(())
}

/// SAT-2c step 3: a Squads 2-of-2 vault transaction carrying
/// `resolve_challenge(window, RootFalse { slash_amount, victim })`.
///
/// `victim` is the KEY the ruled slash pays — the program compares it with
/// the slash destination's OWNER — so that key must already hold a USDC
/// token account. It is checked here BEFORE a proposal index is spent: roots
/// are append-only and one ruling answers one challenge, so a ruling naming
/// an unreachable victim would strand this root in `ProvenFalse` for good.
async fn resolve_false(
    ctx: &Ctx,
    window: u64,
    slash_amount: u64,
    victim: Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 1 {
        return Err(format!("root {root} is not Challenged (status={status})").into());
    }
    if slash_amount == 0 {
        return Err("SAT-2c proves a real slash: a RootFalse ruling of 0 is refused here".into());
    }
    let acct = ctx.rpc.get_account(&root).await?;
    let bond_snapshot = u64::from_le_bytes(acct.data[97..105].try_into().expect("bond"));
    if slash_amount > bond_snapshot {
        return Err(format!(
            "slash {slash_amount} exceeds the bond snapshot {bond_snapshot} — the program would refuse (SlashExceedsBond)"
        ).into());
    }
    let victim_ata = ata(&victim, &ctx.usdc, &ctx.token_v3)?;
    let ta = ctx.rpc.get_account(&victim_ata).await?;
    if ta.owner != ctx.token_v3
        || &ta.data[0..32] != ctx.usdc.as_ref()
        || &ta.data[32..64] != victim.as_ref()
    {
        return Err(format!(
            "victim {victim} holds no USDC token account at its canonical ATA {victim_ata} — STOP before spending a proposal index"
        ).into());
    }
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let bond = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool).0;
    let bond_vault = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND_VAULT], &ctx.pool).0;
    // The ruling's wire form: `window` (u64 LE), then the `Ruling` enum —
    // variant byte 1 is `RootFalse`, followed by `slash_amount`, `victim`.
    let ix = pool_ix(
        "resolve_challenge",
        (window, 1u8, slash_amount, victim),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(ctx.vault, true),
            AccountMeta::new(root, false),
            AccountMeta::new_readonly(bond, false),
        ],
    );
    // Cross-check the serialization against a hand-built byte string — the
    // way an external integrator (and the program's own unit test) writes it.
    let mut wire = sighash("global", "resolve_challenge").to_vec();
    wire.extend_from_slice(&window.to_le_bytes());
    wire.push(1);
    wire.extend_from_slice(&slash_amount.to_le_bytes());
    wire.extend_from_slice(victim.as_ref());
    if ix.data != wire {
        return Err(
            "RootFalse instruction bytes differ from the hand-built wire form — STOP".into(),
        );
    }
    let bond_vault_before = token_amount(ctx, &bond_vault).await?;
    let victim_before = token_amount(ctx, &victim_ata).await?;
    println!(
        "[m11-sync] ruling to propose: RootFalse slash_amount={slash_amount} victim={victim} (USDC token account {victim_ata}, balance {victim_before}); bond snapshot {bond_snapshot}, bond vault {bond_vault_before}"
    );
    let ms_state = squads::get_multisig(&ctx.rpc, &ctx.ms).await?;
    let idx = propose_approve_execute(
        ctx,
        &ms_state,
        ix,
        "M11 RootFalse ruling via 2-of-2 (SAT-2c)",
    )
    .await?;
    println!("[m11-sync] RootFalse ruling executed via proposal index {idx}");
    settle_verify(ctx, window).await?;
    let acct = ctx.rpc.get_account(&root).await?;
    let ruled_slash = u64::from_le_bytes(acct.data[169..177].try_into().expect("ruled_slash"));
    let recorded_victim = Pubkey::new_from_array(acct.data[177..209].try_into().expect("victim"));
    let stands_ruled = acct.data[209] != 0;
    if acct.data[88] != 2
        || ruled_slash != slash_amount
        || recorded_victim != victim
        || stands_ruled
    {
        return Err(format!(
            "ruling did not record ProvenFalse/{slash_amount}/{victim}: status={} ruled_slash={ruled_slash} victim={recorded_victim} stands_ruled={stands_ruled}",
            acct.data[88]
        ).into());
    }
    // The ruling only RECORDS: no USDC may move before the permissionless
    // crank (`slash_bond`) executes it.
    let bond_vault_after = token_amount(ctx, &bond_vault).await?;
    let victim_after = token_amount(ctx, &victim_ata).await?;
    if (bond_vault_after, victim_after) != (bond_vault_before, victim_before) {
        return Err("the ruling moved USDC — STOP: only slash_bond may move the bond".into());
    }
    println!(
        "[m11-sync] ruling verified on the root account: status=ProvenFalse(2) ruled_slash={ruled_slash} victim={recorded_victim} stands_ruled={stands_ruled}; bond vault still {bond_vault_after}, victim still {victim_after}"
    );
    Ok(())
}

/// SAT-2c step 4: `slash_bond(window)` — the permissionless crank. The pool
/// program asks for NO signer on this instruction, so the only signature on
/// the transaction is the fee payer's (the deploy key: neither the backend
/// nor a multisig member). The amount and the destination come from the
/// root account, never from arguments.
async fn slash_bond(
    ctx: &Ctx,
    window: u64,
    destination: Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (_, status) = root_posted_at_status(ctx, &root).await?;
    if status != 2 {
        return Err(format!("root {root} is not ProvenFalse (status={status})").into());
    }
    let acct = ctx.rpc.get_account(&root).await?;
    let ruled_slash = u64::from_le_bytes(acct.data[169..177].try_into().expect("ruled_slash"));
    let victim = Pubkey::new_from_array(acct.data[177..209].try_into().expect("victim"));
    // The crank must pay the RECORDED victim: a USDC token account owned by
    // that key. Checked here so a wrong address fails before any fee is spent.
    let d = ctx.rpc.get_account(&destination).await?;
    if d.owner != ctx.token_v3
        || &d.data[0..32] != ctx.usdc.as_ref()
        || &d.data[32..64] != victim.as_ref()
    {
        return Err(format!(
            "destination {destination} is not a USDC token account owned by the recorded victim {victim}"
        ).into());
    }
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let bond_rec = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool).0;
    let bond_vault = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND_VAULT], &ctx.pool).0;
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let ix = pool_ix(
        "slash_bond",
        window,
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new(root, false),
            AccountMeta::new(bond_vault, false),
            AccountMeta::new(bond_rec, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
        ],
    );
    let before = Balances {
        bond_vault: token_amount(ctx, &bond_vault).await?,
        bond_record: prepaid_amount(ctx, &bond_rec).await?,
        victim: token_amount(ctx, &destination).await?,
        escrow: token_amount(ctx, &vault_ta).await?,
    };
    println!(
        "[m11-sync] slash_bond window={window}: ruled_slash={ruled_slash} victim={victim} -> {destination}"
    );
    println!(
        "[m11-sync]   before: bond vault {} bond record {} victim {} escrow {}",
        before.bond_vault, before.bond_record, before.victim, before.escrow
    );
    println!(
        "[m11-sync]   fee payer {} — the only signing key (not the backend, not a multisig member); the pool program asks for no signer",
        ctx.deploy.pubkey()
    );
    let sig = send(ctx, &[ix], &ctx.deploy.pubkey(), &[&ctx.deploy]).await?;
    print_confirmed_signature(ctx, "slash_bond", &sig).await?;
    tx_readback(ctx, &sig).await?;
    let after = Balances {
        bond_vault: token_amount(ctx, &bond_vault).await?,
        bond_record: prepaid_amount(ctx, &bond_rec).await?,
        victim: token_amount(ctx, &destination).await?,
        escrow: token_amount(ctx, &vault_ta).await?,
    };
    println!(
        "[m11-sync]   after: bond vault {} bond record {} victim {} escrow {}",
        after.bond_vault, after.bond_record, after.victim, after.escrow
    );
    settle_verify(ctx, window).await?;
    let acct = ctx.rpc.get_account(&root).await?;
    if acct.data[88] != 3 {
        return Err(format!(
            "slash did not mark the root Slashed: status={}",
            acct.data[88]
        )
        .into());
    }
    // EXACTLY the ruled amount leaves the bond and reaches the victim; the
    // escrow (renter money) is not part of a slash and must not move.
    if before.bond_vault.checked_sub(ruled_slash) != Some(after.bond_vault)
        || before.bond_record.checked_sub(ruled_slash) != Some(after.bond_record)
    {
        return Err(format!(
            "bond delta broken: vault {} -> {} and record {} -> {} for a ruled {ruled_slash}",
            before.bond_vault, after.bond_vault, before.bond_record, after.bond_record
        )
        .into());
    }
    if before.victim.checked_add(ruled_slash) != Some(after.victim) {
        return Err(format!(
            "victim delta broken: {} + {ruled_slash} != {}",
            before.victim, after.victim
        )
        .into());
    }
    if before.escrow != after.escrow {
        return Err(format!(
            "the slash touched the escrow: {} -> {}",
            before.escrow, after.escrow
        )
        .into());
    }
    println!(
        "[m11-sync] slash deltas verified: bond vault -{ruled_slash}, bond record -{ruled_slash}, victim +{ruled_slash}, escrow unchanged, root Slashed(3)"
    );
    Ok(())
}

/// The four USDC balances a slash may and may not touch, read from the
/// chain before and after the crank so the deltas are compared, not assumed.
struct Balances {
    bond_vault: u64,
    bond_record: u64,
    victim: u64,
    escrow: u64,
}

/// SAT-2b step 7: the direct door. The loaded depositor key signs the claim
/// and pays both the transaction fee and the claim marker's rent.
async fn direct_claim(
    ctx: &Ctx,
    window: u64,
    amount: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root_address(ctx, window);
    let (posted_at, status) = root_posted_at_status(ctx, &root).await?;
    if status != 0 {
        return Err(format!("root {root} is not Active (status={status})").into());
    }
    let target = posted_at + CHALLENGE_WINDOW_SLOTS + WINDOW_MARGIN_SLOTS;
    loop {
        let now = ctx.rpc.get_slot().await?;
        if now >= target {
            println!("[m11-sync] challenge window passed: slot {now} >= target {target}");
            break;
        }
        println!("[m11-sync] waiting for slot {target} (now {now})");
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    }
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let destination = ata(&ctx.depositor.pubkey(), &ctx.usdc, &ctx.token_v3)?;
    let before_vault = token_amount(ctx, &vault_ta).await?;
    let before_destination = token_amount(ctx, &destination).await?;
    let ixs = single_leaf_claim_ixs(ctx, window, amount, &ctx.depositor.pubkey())?;
    let sig = send(ctx, &ixs, &ctx.depositor.pubkey(), &[&ctx.depositor]).await?;
    print_confirmed_signature(ctx, "direct claim", &sig).await?;
    settle_verify(ctx, window).await?;
    let after_vault = token_amount(ctx, &vault_ta).await?;
    let after_destination = token_amount(ctx, &destination).await?;
    if after_destination != before_destination + amount {
        return Err(format!(
            "depositor delta broken: {before_destination} + {amount} != {after_destination}"
        )
        .into());
    }
    if after_vault != before_vault - amount {
        return Err(
            format!("vault delta broken: {before_vault} - {amount} != {after_vault}").into(),
        );
    }
    let root_acct = ctx.rpc.get_account(&root).await?;
    let total = u64::from_le_bytes(root_acct.data[80..88].try_into().expect("total"));
    let paid_out = u64::from_le_bytes(root_acct.data[210..218].try_into().expect("paid_out"));
    if paid_out != total || total != amount {
        return Err(format!(
            "root accounting broken: paid_out={paid_out} total={total} amount={amount}"
        )
        .into());
    }
    println!(
        "[m11-sync] direct-claim deltas verified: depositor +{amount}, escrow -{amount}, root paid_out=total={amount}"
    );
    Ok(())
}

/// The exact CLI shape keeps each step explicit in shell history and logs.
fn required_arg(index: usize, usage: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::args()
        .nth(index)
        .ok_or_else(|| usage.to_string().into())
}

/// The PDA of one settlement window.
fn root_address(ctx: &Ctx, window: u64) -> Pubkey {
    Pubkey::find_program_address(&[SEED_ROOT, window.to_le_bytes().as_ref()], &ctx.pool).0
}

/// Read just the two fields every SAT-2b gate needs: `posted_at` and status.
async fn root_posted_at_status(
    ctx: &Ctx,
    root: &Pubkey,
) -> Result<(u64, u8), Box<dyn std::error::Error>> {
    let acct = ctx.rpc.get_account(root).await?;
    let posted_at = i64::from_le_bytes(acct.data[89..97].try_into().expect("posted_at"));
    Ok((u64::try_from(posted_at)?, acct.data[88]))
}

/// SHA-256 of one SAT-2b leaf; for a one-leaf tree this is also the root.
fn single_leaf_root(depositor: &Pubkey, amount: u64) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(40);
    preimage.extend_from_slice(depositor.as_ref());
    preimage.extend_from_slice(&amount.to_le_bytes());
    sha256(&preimage)
}

/// SHA-256, the only 32-byte commitment that can carry the 40-byte reviewer
/// evidence preimage inside the program's `[u8; 32]` field.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(data));
    out
}

/// The canonical ATA for this devnet USDC mint and the Token v3 program.
fn ata(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Result<Pubkey, Box<dyn std::error::Error>> {
    let ata_program = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")?;
    Ok(Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0)
}

/// Associated-token-program `CreateIdempotent`, paid by `payer`; no-ops when
/// the account already exists, so reruns are safe.
fn create_ata_idempotent_ix(
    payer: &Pubkey,
    ata: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
            .expect("ATA program id"),
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![1u8],
    }
}

/// One-leaf `claim` plus an idempotent ATA create. `authority` chooses the
/// door (backend assisted or depositor direct); `payer` pays the marker rent.
fn single_leaf_claim_ixs(
    ctx: &Ctx,
    window: u64,
    amount: u64,
    payer_and_authority: &Pubkey,
) -> Result<Vec<Instruction>, Box<dyn std::error::Error>> {
    let pool_pda = Pubkey::find_program_address(&[SEED_POOL], &ctx.pool).0;
    let root = root_address(ctx, window);
    let vault_ta = Pubkey::find_program_address(&[SEED_POOL, SEED_VAULT], &ctx.pool).0;
    let marker = Pubkey::find_program_address(
        &[SEED_CLAIM_MARKER, root.as_ref(), &0u32.to_le_bytes()],
        &ctx.pool,
    )
    .0;
    let destination = ata(&ctx.depositor.pubkey(), &ctx.usdc, &ctx.token_v3)?;
    let claim = pool_ix(
        "claim",
        (
            window,
            0u32,
            ctx.depositor.pubkey(),
            amount,
            Vec::<[u8; 32]>::new(),
        ),
        vec![
            AccountMeta::new_readonly(pool_pda, false),
            AccountMeta::new_readonly(*payer_and_authority, true),
            AccountMeta::new(root, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(vault_ta, false),
            AccountMeta::new(marker, false),
            AccountMeta::new(*payer_and_authority, true),
            AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
            AccountMeta::new_readonly(ctx.token_v3, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
        ],
    );
    Ok(vec![
        create_ata_idempotent_ix(
            payer_and_authority,
            &destination,
            &ctx.depositor.pubkey(),
            &ctx.usdc,
            &ctx.token_v3,
        ),
        claim,
    ])
}

/// Prints the signature, explorer link, and the slot read back from the RPC.
async fn print_confirmed_signature(
    ctx: &Ctx,
    label: &str,
    sig: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let slot = signature_slot(ctx, sig).await?;
    println!("[m11-sync] {label} sig={sig} slot={slot}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    Ok(slot)
}

/// Waits until the RPC can read the transaction back, then returns its slot.
async fn signature_slot(ctx: &Ctx, sig: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let signature = Signature::from_str(sig)?;
    for _ in 0..40 {
        if let Ok(tx) = ctx
            .rpc
            .get_transaction_with_config(&signature, transaction_config())
            .await
        {
            if let Some(meta) = tx.transaction.meta {
                if let Some(err) = meta.err {
                    return Err(format!("transaction {sig} confirmed with error {err:?}").into());
                }
            }
            return Ok(tx.slot);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(format!("signature {sig} did not become readable").into())
}

/// The expected failed transaction: it must exist in confirmed history and
/// its recorded error must be Anchor custom 6015 (`RootFrozen`). `label`
/// names the refusal in the output (`frozen-claim`, `slashed-claim`).
async fn failed_signature_slot(
    ctx: &Ctx,
    sig: &str,
    label: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let signature = Signature::from_str(sig)?;
    for _ in 0..40 {
        if let Ok(tx) = ctx
            .rpc
            .get_transaction_with_config(&signature, transaction_config())
            .await
        {
            let meta = tx
                .transaction
                .meta
                .ok_or("failed transaction read-back had no meta")?;
            let err = meta
                .err
                .ok_or("expected failed transaction succeeded — STOP")?;
            let text = format!("{err:?}");
            if !text.contains("6015") && !text.contains("0x177f") {
                return Err(format!("failed transaction carried unexpected error {text}").into());
            }
            println!("[m11-sync] {label} read-back meta.err: {text}");
            return Ok(tx.slot);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(format!("failed signature {sig} did not become readable").into())
}

/// Reads ONE landed transaction back from the chain — successful or failed —
/// and prints what the evidence needs, all of it from the RPC response:
/// slot, block time, status and fee; which keys actually signed; every USDC
/// token balance before → after; the CPI (inner) instructions, with any
/// pool-program arguments decoded; and every settlement event decoded from
/// its `Program data:` log line.
async fn tx_readback(ctx: &Ctx, sig_str: &str) -> Result<(), Box<dyn std::error::Error>> {
    let signature = Signature::from_str(sig_str)?;
    let tx = ctx
        .rpc
        .get_transaction_with_config(&signature, transaction_config())
        .await?;
    // Scalars come from the typed response; the lists (account keys,
    // signatures, token balances, CPI, logs) are read through the JSON shape
    // of the same response, whose field names are the RPC's public contract.
    let meta = tx
        .transaction
        .meta
        .as_ref()
        .ok_or("transaction read-back had no meta")?;
    let value = serde_json::to_value(&tx)?;
    // `EncodedTransactionWithStatusMeta` is serde-FLATTENED into this
    // response: `transaction`, `meta` and `version` sit at the top level.
    let tx_json = &value["transaction"];
    let meta_json = &value["meta"];
    let strings = |at: &serde_json::Value| -> Vec<String> {
        at.as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    println!("[m11-sync] read-back {sig_str}");
    println!(
        "[m11-sync]   slot={} block_time={} version={:?} status={:?} fee={} lamports compute_units_consumed={:?}",
        tx.slot,
        tx.block_time.map_or_else(|| "?".to_string(), |t| t.to_string()),
        tx.transaction.version,
        meta.status,
        meta.fee,
        meta.compute_units_consumed
    );
    let keys = strings(&tx_json["message"]["accountKeys"]);
    // Who signed: the RPC pads non-signers with empty signature strings.
    let signed: Vec<String> = strings(&tx_json["signatures"])
        .iter()
        .zip(keys.iter())
        .filter(|(sig, _)| !sig.is_empty())
        .map(|(_, key)| key.clone())
        .collect();
    println!(
        "[m11-sync]   fee payer (account 0): {} | signing keys: {}",
        keys.first().cloned().unwrap_or_default(),
        signed.join(", ")
    );
    // Token balances are the RPC's integer `amount` strings (raw micro-USDC)
    // — never the float `uiAmount`. Each row: (account index, owner, amount).
    let balances = |name: &str| -> Vec<(usize, String, Option<u64>)> {
        meta_json[name]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|b| {
                        (
                            b["accountIndex"].as_u64().unwrap_or(0) as usize,
                            b["owner"].as_str().unwrap_or("?").to_string(),
                            b["uiTokenAmount"]["amount"]
                                .as_str()
                                .and_then(|a| a.parse().ok()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let pre = balances("preTokenBalances");
    let post = balances("postTokenBalances");
    for (index, owner, before) in &pre {
        let after = post
            .iter()
            .find(|(i, _, _)| i == index)
            .and_then(|(_, _, amount)| *amount);
        let delta = match (before, after) {
            (Some(a), Some(z)) => (z as i128 - *a as i128).to_string(),
            _ => "?".to_string(),
        };
        println!(
            "[m11-sync]   token account[{index}] {} owner={owner} amount {:?} -> {:?} (delta {delta})",
            keys.get(*index).cloned().unwrap_or_default(),
            before,
            after
        );
    }
    for group in meta_json["innerInstructions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let group_index = group["index"].as_u64().unwrap_or(0);
        for inner in group["instructions"].as_array().into_iter().flatten() {
            let program_index = inner["programIdIndex"].as_u64().unwrap_or(0) as usize;
            let program = keys.get(program_index).cloned().unwrap_or_default();
            println!("[m11-sync]   CPI of instruction {group_index}: program {program}");
            let account_indexes: Vec<usize> = inner["accounts"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|i| i.as_u64())
                        .map(|i| i as usize)
                        .collect()
                })
                .unwrap_or_default();
            for (position, i) in account_indexes.iter().enumerate() {
                println!(
                    "[m11-sync]     account[{position}] {}",
                    keys.get(*i).cloned().unwrap_or_default()
                );
            }
            let data = inner["data"].as_str().unwrap_or("");
            if program == ctx.pool.to_string() {
                match base58_decode(data).and_then(|b| decode_pool_ix(&b)) {
                    Some(text) => println!("[m11-sync]     decoded: {text}"),
                    None => println!("[m11-sync]     data (base58, undecoded): {data}"),
                }
            }
        }
    }
    for line in strings(&meta_json["logMessages"]) {
        let text = line.trim_start();
        // Drop the runtime's per-instruction bookkeeping (invoke / consumed
        // / success); what remains is program logs, events and failure lines.
        let bookkeeping = text.starts_with("Program ")
            && (text.contains(" invoke [")
                || text.contains(" consumed ")
                || text.ends_with(" success"));
        if bookkeeping {
            continue;
        }
        if let Some(payload) = text.strip_prefix("Program data: ") {
            if let Some(event) = base64_decode(payload).and_then(|b| decode_settlement_event(&b)) {
                println!("[m11-sync]   event: {event}");
                continue;
            }
        }
        println!("[m11-sync]   log: {text}");
    }
    Ok(())
}

/// Decodes one settlement event payload (the bytes after `Program data: `)
/// by its 8-byte Anchor event discriminator. The field layouts are the ones
/// the program's own unit test pins
/// (`settlement_events_match_the_harness_decoder_layout`); an unknown
/// discriminator or a wrong length yields None.
fn decode_settlement_event(data: &[u8]) -> Option<String> {
    let disc: [u8; 8] = data.get(..8)?.try_into().ok()?;
    let body = &data[8..];
    // Bounds-checked readers for the two field shapes the events use.
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(body.get(at..at + 8)?.try_into().ok()?))
    };
    let key_at = |at: usize| -> Option<Pubkey> {
        Some(Pubkey::new_from_array(
            body.get(at..at + 32)?.try_into().ok()?,
        ))
    };
    let name = [
        "RootPosted",
        "RootChallenged",
        "ChallengeResolved",
        "BondSlashed",
        "ClaimPaid",
        "BondFunded",
    ]
    .into_iter()
    .find(|name| sighash("event", name) == disc)?;
    Some(match (name, body.len()) {
        ("RootPosted", 64) => {
            let merkle: [u8; 32] = body[8..40].try_into().ok()?;
            format!(
                "RootPosted window={} merkle_root=0x{} total_payout={} bond={} posted_at={}",
                u64_at(0)?,
                hex32(&merkle),
                u64_at(40)?,
                u64_at(48)?,
                i64::from_le_bytes(body[56..64].try_into().ok()?)
            )
        }
        ("RootChallenged", 72) => {
            let evidence: [u8; 32] = body[40..72].try_into().ok()?;
            format!(
                "RootChallenged window={} challenger={} evidence=0x{}",
                u64_at(0)?,
                key_at(8)?,
                hex32(&evidence)
            )
        }
        ("ChallengeResolved", 49) => format!(
            "ChallengeResolved window={} root_stands={} slash_amount={} victim={}",
            u64_at(0)?,
            body[8] != 0,
            u64_at(9)?,
            key_at(17)?
        ),
        ("BondSlashed", 56) => format!(
            "BondSlashed window={} amount={} victim={} bond_after={}",
            u64_at(0)?,
            u64_at(8)?,
            key_at(16)?,
            u64_at(48)?
        ),
        ("ClaimPaid", 60) => format!(
            "ClaimPaid window={} leaf_index={} depositor={} amount={} vault_balance_after={}",
            u64_at(0)?,
            u32::from_le_bytes(body[8..12].try_into().ok()?),
            key_at(12)?,
            u64_at(44)?,
            u64_at(52)?
        ),
        ("BondFunded", 48) => format!(
            "BondFunded funder={} amount={} bond_after={}",
            key_at(0)?,
            u64_at(32)?,
            u64_at(40)?
        ),
        _ => return None,
    })
}

/// Decodes the ARGUMENTS of a pool-program instruction whose discriminator
/// is one of the settlement handlers this run sends — the on-chain statement
/// of WHAT the multisig executed (its CPI repeats the instruction verbatim).
/// An unknown discriminator or a wrong length yields None.
fn decode_pool_ix(data: &[u8]) -> Option<String> {
    let disc: [u8; 8] = data.get(..8)?.try_into().ok()?;
    let body = &data[8..];
    let is = |name: &str| sighash("global", name) == disc;
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(body.get(at..at + 8)?.try_into().ok()?))
    };
    if is("resolve_challenge") {
        let window = u64_at(0)?;
        return match (body.get(8).copied()?, body.len()) {
            (0, 9) => Some(format!("resolve_challenge(window={window}, RootStands)")),
            (1, 49) => Some(format!(
                "resolve_challenge(window={window}, RootFalse {{ slash_amount: {}, victim: {} }})",
                u64_at(9)?,
                Pubkey::new_from_array(body[17..49].try_into().ok()?)
            )),
            _ => None,
        };
    }
    if is("slash_bond") && body.len() == 8 {
        return Some(format!("slash_bond(window={})", u64_at(0)?));
    }
    if is("challenge") && body.len() == 40 {
        return Some(format!(
            "challenge(window={}, evidence=0x{})",
            u64_at(0)?,
            hex32(&body[8..40].try_into().ok()?)
        ));
    }
    None
}

/// Minimal standard-alphabet base64 decoder (RFC 4648; `=` ends the
/// payload). Anchor emits events as `Program data: <base64>` log lines.
/// Written inline so this ops bin adds no dependency to the soak crate.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut quad = [0u8; 4];
    let mut filled = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        quad[filled] = value;
        filled += 1;
        if filled == 4 {
            out.push((quad[0] << 2) | (quad[1] >> 4));
            out.push((quad[1] << 4) | (quad[2] >> 2));
            out.push((quad[2] << 6) | quad[3]);
            filled = 0;
        }
    }
    // The trailing group holds 0, 1 or 2 payload bytes (never a lone char).
    match filled {
        0 => {}
        2 => out.push((quad[0] << 2) | (quad[1] >> 4)),
        3 => {
            out.push((quad[0] << 2) | (quad[1] >> 4));
            out.push((quad[1] << 4) | (quad[2] >> 2));
        }
        _ => return None,
    }
    Some(out)
}

/// Minimal base58 (Bitcoin alphabet) decoder — the RPC returns inner-
/// instruction `data` as a base58 string. Digits are accumulated as a
/// little-endian byte vector (multiply-by-58, carry), then reversed.
fn base58_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::new();
    let mut leading_zeros = 0usize;
    for ch in text.chars() {
        let value = ALPHABET.find(ch)? as u32;
        if value == 0 && digits.is_empty() {
            // A leading '1' encodes a leading zero byte.
            leading_zeros += 1;
            continue;
        }
        let mut carry = value;
        for digit in digits.iter_mut() {
            carry += (*digit as u32) * 58;
            *digit = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            digits.push(carry as u8);
            carry >>= 8;
        }
    }
    let mut out = vec![0u8; leading_zeros];
    out.extend(digits.into_iter().rev());
    Some(out)
}

/// All transaction read-backs use confirmed commitment and explicit v0
/// support, which is also sufficient for the legacy transactions sent here.
fn transaction_config() -> RpcTransactionConfig {
    RpcTransactionConfig {
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
        ..RpcTransactionConfig::default()
    }
}

/// True when either the RPC error or program logs identify `RootFrozen`
/// (Anchor code 6015 == 0x177f).
fn root_frozen_error(error_text: &str, logs: &[String]) -> bool {
    error_text.contains("RootFrozen")
        || error_text.contains("6015")
        || error_text.contains("0x177f")
        || logs.iter().any(|line| {
            line.contains("RootFrozen") || line.contains("6015") || line.contains("0x177f")
        })
}

/// Verifies the backend's USDC ATA derivation against the LIVE ATA program
/// (read-only simulation of create-idempotent; never lands on-chain).
///
/// Why this exists (live finding 2026-11-28): the ATA program was REDEPLOYED
/// — the canonical address is ...JA8knL on both mainnet and devnet, while
/// the legacy ...JA8knN deployment is gone. The M11 glue + this soak crate
/// both carried the stale ...knN id, which (a) derives a NON-ATA address and
/// (b) calls a program that no longer exists. With the corrected id, the
/// solana-sdk and anchor-spl derivations agree on the faucet-funded account
/// 7x8yfjE… — i.e. the Circle faucet had created the canonical ATA all along
/// (the earlier "derivations disagree" scare was purely the stale id).
async fn ata_probe(ctx: &Ctx) -> Result<(), Box<dyn std::error::Error>> {
    let ata_program = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")?;
    let owner = ctx.backend.pubkey();
    let candidates = [
        (
            "solana-sdk find_program_address",
            Pubkey::find_program_address(
                &[owner.as_ref(), ctx.token_v3.as_ref(), ctx.usdc.as_ref()],
                &ata_program,
            )
            .0,
        ),
        (
            "anchor-spl get_associated_token_address",
            anchor_spl::associated_token::get_associated_token_address(&owner, &ctx.usdc),
        ),
    ];
    for (label, ata) in candidates {
        let ix = Instruction {
            program_id: ata_program,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new_readonly(ctx.usdc, false),
                AccountMeta::new_readonly(solana_program::system_program::ID, false),
                AccountMeta::new_readonly(ctx.token_v3, false),
            ],
            data: vec![1u8],
        };
        let blockhash = ctx.rpc.get_latest_blockhash().await?;
        let mut tx = Transaction::new_unsigned(Message::new(&[ix], Some(&owner)));
        tx.sign(&[&ctx.backend], blockhash);
        let res = ctx.rpc.simulate_transaction(&tx).await?;
        let ok = res.value.err.is_none();
        println!(
            "[m11-sync] probe {label} -> {ata} : {}",
            if ok { "ACCEPTED" } else { "REJECTED" }
        );
        if let Some(err) = res.value.err {
            println!("[m11-sync]   err: {err:?}");
        }
        for line in res.value.logs.unwrap_or_default() {
            println!("[m11-sync]   log: {line}");
        }
    }
    Ok(())
}

/// Full 2-of-2 vault-transaction lifecycle at the LIVE next index (lesson
/// 3). Prints the create, both approval, and the execute signatures.
/// Returns the transaction index used.
async fn propose_approve_execute(
    ctx: &Ctx,
    ms_state: &squads_state::Multisig,
    inner: Instruction,
    label: &str,
) -> Result<u32, Box<dyn std::error::Error>> {
    let tx_index = ms_state.transaction_index + 1;
    println!(
        "[m11-sync] proposing at index {tx_index} (live transaction_index was {})",
        ms_state.transaction_index
    );
    let (tx_pda, _) = pda::get_transaction_pda(&ctx.ms, tx_index, None);
    let (proposal, _) = pda::get_proposal_pda(&ctx.ms, tx_index, None);
    let message = squads_multisig_program::TransactionMessage::try_compile(
        &ctx.vault,
        std::slice::from_ref(&inner),
        &[],
    )?;
    let create = squads::vault_transaction_create(
        squads::VaultTransactionCreateAccounts {
            multisig: ctx.ms,
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
            multisig: ctx.ms,
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
    let sig = send(
        ctx,
        &[create, prop],
        &ctx.deploy.pubkey(),
        &[&ctx.deploy, &ctx.founder],
    )
    .await?;
    let slot = signature_slot(ctx, &sig).await?;
    println!("[m11-sync]   index {tx_index} create+proposal sig={sig} slot={slot}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    // BOTH of today's members approve — founder AND teammate-rotated.
    for member in [&ctx.founder, &ctx.teammate_rotated] {
        let approve = squads::proposal_approve(
            squads::ProposalVoteAccounts {
                multisig: ctx.ms,
                proposal,
                member: member.pubkey(),
            },
            squads::ProposalVoteArgs { memo: None },
            None,
        );
        let sig = send(
            ctx,
            &[approve],
            &ctx.deploy.pubkey(),
            &[&ctx.deploy, member],
        )
        .await?;
        let slot = signature_slot(ctx, &sig).await?;
        println!(
            "[m11-sync]   index {tx_index} APPROVED by {} sig={sig} slot={slot}",
            member.pubkey()
        );
        println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    }
    let exec = squads::vault_transaction_execute(
        squads::VaultTransactionExecuteAccounts {
            multisig: ctx.ms,
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
    let sig = send(
        ctx,
        &[exec],
        &ctx.deploy.pubkey(),
        &[&ctx.deploy, &ctx.founder],
    )
    .await?;
    let slot = signature_slot(ctx, &sig).await?;
    println!("[m11-sync]   index {tx_index} EXECUTED sig={sig} slot={slot}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
    // Re-read the execution from the chain: the signers, the CPI the
    // multisig made, and the event the pool program emitted. Best-effort —
    // the execution has landed by now; a read hiccup must not read as a
    // failed proposal.
    if let Err(e) = tx_readback(ctx, &sig).await {
        println!("[m11-sync]   (execute read-back skipped: {e})");
    }
    Ok(u32::try_from(tx_index).expect("tx index fits u32"))
}

// ---------------------------------------------------------------------------
// helpers (same shapes as the soak client; kept self-contained so the soak
// main.rs is untouched)
// ---------------------------------------------------------------------------

/// Loads a 64-byte JSON keypair from M11_KEYS_DIR (never logged).
fn load_key(name: &str) -> Result<Keypair, Box<dyn std::error::Error>> {
    let dir = std::env::var("M11_KEYS_DIR").unwrap_or_else(|_| "../../target/m8-keys".into());
    let raw = std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{name}.json")))?;
    let bytes: Vec<u8> = serde_json::from_str(&raw)?;
    if bytes.len() != 64 {
        return Err("keypair file must hold 64 bytes".into());
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Ok(Keypair::from_bytes(&arr)?)
}

/// Anchor instruction discriminator: sha256("global:<name>")[..8].
fn sighash(namespace: &str, name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("{namespace}:{name}").as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&h[..8]);
    out
}

/// A pool-program instruction: discriminator + borsh args, no anchor client.
fn pool_ix<A: AnchorSerialize>(name: &str, args: A, accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = sighash("global", name).to_vec();
    data.extend(args.try_to_vec().expect("borsh args"));
    Instruction {
        program_id: Pubkey::new_from_array(POOL_PROGRAM_BYTES),
        accounts,
        data,
    }
}

/// Signs and sends a transaction; `payer` pays fees (deploy key), so member
/// keys need no lamports. Returns the confirmed signature string.
async fn send(
    ctx: &Ctx,
    ixs: &[Instruction],
    payer: &Pubkey,
    signers: &[&Keypair],
) -> Result<String, Box<dyn std::error::Error>> {
    let blockhash = ctx.rpc.get_latest_blockhash().await?;
    let mut tx = Transaction::new_unsigned(Message::new(ixs, Some(payer)));
    tx.sign(signers, blockhash);
    // Never broadcast a success-path transaction that the RPC has not first
    // simulated successfully. The intentional RootFrozen refusal uses its
    // own explicit skip-preflight path in claim_frozen.
    let sim = ctx.rpc.simulate_transaction(&tx).await?;
    if let Some(err) = sim.value.err {
        for line in sim.value.logs.unwrap_or_default() {
            eprintln!("[m11-sync] simulation log: {line}");
        }
        return Err(format!("transaction simulation refused broadcast: {err:?}").into());
    }
    ctx.rpc
        .send_and_confirm_transaction(&tx)
        .await
        .map(|s| s.to_string())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Raw u64 amount of a Token v3 account (mint 32 + owner 32 + amount u64 at
/// offset 64).
async fn token_amount(ctx: &Ctx, acct: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let ai = ctx.rpc.get_account(acct).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&ai.data[64..72]);
    Ok(u64::from_le_bytes(b))
}

/// A renter prepaid record's `amount` (8-byte discriminator, then u64 LE).
async fn prepaid_amount(ctx: &Ctx, pda: &Pubkey) -> Result<u64, Box<dyn std::error::Error>> {
    let acct = ctx.rpc.get_account(pda).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&acct.data[8..16]);
    Ok(u64::from_le_bytes(b))
}

/// base58 string of a 32-byte id (for the startup self-checks) — the SDK's
/// own codec, via Pubkey's Display.
fn bs58_encode(bytes: &[u8; 32]) -> String {
    Pubkey::new_from_array(*bytes).to_string()
}

/// Lowercase hex of a 32-byte value (the Merkle root evidence).
fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
