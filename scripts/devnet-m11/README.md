# devnet-m11 — M11 settlement GLUE (devnet, USDC only)

A thin ops crate that turns the backend's `POST /settle` output into
on-chain settlement on Solana devnet. It is modelled on
`scripts/devnet-soak/` (standalone workspace, its own `Cargo.lock`, built
on its own — the main workspace's unified graph cannot host a Solana 1.x
client).

## What it does, in three phases

1. `fund_bond` — the operator bonds USDC from its own wallet (a bonded
   operator backs its roots; `post_root` needs an existing bond).
2. `post_root` — one Merkle root for the settlement window, built from the
   payouts JSON with the EXACT recipe the program verifies on-chain
   (`src/merkle.rs`, self-checked against the harness's `sha2`).
3. `claim` — after the challenge window has passed (invariant 11, 100
   slots), one assisted-door claim per leaf, paying each depositor's USDC
   ATA (created idempotently if missing).

Every step is printed with its signature and explorer link.

## Inputs (env)

| Var | Meaning |
|---|---|
| `M11_BACKEND_KEYPAIR` | path to the backend keypair JSON (64-byte array) — signs `fund_bond`/`post_root`, opens the assisted door of `claim`, pays ATA rent |
| `M11_PAYOUTS` | path to the payouts JSON: `[{"depositor": "<base58 pubkey>", "amount": <micro-USDC u64>}, ...]` |
| `M11_RPC` | RPC URL (default `https://api.devnet.solana.com`) |
| `M11_WINDOW` | settlement window u64 (default: current unix time) |
| `M11_BOND_AMOUNT` | bond in micro-USDC (default: the payouts' total) |

The backend wallet's USDC ATA must already hold at least the bond amount —
the GLUE does not mint USDC (fund it via the devnet faucet).

## Run

```sh
cargo build --offline          # the dependency tree is large; first build is slow
M11_BACKEND_KEYPAIR=/path/to/backend.json \
M11_PAYOUTS=/path/to/payouts.json \
M11_WINDOW=42 \
cargo run --offline
```

## Verified locally (offline)

`cargo test` runs the Merkle self-check: every leaf's proof reaches the root
for tree sizes 1..=9, and a wrong index or a tampered amount fails
verification — the same recipe the pool program enforces on-chain.
