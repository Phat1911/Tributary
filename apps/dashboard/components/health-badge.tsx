// Provider-key health pill, shared by the host overview and pool pages.
"use client";

import { Badge } from "@/components/ui/badge";

export function HealthBadge({ health }: { health: string }) {
  const variant =
    health === "ok"
      ? "default"
      : health === "invalid"
        ? "destructive"
        : "secondary";
  return <Badge variant={variant}>{health}</Badge>;
}
