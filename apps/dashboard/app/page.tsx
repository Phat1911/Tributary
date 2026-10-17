// Landing = router by role. The control API is the authority (cookie auth);
// the page is a thin client that redirects to the right home.
"use client";

import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { api, type PublicUser } from "@/lib/api";

const HOME_BY_ROLE: Record<string, string> = {
  host_admin: "/host",
  org_admin: "/org",
  member: "/me",
};

export default function Home() {
  const router = useRouter();
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api
      .get<PublicUser>("/auth/me")
      .then((user) => router.replace(HOME_BY_ROLE[user.role] ?? "/me"))
      .catch(() => router.replace("/login"));
    // A stuck fetch should still show something (very slow networks).
    const timer = setTimeout(() => setFailed(true), 8000);
    return () => clearTimeout(timer);
  }, [router]);

  return (
    <main className="flex min-h-dvh flex-col items-center justify-center gap-3">
      <h1 className="font-display text-2xl font-semibold tracking-tight">
        Tributary
      </h1>
      <p className="text-sm text-muted-foreground">
        {failed ? "Could not reach the control plane — is `serve` running?" : "…"}
      </p>
    </main>
  );
}
