// Inline-SVG sparkline over daily usage totals — deliberately chart-lib
// free (workflow step 10): a polyline + area fill is all a "last 14 days"
// trend needs. Data comes pre-aggregated from /api/v1/usage/summary.
"use client";

import { formatTokens } from "@/lib/format";
import type { DailyUsage } from "@/lib/api";

const W = 560;
const H = 72;
const PAD = 6;

export function UsageSparkline({ days }: { days: DailyUsage[] }) {
  if (days.length === 0) {
    return (
      <div className="flex h-[72px] items-center justify-center text-xs text-muted-foreground">
        No usage in this window yet.
      </div>
    );
  }
  const max = Math.max(...days.map((d) => d.tokens), 1);
  const stepX = (W - PAD * 2) / Math.max(days.length - 1, 1);
  // Points: tokens on the y axis (pad to keep the stroke inside the box).
  const points = days.map((d, i) => {
    const x = PAD + i * stepX;
    const y = H - PAD - ((d.tokens / max) * (H - PAD * 2));
    return [x, y] as const;
  });
  const line = points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = `${PAD},${H - PAD} ${line} ${W - PAD},${H - PAD}`;

  return (
    <figure aria-label="Daily token usage sparkline">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="h-[72px] w-full"
        preserveAspectRatio="none"
        role="img"
      >
        <polygon points={area} fill="var(--primary)" fillOpacity="0.12" />
        <polyline
          points={line}
          fill="none"
          stroke="var(--primary)"
          strokeWidth="2"
          strokeLinejoin="round"
          strokeLinecap="round"
        />
      </svg>
      <figcaption className="mt-1 text-xs text-muted-foreground">
        Peak day: {formatTokens(max)} tokens · {days.length} day
        {days.length === 1 ? "" : "s"} with activity
      </figcaption>
    </figure>
  );
}
