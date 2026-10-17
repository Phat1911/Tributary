// Login: the one public page. Authenticates against /auth/login — the
// response sets the secure HttpOnly session cookie AND returns the user, so
// we can route by role without a second round-trip.
"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import { Waves } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError, type PublicUser } from "@/lib/api";

const HOME_BY_ROLE: Record<string, string> = {
  host_admin: "/host",
  org_admin: "/org",
  member: "/me",
};

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const { user } = await api.post<{ user: PublicUser }>("/auth/login", {
        email,
        password,
      });
      toast.success(`Welcome back, ${user.email}`);
      router.replace(HOME_BY_ROLE[user.role] ?? "/me");
    } catch (err) {
      if (err instanceof ApiError && err.code === "rate_limited") {
        setError(err.message);
      } else if (err instanceof ApiError && err.status === 401) {
        setError("Invalid email or password.");
      } else {
        setError(
          "Could not reach the control plane. Is `serve` running on :8080?",
        );
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="flex min-h-dvh items-center justify-center p-6">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex flex-col items-center gap-2 text-center">
          <span className="flex size-11 items-center justify-center rounded-xl bg-primary/15 text-primary">
            <Waves className="size-5" aria-hidden />
          </span>
          <h1 className="font-display text-2xl font-semibold tracking-tight">
            Tributary
          </h1>
          <p className="text-sm text-muted-foreground">
            Small streams. Pooled power.
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle className="font-display">Sign in</CardTitle>
            <CardDescription>
              Use your Tributary account (admin or member).
            </CardDescription>
          </CardHeader>
          <form onSubmit={onSubmit}>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="email">Email</Label>
                <Input
                  id="email"
                  type="email"
                  autoComplete="username"
                  placeholder="you@example.com"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="password">Password</Label>
                <Input
                  id="password"
                  type="password"
                  autoComplete="current-password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                />
              </div>
              {error ? (
                <p className="text-sm text-destructive" role="alert">
                  {error}
                </p>
              ) : null}
            </CardContent>
            <CardFooter className="mt-6">
              <Button className="w-full" type="submit" disabled={busy}>
                {busy ? "Signing in…" : "Sign in"}
              </Button>
            </CardFooter>
          </form>
        </Card>

        <p className="text-center text-xs text-muted-foreground">
          Failed logins are throttled (5 per 15 min per email + IP).
        </p>
      </div>
    </main>
  );
}
