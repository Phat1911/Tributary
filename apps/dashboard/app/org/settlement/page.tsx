// Settlement screen (M9 depositor mode, SPEC D43) — a root pool's books
// and the run that pays its depositors. One page, one story: pick a
// root pool → read every depositor × bucket position → settle the last
// hour at the 8% operator fee → read the payouts, fee and remainder it
// produced. Deposits stay an API call for now (no form here, by scope).
"use client";

import { useState } from "react";
import { HandCoins, RefreshCw, Users } from "lucide-react";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  api,
  ApiError,
  type BucketPosition,
  type PublicPool,
  type SettleRequest,
  type SettleResponse,
} from "@/lib/api";
import {
  formatDateTime,
  formatMicroUsdExact,
  formatMicroUsdRaw,
} from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** The operator fee, in basis points: 8% is rule 8's target (D21) and
 *  the server enforces the 5–10% band. A constant, not a form field, on
 *  purpose — changing the fee needs explicit user approval and a D-log
 *  entry (rule 8), never a knob on a screen. */
const FEE_BPS = 800;
const FEE_PERCENT = FEE_BPS / 100;

/** "Last hour", in epoch milliseconds. */
const HOUR_MS = 3_600_000;

/** A settle run's window in epoch milliseconds. On the server this pair
 *  IS the run's identity: the same window on the same pool replays its
 *  first answer verbatim (M9 idempotency). */
interface SettleWindow {
  fromMs: number;
  toMs: number;
}

/** A dollar figure with its exact integer one hover away — readable on
 *  the surface (nothing rounded, see formatMicroUsdExact), auditable
 *  underneath. Native `title`: the dashboard has no tooltip primitive
 *  and this screen adds none. */
function Money({ micros, className }: { micros: number; className?: string }) {
  return (
    <span className={className} title={formatMicroUsdRaw(micros)}>
      {formatMicroUsdExact(micros)}
    </span>
  );
}

/** One headline figure of a run (a <dt>/<dd> pair). `hint` is the hover
 *  text for the LABEL — the figure's own hover is its exact micro-USD. */
function Figure({
  label,
  micros,
  hint,
}: {
  label: string;
  micros: number;
  hint?: string;
}) {
  return (
    <div className="space-y-1">
      <dt className="text-xs text-muted-foreground" title={hint}>
        {label}
      </dt>
      <dd className="font-display text-xl font-semibold tabular-nums">
        <Money micros={micros} />
      </dd>
    </div>
  );
}

/** Every depositor × bucket position, with a totals footer. A position
 *  is identified by (depositor, provider, model family) — the same tuple
 *  the server upserts on, so it is the row key here. */
function PositionsTable({ positions }: { positions: BucketPosition[] }) {
  type Column = "contributed_micros" | "burned_micros" | "earned_micros";
  // A plain integer sum: exact while the total stays a safe integer (and
  // formatMicroUsdExact answers "—" rather than print a figure past it).
  const total = (field: Column) =>
    positions.reduce((sum, row) => sum + row[field], 0);

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Depositor</TableHead>
          <TableHead>Provider</TableHead>
          <TableHead>Model family</TableHead>
          <TableHead className="text-right">Contributed</TableHead>
          <TableHead className="text-right">Burned</TableHead>
          <TableHead className="text-right">Earned</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {positions.map((row) => (
          <TableRow
            key={`${row.depositor_id}|${row.provider}|${row.model_family}`}
          >
            <TableCell className="font-mono text-xs">
              {row.depositor_id}
            </TableCell>
            <TableCell className="text-xs">{row.provider}</TableCell>
            <TableCell className="font-mono text-xs">
              {row.model_family}
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money micros={row.contributed_micros} />
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money micros={row.burned_micros} />
            </TableCell>
            <TableCell className="text-right tabular-nums">
              <Money micros={row.earned_micros} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
      <TableFooter>
        <TableRow>
          <TableCell colSpan={3}>
            Total · {positions.length}{" "}
            {positions.length === 1 ? "position" : "positions"}
          </TableCell>
          <TableCell className="text-right tabular-nums">
            <Money micros={total("contributed_micros")} />
          </TableCell>
          <TableCell className="text-right tabular-nums">
            <Money micros={total("burned_micros")} />
          </TableCell>
          <TableCell className="text-right tabular-nums">
            <Money micros={total("earned_micros")} />
          </TableCell>
        </TableRow>
      </TableFooter>
    </Table>
  );
}

/** One finished run: the window it answered for, how the distributable
 *  split, and the payouts — the server's response, laid out to be read. */
function RunResult({
  settled,
  response,
}: {
  settled: SettleWindow;
  response: SettleResponse;
}) {
  // Σ payouts is derived here (the server sends the rows, not the sum).
  const paidOut = response.payouts.reduce(
    (sum, payout) => sum + payout.amount_micros,
    0,
  );

  return (
    <div className="space-y-5">
      <div className="space-y-1">
        <h3 className="text-sm font-medium">
          Settled {formatDateTime(settled.fromMs)} →{" "}
          {formatDateTime(settled.toMs)}
        </h3>
        {/* Exactly what was sent, so the same call can be replayed by hand. */}
        <p className="font-mono text-xs text-muted-foreground">
          from_ms {settled.fromMs} · to_ms {settled.toMs} · fee_bps {FEE_BPS}
        </p>
      </div>

      <dl className="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <Figure label="Distributable" micros={response.distributable_micros} />
        <Figure label={`Fee (${FEE_PERCENT}%)`} micros={response.fee_micros} />
        <Figure label="Paid out" micros={paidOut} />
        <Figure
          label="Undistributed"
          micros={response.undistributed_micros}
          hint="Distributable that no depositor earned — a bucket with no burn pays nobody (D43). It stays in the books for a later run."
        />
      </dl>
      <p className="text-xs text-muted-foreground">
        Distributable = fee + paid out + undistributed, to the micro-unit.
      </p>

      <div className="space-y-3">
        <h3 className="text-sm font-medium">Payouts</h3>
        {response.payouts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No payouts — every burn in this pool was already settled.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Depositor</TableHead>
                <TableHead>Provider</TableHead>
                <TableHead>Model family</TableHead>
                <TableHead className="text-right">Amount</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {response.payouts.map((payout) => (
                <TableRow
                  key={`${payout.depositor_id}|${payout.provider}|${payout.model_family}`}
                >
                  <TableCell className="font-mono text-xs">
                    {payout.depositor_id}
                  </TableCell>
                  <TableCell className="text-xs">{payout.provider}</TableCell>
                  <TableCell className="font-mono text-xs">
                    {payout.model_family}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    <Money micros={payout.amount_micros} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
            <TableFooter>
              <TableRow>
                <TableCell colSpan={3}>Paid out</TableCell>
                <TableCell className="text-right tabular-nums">
                  <Money micros={paidOut} />
                </TableCell>
              </TableRow>
            </TableFooter>
          </Table>
        )}
      </div>
    </div>
  );
}

/** The settle action and the answer it produced. The parent remounts it
 *  per pool (`key={pool.id}`), so a run on screen always belongs to the
 *  pool on screen. */
function SettlePanel({
  poolId,
  canSettle,
  onSettled,
}: {
  poolId: string;
  /** False only once the books are loaded and show no depositor at all —
   *  then a run could pay nobody, so the button rests. */
  canSettle: boolean;
  onSettled: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The window of an attempt that got NO server answer. Until it is
  // resolved every click resends THIS window: an identical window replays
  // its stored answer, so a request that did land can neither settle a
  // second time nor have its payouts replaced by an empty later run.
  const [unanswered, setUnanswered] = useState<SettleWindow | null>(null);
  const [run, setRun] = useState<{
    settled: SettleWindow;
    response: SettleResponse;
  } | null>(null);

  async function settle() {
    let target = unanswered;
    if (!target) {
      const toMs = Date.now();
      target = { fromMs: toMs - HOUR_MS, toMs };
    }
    setBusy(true);
    setError(null);
    setUnanswered(target);
    try {
      // Typed against the documented wire contract, so the body cannot
      // drift from it (a renamed field is a compile error, not a 400).
      const request: SettleRequest = {
        from_ms: target.fromMs,
        to_ms: target.toMs,
        fee_bps: FEE_BPS,
      };
      const response = await api.post<SettleResponse>(
        `/pools/${poolId}/settle`,
        request,
      );
      setUnanswered(null);
      setRun({ settled: target, response });
      // `earned` moved: re-read the books the run just changed.
      onSettled();
    } catch (err) {
      // An HTTP status means the server RULED on this window (refused,
      // conflicted, failed): it is spent or never opened, and the next
      // click opens a fresh one. No status at all leaves its fate unknown
      // — `unanswered` stays set, so the retry replays instead of
      // re-settling.
      if (err instanceof ApiError) setUnanswered(null);
      setError(
        err instanceof ApiError
          ? err.message
          : "No answer from the server — the request may still have settled. Settle again: the same window replays its answer instead of settling twice.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-display text-base">
          <HandCoins className="size-4 text-primary" aria-hidden />
          Settle
        </CardTitle>
        <CardDescription>
          Pays every unsettled burn — less the {FEE_PERCENT}% operator fee —
          to the depositors whose credits burned it, and posts the run to
          the ledger at once. A run cannot be undone.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <Button onClick={settle} disabled={busy || !canSettle}>
          {busy ? "Settling…" : `Settle last hour (${FEE_PERCENT}%)`}
        </Button>
        <div aria-live="polite" className="space-y-4">
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
          {run ? (
            <RunResult settled={run.settled} response={run.response} />
          ) : !error ? (
            <p className="text-sm text-muted-foreground">
              {canSettle
                ? "No run yet. Send a request through the pool, then settle — its payouts appear here."
                : "Nothing to settle: this pool has no depositors yet."}
            </p>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}

export default function SettlementPage() {
  const pools = useApi<PublicPool[]>("/pools");
  // Depositor mode is a ROOT-pool feature — both endpoints refuse an org
  // (V1) pool with a 400 — so the picker only ever offers kind "root".
  const rootPools = (pools.data ?? []).filter((pool) => pool.kind === "root");
  const [pickedId, setPickedId] = useState("");
  // The admin's pick, else the first root pool (one pool is the common
  // case) — and never a pool that has since disappeared.
  const pool =
    rootPools.find((candidate) => candidate.id === pickedId) ??
    rootPools[0] ??
    null;
  const positions = useApi<BucketPosition[]>(
    pool ? `/pools/${pool.id}/bucket-positions` : null,
  );
  const rows = positions.data ?? [];

  return (
    <AppShell>
      <PageHeader
        title="Settlement"
        description="A root pool's books — what each depositor contributed, burned and earned — and the run that pays them."
      />

      {pools.loading ? (
        <div className="max-w-sm space-y-2">
          <Label htmlFor="settlement-pool">Root pool</Label>
          <Skeleton className="h-9 w-full" />
        </div>
      ) : pools.error ? (
        <ErrorState message={pools.error.message} onRetry={pools.reload} />
      ) : rootPools.length === 0 ? (
        <EmptyState
          title="No root pool yet"
          hint='Create one through the API: POST /api/v1/pools with kind "root".'
        />
      ) : (
        <div className="max-w-sm space-y-2">
          <Label htmlFor="settlement-pool">Root pool</Label>
          <Select
            value={pool?.id ?? ""}
            onValueChange={setPickedId}
          >
            <SelectTrigger id="settlement-pool" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {rootPools.map((candidate) => (
                <SelectItem key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {pool ? (
        <>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center justify-between gap-2 font-display text-base">
                <span className="flex items-center gap-2">
                  <Users className="size-4 text-primary" aria-hidden />
                  Depositors
                </span>
                {/* Manual on purpose: a burn made outside this tab (a curl
                    through the pool) shows up on the next refresh. */}
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Refresh positions"
                  title="Refresh"
                  onClick={positions.reload}
                >
                  <RefreshCw className="size-4" />
                </Button>
              </CardTitle>
              <CardDescription>
                Each depositor&apos;s position per bucket (provider × model
                family) — the off-chain books. Hover an amount for its exact
                micro-USD.
              </CardDescription>
            </CardHeader>
            <CardContent>
              {positions.error ? (
                <div className="flex flex-wrap items-center gap-3">
                  <p role="alert" className="text-sm text-destructive">
                    {positions.error.message}
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={positions.reload}
                  >
                    Try again
                  </Button>
                </div>
              ) : positions.loading ? (
                <TableSkeleton />
              ) : rows.length === 0 ? (
                <EmptyState
                  title="No depositors yet"
                  hint="Deposit a provider key through the API: POST /api/v1/pools/{id}/deposits."
                />
              ) : (
                <PositionsTable positions={rows} />
              )}
            </CardContent>
          </Card>

          <SettlePanel
            key={pool.id}
            poolId={pool.id}
            canSettle={positions.data === null || rows.length > 0}
            onSettled={positions.reload}
          />
        </>
      ) : null}
    </AppShell>
  );
}
