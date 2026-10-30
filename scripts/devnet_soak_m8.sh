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
balance() { solana balance --url https://api.devnet.solana.com | awk '{print $1}'; }
for i in $(seq 1 6); do
  BAL=$(solana balance | awk '{print $1}')
  OK=$(python3 -c "print(1 if float('$BAL') >= 4.0 else 0)")
  [ "$OK" = "1" ] && break
  echo "== [fund] balance=${BAL} SOL — airdropping 2 SOL (attempt $i)"
  solana airdrop 2 || sleep 20
done
BAL=$(solana balance | awk '{print $1}')
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

# --- 4. USDC for the depositor (Circle faucet) ---------------------------------
DEPOSITOR_PUBKEY="$(solana-keygen pubkey target/m8-keys/depositor.json)"
USDC_MINT="4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
usdc_balance() {
  spl-token balance --address "$(spl-token address --token "$USDC_MINT" --owner "$DEPOSITOR_PUBKEY" 2>/dev/null | tail -1)" \
    --url https://api.devnet.solana.com 2>/dev/null | awk '{print $1}'
}
echo "== [usdc] depositor wallet: $DEPOSITOR_PUBKEY"
echo "== [usdc] try Circle's devnet faucet API (best effort)…"
curl -sS -X POST https://faucet.circle.com/api/devnet-uxdc/faucet \
  -H 'Content-Type: application/json' \
  -d "{\"address\": \"$DEPOSITOR_PUBKEY\", \"chain\": \"SOL-DEVNET\"}" || true
echo
# Poll for the ATA balance a few times (faucet is async), then only fall
# back to the manual browser step if it really did not land.
for i in 1 2 3 4 5 6; do
  sleep 15
  UBAL="$(usdc_balance)"
  echo "== [usdc] depositor USDC balance: ${UBAL:-0}"
  if [ -n "${UBAL:-}" ] && python3 -c "import sys; sys.exit(0 if float('$UBAL') >= 25.0 else 1)"; then
    echo "== [usdc] faucet landed (${UBAL} USDC)"; break
  fi
  if [ "$i" = "6" ]; then
    echo "== [usdc] faucet never landed. Open https://faucet.circle.com in a browser,"
    echo "== [usdc] pick USDC on Solana Devnet, send 50 USDC to $DEPOSITOR_PUBKEY,"
    echo "== [usdc] then re-run this script (it is re-runnable; earlier steps skip)."
    exit 1
  fi
done

# --- 5. upgrade buffer for the multisig-upgrade proof ---------------------------
if [ -z "${SOAK_BUFFER:-}" ]; then
  echo "== [buffer] writing a fresh buffer of the SAME .so (upgrade proof needs it)"
  BUFFER=$(solana program write-buffer "$ROOT/target/deploy/pool.so" \
    --url https://api.devnet.solana.com \
    --buffer-authority "$DEPLOY_PUBKEY" \
    --output json | python3 -c 'import json,sys; print(json.load(sys.stdin)["buffer"])')
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
