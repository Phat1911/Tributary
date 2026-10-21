// App chrome: brand, role-aware navigation, session block with logout.
// Every authenticated page renders inside this shell; the /auth/me bootstrap
// doubles as the client-side auth gate (the API enforces the real rules).
"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, type ReactNode } from "react";
import { Settings2,
  KeyRound,
  LogOut,
  Scale,
  ScrollText,
  ShieldAlert,
  Users,
  Waves,
  Wifi,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { api, type PublicUser } from "@/lib/api";
import { useApi } from "@/lib/use-api";
import { cn } from "cn";

interface NavItem {
  href: string;
  label: string;
  icon: ReactNode;
  roles: string[];
}

const NAV: NavItem[] = [
  {
    href: "/host",
    label: "Instance",
    icon: <Wifi className="size-4" aria-hidden />,
    roles: ["host_admin"],
  },
  {
    href: "/org",
    label: "Organization",
    icon: <Users className="size-4" aria-hidden />,
    roles: ["org_admin"],
  },
  {
    href: "/org/review",
    label: "Review queue",
    icon: <ShieldAlert className="size-4" aria-hidden />,
    roles: ["org_admin"],
  },
  {
    href: "/org/classifier",
    label: "Purpose classifier",
    icon: <Scale className="size-4" aria-hidden />,
    roles: ["org_admin"],
  },
  {
    href: "/org/settings",
    label: "Detection settings",
    icon: <Settings2 className="size-4" aria-hidden />,
    roles: ["org_admin"],
  },
  {
    href: "/me",
    label: "My keys & usage",
    icon: <KeyRound className="size-4" aria-hidden />,
    roles: ["member"],
  },
  {
    href: "/audit",
    label: "Audit log",
    icon: <ScrollText className="size-4" aria-hidden />,
    roles: ["host_admin", "org_admin"],
  },
];

export function AppShell({ children }: { children: ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const me = useApi<PublicUser>("/auth/me");

  // Auth gate: unauthenticated sessions belong on /login.
  useEffect(() => {
    if (me.error && me.error.status === 401) router.replace("/login");
  }, [me.error, router]);

  async function logout() {
    try {
      await api.post("/auth/logout");
    } finally {
      // Even if the call fails, drop the local context and bounce to login.
      router.replace("/login");
    }
  }

  const user = me.data;
  const items = NAV.filter((item) => user && item.roles.includes(user.role));

  return (
    <div className="min-h-dvh md:grid md:grid-cols-[220px_1fr]">
      <aside className="border-r bg-sidebar md:sticky md:top-0 md:h-dvh">
        <div className="flex h-full flex-col gap-6 p-4">
          <Link href="/" className="flex items-center gap-2 px-2 py-1">
            <span className="flex size-8 items-center justify-center rounded-lg bg-primary/15 text-primary">
              <Waves className="size-4" aria-hidden />
            </span>
            <span className="font-display text-lg font-semibold tracking-tight">
              Tributary
            </span>
          </Link>

          <nav className="flex flex-col gap-1" aria-label="Main">
            {items.map((item) => (
              <Link
                key={item.href}
                href={item.href}
                className={cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                  pathname.startsWith(item.href) &&
                    "bg-sidebar-accent text-sidebar-accent-foreground",
                )}
              >
                {item.icon}
                {item.label}
              </Link>
            ))}
          </nav>

          <div className="mt-auto space-y-2 border-t pt-4">
            <div className="px-3 text-xs leading-relaxed text-muted-foreground">
              <p className="truncate font-medium text-sidebar-foreground">
                {user?.email ?? "…"}
              </p>
              <p>{user?.role.replace("_", " ") ?? ""}</p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              className="w-full justify-start text-muted-foreground"
              onClick={logout}
            >
              <LogOut className="size-4" aria-hidden />
              Sign out
            </Button>
          </div>
        </div>
      </aside>

      <main className="min-w-0 p-6 md:p-8">
        <div className="mx-auto max-w-5xl space-y-6">{children}</div>
      </main>
    </div>
  );
}

// Page header: consistent title + optional description + actions slot.
export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-3">
      <div>
        <h1 className="font-display text-2xl font-semibold tracking-tight">
          {title}
        </h1>
        {description ? (
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {actions}
    </header>
  );
}
