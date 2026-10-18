// One-time raw-secret reveal: the ONLY place a raw pool key may render.
// Shown immediately after issuance, behind an explicit reveal toggle with
// a copy button and a "we cannot show it again" warning. Masked display
// (prefix + last-4) is the durable form everywhere else.
"use client";

import { Check, Copy, Eye, EyeOff, KeyRound } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";

export function SecretReveal({
  secret,
  label,
}: {
  secret: string;
  label: string;
}) {
  const [visible, setVisible] = useState(false);
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(secret);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard API can be unavailable (insecure context); the raw text
      // is selectable as the fallback.
    }
  }

  return (
    <div className="space-y-2 rounded-lg border border-primary/30 bg-primary/5 p-3">
      <div className="flex items-center gap-2 text-sm font-medium">
        <KeyRound className="size-4 text-primary" aria-hidden />
        {label}
      </div>
      {/* Wrap the secret (break-all) instead of truncating: a revealed key
          must be fully readable, and nowrap here bursts the dialog's width. */}
      <div className="flex items-start gap-2">
        <code className="min-w-0 flex-1 break-all rounded bg-background px-2 py-1.5 font-mono text-xs">
          {visible ? secret : "•".repeat(Math.min(secret.length, 40))}
        </code>
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label={visible ? "Hide secret" : "Reveal secret"}
          onClick={() => setVisible((v) => !v)}
        >
          {visible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label="Copy secret"
          onClick={copy}
        >
          {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Shown once — copy it now. It is stored only as a hash and cannot be
        recovered later.
      </p>
    </div>
  );
}
