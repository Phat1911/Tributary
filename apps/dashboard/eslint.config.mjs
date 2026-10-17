// Flat ESLint config (Next 16 dropped `next lint`; the CLI + config is the
// supported path). Two presets: core-web-vitals (a11y, hooks, Next rules)
// and typescript (typed-lint lite — full type checking is tsc's job).
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const config = [
  {
    ignores: [".next/**", "node_modules/**", "next-env.d.ts"],
  },
  ...nextVitals,
  ...nextTs,
];

export default config;
