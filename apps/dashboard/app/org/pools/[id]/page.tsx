// Pool detail — the org admin's workbench (SPEC §7): member pool keys
// (issue/revoke, raw shown ONCE), provider keys (masked; validate on save,
// revalidate, revoke), quota rules (budgets), and the usage table with a
// per-member filter + sparkline.
"use client";

import { useState } from "react";
import { useParams } from "next/navigation";
import { KeyRound, Plus, RefreshCw, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { SecretReveal } from "@/components/secret-reveal";
import { UsageSparkline } from "@/components/sparkline";
import { UsageTable } from "@/components/usage-table";
import { HealthBadge } from "@/components/health-badge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  api,
  ApiError,
  type IssuedKey,
  type PublicMember,
  type PublicPool,
  type PublicPoolKey,
  type PublicProviderKey,
  type QuotaRule,
  type UsageList,
  type UsageSummary,
} from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** Wraps a mutation with toast feedback + list refresh, the common shape. */
async function run(
  action: () => Promise<unknown>,
  success: string,
  refresh: () => void,
) {
  try {
    await action();
    toast.success(success);
    refresh();
  } catch (err) {
    toast.error(err instanceof ApiError ? err.message : "Request failed");
  }
}

function IssueKeyCard({
  poolId,
  members,
  onIssued,
}: {
  poolId: string;
  members: PublicMember[];
  onIssued: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [memberId, setMemberId] = useState("");
  const [busy, setBusy] = useState(false);
  // The raw key exists ONLY in this state, until the dialog closes.
  const [issued, setIssued] = useState<IssuedKey | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      const key = await api.post<IssuedKey>(`/pools/${poolId}/keys`, {
        user_id: memberId,
      });
      setIssued(key);
      onIssued();
    } catch (err) {
      toast.error(err instanceof ApiError ? err.message : "Failed to issue");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next: boolean) => {
        setOpen(next);
        // Closing the dialog destroys the raw key forever (by design).
        if (!next) setIssued(null);
      }}
    >
      <DialogTrigger asChild>
        <Button>
          <Plus className="size-4" aria-hidden />
          Issue key
        </Button>
      </DialogTrigger>
      <DialogContent>
        {issued ? (
          <div className="space-y-4">
            <DialogHeader>
              <DialogTitle className="font-display">Key issued</DialogTitle>
              <DialogDescription>
                Hand it to the member over a secure channel.
              </DialogDescription>
            </DialogHeader>
            <SecretReveal
              secret={issued.raw}
              label={`Pool key ${issued.key_prefix}`}
            />
          </div>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="font-display">Issue pool key</DialogTitle>
              <DialogDescription>
                A per-user key for this pool. Quotas can attach to the key,
                the member, or the whole pool.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={submit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="key-member">Member</Label>
                <Select value={memberId} onValueChange={setMemberId} required>
                  <SelectTrigger id="key-member">
                    <SelectValue placeholder="Choose member" />
                  </SelectTrigger>
                  <SelectContent>
                    {members.map((member) => (
                      <SelectItem key={member.id} value={member.id}>
                        {member.email}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <DialogFooter>
                <Button type="submit" disabled={busy || !memberId}>
                  {busy ? "Issuing…" : "Issue key"}
                </Button>
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function AddProviderKeyDialog({
  poolId,
  onAdded,
}: {
  poolId: string;
  onAdded: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [key, setKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      const created = await api.post<PublicProviderKey>(
        `/pools/${poolId}/provider-keys`,
        { key, base_url: baseUrl },
      );
      toast.success(
        created.health === "ok"
          ? `Provider key ${created.key_prefix} validated and sealed`
          : `Provider key ${created.key_prefix} sealed, but the probe said ${created.health} — check the key/base URL`,
      );
      setOpen(false);
      setKey("");
      onAdded();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to add provider key",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline">
          <Plus className="size-4" aria-hidden />
          Add provider key
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">Add provider key</DialogTitle>
          <DialogDescription>
            Sealed with envelope encryption before storage; only the masked
            prefix is ever displayed again.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="provider-key">API key</Label>
            <Input
              id="provider-key"
              type="password"
              autoComplete="off"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              placeholder="sk-…"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="base-url">Base URL</Label>
            <Input
              id="base-url"
              type="url"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.openai.com/v1"
              required
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Validating…" : "Add & validate"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function AddBudgetDialog({
  poolId,
  onAdded,
}: {
  poolId: string;
  onAdded: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState("pool");
  const [subjectId, setSubjectId] = useState("");
  const [period, setPeriod] = useState("day");
  const [limitTokens, setLimitTokens] = useState("");
  const [busy, setBusy] = useState(false);
  const members = useApi<PublicMember[]>(open ? "/members" : null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post("/quota-rules", {
        subject_kind: kind,
        subject_id: kind === "pool" ? poolId : subjectId,
        period,
        limit_tokens: limitTokens ? Number(limitTokens) : null,
      });
      toast.success("Budget saved");
      setOpen(false);
      setLimitTokens("");
      onAdded();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to save budget",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline">
          <Plus className="size-4" aria-hidden />
          Set budget
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">Set a budget</DialogTitle>
          <DialogDescription>
            Token quota per window — enforced at the gateway BEFORE any
            upstream call (rule 4).
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="budget-scope">Applies to</Label>
              <Select value={kind} onValueChange={setKind}>
                <SelectTrigger id="budget-scope">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="pool">Whole pool</SelectItem>
                  <SelectItem value="user">One member</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="budget-period">Window</Label>
              <Select value={period} onValueChange={setPeriod}>
                <SelectTrigger id="budget-period">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="minute">Minute (requests)</SelectItem>
                  <SelectItem value="day">Day</SelectItem>
                  <SelectItem value="month">Month</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {kind === "user" ? (
            <div className="space-y-2">
              <Label htmlFor="budget-member">Member</Label>
              <Select value={subjectId} onValueChange={setSubjectId} required>
                <SelectTrigger id="budget-member">
                  <SelectValue placeholder="Choose member" />
                </SelectTrigger>
                <SelectContent>
                  {(members.data ?? []).map((member) => (
                    <SelectItem key={member.id} value={member.id}>
                      {member.email}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          ) : null}
          <div className="space-y-2">
            <Label htmlFor="budget-tokens">
              {period === "minute" ? "Requests per minute" : "Tokens per window"}
            </Label>
            <Input
              id="budget-tokens"
              type="number"
              min={1}
              value={limitTokens}
              onChange={(e) => setLimitTokens(e.target.value)}
              placeholder="100000"
              required
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Saving…" : "Save budget"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default function PoolDetailPage() {
  const params = useParams<{ id: string }>();
  const poolId = params.id;
  const pool = useApi<PublicPool>(`/pools/${poolId}`);
  const members = useApi<PublicMember[]>("/members");
  const keys = useApi<PublicPoolKey[]>(`/pools/${poolId}/keys`);
  const providerKeys = useApi<PublicProviderKey[]>(
    `/pools/${poolId}/provider-keys`,
  );
  const rules = useApi<QuotaRule[]>("/quota-rules");
  const [memberFilter, setMemberFilter] = useState<string>("all");
  const usage = useApi<UsageList>(
    `/usage?pool_id=${poolId}${memberFilter === "all" ? "" : `&user_id=${memberFilter}`}&limit=50`,
  );
  const summary = useApi<UsageSummary>(`/usage/summary?pool_id=${poolId}`);

  const memberName = (id: string | null) =>
    (members.data ?? []).find((m) => m.id === id)?.email ?? "—";
  const poolRules = (rules.data ?? []).filter(
    (rule) => rule.subject_id === poolId || rule.subject_kind === "user",
  );

  return (
    <AppShell>
      {pool.error ? (
        <ErrorState message={pool.error.message} onRetry={pool.reload} />
      ) : (
        <PageHeader
          title={pool.data?.name ?? "Pool"}
          description={
            pool.data
              ? `${pool.data.model_allowlist.join(", ")} · ${pool.data.status}`
              : "…"
          }
        />
      )}

      <Tabs defaultValue="keys">
        <TabsList>
          <TabsTrigger value="keys">Keys</TabsTrigger>
          <TabsTrigger value="provider">Provider keys</TabsTrigger>
          <TabsTrigger value="budgets">Budgets</TabsTrigger>
          <TabsTrigger value="usage">Usage</TabsTrigger>
        </TabsList>

        {/* ---- Member pool keys ---- */}
        <TabsContent value="keys" className="space-y-4">
          <div className="flex justify-end">
            <IssueKeyCard
              poolId={poolId}
              members={members.data ?? []}
              onIssued={keys.reload}
            />
          </div>
          {keys.loading ? (
            <TableSkeleton />
          ) : keys.error ? (
            <ErrorState message={keys.error.message} onRetry={keys.reload} />
          ) : (keys.data ?? []).length === 0 ? (
            <EmptyState
              title="No pool keys"
              hint="Issue one per member — keys are per-user by rule 4."
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Member</TableHead>
                  <TableHead>Key</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Issued</TableHead>
                  <TableHead className="w-12" aria-label="Actions" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {(keys.data ?? []).map((key) => (
                  <TableRow key={key.id}>
                    <TableCell>{memberName(key.user_id)}</TableCell>
                    <TableCell className="font-mono text-xs">
                      <span className="inline-flex items-center gap-1.5">
                        <KeyRound className="size-3 text-muted-foreground" aria-hidden />
                        {key.key_prefix}
                      </span>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={key.status === "active" ? "default" : "secondary"}
                      >
                        {key.status}
                      </Badge>
                    </TableCell>
                    <TableCell>{formatDateTime(key.created_at)}</TableCell>
                    <TableCell>
                      {key.status === "active" ? (
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Revoke key"
                          onClick={() =>
                            run(
                              () => api.del(`/pools/${poolId}/keys/${key.id}`),
                              `Key ${key.key_prefix} revoked`,
                              keys.reload,
                            )
                          }
                        >
                          <Trash2 className="size-4 text-destructive" />
                        </Button>
                      ) : null}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </TabsContent>

        {/* ---- Provider keys ---- */}
        <TabsContent value="provider" className="space-y-4">
          <div className="flex justify-end">
            <AddProviderKeyDialog
              poolId={poolId}
              onAdded={providerKeys.reload}
            />
          </div>
          {providerKeys.loading ? (
            <TableSkeleton />
          ) : providerKeys.error ? (
            <ErrorState
              message={providerKeys.error.message}
              onRetry={providerKeys.reload}
            />
          ) : (providerKeys.data ?? []).length === 0 ? (
            <EmptyState
              title="No provider keys"
              hint="Fund the pool with an upstream API key — it funds the 'electricity'."
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Key</TableHead>
                  <TableHead>Base URL</TableHead>
                  <TableHead>Health</TableHead>
                  <TableHead>Validated</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="w-24" aria-label="Actions" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {(providerKeys.data ?? []).map((providerKey) => (
                  <TableRow key={providerKey.id}>
                    <TableCell className="font-mono text-xs">
                      {providerKey.key_prefix}
                    </TableCell>
                    <TableCell className="max-w-48 truncate font-mono text-xs">
                      {providerKey.base_url}
                    </TableCell>
                    <TableCell>
                      <HealthBadge health={providerKey.health} />
                    </TableCell>
                    <TableCell className="text-xs">
                      {providerKey.last_validated_at
                        ? formatDateTime(providerKey.last_validated_at)
                        : "never"}
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={
                          providerKey.status === "active" ? "default" : "secondary"
                        }
                      >
                        {providerKey.status}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Revalidate key"
                          disabled={providerKey.status !== "active"}
                          onClick={() =>
                            run(
                              () =>
                                api.post(
                                  `/pools/${poolId}/provider-keys/${providerKey.id}/revalidate`,
                                ),
                              `Probe complete: see health badge`,
                              providerKeys.reload,
                            )
                          }
                        >
                          <RefreshCw className="size-4" />
                        </Button>
                        {providerKey.status === "active" ? (
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label="Revoke provider key"
                            onClick={() =>
                              run(
                                () =>
                                  api.del(
                                    `/pools/${poolId}/provider-keys/${providerKey.id}`,
                                  ),
                                `Provider key ${providerKey.key_prefix} revoked`,
                                providerKeys.reload,
                              )
                            }
                          >
                            <Trash2 className="size-4 text-destructive" />
                          </Button>
                        ) : null}
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </TabsContent>

        {/* ---- Budgets (quota rules) ---- */}
        <TabsContent value="budgets" className="space-y-4">
          <div className="flex justify-end">
            <AddBudgetDialog poolId={poolId} onAdded={rules.reload} />
          </div>
          {rules.loading ? (
            <TableSkeleton />
          ) : rules.error ? (
            <ErrorState message={rules.error.message} onRetry={rules.reload} />
          ) : poolRules.length === 0 ? (
            <EmptyState
              title="No budgets"
              hint="Set token or request caps so one member can't drain the pool."
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Scope</TableHead>
                  <TableHead>Window</TableHead>
                  <TableHead>Limit</TableHead>
                  <TableHead className="w-12" aria-label="Actions" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {poolRules.map((rule) => (
                  <TableRow key={rule.id}>
                    <TableCell>
                      {rule.subject_kind === "pool"
                        ? "Whole pool"
                        : memberName(rule.subject_id)}
                    </TableCell>
                    <TableCell>{rule.period}</TableCell>
                    <TableCell>
                      {rule.limit_tokens != null
                        ? `${rule.limit_tokens.toLocaleString()} tokens`
                        : `${rule.limit_requests?.toLocaleString() ?? "—"} requests`}
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label="Delete budget"
                        onClick={() =>
                          run(
                            () => api.del(`/quota-rules/${rule.id}`),
                            "Budget removed",
                            rules.reload,
                          )
                        }
                      >
                        <Trash2 className="size-4 text-destructive" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </TabsContent>

        {/* ---- Usage ---- */}
        <TabsContent value="usage" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="font-display text-base">
                Tokens per day (14 days)
              </CardTitle>
            </CardHeader>
            <CardContent>
              {summary.loading ? (
                <div className="h-[72px]" />
              ) : summary.error ? (
                <p className="text-sm text-destructive">
                  {summary.error.message}
                </p>
              ) : (
                <UsageSparkline days={summary.data?.days ?? []} />
              )}
            </CardContent>
          </Card>
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">Member:</span>
            <Select value={memberFilter} onValueChange={setMemberFilter}>
              <SelectTrigger className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">Everyone</SelectItem>
                {(members.data ?? []).map((member) => (
                  <SelectItem key={member.id} value={member.id}>
                    {member.email}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Separator orientation="vertical" className="h-6" />
          </div>
          <UsageTable
            events={usage.data?.events ?? []}
            loading={usage.loading}
          />
        </TabsContent>
      </Tabs>
    </AppShell>
  );
}
