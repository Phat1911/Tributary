// Member self-service: OWN keys (revoke-only) and OWN usage — nothing of
// anyone else's is reachable, enforced server-side by the /me endpoints.
"use client";

import { Eye, KeyRound, ShieldAlert, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { UsageSparkline } from "@/components/sparkline";
import { UsageTable } from "@/components/usage-table";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
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
import {
  api,
  ApiError,
  type Disclosure,
  type OwnPoolKey,
  type UsageList,
  type UsageSummary,
} from "@/lib/api";
import { formatDateTime, formatTokens } from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** The M4 disclosure banner: shown whenever classification runs at all
 * (Alert mode included) — the wording strengthens when auto-block is live. */
function DisclosureBanner({ disclosure }: { disclosure: Disclosure }) {
  if (!disclosure.classification_active) return null;
  const live = disclosure.auto_block_live;
  return (
    <div
      className={`flex items-start gap-3 rounded-lg border p-4 text-sm ${
        live
          ? "border-amber-500/50 bg-amber-500/5"
          : "border-border bg-muted/40"
      }`}
      role="status"
    >
      {live ? (
        <ShieldAlert className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" aria-hidden />
      ) : (
        <Eye className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
      )}
      <p className="leading-relaxed">
        Requests made with your keys are classified for purpose alignment.{" "}
        {live ? (
          <strong>
            Off-purpose requests may be blocked — your key is suspended
            pending review, and your administrator can restore it.
          </strong>
        ) : (
          <span className="text-muted-foreground">
            Off-purpose requests may be flagged for your administrator&rsquo;s
            review.
          </span>
        )}
      </p>
    </div>
  );
}

export default function MePage() {
  const disclosure = useApi<Disclosure>("/me/disclosure");
  const keys = useApi<OwnPoolKey[]>("/me/keys");
  const usage = useApi<UsageList>("/me/usage?limit=50");
  const summary = useApi<UsageSummary>("/me/usage/summary");

  async function revoke(key: OwnPoolKey) {
    try {
      await api.del(`/me/keys/${key.id}`);
      toast.success(`Key ${key.key_prefix} revoked`);
      keys.reload();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to revoke key",
      );
    }
  }

  const totalTokens = summary.data?.totals.tokens ?? 0;
  const totalRequests = summary.data?.totals.requests ?? 0;

  return (
    <AppShell>
      <PageHeader
        title="My keys & usage"
        description="Only you can see this. Point your OpenAI-compatible client at the gateway with one of your keys."
      />

      {/* M4 disclosure — required whenever classification runs at all */}
      {disclosure.data ? (
        <DisclosureBanner disclosure={disclosure.data} />
      ) : null}

      {/* Keys */}
      <section className="space-y-3">
        <h2 className="font-display text-lg font-medium">My pool keys</h2>
        {keys.loading ? (
          <TableSkeleton />
        ) : keys.error ? (
          <ErrorState message={keys.error.message} onRetry={keys.reload} />
        ) : (keys.data ?? []).length === 0 ? (
          <EmptyState
            title="No keys yet"
            hint="Ask your org admin to issue you one."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Pool</TableHead>
                <TableHead>Key</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Issued</TableHead>
                <TableHead className="w-12" aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {(keys.data ?? []).map((key) => (
                <TableRow key={key.id}>
                  <TableCell className="font-medium">{key.pool_name}</TableCell>
                  <TableCell className="font-mono text-xs">
                    <span className="inline-flex items-center gap-1.5">
                      <KeyRound className="size-3 text-muted-foreground" aria-hidden />
                      {key.key_prefix}
                    </span>
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={
                            key.status === "active"
                              ? "default"
                              : key.status === "suspended"
                                ? "destructive"
                                : "secondary"
                          }
                    >
                      {key.status}
                    </Badge>
                  </TableCell>
                  <TableCell>{formatDateTime(key.created_at)}</TableCell>
                  <TableCell>
                    {key.status === "active" ? (
                      <AlertDialog>
                        <AlertDialogTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label={`Revoke key ${key.key_prefix}`}
                          >
                            <Trash2 className="size-4 text-destructive" />
                          </Button>
                        </AlertDialogTrigger>
                        <AlertDialogContent>
                          <AlertDialogHeader>
                            <AlertDialogTitle className="font-display">
                              Revoke {key.key_prefix}?
                            </AlertDialogTitle>
                            <AlertDialogDescription>
                              Requests using this key stop working
                              immediately. This cannot be undone (a new key
                              can be issued by your admin).
                            </AlertDialogDescription>
                          </AlertDialogHeader>
                          <AlertDialogFooter>
                            <AlertDialogCancel>Keep it</AlertDialogCancel>
                            <AlertDialogAction onClick={() => revoke(key)}>
                              Revoke key
                            </AlertDialogAction>
                          </AlertDialogFooter>
                        </AlertDialogContent>
                      </AlertDialog>
                    ) : null}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>

      {/* Usage */}
      <section className="space-y-3">
        <h2 className="font-display text-lg font-medium">My usage</h2>
        <Card>
          <CardHeader className="flex flex-row items-baseline justify-between">
            <CardTitle className="font-display text-base">
              Tokens per day (14 days)
            </CardTitle>
            <p className="text-sm text-muted-foreground">
              {formatTokens(totalRequests)} requests ·{" "}
              {formatTokens(totalTokens)} tokens
            </p>
          </CardHeader>
          <CardContent>
            {summary.loading ? (
              <div className="h-[72px]" />
            ) : summary.error ? (
              <p className="text-sm text-destructive">{summary.error.message}</p>
            ) : (
              <UsageSparkline days={summary.data?.days ?? []} />
            )}
          </CardContent>
        </Card>
        <UsageTable events={usage.data?.events ?? []} loading={usage.loading} />
      </section>
    </AppShell>
  );
}
