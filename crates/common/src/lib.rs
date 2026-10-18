//! Shared foundation used by every Tributary crate.
//!
//! - [`db`]: the portable sqlx-`Any` adapter (one query path for SQLite AND
//!   PostgreSQL — AGENTS.md DB convention). All DB-specific SQL lives there.
//! - [`keys`]: pool-key material handling (generation + hashing).
//! - [`net`]: client-IP extraction and CIDR allowlist matching (M4, SPEC §9).
//! - [`time`]: UTC period math for the quota engine and RFC 3339 formatting.

pub mod db;
pub mod keys;
pub mod net;
pub mod time;
