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
