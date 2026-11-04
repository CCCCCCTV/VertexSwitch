// 与核心返回的 JSON 对应（见 docs/SPEC.md §12）。

export type Tier = "standard" | "flex" | "priority";

export interface ServerInfo {
  running: boolean;
  addr: string | null;
  error: string | null;
}

export interface Status {
  status: "ok" | "degraded";
  version: string;
  region: string;
  gemini_region: string;
  gemini_tier: Tier;
  google_token: string;
  project_id?: string;
  listen: string;
  uptime_secs: number;
  in_flight: number;
  models: { claude: number; gemini: number; maas: number; ollama: number };
  config_path: string | null;
  server: ServerInfo;
}

export interface Settings {
  server: { host: string; port: number; api_key: string; keep_alive_seconds: number };
  google: {
    credentials_file: string;
    project_id: string;
    region: string;
    token_refresh_minutes: number;
  };
  timeouts: { connect: number; read: number; stream_idle: number; write: number; pool: number };
  retry: { attempts: number; backoff_seconds: number; quota_hang_seconds: number };
  claude: { anthropic_version: string; drop_body_fields: string[] };
  gemini: {
    region: string;
    tier: Tier;
    shared_only: boolean;
    flex_server_timeout: number;
    flex_unsupported_models: string[];
    openai_api_version: string;
  };
  maas: { region: string; api_version: string };
  ollama: { enabled: boolean; base_url: string };
  openai: { default_max_tokens: number };
}

export interface PatchResult {
  settings: Settings;
  restart_required: boolean;
  restart_error?: string | null;
  server?: ServerInfo;
}

export interface LogEntry {
  seq: number;
  ts_ms: number;
  level: "ERROR" | "WARN" | "INFO";
  target: string;
  message: string;
}

export type Outcome = "ok" | "error" | "stream_error" | "cancelled";

export interface RequestRecord {
  seq: number;
  ts_ms: number;
  method: string;
  path: string;
  protocol: "anthropic" | "gemini" | "openai";
  model: string | null;
  backend: string | null;
  tier: Tier | null;
  stream: boolean;
  status: number;
  duration_ms: number;
  input_tokens: number | null;
  output_tokens: number | null;
  outcome: Outcome;
  error: string | null;
}

export interface MetricsRow {
  protocol: string;
  backend: string;
  status: number;
  requests: number;
  duration_ms: number;
  input_tokens: number;
  output_tokens: number;
}

export interface Metrics {
  uptime_secs: number;
  in_flight: number;
  totals: MetricsRow[];
}

export interface Models {
  claude: Record<string, string>;
  gemini: Record<string, string>;
  maas: Record<string, string>;
  ollama: string[];
  source: string;
  user_file: string | null;
}

export interface TestResult {
  ok: boolean;
  status: number;
  latency_ms: number;
  reply: string | null;
  error: { message?: string } | null;
}

export interface AppInfo {
  version: string;
  config_dir: string;
  config_path: string | null;
  admin_token_location: string;
}

/** 窗口背景材质（Windows 11）：Mica、Mica Alt、亚克力，或不透明 */
export type Backdrop = "mica" | "tabbed" | "acrylic" | "none";

/** 实时取色的范围：整张壁纸，或窗口后方（包括后面的其他应用） */
export type LiveScope = "wallpaper" | "behind";

/** 一次实时取色的结果：种子色，加上一张按位置排列的颜色场（cols × rows，按行） */
export interface BackdropSample {
  seed: string;
  cols: number;
  rows: number;
  field: string[];
}
