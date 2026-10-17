import type { NextConfig } from "next";

// Dev wiring (M3): the dashboard is a pure client of the single Rust binary
// — same-origin /api/* requests are proxied to the local `serve`. Production
// packaging (reverse proxy in compose) is M5; nothing here ships.
const controlPlaneUrl = process.env.TRIBUTARY_API_URL ?? "http://127.0.0.1:8080";

const nextConfig: NextConfig = {
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
