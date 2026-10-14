# Tributary

**Small streams. Pooled power.**

Self-hostable AI token pools — govern your org's AI usage with per-member keys,
quotas, and leak detection; or deposit surplus credits into a Solana-settled
marketplace.

> **Status:** pre-alpha scaffold (milestone M0). The gateway, pools, and
> dashboard land over the coming milestones — see [`SPEC.md`](SPEC.md) §14
> for the roadmap. Real quickstart docs arrive with M5.

## Repository layout

```
crates/            Rust workspace (gateway, control, ledger, crypto, common)
apps/dashboard/    Next.js + TypeScript dashboard (pnpm workspace)
```

## Developing

Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Dashboard:

```sh
pnpm install
pnpm -C apps/dashboard build
```

CI (`.github/workflows/ci.yml`) runs all of the above plus `cargo-deny`.

## License

Copyright 2026 Tran Dinh Hong Phat. Licensed under the
[Functional Source License, Version 1.1, ALv2 Future License](LICENSE)
(FSL-1.1-ALv2). Each version converts to Apache-2.0 two years after it is
made available.
