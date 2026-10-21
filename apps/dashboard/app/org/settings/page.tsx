"use client";

// Org-level detector settings (D32). Today one knob: the distinct-IP
// concurrency window. Admin-only; every save is audit-logged server-side
// (`org.settings_updated`), so this page never needs its own trail.

import { useState } from "react";
import { Settings2 } from "lucide-react";

import { AppShell, PageHeader } from "@/components/app-shell";
import { ErrorState } from "@/components/data-state";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useApi } from "@/lib/use-api";
import { api } from "@/lib/api";
// Mirror of the control plane's `OrgSettings` row. `updated_at: 0` means
// "compiled defaults, never tuned" — shown as a hint instead of a date.
interface OrgSettings {
  concurrency_window_secs: number;
  updated_at: number;
}

export default function OrgSettingsPage() {
  // The org id comes from the session bootstrap (/auth/me carries org_id).
  const me = useApi<{ org_id: string | null }>("/auth/me");
  const orgId = me.data?.org_id ?? null;
  const settings = useApi<OrgSettings | null>(
    orgId ? `/orgs/${orgId}/settings` : null,
  );
  // Input state is `null` until the user types — before that the field
  // DERIVED-displays the server value (which may arrive late and may be
  // org-tuned). No effect needed: derived-during-render beats
  // setState-in-effect (cascading renders).
  const [typed, setTyped] = useState<string | null>(null);
  const window_ = typed ?? String(settings.data?.concurrency_window_secs ?? 300);
  const setWindow = setTyped;
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (me.error) {
    return (
      <AppShell>
        <ErrorState message={me.error.message} onRetry={me.reload} />
      </AppShell>
    );
  }

  async function save() {
    setBusy(true);
    setSaved(false);
    setError(null);
    try {
      // The clamped contract lives server-side (60–86400); the page
      // only forwards the number and shows the server's verdict.
      await api.put(`/orgs/${orgId}/settings`, {
        concurrency_window_secs: Number(window_),
      });
      setSaved(true);
      settings.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Save failed");
    } finally {
      setBusy(false);
    }
  }

  const current = settings.data?.concurrency_window_secs;
  const tuned = (settings.data?.updated_at ?? 0) > 0;

  return (
    <AppShell>
      <PageHeader
        title="Detection settings"
        description="Org-level knobs for the leak-prevention detectors."
      />
      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 font-display text-base">
              <Settings2 className="size-4 text-primary" aria-hidden />
              Distinct-IP concurrency window
            </CardTitle>
            <CardDescription>
              How far back the detector looks when counting a key&apos;s
              active networks: two different /24 networks inside this many
              seconds = one alert (alert-only — nothing is ever blocked).
              Longer = more sensitive; shorter = only truly simultaneous use
              counts.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="window">Window (seconds, 60–86400)</Label>
              <Input
                id="window"
                type="number"
                min={60}
                max={86400}
                value={window_}
                onChange={(e) => {
                  setWindow(e.target.value);
                  setSaved(false);
                }}
                className="max-w-40"
              />
              <p className="text-muted-foreground text-xs">
                Current value: <strong>{current ?? "…"}</strong>
                {tuned ? " (org-tuned)" : " (compiled default)"}. Changes
                take effect within about 5 seconds and are audit-logged.
              </p>
            </div>
            {error ? <p className="text-destructive text-sm">{error}</p> : null}
            {saved ? (
              <p className="text-sm text-emerald-600">Saved ✓</p>
            ) : null}
            <Button onClick={save} disabled={busy || !orgId}>
              {busy ? "Saving…" : "Save"}
            </Button>
          </CardContent>
        </Card>
      </div>
    </AppShell>
  );
}
