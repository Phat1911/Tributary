#!/usr/bin/env bash
# Tributary repo bootstrap — run once from the repo root:  bash scripts/bootstrap_repo.sh
set -euo pipefail

# 1. init (idempotent) with main branch
if [ ! -d .git ]; then
  git init -b main 2>/dev/null || { git init; git symbolic-ref HEAD refs/heads/main; }
fi

# 2. identity fallback (uses existing global config if set)
git config user.name  >/dev/null 2>&1 || git config user.name "Phat1911"
git config user.email >/dev/null 2>&1 || git config user.email "Phat1911@users.noreply.github.com"

# 3. first commit
git add -A
git commit -m "chore: bootstrap Tributary — SPEC v0.3 + lifecycle docs (AGENTS/PLAN/AUDIT)" || echo "(nothing to commit)"

# 4. normalize branch name (git may default to master) + remote + push
git branch -M main
git remote add origin https://github.com/Phat1911/Tributary.git 2>/dev/null \
  || git remote set-url origin https://github.com/Phat1911/Tributary.git
git push -u origin main && echo "✅ pushed to github.com/Phat1911/Tributary" \
  || echo "❌ push failed — check credentials (gh auth login / SSH), then: git push -u origin main"
