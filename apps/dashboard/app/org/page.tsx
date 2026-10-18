// Org admin home: members CRUD + the org's pools. Members get keys from the
// pool detail page (/org/pools/[id]); this page is the roster.
"use client";

import Link from "next/link";
import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
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
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
  type PublicMember,
  type PublicPool,
} from "@/lib/api";
import { formatDate } from "@/lib/format";
import { useApi } from "@/lib/use-api";

function AddMemberDialog({
  orgId,
  onAdded,
}: {
  orgId: string | null;
  onAdded: () => void;
}) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      if (!orgId) throw new ApiError(0, "no_org", "No organization found for your account");
      await api.post("/members", { org_id: orgId, email, password });
      toast.success(`Member ${email} added`);
      setOpen(false);
      setEmail("");
      setPassword("");
      onAdded();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to add member",
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
          Add member
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">Add member</DialogTitle>
          <DialogDescription>
            They sign in with these credentials and see only their own keys
            and usage.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="member-email">Email</Label>
            <Input
              id="member-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="teammate@acme.com"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="member-password">Temporary password</Label>
            <Input
              id="member-password"
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
              {busy ? "Adding…" : "Add member"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Delete confirmation with the member's email in the warning. */
function DeleteMemberButton({
  member,
  onDeleted,
}: {
  member: PublicMember;
  onDeleted: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function remove() {
    setBusy(true);
    try {
      await api.del(`/members/${member.id}`);
      toast.success(`Member ${member.email} removed (their keys revoked)`);
      onDeleted();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to remove member",
      );
    } finally {
      setBusy(false);
      setOpen(false);
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`Remove ${member.email}`}
        onClick={() => setOpen(true)}
      >
        <Trash2 className="size-4 text-destructive" />
      </Button>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-display">
            Remove {member.email}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            Their pool keys are revoked and they lose access immediately.
            Usage history stays for accounting.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={remove} disabled={busy}>
            {busy ? "Removing…" : "Remove member"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Creates a pool in the org (org_admin scope, server-enforced). The
 * model allowlist is optional — empty means the deployment default. */
function CreatePoolDialog({
  orgId,
  onCreated,
}: {
  orgId: string | null;
  onCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [models, setModels] = useState("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      if (!orgId) throw new ApiError(0, "no_org", "No organization found for your account");
      // Comma-separated → array; empty input → omit (deployment default).
      const allowlist = models
        .split(",")
        .map((m) => m.trim())
        .filter(Boolean);
      await api.post("/pools", {
        org_id: orgId,
        name,
        ...(allowlist.length > 0 ? { model_allowlist: allowlist } : {}),
      });
      toast.success(`Pool "${name}" created`);
      setOpen(false);
      setName("");
      setModels("");
      onCreated();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Failed to create pool",
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
          New pool
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-display">New pool</DialogTitle>
          <DialogDescription>
            A pool funds one shared upstream budget — members get per-user
            keys into it.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="pool-name">Name</Label>
            <Input
              id="pool-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="default"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="pool-models">Model allowlist (optional)</Label>
            <Input
              id="pool-models"
              value={models}
              onChange={(e) => setModels(e.target.value)}
              placeholder="gpt-4o-mini, gpt-4o — leave empty for the default"
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy}>
              {busy ? "Creating…" : "Create pool"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default function OrgPage() {
  // An org_admin's /orgs returns exactly their org. The members list
  // REQUIRES ?org_id= (the API scopes by it), so it waits for the org.
  const orgs = useApi<{ id: string }[]>("/orgs");
  const orgId = orgs.data?.[0]?.id ?? null;
  const members = useApi<PublicMember[]>(
    orgId ? `/members?org_id=${orgId}` : null,
  );
  const pools = useApi<PublicPool[]>("/pools");

  return (
    <AppShell>
      <PageHeader
        title="Organization"
        description="Your members and pools. Issue keys from a pool's page."
        actions={<AddMemberDialog orgId={orgId} onAdded={members.reload} />}
      />

      <section className="space-y-3">
        <h2 className="font-display text-lg font-medium">Members</h2>
        {members.loading ? (
          <TableSkeleton />
        ) : members.error ? (
          <ErrorState message={members.error.message} onRetry={members.reload} />
        ) : (members.data ?? []).length === 0 ? (
          <EmptyState
            title="No members yet"
            hint="Add your first member, then issue them a pool key."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Email</TableHead>
                <TableHead>Role</TableHead>
                <TableHead>Joined</TableHead>
                <TableHead className="w-12" aria-label="Actions" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {(members.data ?? []).map((member) => (
                <TableRow key={member.id}>
                  <TableCell className="font-medium">{member.email}</TableCell>
                  <TableCell>
                    <Badge variant="secondary">{member.role}</Badge>
                  </TableCell>
                  <TableCell>{formatDate(member.created_at)}</TableCell>
                  <TableCell>
                    <DeleteMemberButton
                      member={member}
                      onDeleted={members.reload}
                    />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="font-display text-lg font-medium">Pools</h2>
          <CreatePoolDialog orgId={orgId} onCreated={pools.reload} />
        </div>
        {pools.loading ? (
          <TableSkeleton />
        ) : pools.error ? (
          <ErrorState message={pools.error.message} onRetry={pools.reload} />
        ) : (pools.data ?? []).length === 0 ? (
          <EmptyState
            title="No pools"
            hint="Create your first pool, then issue member keys into it."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Pool</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Models</TableHead>
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
