// Pool detail — the org admin's workbench (SPEC §7): member pool keys
// (issue/revoke, raw shown ONCE), provider keys (masked; validate on save,
// revalidate, revoke), quota rules (budgets), and the usage table with a
// per-member filter + sparkline.
"use client";

import { useState } from "react";
import { useParams } from "next/navigation";
import {
  CalendarClock,
  Fingerprint,
  KeyRound,
  Plus,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
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
  type CanaryManagementView,
  type CanaryModel,
  type IssuedKey,
  type IpAllowlistEntry,
  type ManagedPoolKey,
  type PlantedCanaryKeys,
  type PublicMember,
  type PublicPool,
  type PublicProviderKey,
  type QuotaRule,
  type RotatedKey,
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
  members,
  onAdded,
}: {
  poolId: string;
  members: PublicMember[];
  onAdded: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState("pool");
  const [subjectId, setSubjectId] = useState("");
  const [period, setPeriod] = useState("day");
  const [limitTokens, setLimitTokens] = useState("");
  const [busy, setBusy] = useState(false);

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
                  {members.map((member) => (
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

/** Key-governance dialog: IP allowlist editor for one live key. */
function AllowlistDialog({
  poolId,
  poolKey,
  onClose,
}: {
  poolId: string;
  poolKey: ManagedPoolKey;
  onClose: () => void;
}) {
  // useApi fetches on mount (the dialog only mounts while open) and
  // `reload` re-fetches after each add/remove — no manual effect.
  const allowlists = useApi<IpAllowlistEntry[]>(
    `/pools/${poolId}/keys/${poolKey.id}/allowlists`,
  );
  const [cidr, setCidr] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  async function add(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post(`/pools/${poolId}/keys/${poolKey.id}/allowlists`, {
        cidr,
        note: note || null,
      });
      setCidr("");
      setNote("");
      allowlists.reload();
    } catch (err) {
      toast.error(err instanceof ApiError ? err.message : "Failed to add");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(next: boolean) => !next && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="font-display">
            IP allowlist · {poolKey.key_prefix}
          </DialogTitle>
          <DialogDescription>
            With at least one CIDR set, requests from other source IPs are
            denied. Direct (proxy-less) requests fail closed — the gateway
            sees no forwarding header.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={add} className="flex items-end gap-2">
          <div className="flex-1 space-y-1">
            <Label htmlFor="cidr">CIDR</Label>
            <Input
              id="cidr"
              value={cidr}
              onChange={(e) => setCidr(e.target.value)}
              placeholder="203.0.113.0/24"
              className="font-mono text-xs"
              required
            />
          </div>
          <div className="flex-1 space-y-1">
            <Label htmlFor="cidr-note">Note</Label>
            <Input
              id="cidr-note"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder="office"
            />
          </div>
          <Button type="submit" disabled={busy}>
            Add
          </Button>
        </form>
        {allowlists.error ? (
          <p className="text-sm text-destructive">
            {allowlists.error.message}
          </p>
        ) : allowlists.loading && !allowlists.data ? (
          <TableSkeleton />
        ) : (allowlists.data ?? []).length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No CIDRs yet — this key accepts requests from any source IP.
          </p>
        ) : (
          <ul className="space-y-1">
            {(allowlists.data ?? []).map((entry) => (
              <li
                key={entry.id}
                className="flex items-center justify-between rounded-md border px-3 py-1.5"
              >
                <span className="font-mono text-xs">{entry.cidr}</span>
                <span className="flex items-center gap-2 text-xs text-muted-foreground">
                  {entry.note ?? ""}
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={`Remove ${entry.cidr}`}
                    onClick={() =>
                      run(
                        () =>
                          api.del(
                            `/pools/${poolId}/keys/${poolKey.id}/allowlists/${entry.id}`,
                          ),
                        `${entry.cidr} removed`,
                        allowlists.reload,
                      )
                    }
                  >
                    <Trash2 className="size-4 text-destructive" />
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Rotate flow: confirms, then reveals the new raw key exactly once. */
function RotateKeyDialog({
  poolId,
  poolKey,
  onClose,
  onRotated,
}: {
  poolId: string;
  poolKey: ManagedPoolKey;
  onClose: () => void;
  onRotated: () => void;
}) {
  const [rotated, setRotated] = useState<RotatedKey | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function rotate() {
    setBusy(true);
    try {
      setRotated(
        await api.post<RotatedKey>(
          `/pools/${poolId}/keys/${poolKey.id}/rotate`,
        ),
      );
      onRotated();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Rotation failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(next: boolean) => !next && onClose()}>
      <DialogContent>
        {rotated ? (
          <div className="space-y-4">
            <DialogHeader>
              <DialogTitle className="font-display">Key rotated</DialogTitle>
              <DialogDescription>
                The old key {poolKey.key_prefix} stopped working the moment
                you pressed rotate. Hand the new one to the member over a
                secure channel — it is shown only now.
              </DialogDescription>
            </DialogHeader>
            <SecretReveal
              secret={rotated.raw}
              label={`Pool key ${rotated.key_prefix}`}
            />
          </div>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="font-display">
                Rotate {poolKey.key_prefix}?
              </DialogTitle>
              <DialogDescription>
                Issues a replacement key and revokes this one immediately.
                Any expiry carries over.
              </DialogDescription>
            </DialogHeader>
            {error ? (
              <p className="text-sm text-destructive">{error}</p>
            ) : null}
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button onClick={rotate} disabled={busy}>
                {busy ? "Rotating…" : "Rotate now"}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Set-or-clear expiry dialog (epoch ms sent; input is a date). */
function ExpiryDialog({
  poolId,
  poolKey,
  onClose,
  onSaved,
}: {
  poolId: string;
  poolKey: ManagedPoolKey;
  onClose: () => void;
  onSaved: () => void;
}) {
  const current = poolKey.expires_at
    ? new Date(poolKey.expires_at).toISOString().slice(0, 10)
    : "";
  const [date, setDate] = useState(current);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post(`/pools/${poolId}/keys/${poolKey.id}/expiry`, {
        expires_at: date ? new Date(date).getTime() : null,
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to save");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(next: boolean) => !next && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">
            Expiry · {poolKey.key_prefix}
          </DialogTitle>
          <DialogDescription>
            The gateway rejects expired keys at auth. Clear the field to make
            the key non-expiring.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={save} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="expiry-date">Expires on (local midnight)</Label>
            <Input
              id="expiry-date"
              type="date"
              value={date}
              min={new Date().toISOString().slice(0, 10)}
              onChange={(e) => setDate(e.target.value)}
            />
          </div>
          {error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : null}
          <DialogFooter>
            <Button variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? "Saving…" : "Save expiry"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Plant honey keys: the raw baits are revealed exactly once. */
function PlantCanariesDialog({
  poolId,
  onClose,
  onPlanted,
}: {
  poolId: string;
  onClose: () => void;
  onPlanted: () => void;
}) {
  const [count, setCount] = useState("3");
  const [planted, setPlanted] = useState<PlantedCanaryKeys | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function plant(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      setPlanted(
        await api.post<PlantedCanaryKeys>(`/pools/${poolId}/canary-keys`, {
          count: Number(count),
        }),
      );
      onPlanted();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Planting failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(next: boolean) => !next && onClose()}>
      <DialogContent>
        {planted ? (
          <div className="space-y-4">
            <DialogHeader>
              <DialogTitle className="font-display">
                {planted.keys.length} honey key
                {planted.keys.length === 1 ? "" : "s"} planted
              </DialogTitle>
              <DialogDescription>
                Drop these wherever key lists get shared or scraped. Their
                use anywhere fires an alert and NEVER serves traffic. The
                raws below appear exactly once.
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2">
              {planted.keys.map((key) => (
                <SecretReveal
                  key={key.id}
                  secret={key.raw}
                  label={`Honey key ${key.key_prefix}`}
                />
              ))}
            </div>
          </div>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="font-display">
                Plant honey keys
              </DialogTitle>
              <DialogDescription>
                Decoy keys that ride the shared listing shape-identically
                (backdated into the pool&rsquo;s issuance history).
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={plant} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="canary-count">How many (1–10)</Label>
                <Input
                  id="canary-count"
                  type="number"
                  min={1}
                  max={10}
                  value={count}
                  onChange={(e) => setCount(e.target.value)}
                  required
                />
              </div>
              {error ? (
                <p className="text-sm text-destructive">{error}</p>
              ) : null}
              <DialogFooter>
                <Button variant="outline" onClick={onClose}>
                  Cancel
                </Button>
                <Button type="submit" disabled={busy}>
                  {busy ? "Planting…" : "Plant"}
                </Button>
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Add-canary-alias form row (inline, not a dialog). */
function AddCanaryModelForm({
  poolId,
  members,
  onAdded,
}: {
  poolId: string;
  members: PublicMember[];
  onAdded: () => void;
}) {
  const [alias, setAlias] = useState("");
  const [scope, setScope] = useState("pool");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post(`/pools/${poolId}/canary-models`, {
        alias,
        user_id: scope === "pool" ? null : scope,
      });
      toast.success(`Canary alias ${alias} armed`);
      setAlias("");
      onAdded();
    } catch (err) {
      toast.error(err instanceof ApiError ? err.message : "Failed to add");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex items-end gap-2">
      <div className="flex-1 space-y-1">
        <Label htmlFor="canary-alias">Alias</Label>
        <Input
          id="canary-alias"
          value={alias}
          onChange={(e) => setAlias(e.target.value)}
          placeholder="gpt-4o-preview-canary"
          className="font-mono text-xs"
          required
        />
      </div>
      <div className="space-y-1">
        <Label htmlFor="canary-scope">Visible to</Label>
        <Select value={scope} onValueChange={setScope}>
          <SelectTrigger id="canary-scope" className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="pool">Everyone in pool</SelectItem>
            {members.map((member) => (
              <SelectItem key={member.id} value={member.id}>
                {member.email}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <Button type="submit" disabled={busy}>
        <Plus className="size-4" aria-hidden />
        Arm
      </Button>
    </form>
  );
}

export default function PoolDetailPage() {
  const params = useParams<{ id: string }>();
  const poolId = params.id;
  const pool = useApi<PublicPool>(`/pools/${poolId}`);
  // GET /members REQUIRES ?org_id= (org-scoped) — the pool row names it.
  const orgId = pool.data?.org_id ?? null;
  const members = useApi<PublicMember[]>(
    orgId ? `/members?org_id=${orgId}` : null,
  );
  // M4: the keys tab reads the MANAGEMENT view (kind/owner/expiry live
  // here, and here ONLY — the shared listing is shape-uniform by design).
  const keys = useApi<ManagedPoolKey[]>(`/pools/${poolId}/keys/manage`);
  const canaries = useApi<CanaryManagementView>(
    `/pools/${poolId}/canaries`,
  );
  const [allowlistKey, setAllowlistKey] = useState<ManagedPoolKey | null>(null);
  const [rotateKey, setRotateKey] = useState<ManagedPoolKey | null>(null);
  const [expiryKey, setExpiryKey] = useState<ManagedPoolKey | null>(null);
  const [planting, setPlanting] = useState(false);
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
          <TabsTrigger value="canaries">Canaries</TabsTrigger>
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
                  <TableHead>Expires</TableHead>
                  <TableHead>Issued</TableHead>
                  <TableHead className="w-40" aria-label="Actions" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {(keys.data ?? []).map((key) => (
                  <TableRow key={key.id} className={key.kind === "canary" ? "bg-amber-500/5" : undefined}>
                    <TableCell>
                      {key.kind === "canary" ? (
                        <span className="inline-flex items-center gap-1.5 text-amber-600 dark:text-amber-400">
                          <Fingerprint className="size-3" aria-hidden />
                          honey (no owner)
                        </span>
                      ) : (
                        memberName(key.user_id)
                      )}
                    </TableCell>
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
                    <TableCell className="text-xs">
                      {key.expires_at ? (
                        <span className="inline-flex items-center gap-1.5">
                          <CalendarClock className="size-3 text-muted-foreground" aria-hidden />
                          {formatDateTime(key.expires_at)}
                        </span>
                      ) : (
                        "—"
                      )}
                    </TableCell>
                    <TableCell>{formatDateTime(key.created_at)}</TableCell>
                    <TableCell>
                      {key.status === "active" || key.status === "suspended" ? (
                        <div className="flex gap-1">
                          {key.kind === "live" ? (
                            <>
                              <Button
                                variant="ghost"
                                size="icon"
                                aria-label="Rotate key"
                                title="Rotate"
                                onClick={() => setRotateKey(key)}
                              >
                                <RotateCcw className="size-4" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                aria-label="Set expiry"
                                title="Expiry"
                                onClick={() => setExpiryKey(key)}
                              >
                                <CalendarClock className="size-4" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                aria-label="IP allowlist"
                                title="IP allowlist"
                                onClick={() => setAllowlistKey(key)}
                              >
                                <ShieldCheck className="size-4" />
                              </Button>
                            </>
                          ) : null}
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
                        </div>
                      ) : null}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
          {allowlistKey ? (
            <AllowlistDialog
              poolId={poolId}
              poolKey={allowlistKey}
              onClose={() => setAllowlistKey(null)}
            />
          ) : null}
          {rotateKey ? (
            <RotateKeyDialog
              poolId={poolId}
              poolKey={rotateKey}
              onClose={() => setRotateKey(null)}
              onRotated={keys.reload}
            />
          ) : null}
          {expiryKey ? (
            <ExpiryDialog
              poolId={poolId}
              poolKey={expiryKey}
              onClose={() => setExpiryKey(null)}
              onSaved={keys.reload}
            />
          ) : null}
        </TabsContent>

        {/* ---- Canaries (honey keys + model aliases) ---- */}
        <TabsContent value="canaries" className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 font-display text-base">
                <Fingerprint className="size-4 text-amber-600 dark:text-amber-400" aria-hidden />
                Honey keys
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <p className="text-sm text-muted-foreground">
                Decoy keys shaped exactly like real ones in every listing.
                Anyone using one anywhere trips an instant alert — and they
                never serve a single request. This admin view is the only
                place that can tell them apart.
              </p>
              <div className="flex justify-end">
                <Button variant="outline" onClick={() => setPlanting(true)}>
                  <Plus className="size-4" aria-hidden />
                  Plant honey keys
                </Button>
              </div>
              {(canaries.data?.keys ?? []).length === 0 ? (
                <EmptyState
                  title="No honey keys planted"
                  hint="Plant a few and leak one on purpose — the alert proves the tripwire."
                />
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Key</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Backdated to</TableHead>
                      <TableHead className="w-12" aria-label="Actions" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(canaries.data?.keys ?? []).map((key) => (
                      <TableRow key={key.id}>
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
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label="Revoke honey key"
                            onClick={() =>
                              run(
                                () => api.del(`/pools/${poolId}/keys/${key.id}`),
                                `Honey key ${key.key_prefix} revoked`,
                                () => {
                                  keys.reload();
                                  canaries.reload();
                                },
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
              {planting ? (
                <PlantCanariesDialog
                  poolId={poolId}
                  onClose={() => setPlanting(false)}
                  onPlanted={() => {
                    keys.reload();
                    canaries.reload();
                  }}
                />
              ) : null}
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle className="font-display text-base">
                Canary model aliases
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <p className="text-sm text-muted-foreground">
                Fake model names that ride the member&rsquo;s /v1/models
                listing. Requesting one is denied locally with the same
                message as any disallowed model — and fires an alert: the
                model list was exfiltrated.
              </p>
              <AddCanaryModelForm
                poolId={poolId}
                members={members.data ?? []}
                onAdded={canaries.reload}
              />
              {(canaries.data?.models ?? []).length === 0 ? (
                <EmptyState
                  title="No canary aliases"
                  hint="Arm one that looks like a tempting unreleased model."
                />
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Alias</TableHead>
                      <TableHead>Visible to</TableHead>
                      <TableHead>Armed</TableHead>
                      <TableHead className="w-12" aria-label="Actions" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {(canaries.data?.models ?? []).map((model: CanaryModel) => (
                      <TableRow key={model.id}>
                        <TableCell className="font-mono text-xs">
                          {model.alias}
                        </TableCell>
                        <TableCell className="text-xs">
                          {model.user_id
                            ? memberName(model.user_id)
                            : "Everyone in pool"}
                        </TableCell>
                        <TableCell>{formatDateTime(model.created_at)}</TableCell>
                        <TableCell>
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label="Remove alias"
                            onClick={() =>
                              run(
                                () =>
                                  api.del(
                                    `/pools/${poolId}/canary-models/${model.id}`,
                                  ),
                                `Alias ${model.alias} removed`,
                                canaries.reload,
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
            </CardContent>
          </Card>
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
            <AddBudgetDialog
              poolId={poolId}
              members={members.data ?? []}
              onAdded={rules.reload}
            />
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
