# M11 devnet sync — upgrade through the 2-of-2 + a PAID claim (SAT-2, 2026-11-28)

Status: **RUN COMPLETE (2026-11-28).** The M8 pool program on devnet was
upgraded to the M11 settlement code through the existing Squads 2-of-2
(proposal **index 11**), and a REAL settle run then PAID a depositor
**149,040 micro-USDC** out of the escrow vault. Every number below was read
back from the chain (not from tool output); explorer links use
`?cluster=devnet`.

- Branch: `m11/devnet-sync` (cut from `main` `02cbac8`)
- Toolchain: solana-cli 4.1.2 · cargo-build-sbf 4.1.0 (platform-tools v1.54)
- Machine log archive (gitignored): `target/soak-logs/m11-*.log`

## 0. The artifact

| Fact | Value |
|---|---|
| Program | `9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX` (unchanged id; keypair `target/m8-keys/pool-program.json` matches) |
| ProgramData | `FRQmdoLx5t7JFgoyQWFpB2p3Fr5nMg9anNKUUhmYd2HP` |
| M11 ELF | `target/deploy/pool.so` — **406,128 bytes** |
| ELF sha256 | `3155eb73f6e2245bea88b9a7c3e6a30e8ec3980753f8403c10c69d177be523be` |
| Multisig (Squads v4, 2-of-2) | `GuSeFfFkdKtjkubeLUEeBHW5pfHSJctGnz8AFk7xrCk5` |
| Vault (index 0) | `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq` |
| Members TODAY | founder `HGHF2iuEphK4zFuqFAC5dXDfk4US6AAvMVqWwfwB7gzz` + teammate-rotated `2ys6xNTANVqoNar7RQHyQvtdDhCwYbsE3XEP4LAjpzbq` (verified live at slot 527064076) |
| Pool PDA | `6pHRZzYZNFbAtueQx1bWUC2QJJeoTSeHvv7hwX1vtjRT` |
| Escrow vault (pool-owned USDC) | `Dhp3t3vL6crixL1AzmXEFFa1bfMZ4KsaxEaSyZjFYksd` |
| Circle devnet USDC mint | `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU` (Token v3, 6 decimals) |

## 1. Preflight (slot 527064076, read-only)

- Upgrade authority = vault ✓; last deployed slot 517759050 (the 2026-11-02 sync).
- ProgramData allocated **348,781 B** (program area 348,736 B).
- Pool: admin = vault, backend = `AJmEJPsCBT7VpZ3CkxDWpcoeuBdqp3JzrcPr43ZntJpb`, **paused = 0 (ACTIVE)** — it did NOT arrive paused, so no resume was needed.
- Multisig live `transaction_index` = **10** → the next proposal index is 11. Stale approved-but-unexecuted proposals 7 and 8 (lesson 3) are skipped by construction: the index is read live, never hardcoded.

## 2. Buffer (lesson 2 applied)

- Fresh buffer keypair: `target/m11-keys/buffer.json` → buffer **`7bmhiCZ14PnjHMg8zgjGNetVPfJUqAh2jMfvNbZ3T2Qs`** (406,165 B = 37 B header + 406,128 B ELF), written by the deploy key.
- Buffer authority handed to the vault — sig [`4xVgtvGz…`](https://solscan.io/tx/4xVgtvGzWSuXXsGFNmRUa9NbsfvKchmBfE4HSzaj31rckV27iQfDBhZ8QNSPJoBVRLvfU8htFoFJFvxmHy3vV57r?cluster=devnet) ([explorer](https://explorer.solana.com/tx/4xVgtvGzWSuXXsGFNmRUa9NbsfvKchmBfE4HSzaj31rckV27iQfDBhZ8QNSPJoBVRLvfU8htFoFJFvxmHy3vV57r?cluster=devnet)) @ slot **527060524**.
- Authority read back at bytes **5..37** of the buffer account (`[u32 tag=1][u8 option][32 B key]`) = the vault ✓. (Lesson 2: the old soak client's 4..36 parse is off by one; the new bin parses 5..37.)

## 3. Extend FIRST (lesson 1 applied)

The M11 ELF (**406,128 B**) did NOT fit the allocated program area (**348,736 B**; deficit 57,392 B), so the ProgramData was extended BEFORE any multisig step, as a plain top-level tx paid by the deploy key (the runtime rejects `ExtendProgram` from inner instructions):

- `solana program extend … 65536` → ProgramData now **414,317 B** (program area 414,272 ≥ 406,128, 8,144 B headroom) — sig [`22vzNRpE…`](https://solscan.io/tx/22vzNRpET3zQmsdo2Z1QXXJc13yZJ2LmE5G2o9zXWDLFUUU7DzfDy2m4ikfyRxdFCWMSDnARcnyqR9NEWWzwpZsN?cluster=devnet) ([explorer](https://explorer.solana.com/tx/22vzNRpET3zQmsdo2Z1QXXJc13yZJ2LmE5G2o9zXWDLFUUU7DzfDy2m4ikfyRxdFCWMSDnARcnyqR9NEWWzwpZsN?cluster=devnet)) @ slot **527060660**.

## 4. The upgrade through the 2-of-2 — proposal INDEX 11

Vault-signed `Upgrade` (authority = vault, spill = deploy key). Both of today's members approved.

| Step | Signature | Slot |
|---|---|---|
| create vault-tx + proposal (index 11) | [`5YhVWgjU…`](https://solscan.io/tx/5YhVWgjUyLRm4HjBkxebn8MpTLwGXF4nb2opAkVE5svKQ1o6FwCb6kVtoQsvGT5xoVsTpiU2GWLkBDHbx4CzSom7?cluster=devnet) · [explorer](https://explorer.solana.com/tx/5YhVWgjUyLRm4HjBkxebn8MpTLwGXF4nb2opAkVE5svKQ1o6FwCb6kVtoQsvGT5xoVsTpiU2GWLkBDHbx4CzSom7?cluster=devnet) | 527064246 |
| approve — founder | [`5ShnAmi9…`](https://solscan.io/tx/5ShnAmi9gau9EUFZ8WFno6se21UPnkpJjRwKtwndFuFTJZ1LW9L8CFKKZjW1SmLzbQyWrgcDSHXAmQyqgtToxwZy?cluster=devnet) · [explorer](https://explorer.solana.com/tx/5ShnAmi9gau9EUFZ8WFno6se21UPnkpJjRwKtwndFuFTJZ1LW9L8CFKKZjW1SmLzbQyWrgcDSHXAmQyqgtToxwZy?cluster=devnet) | 527064255 |
| approve — teammate-rotated | [`2KvuggXA…`](https://solscan.io/tx/2KvuggXAYiP8e1tKUpL4EHsHzEqNK1fTYpwVYBXxwUM4ma34RfqYgMHU1mfW5yqBB8ecgVJAyJ8dutcWCAHeQsnX?cluster=devnet) · [explorer](https://explorer.solana.com/tx/2KvuggXAYiP8e1tKUpL4EHsHzEqNK1fTYpwVYBXxwUM4ma34RfqYgMHU1mfW5yqBB8ecgVJAyJ8dutcWCAHeQsnX?cluster=devnet) | 527064264 |
| **execute** | [`37JFtEwY…`](https://solscan.io/tx/37JFtEwYtSxHguzvGYjTv97vyMwR7ZSbyXddwFte6cgwjtwEcjn8r8CTsP6wTsCrZM4rm2v1mTEwZR5DvNeZ7KHN?cluster=devnet) · [explorer](https://explorer.solana.com/tx/37JFtEwYtSxHguzvGYjTv97vyMwR7ZSbyXddwFte6cgwjtwEcjn8r8CTsP6wTsCrZM4rm2v1mTEwZR5DvNeZ7KHN?cluster=devnet) | **527064271** |

Slots: before **527064235** → after **527064273** (deployed at 527064271).

## 5. Post-upgrade verification (re-read at slot 527064275 and again at 527078533)

- Program authority is **STILL the vault** `BhLQqw…` ✓
- Pool admin = vault ✓, backend = `AJmEJPs…` ✓ (both unchanged)
- Pool **paused = 0 → ACTIVE** ✓ (no resume needed)
- ProgramData last-deployed slot = 527064271 ✓; multisig `transaction_index` now 11.

## 6. Money (devnet only)

- **Backend SOL.** The public devnet airdrop was rate-limited (3 attempts: "airdrop request failed… rate limit"). Backend `AJmEJPsCBT7VpZ3CkxDWpcoeuBdqp3JzrcPr43ZntJpb` already held 0.0976 SOL (≈10× the run's ~0.009 SOL rent+fee need), so it was never blocking; the founder then topped it up via https://faucet.solana.com (5 SOL, browser) → 5.0976 SOL.
- **Backend USDC (the bond).** The Circle faucet API is still retired (HTTP 404 + reCAPTCHA — same finding as 2026-11-01), so the founder claimed **20 USDC** on https://faucet.circle.com (browser) to the backend wallet. It landed in **`7x8yfjEnGYQtWD1LCPuBRRLtJufTcb47PdbZBdm7MYzg`** = the backend's CANONICAL USDC ATA (owner = backend, mint = USDC, 20,000,000 micro). See §9 for why that address is the ATA.
- **Renter USDC.** The custodial renter wallet `3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` still held the 20 USDC from the M8 soak — no new faucet needed.
- **USDC into the vault (`prepay_renter`, 200,000 micro)** — sig [`5jU52D22…`](https://solscan.io/tx/5jU52D22NyRXuUKRpsyM5rG8X5mMThFjq4v3txdqAK3iKoenkDMYafqLF9ayyeDHA686EiVkctDWWTQ8h9CYCQ6g?cluster=devnet) ([explorer](https://explorer.solana.com/tx/5jU52D22NyRXuUKRpsyM5rG8X5mMThFjq4v3txdqAK3iKoenkDMYafqLF9ayyeDHA686EiVkctDWWTQ8h9CYCQ6g?cluster=devnet)) @ slot **527073092**: renter wallet 20,000,000 → 19,800,000; escrow vault 0 → **200,000**; renter prepaid record = 200,000. (The 2026-11-02 soak had left the vault at 0.)

## 7. The settle run (the founder's video numbers)

`payouts.json` (one leaf — depositor `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`):
`[{"depositor":"Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ","amount":149040}]`
— 149,040 = the 162,000 micro-USDC burn minus the 8% operator fee (12,960).

Run by `scripts/devnet-m11` (window **1795831842**, Merkle root
`0x40b04b3c5b5737d66d4d76547a52834dffe142116e79081d3823ae64c709ea66`):

| Step | Signature | Slot | On-chain deltas (from the tx meta) |
|---|---|---|---|
| `fund_bond` 149,040 | [`35EmRfxT…`](https://solscan.io/tx/35EmRfxTXNBc4kTZ4xMmKbyeBF4tGjUMev7phkkjg6iyJdba2UHNyqhfBnUJMB1CzFS7ZsShLsygSG1TUxB4NJRz?cluster=devnet) · [explorer](https://explorer.solana.com/tx/35EmRfxTXNBc4kTZ4xMmKbyeBF4tGjUMev7phkkjg6iyJdba2UHNyqhfBnUJMB1CzFS7ZsShLsygSG1TUxB4NJRz?cluster=devnet) | 527077528 | backend ATA 20,000,000 → 19,850,960 (−149,040); bond vault 0 → 149,040 |
| `post_root` | [`rCCt6PWn…`](https://solscan.io/tx/rCCt6PWnthZxrkkHo9DgmvTishS9QfgWEa7Va6enug39wowTpwVNwXnuPoViy1qNS6e8qSneaywfNqjkfmf3TKR?cluster=devnet) · [explorer](https://explorer.solana.com/tx/rCCt6PWnthZxrkkHo9DgmvTishS9QfgWEa7Va6enug39wowTpwVNwXnuPoViy1qNS6e8qSneaywfNqjkfmf3TKR?cluster=devnet) | 527077535 | root account born (posted_at = 527077535) |
| wait | — | — | challenge window 100 slots + 5 margin (invariant 11) |
| **`claim` leaf 0 (PAID)** | [`278rWKHN…`](https://solscan.io/tx/278rWKHN8aSEr26Fmbwe9H4CRBXWeU3e6iWjBGVZNj3fuzseHiNiuCtANDowtyvmg1CkkwbskBRoZGNCRuywMpJR?cluster=devnet) · [explorer](https://explorer.solana.com/tx/278rWKHN8aSEr26Fmbwe9H4CRBXWeU3e6iWjBGVZNj3fuzseHiNiuCtANDowtyvmg1CkkwbskBRoZGNCRuywMpJR?cluster=devnet) | **527077652** | depositor ATA 19,800,000 → 19,949,040 (**+149,040**); escrow vault 200,000 → 50,960 (**−149,040**) |

## 8. State read back from the chain (slot 527078493+)

| Account | Address | Read-back |
|---|---|---|
| SettlementRoot (window 1795831842) | `AyNb6DyMCo2j82X5adbJnAPHknt98Kr6L9iSguhzQJyQ` | total_payout = **149,040**, paid_out = **149,040**, status = Active(0), posted_at = 527077535, bond snapshot = 149,040, merkle_root = `0x40b0…9ea66` |
| ClaimMarker (leaf 0) | `G2pUPk4JPJdfpDa3mGqZRurURxfMP5fKMeaBwP8XqA2a` | depositor = `Ah3Tyju…`, amount = **149,040** |
| OperatorBond record | `3NRdsizTeHbEq9cE9YZaAM3iwXzs7mTYSUZuXZnVqCWn` | amount = 149,040 |
| Bond vault | `DoyipWb8ZqbaAgarN2Qdhp2tu9HbUNPVM1eHDtP548cG` | 149,040 (the bond stays — no slash was ruled) |
| Escrow vault | `Dhp3t3vL6crixL1AzmXEFFa1bfMZ4KsaxEaSyZjFYksd` | 50,960 (200,000 − 149,040; the renter's unspent remainder) |
| Depositor USDC ATA | `3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` | 19,949,040 — the claim leg alone was exactly **+149,040** (19,800,000 → 19,949,040); the wallet also funded the 200,000 prepay, so its net vs the 20 USDC start is −50,960 |

## 9. Ops lessons found LIVE this run (for the next sync)

1. **The ATA program moved.** The canonical Associated-Token-Account program
   is `ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL` (**…knL**) on BOTH
   mainnet and devnet; the legacy `…JA8knN` deployment no longer exists
   (checked on both clusters; `spl-associated-token-account` 2.3.0 agrees).
   The M11 glue and the M8 soak client both carried the stale `…knN` id —
   with it, every ATA derivation lands on a NON-ATA address and every
   ATA-program call targets a program that does not exist. Fixed in
   `scripts/devnet-m11/src/main.rs`, `scripts/devnet-soak/src/main.rs`, and
   the new `scripts/devnet-soak/src/bin/m11_sync.rs` (one byte: `0x5b` →
   `0x59`). This was the first run to ever touch the ATA program on-chain
   (M8 used a provisioned wallet), so the bug was latent until now.
2. **The M8 "provisioned renter wallet" was the canonical ATA all along.**
   `3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` derives as the depositor's
   ATA under the CORRECT program id; the "no seed derivation reproduces it"
   note in the M8 runbook was an artifact of the stale id. Same story for the
   backend's `7x8yfjE…` (the Circle faucet creates canonical ATAs).
3. **The Circle faucet API is still retired** (404 + reCAPTCHA, as on
   2026-11-01). The browser is the only path; the founder claimed both the
   SOL and the USDC by hand.
4. **The agave 4.1.2 `spl-token` CLI resolves "wallet → ATA" by RPC lookup,
   not derivation** — `create-account`/`transfer <wallet>` reuse an existing
   token account for the mint. One harmless no-op self-transfer probe
   (`2AdtTndD…` @ slot 527073597, 20 USDC in-and-out of the same account)
   was fired while diagnosing this; it moved nothing.
5. The three 2026-11-02 lessons all held: extend-top-level-first (§3), the
   buffer-authority layout `[u32 tag][u8 option][32 B key]` at 5..37 (§2),
   and stale proposals skipped via the live index (§4).

## 10. What the verifier should click

1. **The execute tx** (§4) — on either explorer, confirm the inner
   `Upgrade` instruction ran with the vault as upgrade authority, and the
   program's `Last Deployed In Slot` became 527064271.
2. **`solana program show 9pEmyx…`** — Authority must still be
   `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq`.
3. **The claim tx** (§7) — token-balance changes: depositor ATA +149,040,
   escrow vault −149,040.
4. **The root account** (§8) — `total_payout` 149,040 == `paid_out`.
5. The depositor's USDC ATA `3GuSWo…` on solscan — final balance
   19,949,040 micro-USDC (19.94904 USDC).

## Part 2 — challenge, ruling, direct claim (SAT-2b, 2026-11-28)

Status: **RUN COMPLETE (2026-11-28), DEVNET ONLY.** This continuation ran on
branch `m11/devnet-proof` from main `7f88988`. It used the same deployed M11
program, the same Squads 2-of-2, and the same custodial renter/depositor key as
Part 1. No product code changed (`programs/`, `crates/`, `apps/`, and
`migrations/` untouched); the only code change was the devnet ops tool in
`scripts/devnet-soak/src/bin/m11_sync.rs`.

The SAT-2b sequence was executed in this order: prepay → post root → challenge
→ assisted claim refused while frozen → 2-of-2 `RootStands` ruling → direct-door
claim after the window. Every balance and root field below was read back from
devnet accounts after the relevant transaction.

### 11. Part-2 preflight (slot 527,097,327)

- Escrow vault `Dhp3t3vL6crixL1AzmXEFFa1bfMZ4KsaxEaSyZjFYksd` held **50,960**
  micro-USDC (Part 1's remainder).
- Multisig `GuSeFfFkdKtjkubeLUEeBHW5pfHSJctGnz8AFk7xrCk5` was 2-of-2 with live
  `transaction_index = 11`; the ruling proposal therefore had to be index **12**.
- Program authority was still the vault `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq`;
  pool admin/backend were unchanged and the pool was active.
- Depositor key loaded from `target/m8-keys/depositor.json` and verified by
  pubkey only: `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`.

### 12. Second custodial prepay — 0.20 USDC into escrow

`prepay_renter(200000)` moved 200,000 micro-USDC from the custodial renter ATA
`3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` into the program escrow.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `prepay_renter` | [`2AVtEgsD…`](https://explorer.solana.com/tx/2AVtEgsDuJ4HZLQq5Y3Y2hf3RZfrp9USRU4Lcy8na7cw8xYnKXPxJQjJmbEqyMK3FmAabYaMaBR2g2ZV1auQPj7g?cluster=devnet) | 527,099,584 | escrow 50,960 → **250,960**; renter prepaid record **400,000** |

The depositor/renter ATA later read back as 19,749,040 immediately after the
root was posted (19,949,040 − 200,000).

### 13. Second root — window 1795831843, one leaf

The tree has one leaf: depositor `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`
receives **100,000 micro-USDC**. For a one-leaf tree the leaf hash is the root:
`0xd9c5352697fa8bf82bf9725ed6fed8488fa0d4cda54168d8235a574c0feb9a3a`.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `post_root` | [`5JZUU6td…`](https://explorer.solana.com/tx/5JZUU6tdn7xdPh7XkhwrZYd7BMJYVH7tFA1KVy7hDE1YyJGk91rgY3SpvPyQH8Gcyw4BVM3i1Y2rHGRhjACZNVfS?cluster=devnet) | **527,099,690** | root `HNVozXXxMsheuDSZEgSYL5J84mBAczqjGpNpPXME9VGt`; `total_payout=100000`; `paid_out=0`; `bond_snapshot=149040`; escrow **250,960** |

### 14. Permissionless challenge — root frozen

The program's evidence field is `[u8; 32]`. The reviewer-supplied evidence
preimage is 40 bytes (`"reviewer-requested challenge, 2026-11-28"`), so the
on-chain commitment is its SHA-256:
`0x42c5e309476d8f24631d10fcbaf051de1cb3ec00230abe15f45b6ceaf1405bdb`.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `challenge` | [`2GfqoxhM…`](https://explorer.solana.com/tx/2GfqoxhMYdZkEH5q72zwWfVnDUCTvETmSHvsJFwT4PTMqREyPhVUkjrJXPx2PwNcuDywomihUWsysmu7VHELj4D6?cluster=devnet) | **527,099,746** | root status `Challenged`; challenger recorded as `DtNTR9Nn9A7Ld1j3KRGoxVC18oeazSRKrD7MZydTxjgk`; evidence digest recorded; `paid_out` still 0 |

The challenger key was neither the backend/operator
(`AJmEJPsCBT7VpZ3CkxDWpcoeuBdqp3JzrcPr43ZntJpb`) nor either multisig member.
The challenge landed 56 slots after posting, inside the 100-slot window.

### 15. Assisted claim while frozen — REFUSED ON DEVNET

This was an intentional failure transaction: the exact claim was first
simulated and had to return `RootFrozen`; it was then broadcast with
`skip_preflight` so the refusal itself has a devnet signature. No marker was
created and no USDC moved (only the 5,000-lamport fee was charged).

| Step | Signature | Slot | Exact refusal |
|---|---|---:|---|
| assisted `claim` against challenged root | [`5VhiGct8…`](https://explorer.solana.com/tx/5VhiGct8FVddSNy76HtA1yUUTnoA2ZtBhzzrUrZ7Peg54Lcma8bbg5SaUpsebfuQtMzHADgyVVcFftkvykjszFzd?cluster=devnet) | **527,099,823** | `InstructionError(1, Custom(6015))`; program log: `AnchorError thrown in programs/pool/src/lib.rs:754. Error Code: RootFrozen. Error Number: 6015. Error Message: Root payouts are frozen — the root is not Active.`; transaction status: `Error processing Instruction 1: custom program error: 0x177f` |

Post-refusal read-back: root still `Challenged`, `paid_out=0`, marker
`HvhdUe4fUkeWT1wRPvwcUHP4msmnKA7VhDWmbjQ5Ee7a` still absent, escrow still
**250,960**, depositor ATA still **19,749,040**.

### 16. The 2-of-2 ruling — `RootStands`, proposal index 12

The Squads vault transaction carried `resolve_challenge(1795831843,
RootStands)`. Both live members approved before execution.

| Step | Member / actor | Signature | Slot |
|---|---|---|---:|
| create vault transaction + proposal | deploy rent payer + founder creator | [`58mFvT4f…`](https://explorer.solana.com/tx/58mFvT4fuq8vfnwMghmQedKHzP1GuSy3wn1CcWSs92xjUkQmT58UKVnk24mMu8znKe6a92DzNuHCyRQrDuFbUJRM?cluster=devnet) | 527,099,880 |
| approve | founder `HGHF2iuEphK4zFuqFAC5dXDfk4US6AAvMVqWwfwB7gzz` | [`5ADanFEF…`](https://explorer.solana.com/tx/5ADanFEFbTXc9z42m4L7mDekeMvTdP7UEcE3fJWMQi4enfZu1bgYqXJsmznXtgQJfuQtWm7gsyHA7LLeZ8KhC21?cluster=devnet) | 527,099,889 |
| approve | teammate-rotated `2ys6xNTANVqoNar7RQHyQvtdDhCwYbsE3XEP4LAjpzbq` | [`nG759GoZ…`](https://explorer.solana.com/tx/nG759GoZ6s7984LmEJmbeNW4QcX9GJqqhHxU9ve94p7fdhu6ZUHZ7dNm42dgNyx53xMYr85kE8a7RTw6Pyy7tBk?cluster=devnet) | 527,099,899 |
| **execute** | founder member executes the approved vault transaction | [`Nwg7BhMM…`](https://explorer.solana.com/tx/Nwg7BhMMmJ2iHSF6xjMJ4WYPyzpP2xzWh9Keqj3zUfH1ZUB57ZfVgeDX6UsE2ZhTx4W6BYT8WWocT11s4ckwEih?cluster=devnet) | **527,099,908** |

Post-ruling root read-back: status **Active**, `stands_ruled=true`, challenger
and evidence preserved, `ruled_slash=0`, victim = system program default,
`paid_out=0`.

### 17. Direct-door claim — depositor signs and pays marker rent

The direct claim was submitted only after the chain slot passed the required
target: posted_at 527,099,690 + 100-slot window + 5-slot margin = **527,099,795**.
The tool observed slot **527,099,987** before sending. In the transaction the
depositor `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ` is the fee payer, the
claim `authority`, and the marker `payer` — the direct door, with no backend
signature.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| direct `claim` leaf 0 | [`3bRK6dtn…`](https://explorer.solana.com/tx/3bRK6dtnLuyaArVxDq3UMiqUsU18qCMRyjEwj6dKfKXft5Zze2TSK75HQiPQbZ98tgxJtm1LEAdXPJQdyU2ygJ8J?cluster=devnet) | **527,099,994** | depositor ATA 19,749,040 → **19,849,040** (+100,000); escrow 250,960 → **150,960** (−100,000); root `paid_out=100000 == total_payout=100000`; marker `HvhdUe4f…` names the depositor and amount 100,000 |

The transaction's `ClaimPaid` event was decoded from its on-chain program data:
window **1795831843**, leaf index **0**, depositor
`Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`, amount **100,000**, and
`vault_balance_after=150960`. The depositor paid 0.00108204 SOL of marker rent
plus the 0.000005 SOL fee; the USDC ATA already existed, so no ATA rent was
created in this transaction.

### 18. Final Part-2 read-back (slot 527,100,548)

| Fact | Final value |
|---|---|
| Root `HNVozXXxMsheuDSZEgSYL5J84mBAczqjGpNpPXME9VGt` | window 1795831843; Active; `stands_ruled=true`; total 100,000; paid_out 100,000 |
| Claim marker `HvhdUe4fUkeWT1wRPvwcUHP4msmnKA7VhDWmbjQ5Ee7a` | depositor `Ah3Tyju…`; amount 100,000 |
| Escrow vault `Dhp3t3vL6crixL1AzmXEFFa1bfMZ4KsaxEaSyZjFYksd` | **150,960** micro-USDC |
| Bond record / bond vault | **149,040** / **149,040** micro-USDC; unchanged by the stand ruling |
| Depositor ATA `3GuSWo…` | **19,849,040** micro-USDC |
| Multisig live `transaction_index` | **12** |
| Pool / program authority | active; authority still the Squads vault |

### 19. What the Part-2 verifier should click

1. **Prepay** — token-balance changes: renter ATA −200,000 and escrow +200,000.
2. **Root posting** — account `HNVozXXx…`: window 1795831843, total 100,000,
   posted_at slot 527,099,690.
3. **Challenge** — root status becomes Challenged and challenger is
   `DtNTR9Nn…`; evidence is the SHA-256 commitment printed in §14.
4. **Frozen claim** — open the transaction logs: instruction 1 fails with
   `RootFrozen` / 6015 / `0x177f`; balances and marker remain unchanged.
5. **Ruling** — proposal index 12 shows founder approval, teammate-rotated
   approval, then execute; root account reads Active with `stands_ruled=true`.
6. **Direct claim** — the transaction account list shows the depositor as fee
   payer and claim signer; token balances show depositor +100,000 and escrow
   −100,000; the emitted `ClaimPaid` data matches the root account.

## Part 3 — the slash (SAT-2c, 2026-11-28)

Status: **RUN COMPLETE (2026-11-28), DEVNET ONLY.** This continuation ran on
branch `m11/devnet-slash` from main `1069d44`. Same deployed M11 program
(last deployed slot 527,064,271), same Squads 2-of-2, same custodial
renter/depositor key. No product code changed (`programs/`, `crates/`,
`apps/`, and `migrations/` untouched); the only code change is the devnet
ops tool `scripts/devnet-soak/src/bin/m11_sync.rs`.

This is the one path of the dispute flow that had never run on devnet: a
root ruled FALSE, and its slash executed. The sequence: post a third root →
challenge it inside the window → 2-of-2 `RootFalse` ruling → the
permissionless `slash_bond` crank → a claim on the slashed root, refused.
Every number below was read back from devnet accounts and transactions
after the relevant step; the closing balances were then read a second time
from raw RPC JSON with no ops-tool code involved.

All eight transactions, in order (block times are UTC+7, read from the
transactions):

| # | Step | Signature | Slot | Block time |
|---|---|---|---:|---|
| 1 | `post_root` window 1795831844 | [`3EwF9YTe…`](https://explorer.solana.com/tx/3EwF9YTejhA9ceGqTqqqnAGmcn2xdTzNHHZuHmUJ1FfKa148c2rTvnhZxqoNiuWNwLVvBhDbGx9txxgKrXaoSkxX?cluster=devnet) | 527,191,516 | 17:04:07 |
| 2 | `challenge` (rogue-upgrader) | [`5nkt9mFf…`](https://explorer.solana.com/tx/5nkt9mFfe7WfvN9uvD3YdpfdoZKHi9ZHNE4pp9HUUCv8h38Z1vpESqtfD2oQZnqqW7jTSbpFC36o7NHHPDdLQgFM?cluster=devnet) | 527,191,536 | 17:04:12 |
| 3 | proposal 13: create vault transaction + proposal | [`5ZwPwxGK…`](https://explorer.solana.com/tx/5ZwPwxGKkeRqmmXhwzLvquNHoiJfW1AyNmmdSwCs3zazDxVquzSbkzdiwnNUvpdBi7a6VL5QpjcxdWzHZN5PRhj9?cluster=devnet) | 527,191,663 | 17:04:42 |
| 4 | proposal 13: approve — founder | [`5dn4BLNi…`](https://explorer.solana.com/tx/5dn4BLNif17zDya2dkb17LYeNuLdLocXSVhSKhW8vp7CfuL4BCSiN5qe3vg4mb2fX4517TLfv34KK3NyWwVBmymq?cluster=devnet) | 527,191,673 | 17:04:45 |
| 5 | proposal 13: approve — teammate-rotated | [`4f9frHbS…`](https://explorer.solana.com/tx/4f9frHbSNV53VBLNANxykShZseXZv1Ju17zQpXXXqMGZG4E6oMWSoK8LS4aNm5mwQRLdPFG4F2Ns4iyGABHxxCiq?cluster=devnet) | 527,191,683 | 17:04:47 |
| 6 | proposal 13: **execute** the `RootFalse` ruling | [`5EJEwAd8…`](https://explorer.solana.com/tx/5EJEwAd8TF3j8yDBZfwW2CbmpXpUhhWpMP3JcquYP6DEk9cFhajWD8akiaEtuNKC7rLu5GLyQfGNBzKyh5Dpkkg3?cluster=devnet) | 527,191,692 | 17:04:49 |
| 7 | `slash_bond` — permissionless | [`3CzUi4ds…`](https://explorer.solana.com/tx/3CzUi4dssADKACmsnk3t8uyXQZ9qgSoqubrJyw64dRbhH3whSCbauFH4p26gR2RuhKLxYqAh5WD2QXXBp2JTdhkG?cluster=devnet) | 527,191,774 | 17:05:09 |
| 8 | assisted `claim` on the slashed root — **REFUSED** | [`4yeTfKsD…`](https://explorer.solana.com/tx/4yeTfKsDjVN8k72LmkGZVdypuuCxQX7BZ9uzY2gjZ36KKDKSZE77o3RSQDyaiDzX7MJLpSV4gPjuWEyFhe62LQwh?cluster=devnet) | 527,191,834 | 17:05:23 |

### 20. Part-3 preflight (slot 527,188,963, read-only)

- Escrow vault `Dhp3t3vL6crixL1AzmXEFFa1bfMZ4KsaxEaSyZjFYksd` held
  **150,960** micro-USDC; bond vault
  `DoyipWb8ZqbaAgarN2Qdhp2tu9HbUNPVM1eHDtP548cG` held **149,040** (bond
  record 149,040) — the state the kickoff named, confirmed on-chain.
- Multisig `GuSeFfFkdKtjkubeLUEeBHW5pfHSJctGnz8AFk7xrCk5` was 2-of-2 with
  live `transaction_index = 12`; the ruling proposal would therefore be
  index **13**.
- Program authority was still the vault `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq`;
  pool admin/backend unchanged; pool active; last deployed slot 527,064,271.
- No root existed for window 1795831844: its PDA
  `6491XukKY4bzCr6FcPmCF5SStW5VAZGzgHRRypZH4Dr7` read `AccountNotFound`.
- The custodial renter's USDC token account
  `3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` (owner
  `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`) held **19,849,040**.

### 21. Third root — window 1795831844, one leaf

The tree has one leaf: depositor `Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`
receives **100,000 micro-USDC**. The leaf is identical to Part 2's, so the
one-leaf root hash is too: `0xd9c5352697fa8bf82bf9725ed6fed8488fa0d4cda54168d8235a574c0feb9a3a`.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `post_root` | [`3EwF9YTe…`](https://explorer.solana.com/tx/3EwF9YTejhA9ceGqTqqqnAGmcn2xdTzNHHZuHmUJ1FfKa148c2rTvnhZxqoNiuWNwLVvBhDbGx9txxgKrXaoSkxX?cluster=devnet) | **527,191,516** | root `6491XukK…` born `Active`; `total_payout=100000`; `paid_out=0`; `bond_snapshot=149040`; `posted_at=527,191,516`; escrow unchanged **150,960** |

The transaction was signed by the backend alone (fee payer and signer). Its
`RootPosted` event, decoded from the on-chain program data:
`RootPosted window=1795831844 merkle_root=0xd9c5352697fa8bf82bf9725ed6fed8488fa0d4cda54168d8235a574c0feb9a3a total_payout=100000 bond=149040 posted_at=527191516`.

### 22. Challenge — inside the window, root frozen

The challenger is `target/m8-keys/rogue-upgrader.json`
(`DtNTR9Nn9A7Ld1j3KRGoxVC18oeazSRKrD7MZydTxjgk`) — neither the backend
(`AJmEJPsCBT7VpZ3CkxDWpcoeuBdqp3JzrcPr43ZntJpb`) nor either multisig
member. The deploy key paid only the fee; the transaction's signing keys
read back as exactly those two. The evidence preimage (123 bytes) is:

> SAT-2c deliberately false root 1795831844: its leaf is owed nothing, a payout would spend the renter's prepaid USDC wrongly

committed on-chain as SHA-256 `0x22c8492b49407dfd077ce1492637fb97497ace2d98db374c29494805dd14d467`
(recomputed independently with `sha256sum`; it matches the on-chain digest).

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `challenge` | [`5nkt9mFf…`](https://explorer.solana.com/tx/5nkt9mFfe7WfvN9uvD3YdpfdoZKHi9ZHNE4pp9HUUCv8h38Z1vpESqtfD2oQZnqqW7jTSbpFC36o7NHHPDdLQgFM?cluster=devnet) | **527,191,536** | root status `Challenged`; challenger and evidence digest recorded; `paid_out=0` |

The challenge landed **20 slots** after posting, inside the
100-slot window. Its `RootChallenged` event, decoded from program data:
`RootChallenged window=1795831844 challenger=DtNTR9Nn9A7Ld1j3KRGoxVC18oeazSRKrD7MZydTxjgk evidence=0x22c8492b49407dfd077ce1492637fb97497ace2d98db374c29494805dd14d467`.

### 23. The 2-of-2 ruling — `RootFalse`, proposal index 13

The Squads vault transaction carried
`resolve_challenge(1795831844, RootFalse { slash_amount: 50,000, victim: Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ })`.
Both live members approved before execution.

**What `victim` names.** The kickoff words it as "the custodial renter key's
USDC token account". The program's contract is that `victim` is the KEY that
owns the slash destination: `slash_bond` requires
`destination.owner == settlement_root.victim` (the `WrongDestinationOwner`
check in `programs/pool/src/lib.rs`), and the doctrine suite's test 6
passes the victim's key as `victim` and the victim's token account as the
destination. So the ruling names the custodial renter KEY
`Ah3TyjuC…` — the party whose prepayment a false root would have paid out
wrongly — and the crank's destination (§24) is that key's USDC token
account `3GuSWoYU…`. Read literally, the kickoff's wording would have
recorded a token-account address as `victim`; no USDC token account can
exist whose owner is that address (creating one needs that address's
signature, and an ATA address cannot sign), so the slash could never have
executed and the root would have stayed `ProvenFalse` for good. The ops tool therefore now
refuses to propose a ruling whose victim holds no USDC token account —
before any proposal index is spent (AUDIT.md A-066).

| Step | Member / actor | Signature | Slot |
|---|---|---|---:|
| create vault transaction + proposal (index 13) | deploy rent payer + founder creator | [`5ZwPwxGK…`](https://explorer.solana.com/tx/5ZwPwxGKkeRqmmXhwzLvquNHoiJfW1AyNmmdSwCs3zazDxVquzSbkzdiwnNUvpdBi7a6VL5QpjcxdWzHZN5PRhj9?cluster=devnet) | 527,191,663 |
| approve | founder `HGHF2iuEphK4zFuqFAC5dXDfk4US6AAvMVqWwfwB7gzz` | [`5dn4BLNi…`](https://explorer.solana.com/tx/5dn4BLNif17zDya2dkb17LYeNuLdLocXSVhSKhW8vp7CfuL4BCSiN5qe3vg4mb2fX4517TLfv34KK3NyWwVBmymq?cluster=devnet) | 527,191,673 |
| approve | teammate-rotated `2ys6xNTANVqoNar7RQHyQvtdDhCwYbsE3XEP4LAjpzbq` | [`4f9frHbS…`](https://explorer.solana.com/tx/4f9frHbSNV53VBLNANxykShZseXZv1Ju17zQpXXXqMGZG4E6oMWSoK8LS4aNm5mwQRLdPFG4F2Ns4iyGABHxxCiq?cluster=devnet) | 527,191,683 |
| **execute** | founder member executes the approved vault transaction | [`5EJEwAd8…`](https://explorer.solana.com/tx/5EJEwAd8TF3j8yDBZfwW2CbmpXpUhhWpMP3JcquYP6DEk9cFhajWD8akiaEtuNKC7rLu5GLyQfGNBzKyh5Dpkkg3?cluster=devnet) | **527,191,692** |

Each approval's signing keys were read back from the transaction itself:
the deploy key (fee payer) plus the approving member, and no one else. The
vault transaction PDA is `GmqBzbsWrpwJFxr59eRPbnv4efN21EpU4mKoAB9VSb5q` and the proposal
PDA `GMwtNnYfiJnbB5Q2QE4hrAjjJossjLPRm5czWe8hjCLC`.

The execute transaction's inner instruction — the call the multisig made
into the pool program, decoded from its base58 data — is
`resolve_challenge(window=1795831844, RootFalse { slash_amount: 50000, victim: Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ })`,
with accounts pool / vault (the signing admin authority) / root / bond
record. Its `ChallengeResolved` event, decoded from program data:
`ChallengeResolved window=1795831844 root_stands=false slash_amount=50000 victim=Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`.

Post-ruling root read-back: status **ProvenFalse**,
`ruled_slash=50000`, `victim=Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ`,
`stands_ruled=false`, `paid_out=0`; challenger and evidence preserved. The
ruling only RECORDS: the bond vault still read **149,040** and
the victim's token account **19,849,040** — nothing had moved.

The ruling executed 176 slots after posting, i.e. after the
natural 100-slot window had already closed — the same timing as Part 2's
ruling. A root stays frozen while it is `Challenged`, so the closed window
changes nothing.

### 24. The slash — `slash_bond`, permissionless

`slash_bond(1795831844)` was cranked with the victim's token account
`3GuSWoYUdyu3PHHs7M7FijfUGih4ySo2w76puJW9Y2xs` as the
destination. The pool program asks for NO signer on this instruction: the
transaction's only signing key is its fee payer, the deploy key
`4N2nTJ8WWwDKVoULt9bymaB6XsrB7HP973y8Ebw6tQhV` — not the backend, not a
multisig member. The amount and the destination come from the root account,
never from arguments.

| Step | Signature | Slot | Read-back |
|---|---|---:|---|
| `slash_bond` | [`3CzUi4ds…`](https://explorer.solana.com/tx/3CzUi4dssADKACmsnk3t8uyXQZ9qgSoqubrJyw64dRbhH3whSCbauFH4p26gR2RuhKLxYqAh5WD2QXXBp2JTdhkG?cluster=devnet) | **527,191,774** | bond vault 149,040 → **99,040** (−50,000); victim token account 19,849,040 → **19,899,040** (+50,000); bond record 149,040 → **99,040**; root **Slashed** |

The token movement appears in the transaction as a CPI to the Token
program — bond vault → victim token account, authority = the pool PDA — and
in its token balances (`DoyipWb8Zq…` −50,000, `3GuSWoYU…` +50,000). Its
`BondSlashed` event, decoded from program data:
`BondSlashed window=1795831844 amount=50000 victim=Ah3TyjuC3agXQcsYty1VpNBWLRg4DkFWkhugsyoGLUuJ bond_after=99040`.
The escrow vault is not part of a slash and still read **150,960**.

### 25. Claim on the slashed root — REFUSED ON DEVNET

The exact claim of §21's leaf (assisted door: the backend signs and pays
the fee) was first simulated and had to return `RootFrozen`; it was then
broadcast with `skip_preflight` so the refusal itself has a devnet
signature. The root is `Slashed`, not `Active`, so the status gate refuses
before the challenge-window check is ever reached.

| Step | Signature | Slot | Exact refusal |
|---|---|---:|---|
| assisted `claim` against the slashed root | [`4yeTfKsD…`](https://explorer.solana.com/tx/4yeTfKsDjVN8k72LmkGZVdypuuCxQX7BZ9uzY2gjZ36KKDKSZE77o3RSQDyaiDzX7MJLpSV4gPjuWEyFhe62LQwh?cluster=devnet) | **527,191,834** | `Err(InstructionError(1, Custom(6015)))`; program log: `AnchorError thrown in programs/pool/src/lib.rs:754. Error Code: RootFrozen. Error Number: 6015. Error Message: Root payouts are frozen — the root is not Active.`; transaction status: `Error processing Instruction 1: custom program error: 0x177f` |

Post-refusal read-back: root still `Slashed`, `paid_out=0`, marker
`FptyfRWHQ1puAi6TPp94FARQwKPQAtJSZxjmKosnJoWP` still absent, escrow
still **150,960**, victim token account still **19,899,040**. The failed
transaction's own token balances read `19,899,040 → 19,899,040` and
`150,960 → 150,960` (delta 0 each); only the 5,000-lamport fee was
charged.

### 26. Final Part-3 read-back (slot 527,192,269)

| Fact | Final value |
|---|---|
| Root `6491XukKY4bzCr6FcPmCF5SStW5VAZGzgHRRypZH4Dr7` | window 1795831844; **Slashed**; total 100,000; paid_out 0; `ruled_slash` **50,000**; victim `Ah3TyjuC…` |
| Bond record `3NRdsizTeHbEq9cE9YZaAM3iwXzs7mTYSUZuXZnVqCWn` / bond vault `DoyipWb8ZqbaAgarN2Qdhp2tu9HbUNPVM1eHDtP548cG` | **99,040** / **99,040** micro-USDC (149,040 − 50,000) |
| Victim USDC token account `3GuSWoYU…` | **19,899,040** micro-USDC (19,849,040 + 50,000) |
| Escrow vault `Dhp3t3vL6…` | **150,960** micro-USDC — untouched by the ruling, the slash and the refused claim |
| Claim marker (leaf 0) `FptyfRWHQ1puAi6TPp94FARQwKPQAtJSZxjmKosnJoWP` | does not exist — the false root paid nothing |
| Multisig live `transaction_index` | **13** |
| Pool / program authority | active; authority still the Squads vault; last deployed slot 527,064,271 |

The same values were read a second time from raw `getAccountInfo` /
`getTokenAccountBalance` JSON — base64 account data parsed field by field,
no ops-tool code — and agreed on every number.

### 27. What the Part-3 verifier should click

1. **Root posting** — transaction [`3EwF9YTe…`](https://explorer.solana.com/tx/3EwF9YTejhA9ceGqTqqqnAGmcn2xdTzNHHZuHmUJ1FfKa148c2rTvnhZxqoNiuWNwLVvBhDbGx9txxgKrXaoSkxX?cluster=devnet): the backend is the only
   signing key; account `6491XukK…` is born with `total_payout=100000` at
   slot 527,191,516. (Today the account reads `Slashed`, §26.)
2. **Challenge** — transaction [`5nkt9mFf…`](https://explorer.solana.com/tx/5nkt9mFfe7WfvN9uvD3YdpfdoZKHi9ZHNE4pp9HUUCv8h38Z1vpESqtfD2oQZnqqW7jTSbpFC36o7NHHPDdLQgFM?cluster=devnet): the signing keys are the
   deploy key and `DtNTR9Nn…` (the challenger); the `RootChallenged` event
   carries the evidence digest `0x22c8492b…14d467`.
3. **Ruling** — proposal index 13: `5dn4BLNi…` is signed by
   `HGHF2iu…`, `4f9frHbS…` by `2ys6xNTA…`, then `5EJEwAd8…` executes. In the
   execute transaction, the inner instruction to the pool program decodes
   to `RootFalse` (variant byte 1, 50,000, `Ah3TyjuC…`), and its
   `ChallengeResolved` event says `root_stands=false`.
4. **Slash** — transaction [`3CzUi4ds…`](https://explorer.solana.com/tx/3CzUi4dssADKACmsnk3t8uyXQZ9qgSoqubrJyw64dRbhH3whSCbauFH4p26gR2RuhKLxYqAh5WD2QXXBp2JTdhkG?cluster=devnet): the ONLY signing key is the deploy
   key `4N2nTJ8W…`; token-balance changes are the bond vault −50,000 and
   `3GuSWoYU…` +50,000; `BondSlashed` reports `bond_after=99040`.
5. **Refused claim** — transaction [`4yeTfKsD…`](https://explorer.solana.com/tx/4yeTfKsDjVN8k72LmkGZVdypuuCxQX7BZ9uzY2gjZ36KKDKSZE77o3RSQDyaiDzX7MJLpSV4gPjuWEyFhe62LQwh?cluster=devnet): instruction 1 fails with
   `RootFrozen` / 6015 / `0x177f`; both token balances are unchanged.
6. **Bond vault** `DoyipWb8ZqbaAgarN2Qdhp2tu9HbUNPVM1eHDtP548cG` on
   solscan — 99,040 micro-USDC (0.09904 USDC).

### 28. Notes for the next run

1. **`victim` is an owner key, not a token account.** Word it that way in
   the next kickoff; `m11_sync resolve_false` now checks the victim's USDC
   token account before it spends a proposal index (A-066).
2. `challenge <window> [evidence_preimage]` takes an optional evidence
   string (omitted, it keeps Part 2's reviewer text, so that command is
   unchanged). `tx_readback <signature>` re-reads any landed transaction —
   signing keys, token deltas, CPI, decoded events — and every read-back in
   this part comes from it. `resolve_false`, `slash_bond` and
   `claim_slashed` are the three new step commands.
3. The stale approved-but-unexecuted proposals 7 and 8 remain on the
   multisig, unchanged (lesson 3 still holds: the live index skips them).
