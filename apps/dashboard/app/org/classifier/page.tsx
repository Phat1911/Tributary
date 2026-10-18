// Purpose classifier (SPEC §9 / D28): the org's BYOK alignment checker.
// Config saves only after a live probe; auto-block is opt-in, acknowledged,
// and SHADOW-first — the "would have blocked N" counter has to earn the
// go-live click. Never stores prompts, only scores.
"use client";

import { useState } from "react";
import { Scale, ShieldAlert, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { AppShell, PageHeader } from "@/components/app-shell";
import { ErrorState } from "@/components/data-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  api,
  ApiError,
  type ClassifierConfig,
  type ClassifierConfigInput,
} from "@/lib/api";
import { formatDateTime } from "@/lib/format";
import { useApi } from "@/lib/use-api";

/** The config form (create + edit share it). */
function ClassifierForm({
  orgId,
  existing,
  onSaved,
}: {
  orgId: string;
  existing: ClassifierConfig | null;
  onSaved: () => void;
}) {
  const [baseUrl, setBaseUrl] = useState(existing?.base_url ?? "");
  const [model, setModel] = useState(existing?.model ?? "");
  const [apiKey, setApiKey] = useState("");
  const [purpose, setPurpose] = useState(existing?.purpose_statement ?? "");
  const [mode, setMode] = useState<"alert" | "auto_block">(
    existing?.mode ?? "alert",
  );
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    const body: ClassifierConfigInput = {
      base_url: baseUrl.replace(/\/$/, ""),
      model,
      key: apiKey,
      purpose_statement: purpose,
      mode,
      ...(mode === "auto_block" ? { acknowledge_auto_block: ack } : {}),
    };
    setBusy(true);
    try {
      await api.put<ClassifierConfig>(`/orgs/${orgId}/classifier`, body);
      toast.success(
        mode === "auto_block"
          ? "Saved — auto-block starts in SHADOW. Watch the would-have-blocked counter before going live."
          : "Saved — off-purpose requests will be flagged for review",
      );
      setApiKey("");
      onSaved();
    } catch (err) {
      toast.error(
        err instanceof ApiError ? err.message : "Save failed (nothing changed)",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={save} className="space-y-4">
      <div className="grid gap-4 md:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="clf-base">Classifier base URL</Label>
          <Input
            id="clf-base"
            type="url"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://jev.example.dev/v1"
            required
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="clf-model">Model name</Label>
          <Input
            id="clf-model"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder="jev-align-1"
            required
          />
        </div>
      </div>
      <div className="space-y-2">
        <Label htmlFor="clf-key">
          API key {existing ? "(re-enter to replace)" : ""}
        </Label>
        <Input
          id="clf-key"
          type="password"
          autoComplete="off"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder={existing ? `stored as ${existing.key_prefix}` : "sk-…"}
          required
        />
        <p className="text-xs text-muted-foreground">
          Sealed with envelope encryption; probed ONCE at save — a failing
          probe rejects the save entirely.
        </p>
      </div>
      <div className="space-y-2">
        <Label htmlFor="clf-purpose">Purpose statement</Label>
        <Input
          id="clf-purpose"
          value={purpose}
          onChange={(e) => setPurpose(e.target.value)}
          placeholder="Coding assistance for our team."
          required
        />
        <p className="text-xs text-muted-foreground">
          Members see this in their disclosure banner. Requests are scored
          against it — scores are stored, prompts never are.
        </p>
      </div>
      <div className="space-y-2">
        <Label htmlFor="clf-mode">Mode</Label>
        <Select
          value={mode}
          onValueChange={(v) => setMode(v as "alert" | "auto_block")}
        >
          <SelectTrigger id="clf-mode" className="w-72">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="alert">Alert — flag for review</SelectItem>
            <SelectItem value="auto_block">
              Auto-block — suspend off-purpose keys
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      {mode === "auto_block" ? (
        <div className="flex items-start gap-3 rounded-md border border-amber-500/40 bg-amber-500/5 p-3">
          <input
            id="clf-ack"
            type="checkbox"
            checked={ack}
            onChange={(e) => setAck(e.target.checked)}
            className="mt-1 size-4 accent-primary"
          />
          <Label htmlFor="clf-ack" className="font-normal leading-relaxed">
            I understand: going live later will <strong>suspend</strong> the
            keys of off-purpose requests pending review (one-click unblock,
            never auto-ban). Auto-block always starts in shadow mode.
          </Label>
        </div>
      ) : null}
      <Button type="submit" disabled={busy}>
        {busy ? "Probing & saving…" : existing ? "Update" : "Save"}
      </Button>
    </form>
  );
}

/** Shadow stats + the go-live gate (only meaningful in auto_block). */
function ShadowPanel({
  orgId,
  config,
  onChanged,
}: {
  orgId: string;
  config: ClassifierConfig;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);

  async function goLive() {
    setBusy(true);
    try {
      await api.post(`/orgs/${orgId}/classifier/go-live`);
      toast.success(
        "Auto-block is LIVE — off-purpose keys will be suspended pending review",
      );
      onChanged();
    } catch (err) {
      toast.error(err instanceof ApiError ? err.message : "Failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 font-display text-base">
          <ShieldAlert
            className={
              config.auto_block_live ? "size-4 text-destructive" : "size-4"
            }
            aria-hidden
          />
          Auto-block: {config.auto_block_live ? "LIVE" : "shadow"}
        </CardTitle>
        <CardDescription>
          {config.auto_block_live
            ? "Off-purpose requests suspend the key pending your review (one-click unblock)."
            : "Shadow counts what WOULD have been blocked. Nothing is blocked yet."}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-4">
          <div>
            <p className="text-2xl font-semibold tabular-nums">
              {config.shadow_would_have_blocked}
            </p>
            <p className="text-xs text-muted-foreground">
              would-have-blocked requests
            </p>
          </div>
          <div>
            <p className="text-2xl font-semibold tabular-nums">
              {config.shadow_distinct_members}
            </p>
            <p className="text-xs text-muted-foreground">distinct members</p>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          Shadow since{" "}
          {config.shadow_started_at
            ? formatDateTime(config.shadow_started_at)
            : "—"}{" "}
          · sampling ≈{config.sampling_per_hour}/hour · thresholds: flag below{" "}
          {config.alert_threshold}, block below {config.auto_block_threshold}
        </p>
        {config.mode === "auto_block" && !config.auto_block_live ? (
          <Button variant="destructive" disabled={busy} onClick={goLive}>
            Go live — start blocking
          </Button>
        ) : null}
      </CardContent>
    </Card>
  );
}

export default function ClassifierPage() {
  // The org id comes from the session bootstrap (/auth/me carries org_id).
  const me = useApi<{ org_id: string | null }>("/auth/me");
  const orgId = me.data?.org_id ?? null;
  const config = useApi<ClassifierConfig | null>(
    orgId ? `/orgs/${orgId}/classifier` : null,
  );
  const [removed, setRemoved] = useState(false);

  if (me.error) {
    return (
      <AppShell>
        <ErrorState message={me.error.message} onRetry={me.reload} />
      </AppShell>
    );
  }
  if (!orgId) {
    return (
      <AppShell>
        <PageHeader
          title="Purpose classifier"
          description="Host admins manage classifiers per organization."
        />
      </AppShell>
    );
  }

  const existing =
    config.error || config.data == null || removed ? null : config.data;

  return (
    <AppShell>
      <PageHeader
        title="Purpose classifier"
        description="Optional BYOK alignment checker. Members always see a disclosure banner while it is active."
      />
      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 font-display text-base">
              <Scale className="size-4 text-primary" aria-hidden />
              Configuration
              {existing ? (
                <Badge variant={existing.auto_block_live ? "destructive" : "secondary"}>
                  {existing.mode}
                  {existing.auto_block_live ? " · live" : ""}
                </Badge>
              ) : null}
            </CardTitle>
            <CardDescription>
              Dormant until configured. Requests are sampled and scored
              against the purpose statement; prompt text is never stored.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {config.loading && !config.data ? (
              <p className="text-sm text-muted-foreground">Loading…</p>
            ) : (
              <>
                <ClassifierForm
                  orgId={orgId}
                  existing={existing}
                  onSaved={() => {
                    setRemoved(false);
                    config.reload();
                  }}
                />
                {existing ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={async () => {
                      try {
                        await api.del(`/orgs/${orgId}/classifier`);
                        toast.success("Classifier removed — detection dormant");
                        setRemoved(true);
                        config.reload();
                      } catch (err) {
                        toast.error(
                          err instanceof ApiError ? err.message : "Failed",
                        );
                      }
                    }}
                  >
                    <Trash2 className="size-4 text-destructive" aria-hidden />
                    Remove classifier
                  </Button>
                ) : null}
              </>
            )}
          </CardContent>
        </Card>
        {existing ? (
          <ShadowPanel
            orgId={orgId}
            config={existing}
            onChanged={config.reload}
          />
        ) : null}
      </div>
    </AppShell>
  );
}
