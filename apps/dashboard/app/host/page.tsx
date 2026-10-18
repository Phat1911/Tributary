// Host admin home (SPEC §7 host surface): every org, every pool, provider
// key health at a glance. Hosts see the whole instance; org pages handle
// the per-org management.
"use client";

import Link from "next/link";
import { useState } from "react";
import { Building2, Plus, UserCog } from "lucide-react";
import { toast } from "sonner";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { Skeleton } from "@/components/ui/skeleton";
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
  type PublicOrg,
  type PublicPool,
  type PublicProviderKey,
} from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useApi } from "@/lib/use-api";

import { HealthBadge } from "@/components/health-badge";
/**
 * Per-pool key-health cell. One request per pool (M3 scale: a handful of
 * pools) — the control API has no bulk endpoint by design; revisit if the
 * host table ever paginates past ~50 pools.
 */
function PoolHealthCell({ poolId }: { poolId: string }) {
  const keys = useApi<PublicProviderKey[]>(`/pools/${poolId}/provider-keys`);
  if (keys.loading) return <Skeleton className="h-5 w-24" />;
  if (keys.error) return <span className="text-xs text-destructive">?</span>;
  const counts = { ok: 0, invalid: 0, unknown: 0 };
  for (const key of keys.data ?? []) {
    if (key.status !== "active") continue;
    counts[key.health as keyof typeof counts] += 1;
  }
  const total = counts.ok + counts.invalid + counts.unknown;
  if (total === 0) {
    return <span className="text-xs text-muted-foreground">no keys</span>;
  }
  return (
    <span className="flex gap-1">
      {counts.ok > 0 ? <HealthBadge health="ok" /> : null}
      {counts.invalid > 0 ? <HealthBadge health="invalid" /> : null}
      {counts.unknown > 0 ? <HealthBadge health="unknown" /> : null}
    </span>
  );
}

/**
 * Hosts mint each org's FIRST admin here (POST /members with
 * role=org_admin — the only escalation path; org_admins themselves can
 * only create plain members, enforced server-side).
 */
function AddOrgAdminButton({ org }: { org: PublicOrg }) {
  const [open, setOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post("/members", {
        org_id: org.id,
        email,
        password,
        role: "org_admin",
      });
      toast.success(`${email} is now ${org.name}'s admin`);
      setOpen(false);
      setEmail("");
      setPassword("");
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to create admin",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Button
        variant="outline"
        size="sm"
        className="gap-1.5"
        onClick={() => setOpen(true)}
      >
        <UserCog className="size-4" aria-hidden />
        Add admin
      </Button>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">
            Admin for {org.name}
          </DialogTitle>
          <DialogDescription>
            They sign in and manage {org.name}&apos;s members, pools, and
            keys.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="admin-email">Email</Label>
            <Input
              id="admin-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="lead@acme.com"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="admin-password">Temporary password</Label>
            <Input
              id="admin-password"
              type="text"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="min. 8 characters"
              minLength={8}
              required
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Creating…" : "Create org admin"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CreateOrgDialog({ onCreated }: { onCreated: () => void }) {
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post("/orgs", { name, slug });
      toast.success(`Organization "${name}" created`);
      setOpen(false);
      setName("");
      setSlug("");
      onCreated();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to create org",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <Plus className="size-4" aria-hidden />
          New organization
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">New organization</DialogTitle>
          <DialogDescription>
            An org holds members and pools. Its admins manage day-to-day use.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="org-name">Name</Label>
            <Input
              id="org-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Acme Labs"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="org-slug">Slug</Label>
            <Input
              id="org-slug"
              value={slug}
              onChange={(e) => setSlug(e.target.value)}
              placeholder="acme-labs"
              pattern="[a-z0-9\-]+"
              required
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default function HostPage() {
  const orgs = useApi<PublicOrg[]>("/orgs");
  const pools = useApi<PublicPool[]>("/pools");

  return (
    <AppShell>
      <PageHeader
        title="Instance overview"
        description="Every organization, pool, and provider key on this deployment."
        actions={<CreateOrgDialog onCreated={orgs.reload} />}
      />

      <section className="space-y-3">
        <h2 className="flex items-center gap-2 font-display text-lg font-medium">
          <Building2 className="size-4 text-primary" aria-hidden />
          Organizations
        </h2>
        {orgs.loading ? (
          <TableSkeleton />
        ) : orgs.error ? (
          <ErrorState message={orgs.error.message} onRetry={orgs.reload} />
        ) : (orgs.data ?? []).length === 0 ? (
          <EmptyState
            title="No organizations yet"
            hint="Create the first org to start handing out pool access."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Slug</TableHead>
                <TableHead>Created</TableHead>
                <TableHead aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {(orgs.data ?? []).map((org) => (
                <TableRow key={org.id}>
                  <TableCell className="font-medium">{org.name}</TableCell>
                  <TableCell className="font-mono text-xs">{org.slug}</TableCell>
                  <TableCell>{formatDate(org.created_at)}</TableCell>
                  <TableCell>
                    <AddOrgAdminButton org={org} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="font-display text-lg font-medium">Pools</h2>
        {pools.loading ? (
          <TableSkeleton />
        ) : pools.error ? (
          <ErrorState message={pools.error.message} onRetry={pools.reload} />
        ) : (pools.data ?? []).length === 0 ? (
          <EmptyState
            title="No pools"
            hint="Pools appear once org admins create them."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Pool</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Models</TableHead>
                <TableHead>Key health</TableHead>
                <TableHead>Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(pools.data ?? []).map((pool) => (
                <TableRow key={pool.id}>
                  <TableCell className="font-medium">
                    <Link
                      href={`/org/pools/${pool.id}`}
                      className="hover:text-primary hover:underline"
                    >
                      {pool.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={pool.status === "active" ? "default" : "secondary"}
                    >
                      {pool.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="max-w-64 truncate font-mono text-xs">
                    {pool.model_allowlist.join(", ")}
                  </TableCell>
                  <TableCell>
                    <PoolHealthCell poolId={pool.id} />
                  </TableCell>
                  <TableCell>{formatDate(pool.created_at)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>
    </AppShell>
  );
}
