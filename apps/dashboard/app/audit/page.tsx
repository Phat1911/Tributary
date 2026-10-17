// Audit log viewer (admin): the control-plane trail, newest first, with
// expandable before/after diffs. org_admins see their org's rows only —
// scoped server-side at write time (org_id column).
"use client";

import { useState } from "react";
import { ChevronRight, ScrollText } from "lucide-react";
import { AppShell, PageHeader } from "@/components/app-shell";
import { EmptyState, ErrorState, TableSkeleton } from "@/components/data-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { AuditRow } from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useApi } from "@/lib/use-api";

const PAGE_SIZE = 50;

/** action → tint: destructive-ish verbs read red, sessions neutral. */
function ActionBadge({ action }: { action: string }) {
  const variant = /\.(revoked|deleted|disabled|self_revoked)$/.test(action)
    ? "destructive"
    : action.startsWith("session")
      ? "secondary"
      : "default";
  return (
    <Badge variant={variant} className="font-mono text-xs">
      {action}
    </Badge>
  );
}

/** Collapsible before/after payloads (masked snapshots only). */
function DiffCell({ row }: { row: AuditRow }) {
  const [open, setOpen] = useState(false);
  const hasPayload = row.before != null || row.after != null;
  if (!hasPayload) return <span className="text-muted-foreground">—</span>;
  return (
    <button
      type="button"
      className="flex items-center gap-1 text-xs text-primary hover:underline"
      onClick={() => setOpen((v) => !v)}
      aria-expanded={open}
    >
      <ChevronRight
        className={`size-3 transition-transform ${open ? "rotate-90" : ""}`}
        aria-hidden
      />
      {open ? "hide" : "diff"}
      {open ? (
        <span className="block w-64 space-y-1 rounded-md bg-muted p-2 text-left font-mono text-[10px] leading-relaxed">
          {row.before != null ? (
            <>
              <span className="text-muted-foreground">before</span>
              <br />
              {JSON.stringify(row.before, null, 1)}
            </>
          ) : null}
          {row.after != null ? (
            <>
              <span className="text-muted-foreground">after</span>
              <br />
              {JSON.stringify(row.after, null, 1)}
            </>
          ) : null}
        </span>
      ) : null}
    </button>
  );
}

export default function AuditPage() {
  const [offset, setOffset] = useState(0);
  const rows = useApi<AuditRow[]>(`/audit?limit=${PAGE_SIZE}&offset=${offset}`);
  const page = rows.data ?? [];
  // "Next" shows while a full page came back; "Prev" until the top.
  const canNext = page.length === PAGE_SIZE;

  return (
    <AppShell>
      <PageHeader
        title="Audit log"
        description="Every control-plane mutation: who, what, when, from where."
      />

      {rows.loading ? (
        <TableSkeleton rows={8} />
      ) : rows.error ? (
        <ErrorState message={rows.error.message} onRetry={rows.reload} />
      ) : page.length === 0 && offset === 0 ? (
        <EmptyState
          title="Nothing recorded yet"
          hint="Mutations (creates, revocations, budgets…) appear here as they happen."
        />
      ) : (
        <>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>When</TableHead>
                <TableHead>Actor</TableHead>
                <TableHead>Action</TableHead>
                <TableHead>Target</TableHead>
                <TableHead>IP</TableHead>
                <TableHead aria-label="Diff" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="whitespace-nowrap">
                    {formatDateTime(row.ts)}
                  </TableCell>
                  <TableCell className="max-w-48 truncate">
                    {row.actor_email}
                  </TableCell>
                  <TableCell>
                    <ActionBadge action={row.action} />
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {row.target_kind}/{row.target_id.slice(0, 8)}…
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {row.ip ?? "—"}
                  </TableCell>
                  <TableCell>
                    <DiffCell row={row} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(offset - PAGE_SIZE, 0))}
            >
              Previous
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={!canNext}
              onClick={() => setOffset(offset + PAGE_SIZE)}
            >
              Next
            </Button>
          </div>
        </>
      )}

      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <ScrollText className="size-3.5" aria-hidden />
        Snapshots are sanitized: key material is never stored — only masked
        prefixes (prefix + last-4).
      </p>
    </AppShell>
  );
}
