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
//!   verify                   re-read authority / pool / slot after the run.
//!
//! Env: M11_KEYS_DIR (default ../../target/m8-keys), M11_RPC (default
//! https://api.devnet.solana.com), M11_RENTER_WALLET (default the
//! provisioned custodial renter USDC account from the M8 soak).

use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::bpf_loader_upgradeable;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
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
    };
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
    println!("[m11-sync] prepay_renter {amount} micro-USDC sig={sig}");
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
    let paid_out = u64::from_le_bytes(r.data[210..218].try_into().expect("paid_out"));
    println!(
        "[m11-sync]   root.pool={r_pool} window={r_window} status={status} posted_at={posted_at}"
    );
    println!("[m11-sync]   merkle_root=0x{}", hex32(&merkle));
    println!("[m11-sync]   total_payout={total} paid_out={paid_out} bond_snapshot={bond}");
    let (marker, _) = Pubkey::find_program_address(
        &[SEED_CLAIM_MARKER, root.as_ref(), &0u32.to_le_bytes()],
        &ctx.pool,
    );
    println!("[m11-sync]   claim marker (leaf 0) {marker}");
    let m = ctx.rpc.get_account(&marker).await?;
    let m_depositor = Pubkey::new_from_array(m.data[44..76].try_into().expect("depositor"));
    let m_amount = u64::from_le_bytes(m.data[76..84].try_into().expect("amount"));
    println!("[m11-sync]   marker.depositor={m_depositor} amount={m_amount}");
    let (bond_rec, _) = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND], &ctx.pool);
    let (bond_vault, _) = Pubkey::find_program_address(&[SEED_POOL, SEED_BOND_VAULT], &ctx.pool);
    let bond_amt = prepaid_amount(ctx, &bond_rec).await?; // same layout: disc + u64
    let bond_vault_amt = token_amount(ctx, &bond_vault).await?;
    let vault_amt = token_amount(ctx, &vault_ta).await?;
    println!("[m11-sync]   bond record {bond_rec} amount={bond_amt}; bond vault {bond_vault} balance={bond_vault_amt}");
    println!("[m11-sync]   escrow vault {vault_ta} balance={vault_amt}");
    let ata_program = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")?;
    let dep_ata = Pubkey::find_program_address(
        &[
            m_depositor.as_ref(),
            ctx.token_v3.as_ref(),
            ctx.usdc.as_ref(),
        ],
        &ata_program,
    )
    .0;
    let dep_amt = token_amount(ctx, &dep_ata).await?;
    println!("[m11-sync]   depositor USDC ATA {dep_ata} balance={dep_amt}");
    Ok(())
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
    println!("[m11-sync]   index {tx_index} create+proposal sig={sig}");
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
        println!(
            "[m11-sync]   index {tx_index} APPROVED by {} sig={sig}",
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
    println!("[m11-sync]   index {tx_index} EXECUTED sig={sig}");
    println!("[m11-sync]     https://explorer.solana.com/tx/{sig}?cluster=devnet");
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
