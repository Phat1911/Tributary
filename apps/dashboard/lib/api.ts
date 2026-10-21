// Central control-plane client. Requests go same-origin to /api/v1 — in
// dev, next.config.ts rewrites proxy them to the local `serve`; in
// production the deployment puts the dashboard behind the same edge. The
// session cookie (Path=/api/v1) rides along automatically.

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api/v1${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  if (response.status === 204) {
    return undefined as T;
  }
  const body = await response.json().catch(() => null);
  if (!response.ok) {
    // Normalize the control plane's {"error": {message, code}} envelope.
    const message = body?.error?.message ?? response.statusText;
    const code = body?.error?.code ?? "unknown";
    throw new ApiError(response.status, code, message);
  }
  return body as T;
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: "POST", body: JSON.stringify(body ?? {}) }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: "PUT", body: JSON.stringify(body ?? {}) }),
  del: (path: string) => request<void>(path, { method: "DELETE" }),
};

// ---- Shared types (mirror the Rust view structs; the API is the contract) ----

export interface PublicUser {
  id: string;
  email: string;
  role: "host_admin" | "org_admin" | "member";
  org_id: string | null;
  created_at: number;
}

export interface PublicOrg {
  id: string;
  name: string;
  slug: string;
  created_at: number;
}

export interface PublicPool {
  id: string;
  org_id: string;
  name: string;
  kind: string;
  model_allowlist: string[];
  status: string;
  currency: string;
  created_at: number;
}

export interface PublicMember {
  id: string;
  email: string;
  role: string;
  org_id: string | null;
  created_at: number;
}

export interface PublicPoolKey {
  id: string;
  pool_id: string;
  key_prefix: string;
  user_id: string | null;
  status: string;
  created_at: number;
}

export interface OwnPoolKey {
  id: string;
  pool_id: string;
  pool_name: string;
  key_prefix: string;
  status: string;
  created_at: number;
}

export interface IssuedKey {
  id: string;
  pool_id: string;
  user_id: string;
  raw: string;
  key_prefix: string;
}

export interface PublicProviderKey {
  id: string;
  pool_id: string;
  provider: string;
  base_url: string;
  key_prefix: string;
  health: string;
  last_validated_at: number | null;
  status: string;
  created_at: number;
}

export interface QuotaRule {
  id: string;
  subject_kind: string;
  subject_id: string;
  period: string;
  limit_tokens: number | null;
  limit_requests: number | null;
  limit_cost: number;
  created_at: number;
}

export interface UsageEvent {
  id: string;
  ts: number;
  pool_id: string | null;
  pool_name: string | null;
  pool_key_id: string;
  key_prefix: string;
  user_id: string | null;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  cost_micros: number;
  latency_ms: number;
  status: string;
}

export interface UsageList {
  events: UsageEvent[];
}

export interface DailyUsage {
  day_start_ms: number;
  requests: number;
  tokens: number;
}

export interface UsageSummary {
  days: DailyUsage[];
  totals: { requests: number; tokens: number };
}

export interface AuditRow {
  id: string;
  org_id: string | null;
  actor_id: string;
  actor_email: string;
  action: string;
  target_kind: string;
  target_id: string;
  before: unknown;
  after: unknown;
  ip: string | null;
  ts: number;
}

// ---- M4: leak prevention (SPEC §9) ----

/** Admin management view of a pool key — the only surface carrying owner. */
export interface ManagedPoolKey {
  id: string;
  pool_id: string;
  key_prefix: string;
  user_id: string | null;
  status: string; // active | revoked | suspended | rotated
  expires_at: number | null;
  created_at: number;
}

/** Rotate response: the new raw key is shown exactly once. */
export interface RotatedKey {
  id: string;
  raw: string;
  key_prefix: string;
}

export interface IpAllowlistEntry {
  id: string;
  pool_key_id: string;
  cidr: string;
  note: string | null;
  created_at: number;
}

export interface ReviewFlag {
  id: string;
  org_id: string;
  pool_id: string | null;
  pool_key_id: string | null;
  user_id: string | null;
  kind: string; // classifier_flag | classifier_auto_block | impossible_travel | distinct_ip_concurrency
  severity: string;
  score_bp: number | null;
  detail_json: Record<string, unknown>;
  status: string;
  created_at: number;
  resolved_at: number | null;
  resolved_by: string | null;
}

export interface ClassifierConfig {
  org_id: string;
  base_url: string;
  /** Wire contract: "jev" (TypeSafe systemone) | "custom" (generic). */
  dialect: "jev" | "custom";
  model: string;
  key_prefix: string;
  purpose_statement: string;
  mode: "alert" | "auto_block";
  alert_threshold: number;
  auto_block_threshold: number;
  auto_block_live: boolean;
  shadow_started_at: number | null;
  sampling_per_hour: number;
  updated_at: number;
  shadow_would_have_blocked: number;
  shadow_distinct_members: number;
}

/** PUT body for the classifier config. */
export interface ClassifierConfigInput {
  base_url: string;
  dialect?: "jev" | "custom";
  model: string;
  key: string;
  purpose_statement: string;
  mode?: "alert" | "auto_block";
  acknowledge_auto_block?: boolean;
}

/** The member-facing disclosure banner state (GET /me/disclosure). */
export interface Disclosure {
  classification_active: boolean;
  mode: "alert" | "auto_block" | null;
  auto_block_live: boolean;
}
