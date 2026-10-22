// Review queue (SPEC §9): every detection lands here — classifier flags,
// auto-blocks, impossible travel. The admin's one
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
import { api, ApiError, type PendingAction, type ReviewFlag } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** Human label for a detection kind (wire value → dashboard words). */
const KIND_LABELS: Record<string, string> = {
  classifier_flag: "Off-purpose request",
  classifier_auto_block: "Auto-block (key suspended)",
  impossible_travel: "Country change",
  distinct_ip_concurrency: "Key used from multiple networks",
};

/** Human label for a pending sensitive action (D33 wire value → words). */
const PENDING_KIND_LABELS: Record<string, string> = {
  admin_demote: "Demote admin to member",
  admin_key_expiry: "Set expiry on an admin's key",
  admin_key_revoke: "Revoke an admin's key",
  org_policy: "Org policy (classifier purpose / auto-block)",
  pool_disable: "Disable pool",
};

/** The score detail line for classifier kinds (bp → 0–1 alignment). */
function ScoreChip({ flag }: { flag: ReviewFlag }) {
  if (flag.score_bp == null) return null;
  // Scores are basis points of the 0..1 alignment (×10 000 at write
  // time) — dividing by 1000 rendered them 10× high on this surface
  // (final-scan finding F3).
  const alignment = (flag.score_bp / 10000).toFixed(2);
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
    shared_networks?: string[];
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
          {detail.shared_networks?.length
            ? <span>networks {detail.shared_networks.join(", ")}</span>
            : null}
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

/**
 * D33 approvals: pending sensitive actions awaiting a SECOND admin.
 * The requester cannot approve their own action (server-enforced); this
 * panel is where every other admin becomes the second pair of eyes.
 */
function ApprovalsPanel({ onDecided }: { onDecided: () => void }) {
  // The org id comes from the session bootstrap (/auth/me carries org_id).
  const me = useApi<{ org_id: string | null }>("/auth/me");
  const orgId = me.data?.org_id ?? null;
  const actions = useApi<PendingAction[]>(
    orgId ? `/orgs/${orgId}/pending-actions?status=pending` : null,
  );
  // Resolve requester ids → emails: the approver should see WHO asked,
  // next to what exactly they asked for (founder UX report 2026-10-21).
  const members = useApi<{ id: string; email: string }[]>(
    orgId ? `/members?org_id=${orgId}` : null,
  );
  const emailOf = (id: string) =>
    (members.data ?? []).find((m) => m.id === id)?.email ?? "another admin";
  const [busyId, setBusyId] = useState<string | null>(null);

  async function decide(actionId: string, decision: "approve" | "reject") {
    if (!orgId) return;
    setBusyId(actionId);
    try {
      await api.post(`/orgs/${orgId}/pending-actions/${actionId}/${decision}`);
      toast.success(decision === "approve" ? "Approved and executed" : "Rejected");
      actions.reload();
      onDecided();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Decision failed");
    } finally {
      setBusyId(null);
    }
  }

  if (actions.loading) return <TableSkeleton />;
  if (actions.error) {
    return <ErrorState message={actions.error.message} onRetry={actions.reload} />;
  }
  if ((actions.data ?? []).length === 0) {
    return (
      <EmptyState
        title="No approvals waiting"
        hint="Sensitive admin actions (another admin's key, org policy, pool deletion) appear here for a second admin."
      />
    );
  }
  return (
    <div className="space-y-3">
      {(actions.data ?? []).map((action) => (
        <Card key={action.id} data-testid="pending-approval">
          <CardContent className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div className="space-y-1">
              <p className="font-medium">
                {action.summary || PENDING_KIND_LABELS[action.kind] || action.kind}
              </p>
              <p className="text-sm text-muted-foreground">
                Requested by {emailOf(action.requested_by)} ·{" "}
                {formatDateTime(action.created_at)} — needs a second,
                distinct admin
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={busyId === action.id}
                onClick={() => decide(action.id, "reject")}
              >
                Reject
              </Button>
              <Button
                size="sm"
                disabled={busyId === action.id}
                onClick={() => decide(action.id, "approve")}
              >
                Approve
              </Button>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
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
          ? "Detections from the classifier and travel checks land here."
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
              <TabsTrigger value="approvals">Approvals</TabsTrigger>
              <TabsTrigger value="open">Open</TabsTrigger>
              <TabsTrigger value="resolved">Resolved</TabsTrigger>
            </TabsList>
            <TabsContent value="approvals" className="pt-4">
              <ApprovalsPanel key={`approvals-${refreshKey}`} onDecided={bump} />
            </TabsContent>
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
