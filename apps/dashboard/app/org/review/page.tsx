// Review queue (SPEC §9): every detection lands here — honey-key trips,
// canary-model hits, classifier flags, auto-blocks. The admin's one
// decision surface: unblock / revoke / dismiss. Detection is automatic;
// the RESPONSE stays human (D17).
"use client";

import { useState } from "react";
import { ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, ApiError, type ReviewFlag } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** Human label for a detection kind (wire value → dashboard words). */
const KIND_LABELS: Record<string, string> = {
  honey_key_used: "Honey key used",
  canary_model: "Canary model hit",
  classifier_flag: "Off-purpose request",
  classifier_auto_block: "Auto-block (key suspended)",
  impossible_travel: "Country change",
};

/** The score detail line for classifier kinds (bp → 0–1 alignment). */
function ScoreChip({ flag }: { flag: ReviewFlag }) {
  if (flag.score_bp == null) return null;
  const alignment = (flag.score_bp / 1000).toFixed(2);
  return (
    <Badge variant="outline" className="font-mono text-xs">
      alignment {alignment}
    </Badge>
  );
}

/** One queue row: what tripped, when, the sanitized detail, the actions. */
function FlagRow({
  flag,
  onResolved,
}: {
  flag: ReviewFlag;
  onResolved: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const detail = flag.detail_json as {
    key_prefix?: string;
    model?: string;
    ip?: string;
    country?: string;
    note?: string;
    score?: number;
  };

  async function resolve(action: "unblock" | "revoke_key" | "dismiss") {
    setBusy(true);
    try {
      await api.post(`/review-flags/${flag.id}/resolve`, { action });
      toast.success(
        action === "unblock"
          ? "Key restored — the member can work again"
          : action === "revoke_key"
            ? "Key revoked"
            : "Flag dismissed",
      );
      onResolved();
    } catch (err) {
      toast.error(err instanceof ApiError ? err.message : "Failed to resolve");
    } finally {
      setBusy(false);
    }
  }

  return (
    <TableRow>
      <TableCell>
        <span className="flex flex-col gap-1">
          <span className="font-medium">
            {KIND_LABELS[flag.kind] ?? flag.kind}
          </span>
          <ScoreChip flag={flag} />
        </span>
      </TableCell>
      <TableCell>
        <Badge
          variant={
            flag.severity === "high"
              ? "destructive"
              : flag.severity === "medium"
                ? "default"
                : "secondary"
          }
        >
          {flag.severity}
        </Badge>
      </TableCell>
      <TableCell className="font-mono text-xs">
        <span className="flex flex-col gap-0.5">
          {detail.key_prefix ? <span>key {detail.key_prefix}</span> : null}
          {detail.model ? <span>model {detail.model}</span> : null}
          {detail.ip ? <span>ip {detail.ip}</span> : null}
          {detail.country ? <span>now in {detail.country}</span> : null}
        </span>
      </TableCell>
      <TableCell className="text-xs">
        {formatDateTime(flag.created_at)}
      </TableCell>
      <TableCell>
        {flag.status === "open" ? (
          <div className="flex flex-wrap gap-1">
            {flag.kind === "classifier_auto_block" ? (
              <Button
                size="sm"
                disabled={busy}
                onClick={() => resolve("unblock")}
              >
                Unblock key
              </Button>
            ) : null}
            {flag.pool_key_id ? (
              <Button
                size="sm"
                variant="destructive"
                disabled={busy}
                onClick={() => resolve("revoke_key")}
              >
                Revoke key
              </Button>
            ) : null}
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => resolve("dismiss")}
            >
              Dismiss
            </Button>
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">
            resolved {flag.resolved_at ? formatDateTime(flag.resolved_at) : ""}
          </span>
        )}
      </TableCell>
    </TableRow>
  );
}

/** The queue table for one status filter. */
function QueueTable({
  status,
  onResolved,
}: {
  status: "open" | "resolved";
  onResolved: () => void;
}) {
  const flags = useApi<ReviewFlag[]>(`/review-flags?status=${status}&limit=100`);
  return flags.loading ? (
    <TableSkeleton />
  ) : flags.error ? (
    <ErrorState message={flags.error.message} onRetry={flags.reload} />
  ) : (flags.data ?? []).length === 0 ? (
    <EmptyState
      title={status === "open" ? "Queue is clear" : "Nothing resolved yet"}
      hint={
        status === "open"
          ? "Detections from honey keys, canary models and the classifier land here."
          : "Resolved flags stay queryable for the trail."
      }
    />
  ) : (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Detection</TableHead>
          <TableHead>Severity</TableHead>
          <TableHead>Detail</TableHead>
          <TableHead>Raised</TableHead>
          <TableHead className="w-64" aria-label="Actions">
            Action
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {(flags.data ?? []).map((flag) => (
          <FlagRow key={flag.id} flag={flag} onResolved={onResolved} />
        ))}
      </TableBody>
    </Table>
  );
}

export default function ReviewQueuePage() {
  // Resolving a flag changes BOTH tabs' data; remounting the tables via
  // `key` re-runs their fetches (the useApi hook takes no external key).
  const [refreshKey, setRefreshKey] = useState(0);
  const bump = () => setRefreshKey((k) => k + 1);

  return (
    <AppShell>
      <PageHeader
        title="Review queue"
        description="Every detection lands here. Response stays human — auto-block suspends, never bans."
      />
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 font-display text-base">
            <ShieldAlert className="size-4 text-primary" aria-hidden />
            Detections
          </CardTitle>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="open">
            <TabsList>
              <TabsTrigger value="open">Open</TabsTrigger>
              <TabsTrigger value="resolved">Resolved</TabsTrigger>
            </TabsList>
            <TabsContent value="open" className="pt-4">
              <QueueTable key={`open-${refreshKey}`} status="open" onResolved={bump} />
            </TabsContent>
            <TabsContent value="resolved" className="pt-4">
              <QueueTable
                key={`resolved-${refreshKey}`}
                status="resolved"
                onResolved={bump}
              />
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>
    </AppShell>
  );
}
