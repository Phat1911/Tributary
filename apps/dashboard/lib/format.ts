// Display formatters shared by every table/card. Epoch millis in, short
// human strings out — no date library (workflow step 10).

const dateTimeFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

/** "Oct 17, 2026, 9:41 PM" from epoch millis. */
export function formatDateTime(ms: number): string {
  return dateTimeFormat.format(new Date(ms));
}

/** "Oct 17, 2026" from epoch millis. */
export function formatDate(ms: number): string {
  return dateFormat.format(new Date(ms));
}

/** Compact token counts: 1.2k / 3.4M. */
export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** Like formatTokens but drops a trailing ".0" ("100k", not "100.0k") —
 *  used where two counts sit side by side ("12.4k / 100k") and the extra
 *  zeros only add width. */
export function formatTokensCompact(n: number): string {
  return formatTokens(n).replace(/\.0(?=[kM]$)/, "");
}

/** Money stays micro-USD integers end-to-end; format only at the edge. */
export function formatMicroUsd(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(micros % 1_000_000 === 0 ? 0 : 2)}`;
}

/** Thousands separators by string surgery ("1234567" → "1,234,567").
 *  Unlike toLocaleString(), the result is the same in every browser
 *  locale — a money figure must not change shape with the viewer. */
function groupThousands(digits: string): string {
  return digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** The exact integer behind a dollar figure, for hover text:
 *  "2,850 micro-USD". (The off-chain books are denominated in micro-USD;
 *  USDC is the on-chain payout token, never this table's unit — D40.) */
export function formatMicroUsdRaw(micros: number): string {
  return Number.isSafeInteger(micros)
    ? `${groupThousands(String(micros))} micro-USD`
    : "not a whole number of micro-USD";
}

/**
 * Exact micro-USD → dollars, for surfaces where cents are too coarse.
 * `formatMicroUsd` rounds to cents — right for a usage table, wrong for
 * settlement: one real request burns ≈ 2,850 micro-USD, which is "$0.00"
 * at cent precision. Here nothing is rounded: BigInt integer math only,
 * never a float (a float used for money is a security finding). Trailing
 * zeros are trimmed but never past the cent:
 * 2_850 → "$0.00285" · 5_000_000 → "$5.00" · 1_500_000 → "$1.50".
 *
 * A value that is not a safe integer breaks the wire contract (whole
 * micro-units); it renders as "—" rather than a silently wrong figure.
 */
export function formatMicroUsdExact(micros: number): string {
  if (!Number.isSafeInteger(micros)) return "—";
  // BigInt, not `micros / 1_000_000`: house rule — no float ever touches
  // a money value. Every digit below comes out of integer division, so
  // the string is exact by construction. (No `n` literals on purpose:
  // this file targets ES2017, where `1_000_000n` does not parse.)
  const digits = BigInt(Math.abs(micros)).toString().padStart(7, "0");
  const dollars = groupThousands(digits.slice(0, -6));
  // "002850" → "00285" · "500000" → "5" → "50" · "000000" → "" → "00".
  const cents = digits.slice(-6).replace(/0+$/, "").padEnd(2, "0");
  return `${micros < 0 ? "-" : ""}$${dollars}.${cents}`;
}
