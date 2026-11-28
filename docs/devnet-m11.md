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
