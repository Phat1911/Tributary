# DevDay Exchange — demo scenario & Q&A pocket (2026-11-07, Da Nang)

> Event: DevDay Exchange Community (OpenAI/Codex build session), Nov 7 2026, 14:00–20:00,
> Hai Chau, Da Nang. Format: build all afternoon → demo (WIP welcome) → attendee voting
> picks top 5. Hosts include OpenAI DX staff. English lines; VN for after-chat.
> Frozen 2026-11-03. M9 rides as a STORY, not a live leg (director ruling, founder locked).

## The frozen demo shape

| Beat | Time | Form |
|---|---|---|
| 1. Hook | 30s | "Who here shares an API key with a team… through a group chat?" |
| 2. V1 live | 2 min | member key → completion → burst 429 → D32 flag from phone (mobile data, ts.net) → audit log |
| 3. Codex beat | 90s | Codex has been drinking from the pool ALL AFTERNOON — today's usage graph is this session; the event's own API credits are the electricity |
| 4. M8 vault live | 90s | devnet: record_contribution (vault stays 0) → prepay/refund round-trip → two-door burn → Solscan tabs |
| 5. M9 story | 60s | the RED CI run on screen: 13 failing tests named as doctrine sentences, written before any code |
| 6. Thesis | 15s | "My product governs how agents spend tokens; my process governs how agents build the product. Verifiable rules instead of trust." |
| 7. Close | 15s | repo (profile link + event chat), "star it or bring me your team's key problem" |

Fallback ladder per beat: live → recorded clip → screenshot. Solscan tabs pre-opened.

## Beat 5 — the M9 story, spoken

> "This week's milestone started as thirteen failing tests. Each one is named after a rule —
> 'a depositor earns nothing in a period where their credits burned nothing.' No code existed.
> A fresh agent session is building against that spec right now, and the red run is public —
> you can watch it turn green in the repo. That's how I build with agents: the rules are
> written down first, as tests, and the machines have to satisfy them."

(If M9 happens to be green + recorded by Nov 6: optional 20s recorded clip of the settle leg —
zero effort chasing it; drop without regret.)

## Q&A pocket — the RWA question (EXPECTED)

**Q: "Isn't this just tokenizing AI credits — how do you solve the RWA hard problem, the off-chain trust?"**

Three lines:

1. "The hard part of RWA is the chain can't see the off-chain asset. Same problem here — my
   credits are provider accounts, off-chain. You can't solve that at mint time; you cage it."
2. "My cage has four walls: fake claims PAY nothing — earnings follow actual metered burns;
   every mint is a public event naming its signer; books and chain must reconcile or alarms
   fire; and settlement is a Merkle tree anyone can challenge — proven fraud slashes a bond."
3. "And our oracle is easier than gold's: credits are digital, and the providers' own usage
   dashboards are third-party truth any depositor can check. Stock is provable; flow is bonded."

Key line if pressed further: **"RWA brings assets on-chain to trade them; Tributary brings
obligations on-chain to settle them."** (Also the securities answer: receipts, not chips.)

## Q&A pocket — the other likely four

**"Why blockchain at all?"** → "The books decide who's owed; the chain is what makes them pay.
Without it, the operator holds the money AND writes the ledger — that's a relay with a
spreadsheet. With it, the worst an operator can be is slow, never a thief."

**"How is this different from new-api / relay hubs?"** → "Relays sell access with opaque
balances — trust the operator. We sell provable settlement: escrow, ceilings, challenge
windows. Obligations, not tokens."

**"Is the claim a security/token?"** → "It's a coat-check ticket: non-transferable record,
transfers refuse at the token layer. No sale, no market, no trading."

**"Who pays gas for walletless users?"** → "The operator's backend sponsors fees; users never
see a wallet. The chain is back-end plumbing by design." (Kora paymaster pattern.)

## Prep checklist

- [ ] Faucet-funded devnet at HEAD (DONE 2026-11-02 — receipts live)
- [ ] Codex→pool provider config tested AT HOME (base_url = gateway, sk-pool key)
- [ ] Phone mobile-data leg rehearsed ×2 (same laptop, same stack)
- [ ] Seeded v0.1.3 stack; v0.1.3 protected, no M9 dependency
- [ ] Red CI run bookmarked (m9/doctrine-suite branch, Actions)
- [ ] Solscan tabs pre-opened; event chat ready for repo link
- [ ] Claim $50 API credits at check-in → fund the provider key (the electricity beat)
