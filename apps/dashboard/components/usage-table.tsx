// The usage event table shared by admin (per pool/member) and member views.
// Rows are already scoped server-side; this component is presentation only.
"use client";

import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { EmptyState, TableSkeleton } from "@/components/data-state";
import { formatDateTime, formatMicroUsd, formatTokens } from "@/lib/format";
import type { UsageEvent } from "@/lib/api";

export function UsageTable({
  events,
  loading,
  showPool,
}: {
  events: UsageEvent[];
  loading: boolean;
  showPool?: boolean;
}) {
  if (loading) return <TableSkeleton rows={6} />;
  if (events.length === 0) {
    return (
      <EmptyState
        title="No usage yet"
        hint="Metered requests appear here as members use their keys."
      />
    );
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>When</TableHead>
          {showPool ? <TableHead>Pool</TableHead> : null}
          <TableHead>Key</TableHead>
          <TableHead>Model</TableHead>
          <TableHead className="text-right">Prompt</TableHead>
          <TableHead className="text-right">Completion</TableHead>
          <TableHead className="text-right">Cost</TableHead>
          <TableHead className="text-right">Latency</TableHead>
          <TableHead>Status</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {events.map((event) => (
          <TableRow key={event.id}>
            <TableCell className="whitespace-nowrap">
              {formatDateTime(event.ts)}
            </TableCell>
            {showPool ? (
              <TableCell>{event.pool_name ?? "—"}</TableCell>
            ) : null}
            <TableCell className="font-mono text-xs">
              {event.key_prefix}
            </TableCell>
            <TableCell className="font-mono text-xs">{event.model}</TableCell>
            <TableCell className="text-right">
              {formatTokens(event.prompt_tokens)}
            </TableCell>
            <TableCell className="text-right">
              {formatTokens(event.completion_tokens)}
            </TableCell>
            <TableCell className="text-right">
              {formatMicroUsd(event.cost_micros)}
            </TableCell>
            <TableCell className="text-right">{event.latency_ms} ms</TableCell>
            <TableCell>{event.status}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
