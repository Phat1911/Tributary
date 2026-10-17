# Tributary dashboard

Next.js (App Router) + Tailwind CSS v4 + shadcn/ui control-surface for the
Tributary gateway. Browser sessions ride a secure HttpOnly cookie set by
`POST /api/v1/auth/login`.

## Local development

The dashboard is a pure client of the single Rust binary — start `serve`
first, then the dev server (it proxies `/api/*` to `127.0.0.1:8080`):

```bash
# terminal 1 — the binary (DB URL + 64-hex master key)
TRIBUTARY_DATABASE_URL=sqlite://tributary.db \
TRIBUTARY_MASTER_KEY=$(openssl rand -hex 32) \
cargo run -p tributary-gateway -- serve

# terminal 2 — first run: bootstrap the host admin, then the UI
cargo run -p tributary-gateway -- create-admin --email you@example.com --password '…'
pnpm -C apps/dashboard dev        # http://localhost:3000
```

Override the upstream with `TRIBUTARY_API_URL` (default
`http://127.0.0.1:8080`).

## Dev note: the `Secure` cookie on localhost

The session cookie is always sent `Secure`. Modern browsers honor `Secure`
on `http://localhost` (it is a trustworthy context), so login just works in
dev. **If login ever "silently fails" locally — page reloads to /login with
no error — check the `Secure`-on-plain-HTTP assumption first** (e.g. a
non-localhost hostname, an old browser) before debugging anything deeper.
That symptom means the browser refused to store the cookie.

## Checks

```bash
pnpm -C apps/dashboard lint       # ESLint (flat config)
pnpm -C apps/dashboard typecheck  # tsc --noEmit
pnpm -C apps/dashboard build      # production build (CI runs all three)
```
