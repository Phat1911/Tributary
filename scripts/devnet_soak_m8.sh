#!/usr/bin/env bash
# M8 devnet soak runbook — the executable companion to docs/devnet-soak-m8.md.
#
# Everything this script needs lives IN THE REPO TREE (toolchains under
# target/, keys in target/m8-keys) — nothing installs outside the repo.
# Re-runnable: each step skips itself when its artifact already exists.
#
# Usage: bash scripts/devnet_soak_m8.sh 2>&1 | tee target/soak-logs/soak-$(date +%Y%m%d-%H%M%S).log

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# --- in-repo toolchain env (site-restoration friendly) ---------------------
export HOME="$ROOT/target/solana-home"                       # solana CLI state
export SOLANA_INSTALL_DIR="$ROOT/target/solana-install"
export RUSTUP_HOME="$ROOT/target/rustup-home"
export CARGO_HOME="$ROOT/target/cargo-home"
SB="$ROOT/target/solana-home/.local/share/solana/install/active_release/bin"
export PATH="$SB:$CARGO_HOME/bin:$ROOT/target/avm/bin:$PATH"
mkdir -p target/soak-logs

echo "== [env] solana: $(solana --version) | anchor: $(anchor --version 2>&1 | head -1)"
solana config set --url https://api.devnet.solana.com > /dev/null
solana config set --keypair "$ROOT/target/m8-keys/deploy.json" > /dev/null

DEPLOY_PUBKEY="$(solana-keygen pubkey target/m8-keys/deploy.json)"
PROGRAM_PUBKEY="$(solana-keygen pubkey target/m8-keys/pool-program.json)"
echo "== [keys] deploy=$DEPLOY_PUBKEY program=$PROGRAM_PUBKEY"

# --- 1. build the program (SBF) ---------------------------------------------
# cargo-build-sbf writes the ELF to the WORKSPACE-root target dir.
if [ ! -f "$ROOT/target/deploy/pool.so" ]; then
  echo "== [build] cargo-build-sbf (SBF v3)"
  (cd programs/pool && cargo-build-sbf --arch v3)
else
  echo "== [build] $ROOT/target/deploy/pool.so exists (delete to rebuild)"
fi

# --- 2. fund the deploy key ---------------------------------------------------
# The public devnet RPC drops the occasional request ("error sending
# request", seen mid-soak 2026-11-01); balance reads retry instead of
# dying — a re-runnable script must survive a flaky RPC.
solana_balance_retry() {
  for _ in 1 2 3 4 5; do
    BAL="$(solana balance 2>/dev/null | awk '{print $1}')"
    [ -n "$BAL" ] && return 0
    sleep 5
  done
  return 1
}
for i in $(seq 1 6); do
  solana_balance_retry || { echo "!! devnet RPC unreachable after 5 tries; re-run later (script is re-runnable)."; exit 1; }
  OK=$(python3 -c "print(1 if float('$BAL') >= 4.0 else 0)")
  [ "$OK" = "1" ] && break
  echo "== [fund] balance=${BAL} SOL — airdropping 2 SOL (attempt $i)"
  solana airdrop 2 || sleep 20
done
solana_balance_retry || { echo "!! devnet RPC unreachable; re-run later."; exit 1; }
python3 -c "import sys; sys.exit(0 if float('$BAL') >= 4.0 else 1)" || {
  echo "!! devnet airdrop starved (balance ${BAL} SOL). Open https://faucet.solana.com in a browser,"
  echo "!! paste $DEPLOY_PUBKEY, claim, then re-run this script."; exit 1;
}
echo "== [fund] deploy holds ${BAL} SOL"

# --- 3. deploy the program -----------------------------------------------------
if solana program show "$PROGRAM_PUBKEY" --url https://api.devnet.solana.com > /dev/null 2>&1; then
  echo "== [deploy] program already on devnet at $PROGRAM_PUBKEY"
else
  echo "== [deploy] deploying (first time)"
  solana program deploy "$ROOT/target/deploy/pool.so" \
    --program-id target/m8-keys/pool-program.json \
    --url https://api.devnet.solana.com \
    --commitment confirmed
fi
echo "== [deploy] authority BEFORE soak (expect deploy key here; the soak moves it):"
solana program show "$PROGRAM_PUBKEY" | sed -n 's/^.*Authority: /  authority: /p'

# --- 4. USDC for the custodial user key (Circle faucet) -----------------------
# (D40 renter-shaped soak: this ONE wallet plays the renter role — it is
# the only USDC the soak moves. The depositor identity needs NO USDC:
# contributions are records now.)
USER_PUBKEY="$(solana-keygen pubkey target/m8-keys/depositor.json)"  # custodial user key (renter wallet)
USDC_MINT="4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
usdc_balance() {  # never fails: transient RPC errors read as "no balance yet"
  # This CLI build rejects `spl-token address --token X --owner Y` (found
  # live 2026-11-01 — the old poll ALWAYS read empty), so read the wallet's
  # accounts as JSON and pull the USDC row's uiAmountString instead.
  spl-token --url https://api.devnet.solana.com accounts \
    --owner target/m8-keys/depositor.json --output json 2>/dev/null \
    | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
for a in d.get('accounts', []):
    if a.get('mint') == '$USDC_MINT':
        print(a['tokenAmount']['uiAmountString'])
        break
" || true
}
echo "== [usdc] renter wallet (custodial user key): $USER_PUBKEY"
echo "== [usdc] Circle faucet API retired (404 + reCAPTCHA since ~2026-11-01) —"
echo "== [usdc] the browser step is the only path; polling in case it already landed…"
# Poll for the ATA balance a few times (faucet is async), then only fall
# back to the manual browser step if it really did not land.
for i in 1 2 3 4 5 6; do
  sleep 15
  UBAL="$(usdc_balance)"
  echo "== [usdc] renter USDC balance: ${UBAL:-0}"
  if [ -n "${UBAL:-}" ] && python3 -c "import sys; sys.exit(0 if float('$UBAL') >= 10.0 else 1)"; then
    echo "== [usdc] faucet landed (${UBAL} USDC)"; break
  fi
  if [ "$i" = "6" ]; then
    echo "== [usdc] faucet never landed. Open https://faucet.circle.com in a browser,"
    echo "== [usdc] pick USDC on Solana Devnet, send at least 10 USDC to $USER_PUBKEY,"
    echo "== [usdc] then re-run this script (it is re-runnable; earlier steps skip)."
    exit 1
  fi
done

# --- 5. upgrade buffer for the multisig-upgrade proof ---------------------------
if [ -z "${SOAK_BUFFER:-}" ]; then
  echo "== [buffer] writing a fresh buffer of the SAME .so (upgrade proof needs it)"
  # --buffer-authority takes the KEYPAIR FILE, not the bare pubkey: with a
  # bare pubkey this CLI build (agave 4.1.2) demands a signature it never
  # attaches ("Dynamic program error: missing signature", found live
  # 2026-11-01). The keypair path supplies address AND signer together.
  BUFFER=$(solana program write-buffer "$ROOT/target/deploy/pool.so" \
    --url https://api.devnet.solana.com \
    --buffer-authority "$ROOT/target/m8-keys/deploy.json" \
    | awk '/^Buffer:/ {print $2}' | tail -1)
  export SOAK_BUFFER="$BUFFER"
  echo "== [buffer] SOAK_BUFFER=$SOAK_BUFFER"
fi

# --- 6. the soak itself -----------------------------------------------------------
echo "== [soak] running devnet-soak client (standalone crate; evidence below)"
(
  cd scripts/devnet-soak
  SOAK_BUFFER="${SOAK_BUFFER:-}" SOAK_KEYS_DIR="$ROOT/target/m8-keys" \
    "$CARGO_HOME/bin/cargo" run --release
)

# --- 7. post-soak authority check --------------------------------------------------
echo "== [post] program authority AFTER soak (MUST be the Squads vault):"
solana program show "$PROGRAM_PUBKEY" | sed -n 's/^.*Authority: /  authority: /p'
echo "== [post] done — archive the tee'd log under target/soak-logs as evidence."
