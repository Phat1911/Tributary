import type { NextConfig } from "next";

// Dev wiring (M3): the dashboard is a pure client of the single Rust binary
// — same-origin /api/* requests are proxied to the local `serve`. Production
// packaging (reverse proxy in compose) is M5; nothing here ships.
const controlPlaneUrl = process.env.TRIBUTARY_API_URL ?? "http://127.0.0.1:8080";

// M5 packaging: `pnpm build:export` sets TRIBUTARY_STATIC_EXPORT=1 to emit a
// fully static site (stage 1 of deploy/Dockerfile copies `out/` into the
// runtime image). Rewrites are a Next-server feature and are NOT available
// in export mode — the deployed dashboard keeps calling same-origin
// /api/v1, which whatever serves `out/` (the tributary binary / edge proxy)
// routes to the control plane. Plain `next build` (what CI runs) keeps the
// dev/SSR rewrite behavior unchanged.
const staticExport = process.env.TRIBUTARY_STATIC_EXPORT === "1";

const nextConfig: NextConfig = staticExport
  ? {
      // Static export: no Node server, no rewrites; every page must be
      // statically renderable (all pages here are client components).
      output: "export",
      // trailingSlash makes every page emit `page/index.html` instead of
      // the flat `page.html` — ServeDir in the tributary binary (and any
      // static host) then resolves directory URLs to the index without a
      // `.html` extension map.
      trailingSlash: true,
    }
  : {
      async rewrites() {
        return [
          {
            source: "/api/:path*",
            destination: `${controlPlaneUrl}/api/:path*`,
          },
        ];
      },
    };

export default nextConfig;
