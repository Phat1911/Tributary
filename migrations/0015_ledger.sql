-- M7: double-entry ledger — SPEC §6 (chart_of_accounts, ledger_accounts,
-- ledger_entries) + the balances cache (entries are truth; the cache is a
-- derivative — drift must alarm, never silently win).
--
-- Portable SQL: runs identically on SQLite and PostgreSQL (TEXT/BIGINT only,
-- ids minted in Rust, no DB-level foreign keys — house style since 0001:
-- SQLite does not enforce FKs unless each connection sets PRAGMA
-- foreign_keys=ON, so a REFERENCES clause would be decorative on one engine
-- and load-bearing on the other; referential integrity is enforced inside
-- the posting transaction instead, where accounts are resolved before any
-- entry insert).
--
-- Append-only enforcement (rule 3) is ENGINE-SPECIFIC and therefore cannot
-- live in this file (sqlx runs every migration file on BOTH engines):
--   * SQLite: BEFORE UPDATE / BEFORE DELETE triggers with RAISE(ABORT)
--   * PostgreSQL: a non-owner app role holding only SELECT+INSERT
-- Both are applied by `run_ledger_guards` in crates/common/src/db.rs,
-- called from `run_migrations` on every start — the sanctioned home for
-- DB-specific SQL per AGENTS.md.

-- The normative chart of accounts. `code` matches the `AccountCode` enum in
-- crates/ledger/src/chart.rs exactly — a property test fails on drift.
-- Books are kept from the platform-operator perspective (SPEC §6).
-- allowed_counterparties: JSON array of account codes this account may face
-- inside a posting-rule template leg pair (defense in depth — templates are
-- Rust consts, and the chart independently constrains which pairs are legal).
CREATE TABLE IF NOT EXISTS chart_of_accounts (
    code                   TEXT PRIMARY KEY,  -- enum code, e.g. 'cash.escrow' — never freeform names (SPEC §6)
    account_type           TEXT NOT NULL CHECK (account_type IN
                             ('asset', 'liability', 'equity', 'revenue', 'expense')),
    definition             TEXT NOT NULL,     -- what the account means (teaching + audit trail)
    allowed_counterparties TEXT NOT NULL      -- JSON array; validated in Rust on every post
);

-- One row per (account, owner). No freeform names: every account resolves to
-- a chart code, so the shape of the books is fixed by the seed above.
CREATE TABLE IF NOT EXISTS ledger_accounts (
    id         TEXT PRIMARY KEY,              -- 128-bit hex, minted in Rust (portable TEXT pk, house style)
    code       TEXT NOT NULL,                 -- → chart_of_accounts.code
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('renter', 'depositor', 'org', 'operator')),
    owner_id   TEXT NOT NULL,                 -- e.g. users.id / future renter id; 'platform' for operator-level accounts
    currency   TEXT NOT NULL DEFAULT 'USD',   -- micro-units are micro-USD today; column keeps the unit honest
    created_at BIGINT NOT NULL,               -- unix epoch millis
    UNIQUE (code, owner_kind, owner_id, currency)
);

-- The append-only truth. Every row was born from a posting-rule template
-- (template_code names which one) — no free-form inserts exist anywhere in
-- the codebase; UPDATE and DELETE are refused by the database itself (rule 3).
-- amount is ALWAYS a positive integer of micro-units (10^-6 USD); the debit/
-- credit direction carries the sign — a zero, negative, or float amount is a
-- schema/typing error, not a ledger state.
CREATE TABLE IF NOT EXISTS ledger_entries (
    id            TEXT PRIMARY KEY,
    txn_id        TEXT NOT NULL,              -- groups the legs of ONE balanced transaction
    seq           BIGINT NOT NULL,            -- leg ordinal within the transaction (stable ordering)
    template_code TEXT NOT NULL,              -- the posting rule that created this entry (the only path)
    account_id    TEXT NOT NULL,              -- → ledger_accounts.id
    amount        BIGINT NOT NULL CHECK (amount > 0),
    direction     TEXT NOT NULL CHECK (direction IN ('debit', 'credit')),
    ref           TEXT NOT NULL DEFAULT '',   -- external reference (e.g. checkout/session id) — never required
    ts            BIGINT NOT NULL             -- unix epoch millis
);
CREATE INDEX IF NOT EXISTS idx_ledger_entries_txn     ON ledger_entries(txn_id);
CREATE INDEX IF NOT EXISTS idx_ledger_entries_account ON ledger_entries(account_id);

-- Cached derivative of the entries above (SPEC M7: "balances are cache,
-- entries are truth"). balance is SIGNED as Σdebits − Σcredits:
--   asset/expense accounts are debit-normal  → healthy balance ≥ 0
--   liability/equity/revenue                 → healthy balance ≤ 0
-- The reconciler recomputes from entries and alarms on any difference.
CREATE TABLE IF NOT EXISTS ledger_account_balances (
    account_id TEXT PRIMARY KEY,              -- → ledger_accounts.id
    balance    BIGINT NOT NULL DEFAULT 0,     -- signed Σ(debits − credits), micro-units
    updated_at BIGINT NOT NULL
);

-- Normative chart seed (mirrors crates/ledger/src/chart.rs; drift ⇒ test).
-- Rule-7 doctrine (director rulings 2026-10-28):
--   * Σ payable = custodial-cash debts only: payable.depositor + prepaid.renter.
--     Contributed principal rides in claims.contributed OUTSIDE the equation
--     (capacity-backed, governed by the §11 withdrawal liquidity buffer).
--   * The operator fee credits equity.operator — the operator is the
--     accounting entity and does not owe itself custodial debt.
INSERT INTO chart_of_accounts (code, account_type, definition, allowed_counterparties) VALUES
('cash.escrow', 'asset',
 'Custodial cash backing renter prepaid balances and settled depositor payables. The rule-7 numerator: cash.escrow >= Σ payable + Σ prepaid.',
 '["prepaid.renter","payable.depositor"]'),
('capacity.contributed', 'asset',
 'Credited capacity value of depositor-contributed provider keys; backs claims.contributed. Key-death markdown lands with M9.',
 '["claims.contributed"]'),
('prepaid.renter', 'liability',
 'Renter custodial prepaid balances — the rule-7 Σ prepaid term. Consumed by metered usage; never revenue until earned (rule 7).',
 '["cash.escrow","revenue.usage"]'),
('payable.depositor', 'liability',
 'Settled depositor earnings owed in cash — the rule-7 Σ payable term (custodial-cash debts only, director ruling 2026-10-28).',
 '["revenue.usage","cash.escrow"]'),
('claims.contributed', 'liability',
 'Depositor contributed-principal claims — capacity-backed, NOT cash-backed; deliberately outside rule 7, governed by the §11 withdrawal liquidity buffer (director ruling 2026-10-28).',
 '["capacity.contributed"]'),
('equity.operator', 'equity',
 'Operator capital and fees taken. The operator’s own claim on the books — shares are ledger claims, never tokens (rule 9).',
 '["revenue.usage"]'),
('revenue.usage', 'revenue',
 'Metered pool usage revenue. At settlement splits into the operator fee (→ equity.operator) and the distributable (→ payable.depositor).',
 '["prepaid.renter","equity.operator","payable.depositor"]');
