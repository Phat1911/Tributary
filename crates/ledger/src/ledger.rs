//! `LedgerTrait` — the public surface of the double-entry ledger — and
//! `SqlLedger`, its implementation over the portable `common::Db` (SQLite
//! and PostgreSQL through one code path).
//!
//! Design notes you should not have to reverse-engineer:
//!
//! * **Trait shape:** methods return `impl Future + Send` explicitly rather
//!   than `async fn`. Rust's native async-in-trait does not promise `Send`
//!   futures, but axum handlers (the M10+ callers) require them; writing the
//!   explicit form now means swapping backends (SPEC §11: Solana arrives
//!   behind this same trait in M8) never touches call sites.
//! * **Concurrency without row locks:** posting never does read-modify-write
//!   on balances. Cache updates are atomic SQL increments
//!   (`SET balance = balance + ?`), and legs that DRAW DOWN an account carry
//!   their sufficiency check inside the WHERE clause — so two concurrent
//!   posts race on one conditional UPDATE per leg, on both engines, with no
//!   `FOR UPDATE` (which SQLite does not support).
//! * **Balances are caches, entries are truth:** every read API that matters
//!   (trial balance, solvency) recomputes from `ledger_entries`; the cache
//!   exists for fast account lookups, and [`Ledger::reconcile`] alarms on
//!   any disagreement.

use crate::chart::{
    AccountCode, AccountType, Direction, OwnerKind, normal_balance, parse_counterparties,
};
use crate::error::LedgerError;
use crate::money::MicroUsd;
use crate::posting::{self, PostingRequest, TemplateCode};
use sqlx::Row;
use std::collections::HashMap;
use std::future::Future;
use std::str::FromStr;
use tributary_common::db::{Db, new_id, now_ms};

/// A posting that landed: the transaction id under which its legs are
/// grouped (the append-only audit anchor) plus what was posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostingReceipt {
    pub txn_id: String,
    pub template: TemplateCode,
    pub amount: MicroUsd,
}

/// One account's balance from the cache, in BOTH conventions:
/// `signed` is Σdebits − Σcredits (how the DB stores it); `normal` is the
/// accountant's view (positive = the account sits on its healthy side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    pub account_id: String,
    pub code: AccountCode,
    pub account_type: AccountType,
    pub signed: MicroUsd,
    pub normal: MicroUsd,
}

/// Global trial balance: the check that the whole book still balances.
/// `balanced` false means entries were corrupted or bypassed — High event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrialBalance {
    pub total_debits: MicroUsd,
    pub total_credits: MicroUsd,
}

impl TrialBalance {
    /// True when Σdebits == Σcredits across every entry ever written.
    pub fn balanced(&self) -> bool {
        self.total_debits == self.total_credits
    }
}

/// Rule-7 report (SPEC §11): does custodial cash cover custodial debt?
/// `headroom` = escrow − (payable + prepaid); negative is a High finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolvencyReport {
    pub escrow: MicroUsd,
    pub payable: MicroUsd,
    pub prepaid: MicroUsd,
    pub headroom: MicroUsd,
}

/// The public ledger surface. Entries are created ONLY through
/// [`Ledger::post`] (posting-rule templates) — there is no entry-insert
/// method, by design and by rule 3.
pub trait Ledger: Send + Sync {
    /// Opens (idempotently) the account for `code` + owner and returns its
    /// id. Opening also creates the zero balance-cache row.
    fn open_account(
        &self,
        code: AccountCode,
        owner_kind: OwnerKind,
        owner_id: &str,
    ) -> impl Future<Output = Result<String, LedgerError>> + Send;

    /// The existing account id for a code + owner, if opened (`None` is a
    /// legal state — reading a never-opened account is not an error).
    fn account_id(
        &self,
        code: AccountCode,
        owner_kind: OwnerKind,
        owner_id: &str,
    ) -> impl Future<Output = Result<Option<String>, LedgerError>> + Send;

    /// Posts one transaction through a posting-rule template. Returns the
    /// txn id; on any validation failure NOTHING is written (single
    /// transaction, rolled back atomically).
    fn post(
        &self,
        posting: PostingRequest<'_>,
    ) -> impl Future<Output = Result<PostingReceipt, LedgerError>> + Send;

    /// One account's balance (cache-backed; truth-checked by `reconcile`).
    fn account_balance(
        &self,
        account_id: &str,
    ) -> impl Future<Output = Result<AccountBalance, LedgerError>> + Send;

    /// Σ of one chart code's normal-side balances across ALL owner accounts
    /// of that code (e.g. every renter's prepaid combined), recomputed from
    /// entries — the truth, not the cache.
    fn normal_side_by_code(
        &self,
        code: AccountCode,
    ) -> impl Future<Output = Result<MicroUsd, LedgerError>> + Send;

    /// Σdebits vs Σcredits over every entry (from entries, not the cache).
    fn trial_balance(&self) -> impl Future<Output = Result<TrialBalance, LedgerError>> + Send;

    /// Recomputes every balance from entries and compares with the cache.
    /// Also asserts the global trial balance. ANY disagreement is an error
    /// (drift ⇒ alarm, never a silent re-cache — AGENTS.md ledger rule).
    fn reconcile(&self) -> impl Future<Output = Result<(), LedgerError>> + Send;

    /// The rule-7 numbers without judgment (for dashboards/nightly reports).
    fn solvency_report(&self) -> impl Future<Output = Result<SolvencyReport, LedgerError>> + Send;

    /// The rule-7 CHECK: errors loudly on violation (never a warning).
    fn solvency_check(&self) -> impl Future<Output = Result<SolvencyReport, LedgerError>> + Send;
}

/// The SQL implementation of [`Ledger`] over the portable `Db` handle
/// (SQLite and PostgreSQL, one code path — dual-DB rule). Not `Debug`:
/// `Db` wraps live pools and has no debug form.
#[derive(Clone)]
pub struct SqlLedger {
    db: Db,
}

impl SqlLedger {
    /// Wraps a connected, migrated database.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// The underlying portable handle (read access for callers that need
    /// to co-locate queries with the ledger — e.g. future control-plane
    /// listing endpoints).
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Reads the whole chart from the DB and parses it into typed form.
    /// Serves two purposes at post time: counterparty validation, and a
    /// live drift check (a DB row the code enum doesn't know — or a type
    /// mismatch — refuses the posting instead of guessing).
    async fn chart_from_db(
        &self,
    ) -> Result<HashMap<AccountCode, (AccountType, Vec<AccountCode>)>, LedgerError> {
        let sql = self
            .db
            .sql("SELECT code, account_type, allowed_counterparties FROM chart_of_accounts");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(self.db.pool())
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            let code_str: String = row.try_get("code")?;
            let type_str: String = row.try_get("account_type")?;
            let counterparties_str: String = row.try_get("allowed_counterparties")?;
            let code = AccountCode::from_str(&code_str)?;
            let account_type = AccountType::from_str(&type_str)?;
            if account_type != code.account_type() {
                // The DB row claims a different type than the normative
                // enum mapping — the seed was hand-edited. Refuse loudly.
                return Err(LedgerError::UnknownChartCode(format!(
                    "{code_str}: DB says {type_str}, code says {}",
                    code.account_type().as_str()
                )));
            }
            map.insert(
                code,
                (account_type, parse_counterparties(&counterparties_str)?),
            );
        }
        Ok(map)
    }

    /// Resolves one expanded leg's account id inside the caller's
    /// transaction. No FOR UPDATE is needed: the cache's conditional
    /// UPDATE (not this read) is the serialization point for writes.
    async fn resolve_account(
        tx: &mut sqlx::AnyConnection,
        db: &Db,
        leg: &posting::ExpandedLeg,
    ) -> Result<String, LedgerError> {
        let sql = db.sql(
            "SELECT id FROM ledger_accounts \
             WHERE code = ? AND owner_kind = ? AND owner_id = ? AND currency = 'USD'",
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(leg.account.as_str())
            .bind(leg.owner_kind.as_str())
            .bind(&leg.owner_id)
            .fetch_optional(&mut *tx)
            .await?;
        match row {
            Some(row) => {
                let id: String = row.try_get("id")?;
                Ok(id)
            }
            None => Err(LedgerError::AccountNotFound {
                code: leg.account.as_str().to_string(),
                owner_kind: leg.owner_kind.as_str().to_string(),
                owner_id: leg.owner_id.clone(),
            }),
        }
    }
}

impl Ledger for SqlLedger {
    async fn open_account(
        &self,
        code: AccountCode,
        owner_kind: OwnerKind,
        owner_id: &str,
    ) -> Result<String, LedgerError> {
        let mut tx = self.db.pool().begin().await?;
        // ON CONFLICT DO NOTHING makes this idempotent: opening the same
        // (code, owner) twice returns the SAME account, never a duplicate.
        let insert = self.db.sql(
            "INSERT INTO ledger_accounts (id, code, owner_kind, owner_id, currency, created_at) \
             VALUES (?, ?, ?, ?, 'USD', ?) \
             ON CONFLICT (code, owner_kind, owner_id, currency) DO NOTHING",
        );
        sqlx::query(sqlx::AssertSqlSafe(insert.as_str()))
            .bind(new_id())
            .bind(code.as_str())
            .bind(owner_kind.as_str())
            .bind(owner_id)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
        // The balances cache row is born at zero, together with the account.
        let select = self.db.sql(
            "SELECT id FROM ledger_accounts \
             WHERE code = ? AND owner_kind = ? AND owner_id = ? AND currency = 'USD'",
        );
        let id: String = sqlx::query(sqlx::AssertSqlSafe(select.as_str()))
            .bind(code.as_str())
            .bind(owner_kind.as_str())
            .bind(owner_id)
            .fetch_one(&mut *tx)
            .await?
            .try_get("id")?;
        let cache = self.db.sql(
            "INSERT INTO ledger_account_balances (account_id, balance, updated_at) \
             VALUES (?, 0, ?) \
             ON CONFLICT (account_id) DO NOTHING",
        );
        sqlx::query(sqlx::AssertSqlSafe(cache.as_str()))
            .bind(&id)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    async fn account_id(
        &self,
        code: AccountCode,
        owner_kind: OwnerKind,
        owner_id: &str,
    ) -> Result<Option<String>, LedgerError> {
        let sql = self.db.sql(
            "SELECT id FROM ledger_accounts \
             WHERE code = ? AND owner_kind = ? AND owner_id = ? AND currency = 'USD'",
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(code.as_str())
            .bind(owner_kind.as_str())
            .bind(owner_id)
            .fetch_optional(self.db.pool())
            .await?;
        match row {
            Some(row) => {
                let id: String = row.try_get("id")?;
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    async fn post(&self, request: PostingRequest<'_>) -> Result<PostingReceipt, LedgerError> {
        // 1. Expand + validate against the template (pure, no DB).
        let legs = posting::expand(&request)?;
        // 2. Load the chart and validate leg pairs against the normative
        //    allowed-counterparties — a template nobody blessed cannot fire.
        let chart_db = self.chart_from_db().await?;
        for leg in &legs {
            let (_, allowed) = chart_db
                .get(&leg.account)
                .ok_or_else(|| LedgerError::UnknownChartCode(leg.account.as_str().to_string()))?;
            let counterpart = legs
                .iter()
                .find(|other| other.account != leg.account)
                .map(|other| other.account);
            if let Some(counterpart) = counterpart
                && !allowed.contains(&counterpart)
            {
                return Err(LedgerError::UnknownChartCode(format!(
                    "{} may not face {} (chart counterparties)",
                    leg.account.as_str(),
                    counterpart.as_str()
                )));
            }
        }

        // 3. One transaction: entries + cache move together or not at all.
        let mut tx = self.db.pool().begin().await?;
        let txn_id = new_id();
        let ts = now_ms();
        for (seq, leg) in legs.iter().enumerate() {
            let account_id = Self::resolve_account(&mut tx, self.db(), leg).await?;
            let entry = self.db.sql(
                "INSERT INTO ledger_entries \
                 (id, txn_id, seq, template_code, account_id, amount, direction, ref, ts) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            );
            sqlx::query(sqlx::AssertSqlSafe(entry.as_str()))
                .bind(new_id())
                .bind(&txn_id)
                .bind((seq + 1) as i64)
                .bind(request.template.as_str())
                .bind(&account_id)
                .bind(request.amount.micros())
                .bind(leg.direction.as_str())
                .bind(request.reference)
                .bind(ts)
                .execute(&mut *tx)
                .await?;

            // 4. Cache update. A leg that DRAWS DOWN an account (a debit on
            // a credit-normal account, or a credit on a debit-normal one)
            // carries its sufficiency check inside the WHERE clause: the
            // row only moves if the balance covers the amount. Atomic on
            // both engines — this is the race-free replacement for
            // "SELECT balance, check, UPDATE" (which two concurrent posts
            // could interleave into an overdraw).
            let reduces_normal_side = normal_balance(leg.account.account_type()) != leg.direction;
            let signed_delta = match leg.direction {
                Direction::Debit => request.amount.micros(),
                Direction::Credit => -request.amount.micros(),
            };
            let update = if reduces_normal_side {
                let guard = match normal_balance(leg.account.account_type()) {
                    // credit-normal drawn by a debit: stored signed is ≤ 0,
                    // normal-side = -balance, so the guard is -balance ≥ amt
                    Direction::Credit => "-balance >= ?",
                    // debit-normal drawn by a credit: guard is balance ≥ amt
                    Direction::Debit => "balance >= ?",
                };
                self.db.sql(&format!(
                    "UPDATE ledger_account_balances \
                     SET balance = balance + ?, updated_at = ? \
                     WHERE account_id = ? AND {guard}"
                ))
            } else {
                self.db.sql(
                    "UPDATE ledger_account_balances \
                     SET balance = balance + ?, updated_at = ? \
                     WHERE account_id = ?",
                )
            };
            let query = sqlx::query(sqlx::AssertSqlSafe(update.as_str()))
                .bind(signed_delta)
                .bind(ts);
            // Bind order must match the SQL text: delta, ts, account_id,
            // then (guarded form only) the amount for the WHERE guard.
            if reduces_normal_side {
                let result = query
                    .bind(account_id.as_str())
                    .bind(request.amount.micros())
                    .execute(&mut *tx)
                    .await?;
                if result.rows_affected() == 0 {
                    // Guard refused: best-effort diagnostic (current balance
                    // in the still-open tx), then the drop = rollback.
                    let current: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(
                        self.db
                            .sql("SELECT balance FROM ledger_account_balances WHERE account_id = ?")
                            .as_str(),
                    ))
                    .bind(account_id.as_str())
                    .fetch_optional(&mut *tx)
                    .await?;
                    let available =
                        current.map(|b| match normal_balance(leg.account.account_type()) {
                            Direction::Debit => b,
                            Direction::Credit => -b,
                        });
                    return Err(LedgerError::Insufficient {
                        account: format!("{}:{}", leg.account.as_str(), leg.owner_id),
                        needed: request.amount.micros(),
                        available: available.unwrap_or(0),
                    });
                }
            } else {
                let result = query.bind(account_id.as_str()).execute(&mut *tx).await?;
                if result.rows_affected() == 0 {
                    // The cache row is born in open_account; a missing row
                    // here means cache/account drift — loud, never a silent
                    // re-create (the reconciler owns cache repair policy).
                    return Err(LedgerError::BalanceDrift {
                        accounts: vec![account_id.clone()],
                    });
                }
            }
        }
        tx.commit().await?;
        Ok(PostingReceipt {
            txn_id,
            template: request.template,
            amount: request.amount,
        })
    }

    async fn account_balance(&self, account_id: &str) -> Result<AccountBalance, LedgerError> {
        let sql = self.db.sql(
            "SELECT a.code, b.balance \
             FROM ledger_accounts a JOIN ledger_account_balances b ON b.account_id = a.id \
             WHERE a.id = ?",
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(account_id)
            .fetch_one(self.db.pool())
            .await?;
        let code_str: String = row.try_get("code")?;
        let signed: i64 = row.try_get("balance")?;
        let code = AccountCode::from_str(&code_str)?;
        let account_type = code.account_type();
        let normal = match normal_balance(account_type) {
            Direction::Debit => signed,
            Direction::Credit => -signed,
        };
        Ok(AccountBalance {
            account_id: account_id.to_string(),
            code,
            account_type,
            signed: MicroUsd::new(signed),
            normal: MicroUsd::new(normal),
        })
    }

    async fn normal_side_by_code(&self, code: AccountCode) -> Result<MicroUsd, LedgerError> {
        // Recomputed from ENTRIES (truth), not the cache: sums that feed
        // solvency must survive a tampered cache.
        let sql = self.db.sql(
            "SELECT CAST(COALESCE(SUM(\
                CASE WHEN e.direction = 'debit' THEN e.amount ELSE -e.amount END\
            ), 0) AS BIGINT) AS signed \
             FROM ledger_accounts a LEFT JOIN ledger_entries e ON e.account_id = a.id \
             WHERE a.code = ?",
        );
        let signed: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(code.as_str())
            .fetch_one(self.db.pool())
            .await?;
        let normal = match normal_balance(code.account_type()) {
            Direction::Debit => signed,
            Direction::Credit => -signed,
        };
        Ok(MicroUsd::new(normal))
    }

    async fn trial_balance(&self) -> Result<TrialBalance, LedgerError> {
        let sql = self.db.sql(
            "SELECT \
                CAST(COALESCE(SUM(CASE WHEN direction = 'debit'  THEN amount ELSE 0 END), 0) AS BIGINT) AS debits, \
                CAST(COALESCE(SUM(CASE WHEN direction = 'credit' THEN amount ELSE 0 END), 0) AS BIGINT) AS credits \
             FROM ledger_entries",
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_one(self.db.pool())
            .await?;
        Ok(TrialBalance {
            total_debits: MicroUsd::new(row.try_get("debits")?),
            total_credits: MicroUsd::new(row.try_get("credits")?),
        })
    }

    async fn reconcile(&self) -> Result<(), LedgerError> {
        // (a) Global trial balance from the entries themselves.
        let trial = self.trial_balance().await?;
        if !trial.balanced() {
            return Err(LedgerError::TrialBalanceViolation {
                debits: trial.total_debits.micros(),
                credits: trial.total_credits.micros(),
            });
        }
        // (b) Per-account: truth (Σ from entries) vs cache. GROUP BY
        // includes b.balance for Postgres's grouping rules; it is
        // functionally dependent on a.id (the balances PK), so grouping is
        // still one row per account.
        let sql = self.db.sql(
            "SELECT a.id, \
                CAST(COALESCE(SUM(\
                    CASE WHEN e.direction = 'debit' THEN e.amount ELSE -e.amount END\
                ), 0) AS BIGINT) AS truth, \
                CAST(COALESCE(b.balance, 0) AS BIGINT) AS cache \
             FROM ledger_accounts a \
             LEFT JOIN ledger_entries e ON e.account_id = a.id \
             LEFT JOIN ledger_account_balances b ON b.account_id = a.id \
             GROUP BY a.id, b.balance",
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(self.db.pool())
            .await?;
        let drifted: Vec<String> = rows
            .into_iter()
            .filter_map(|row| {
                let id: String = row.try_get("id").ok()?;
                let truth: i64 = row.try_get("truth").ok()?;
                let cache: i64 = row.try_get("cache").ok()?;
                (truth != cache).then_some(id)
            })
            .collect();
        if !drifted.is_empty() {
            return Err(LedgerError::BalanceDrift { accounts: drifted });
        }
        Ok(())
    }

    async fn solvency_report(&self) -> Result<SolvencyReport, LedgerError> {
        let escrow = self.normal_side_by_code(AccountCode::CashEscrow).await?;
        let payable = self
            .normal_side_by_code(AccountCode::PayableDepositor)
            .await?;
        let prepaid = self.normal_side_by_code(AccountCode::PrepaidRenter).await?;
        let owed = payable.checked_add(prepaid)?;
        let headroom = escrow.checked_sub(owed)?;
        Ok(SolvencyReport {
            escrow,
            payable,
            prepaid,
            headroom,
        })
    }

    async fn solvency_check(&self) -> Result<SolvencyReport, LedgerError> {
        let report = self.solvency_report().await?;
        if report.headroom.micros() < 0 {
            return Err(LedgerError::SolvencyViolated {
                escrow: report.escrow.micros(),
                payable: report.payable.micros(),
                prepaid: report.prepaid.micros(),
                short: -report.headroom.micros(),
            });
        }
        Ok(report)
    }
}
