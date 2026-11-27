//! M11 DEVNET SETTLEMENT GLUE (standalone) — the demo path that turns the
//! backend's `POST /settle` output into on-chain settlement on devnet.
//!
//! Pipeline (three phases, each printed with its signature + explorer link):
//!   1. `fund_bond`  — the operator bonds USDC from its own wallet (a bonded
//!      operator backs its roots; the root cannot be posted without a bond).
//!   2. `post_root`  — one Merkle root per settlement window, built from the
//!      payouts JSON with the SAME recipe the program verifies (merkle.rs).
//!   3. `claim`      — after the challenge window has passed (invariant 11),
//!      one assisted-door claim per leaf, paying each depositor's USDC ATA.
//!
//! It is deliberately a THIN integrator: every instruction is hand-built
//! (Anchor discriminator + hand-serialized borsh args + the program's seed
//! derivation) exactly like the pool test harness builds them — no anchor
//! client, and no dependency on the pool crate (which would drag the
//! anchor 1.2 cohort into this standalone lock).
//!
//! # Inputs (env)
//!   M11_BACKEND_KEYPAIR  path to the backend keypair JSON (64-byte array) —
//!                        the operator's hot key: signs fund_bond + post_root,
//!                        opens the assisted door of claim, pays ATA rent.
//!   M11_PAYOUTS          path to the payouts JSON: `[{"depositor": "<base58
//!                        pubkey>", "amount": <micro-USDC u64>}, ...]`.
//!   M11_RPC              RPC URL (default https://api.devnet.solana.com).
//!   M11_WINDOW           settlement window u64 (default: current unix time).
//!   M11_BOND_AMOUNT      bond in micro-USDC (default: the payouts' total).
//!
//! Devnet USDC only: the USDC mint is pinned to the devnet faucet mint below
//! and self-checked at startup.

mod merkle;

use merkle::{Leaf, Tree};
use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;

// ---------------------------------------------------------------------------
// Program ids and protocol constants — decoded from the well-known base58
// addresses OFFLINE, self-checked against the strings at startup (a bad byte
// fails loudly, never silently).
// ---------------------------------------------------------------------------

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
const ATA_PROGRAM_BYTES: [u8; 32] = [
    0x8c, 0x97, 0x25, 0x8f, 0x4e, 0x24, 0x89, 0xf1, 0xbb, 0x3d, 0x10, 0x29, 0x14, 0x8e, 0x0d, 0x83,
    0x0b, 0x5a, 0x13, 0x99, 0xda, 0xff, 0x10, 0x84, 0x04, 0x8e, 0x7b, 0xd8, 0xdb, 0xe9, 0xf8, 0x5b,
];
const SYSTEM_PROGRAM_BYTES: [u8; 32] = [0u8; 32];
const CLOCK_SYSVAR_BYTES: [u8; 32] = [
    0x06, 0xa7, 0xd5, 0x17, 0x18, 0xc7, 0x74, 0xc9, 0x28, 0x56, 0x63, 0x98, 0x69, 0x1d, 0x5e, 0xb6,
    0x8b, 0x5e, 0xb8, 0xa3, 0x9b, 0x4b, 0x6d, 0x5c, 0x73, 0x55, 0x5b, 0x21, 0x00, 0x00, 0x00, 0x00,
];

const POOL_PROGRAM_STR: &str = "9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX";
const USDC_DEVNET_STR: &str = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const TOKEN_V3_STR: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ATA_PROGRAM_STR: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knN";
const SYSTEM_PROGRAM_STR: &str = "11111111111111111111111111111111";
const CLOCK_SYSVAR_STR: &str = "SysvarC1ock11111111111111111111111111111111";

/// Seed prefixes — mirror `programs/pool/src/{state,settlement}.rs` exactly.
const SEED_POOL: &[u8] = b"pool";
const SEED_VAULT: &[u8] = b"vault";
const SEED_BOND: &[u8] = b"bond";
const SEED_BOND_VAULT: &[u8] = b"bond_vault";
const SEED_ROOT: &[u8] = b"settlement_root";
const SEED_CLAIM_MARKER: &[u8] = b"claim_marker";

/// The challenge window the program enforces (invariant 11): 100 slots from
/// the slot the root was posted in. Mirrored from `CHALLENGE_WINDOW_SLOTS`
/// in `programs/pool/src/settlement.rs`.
const CHALLENGE_WINDOW_SLOTS: u64 = 100;
/// Extra slots of margin on top of the window, so a claim is never submitted
/// while the clock is even one slot short of `posted_at + WINDOW`.
const WINDOW_MARGIN_SLOTS: u64 = 5;

/// One payout from the JSON: the depositor's pubkey (base58) and the USDC
/// amount in integer micro-units (1 minor unit == 1 micro-USDC, so no
/// conversion ever touches a float).
#[derive(serde::Deserialize)]
struct Payout {
    depositor: String,
    amount: u64,
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        if let Err(e) = run().await {
            eprintln!("[m11] FAILED: {e:?}");
            std::process::exit(1);
        }
    });
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Self-check the hardcoded ids against their canonical strings.
    for (bytes, s) in [
        (POOL_PROGRAM_BYTES, POOL_PROGRAM_STR),
        (USDC_DEVNET_BYTES, USDC_DEVNET_STR),
        (TOKEN_V3_BYTES, TOKEN_V3_STR),
        (ATA_PROGRAM_BYTES, ATA_PROGRAM_STR),
        (SYSTEM_PROGRAM_BYTES, SYSTEM_PROGRAM_STR),
        (CLOCK_SYSVAR_BYTES, CLOCK_SYSVAR_STR),
    ] {
        assert_eq!(bs58_encode(&bytes), s, "hardcoded program id mismatch");
    }

    let rpc_url =
        std::env::var("M11_RPC").unwrap_or_else(|_| "https://api.devnet.solana.com".into());
    let payouts_path = std::env::var("M11_PAYOUTS")
        .map_err(|_| "M11_PAYOUTS (path to the payouts JSON) is required")?;
    let backend_path = std::env::var("M11_BACKEND_KEYPAIR")
        .map_err(|_| "M11_BACKEND_KEYPAIR (path to the backend keypair JSON) is required")?;
    let window: u64 = std::env::var("M11_WINDOW")
        .map(|s| s.parse().expect("M11_WINDOW must be a u64"))
        .unwrap_or_else(|_| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_secs()
        });

    let backend = load_keypair(&backend_path)?;
    let payouts: Vec<Payout> = serde_json::from_str(&std::fs::read_to_string(&payouts_path)?)?;
    if payouts.is_empty() {
        return Err("payouts JSON is empty — nothing to settle".into());
    }
    let leaves: Vec<Leaf> = payouts
        .iter()
        .map(|p| {
            Ok(Leaf {
                depositor: bs58_decode(&p.depositor)?,
                amount: p.amount,
            })
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    let tree = Tree::new(&leaves);
    let total = tree.total();
    let bond_amount: u64 = std::env::var("M11_BOND_AMOUNT")
        .map(|s| s.parse().expect("M11_BOND_AMOUNT must be a u64"))
        .unwrap_or(total);

    let rpc = RpcClient::new(rpc_url.clone());
    let program = Pubkey::new_from_array(POOL_PROGRAM_BYTES);
    let usdc_mint = Pubkey::new_from_array(USDC_DEVNET_BYTES);
    let backend_ata = ata(&backend.pubkey(), &usdc_mint);

    println!("[m11] RPC: {rpc_url}");
    println!("[m11] program: {POOL_PROGRAM_STR}");
    println!("[m11] backend: {}", backend.pubkey());
    println!(
        "[m11] window: {window} · {} leaves summing to {} micro-USDC",
        leaves.len(),
        total
    );
    println!("[m11] tree root: 0x{}", hex(&tree.root()));

    // ---- Phase 0: the operator's bond money must exist (the GLUE does not
    // mint USDC on devnet — the operator funds its wallet via the faucet).
    let backend_usdc = token_balance(&rpc, &backend_ata).await.unwrap_or(0);
    if backend_usdc < bond_amount {
        return Err(format!(
            "backend USDC ATA {backend_ata} holds {backend_usdc} < bond {bond_amount} micro-USDC — fund it first (devnet faucet)"
        )
        .into());
    }

    // ---- Phase 1: fund_bond — the operator bonds its own USDC.
    let (pool, _) = find_pda(&[SEED_POOL], &program);
    let (bond, _) = find_pda(&[SEED_POOL, SEED_BOND], &program);
    let (bond_vault, _) = find_pda(&[SEED_POOL, SEED_BOND_VAULT], &program);
    let sig = send(
        &rpc,
        &[fund_bond_ix(
            &program, &pool, &backend, &backend_ata, &usdc_mint, &bond_vault, &bond, bond_amount,
        )],
        &backend.pubkey(),
        &[&backend],
    )
    .await?;
    report("1/3 fund_bond", &sig, &format!("bonded {bond_amount} micro-USDC"));

    // ---- Phase 2: post_root — one Merkle root for this window.
    let (vault, _) = find_pda(&[SEED_POOL, SEED_VAULT], &program);
    let (root, _) = find_pda(&[SEED_ROOT, &window.to_le_bytes()], &program);
    let sig = send(
        &rpc,
        &[post_root_ix(
            &program,
            &pool,
            &backend,
            &root,
            &vault,
            &bond,
            window,
            tree.root(),
            total,
        )],
        &backend.pubkey(),
        &[&backend],
    )
    .await?;
    report("2/3 post_root", &sig, &format!("posted root for window {window}"));

    // ---- Wait out the challenge window (invariant 11): no payout leaves the
    // vault until `posted_at + CHALLENGE_WINDOW_SLOTS`. posted_at is read
    // back from the root account (i64 at offset 89 of its data).
    let posted_at = read_root_posted_at(&rpc, &root).await?;
    let target = posted_at + CHALLENGE_WINDOW_SLOTS + WINDOW_MARGIN_SLOTS;
    println!(
        "[m11] root posted at slot {posted_at} — waiting for slot {target} (challenge window + margin)…"
    );
    loop {
        let now = rpc.get_slot().await?;
        if now >= target {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    }
    println!("[m11] challenge window passed — claiming {} leaves…", leaves.len());

    // ---- Phase 3: claim — one assisted-door claim per leaf, to the
    // depositor's USDC ATA (created idempotently if it does not exist).
    let (claim_marker_seed_root, _) = (root, 0u8);
    for index in 0..leaves.len() {
        let leaf = tree.leaf(index);
        let depositor = Pubkey::new_from_array(leaf.depositor);
        let destination = ata(&depositor, &usdc_mint);
        let (marker, _) = find_pda(
            &[SEED_CLAIM_MARKER, claim_marker_seed_root.as_ref(), &(index as u32).to_le_bytes()],
            &program,
        );
        let mut ixs = vec![create_ata_idempotent_ix(
            &backend.pubkey(),
            &destination,
            &depositor,
            &usdc_mint,
        )];
        ixs.push(claim_ix(
            &program,
            &pool,
            &backend.pubkey(), // the assisted door: the backend opens it
            &root,
            &destination,
            &vault,
            &marker,
            &backend.pubkey(), // the marker's rent payer
            window,
            index as u32,
            &depositor,
            leaf.amount,
            &tree.proof(index),
        ));
        let sig = send(&rpc, &ixs, &backend.pubkey(), &[&backend]).await?;
        report(
            &format!("3/3 claim leaf {index}"),
            &sig,
            &format!("paid {} micro-USDC to {depositor}", leaf.amount),
        );
    }

    println!("[m11] ALL {} PAYOUTS SETTLED — archive this output as the run evidence.", leaves.len());
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Prints one settled step with its explorer link (the demo evidence line).
fn report(step: &str, sig: &str, detail: &str) {
    println!("[m11] {step} ✓ {detail}");
    println!("[m11]   tx: https://explorer.solana.com/tx/{sig}?cluster=devnet");
}

/// Loads a keypair from a JSON file holding 64 bytes (secret + public) as a
/// JSON array — the solana-keygen on-disk format. NEVER logged.
fn load_keypair(path: &str) -> Result<Keypair, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    let bytes: Vec<u8> = serde_json::from_str(&raw)?;
    if bytes.len() != 64 {
        return Err("keypair file must hold 64 bytes".into());
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Ok(Keypair::from_bytes(&arr)?)
}

/// Anchor instruction discriminator: sha256("global:<snake_name>")[..8]
/// (anchor-syn `SIGHASH_GLOBAL_NAMESPACE` — the #[program] module name is
/// not part of the preimage).
fn disc(name: &str) -> [u8; 8] {
    let mut out = [0u8; 8];
    out.copy_from_slice(&Sha256::digest(format!("global:{name}").as_bytes())[..8]);
    out
}

/// A pool-program instruction: discriminator + hand-serialized borsh args.
/// (Hand-rolled so this crate needs no borsh dependency; the layouts are the
/// same fixed-width LE fields the harness and the program's wire pin use.)
fn pool_ix(program: &Pubkey, name: &str, mut args: Vec<u8>, accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = disc(name).to_vec();
    data.append(&mut args);
    Instruction {
        program_id: *program,
        accounts,
        data,
    }
}

/// `find_program_address` against the pool program id.
fn find_pda(seeds: &[&[u8]], program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(seeds, program)
}

/// The associated token account of `owner` on `mint` (ATA derivation).
fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    let ata_program = Pubkey::new_from_array(ATA_PROGRAM_BYTES);
    Pubkey::find_program_address(
        &[owner.as_ref(), TOKEN_V3_BYTES.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0
}

/// `fund_bond(amount)` — USDC from the operator's own ATA into the bond
/// vault; the backend signs and pays rent. Accounts mirror `FundBond`.
#[allow(clippy::too_many_arguments)]
fn fund_bond_ix(
    program: &Pubkey,
    pool: &Pubkey,
    backend: &Keypair,
    source: &Pubkey,
    usdc_mint: &Pubkey,
    bond_vault: &Pubkey,
    bond: &Pubkey,
    amount: u64,
) -> Instruction {
    pool_ix(
        program,
        "fund_bond",
        amount.to_le_bytes().to_vec(),
        vec![
            AccountMeta::new_readonly(*pool, false),
            AccountMeta::new(backend.pubkey(), true),
            AccountMeta::new(*source, false),
            AccountMeta::new_readonly(*usdc_mint, false),
            AccountMeta::new(*bond_vault, false),
            AccountMeta::new(*bond, false),
            AccountMeta::new_readonly(Pubkey::new_from_array(TOKEN_V3_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(SYSTEM_PROGRAM_BYTES), false),
        ],
    )
}

/// `post_root(window, merkle_root, total_payout)` — the vault is READ-ONLY
/// (posting measures it, never moves it). Accounts mirror `PostRoot`.
#[allow(clippy::too_many_arguments)]
fn post_root_ix(
    program: &Pubkey,
    pool: &Pubkey,
    backend: &Keypair,
    root: &Pubkey,
    vault: &Pubkey,
    bond: &Pubkey,
    window: u64,
    merkle_root: [u8; 32],
    total_payout: u64,
) -> Instruction {
    let mut args = Vec::with_capacity(48);
    args.extend_from_slice(&window.to_le_bytes());
    args.extend_from_slice(&merkle_root);
    args.extend_from_slice(&total_payout.to_le_bytes());
    pool_ix(
        program,
        "post_root",
        args,
        vec![
            AccountMeta::new_readonly(*pool, false),
            AccountMeta::new(backend.pubkey(), true),
            AccountMeta::new(*root, false),
            AccountMeta::new_readonly(*vault, false),
            AccountMeta::new_readonly(*bond, false),
            AccountMeta::new_readonly(Pubkey::new_from_array(TOKEN_V3_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(CLOCK_SYSVAR_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(SYSTEM_PROGRAM_BYTES), false),
        ],
    )
}

/// `claim(window, leaf_index, depositor, amount, proof)` — the assisted door
/// (the backend opens it); the payout lands in the leaf's depositor-owned
/// destination. Accounts mirror `Claim` (settlement_root is writable — the
/// handler counts the payout on it, invariant 12).
#[allow(clippy::too_many_arguments)]
fn claim_ix(
    program: &Pubkey,
    pool: &Pubkey,
    authority: &Pubkey,
    root: &Pubkey,
    destination: &Pubkey,
    vault: &Pubkey,
    marker: &Pubkey,
    payer: &Pubkey,
    window: u64,
    leaf_index: u32,
    depositor: &Pubkey,
    amount: u64,
    proof: &[[u8; 32]],
) -> Instruction {
    let mut args = Vec::with_capacity(56 + 32 * proof.len());
    args.extend_from_slice(&window.to_le_bytes());
    args.extend_from_slice(&leaf_index.to_le_bytes());
    args.extend_from_slice(depositor.as_ref());
    args.extend_from_slice(&amount.to_le_bytes());
    args.extend_from_slice(&(proof.len() as u32).to_le_bytes());
    for sibling in proof {
        args.extend_from_slice(sibling);
    }
    pool_ix(
        program,
        "claim",
        args,
        vec![
            AccountMeta::new_readonly(*pool, false),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(*root, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new(*marker, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(Pubkey::new_from_array(CLOCK_SYSVAR_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(TOKEN_V3_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(SYSTEM_PROGRAM_BYTES), false),
        ],
    )
}

/// Associated-token-program `CreateIdempotent`: creates the depositor's USDC
/// ATA if it does not exist, no-ops if it does — so the GLUE is safe to
/// re-run. Accounts per the ATA program's create layout.
fn create_ata_idempotent_ix(payer: &Pubkey, ata: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
    Instruction {
        program_id: Pubkey::new_from_array(ATA_PROGRAM_BYTES),
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(Pubkey::new_from_array(SYSTEM_PROGRAM_BYTES), false),
            AccountMeta::new_readonly(Pubkey::new_from_array(TOKEN_V3_BYTES), false),
        ],
        data: vec![1u8], // 1 = CreateIdempotent
    }
}

/// Signs and sends a transaction (backend pays fees + rent), confirming it.
async fn send(
    rpc: &RpcClient,
    ixs: &[Instruction],
    payer: &Pubkey,
    signers: &[&Keypair],
) -> Result<String, Box<dyn std::error::Error>> {
    let blockhash = rpc.get_latest_blockhash().await?;
    let mut tx = Transaction::new_unsigned(Message::new(ixs, Some(payer)));
    tx.sign(signers, blockhash);
    rpc.send_and_confirm_transaction(&tx)
        .await
        .map(|s| s.to_string())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// The raw u64 amount of a Token v3 account (mint 32 + owner 32, then the
/// amount u64 at offset 64).
async fn token_balance(
    rpc: &RpcClient,
    account: &Pubkey,
) -> Result<u64, Box<dyn std::error::Error>> {
    let acct = rpc.get_account(account).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&acct.data[64..72]);
    Ok(u64::from_le_bytes(b))
}

/// Reads `posted_at` (i64) back from a SettlementRoot account. Layout after
/// the 8-byte discriminator: pool(32) window(8) merkle_root(32)
/// total_payout(8) status(1) posted_at(8) — so posted_at sits at 8+32+8+32+8+1
/// = offset 89.
async fn read_root_posted_at(
    rpc: &RpcClient,
    root: &Pubkey,
) -> Result<u64, Box<dyn std::error::Error>> {
    let acct = rpc.get_account(root).await?;
    let mut b = [0u8; 8];
    b.copy_from_slice(&acct.data[89..97]);
    let posted_at = i64::from_le_bytes(b);
    u64::try_from(posted_at).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Lowercase hex of a 32-byte value, for printing the root.
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// base58 decode via the SDK (the same codec the keypair/pubkey strings use).
fn bs58_decode(s: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let bytes = bs58::decode(s).into_vec()?;
    if bytes.len() != 32 {
        return Err("pubkey must decode to 32 bytes".into());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// base58 encode of a 32-byte value (for the startup self-check).
fn bs58_encode(bytes: &[u8; 32]) -> String {
    bs58::encode(bytes).into_string()
}
