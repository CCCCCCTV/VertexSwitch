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
  /** 按模型单独设的档位（键为上游模型 ID），没列出的跟随 gemini_tier */
  gemini_model_tiers: Record<string, Tier>;
  /** 区域切换：models 只列离开过平时区域的模型 */
  gemini_region_switch: { enabled: boolean; home: string; models: ModelRegion[] };
  /** 凭证（SPEC §9b）：list 第一个是主凭证；models 只列离开过第一个凭证的模型 */
  credentials: {
    list: CredentialInfo[];
    reset_minutes: number;
    models: ModelCredential[];
  };
  google_token: string;
  project_id?: string;
  listen: string;
  uptime_secs: number;
  in_flight: number;
  models: { claude: number; gemini: number; maas: number };
  config_path: string | null;
  server: ServerInfo;
}

/** 某个模型的区域切换状态（SPEC §9a） */
export interface ModelRegion {
  model: string;
  region: string;
  away_ms: number;
  /** 离重置回平时区域还有多久 */
  reset_in_ms: number;
  recorded_region: string | null;
  recorded_first_byte_ms: number | null;
  recorded_tokens_per_second: number | null;
  /** 已回到记录的区域并停在那里 */
  settled: boolean;
  /** 这个模型没有的区域（404） */
  missing: string[];
}

/** 一个凭证；凭证号从 1 数。项目要等这个凭证取过 token 才知道 */
export interface CredentialInfo {
  credential: number;
  file: string | null;
  project: string | null;
  /** 上次取 token 失败的原因 */
  error: string | null;
}

/** 某个模型在用的备用凭证 */
export interface ModelCredential {
  model: string;
  credential: number;
  away_ms: number;
  reset_in_ms: number;
  /** 取不到 token、重置前跳过的凭证号 */
  broken: number[];
}

export interface BackupCredential {
  credentials_file: string;
  project_id: string;
}

export interface RegionSwitchSettings {
  enabled: boolean;
  regions: string[];
  slow_first_byte_seconds: number;
  slow_tokens_per_second: number;
  reset_minutes: number;
}

export interface Settings {
  server: { host: string; port: number; api_key: string; keep_alive_seconds: number };
  google: {
    credentials_file: string;
    project_id: string;
    region: string;
    token_refresh_minutes: number;
    /** 第一次离开第一个凭证满这么多分钟回到它 */
    credential_reset_minutes: number;
    /** 备用凭证，按顺序换用 */
    backup_credentials: BackupCredential[];
  };
  timeouts: { connect: number; read: number; stream_idle: number; write: number; pool: number };
  retry: {
    attempts: number;
    backoff_seconds: number;
    quota_hang_seconds: number;
    busy: boolean;
    busy_attempts: number;
    busy_wait: string;
  };
  claude: { anthropic_version: string; drop_body_fields: string[] };
  gemini: {
    region: string;
    tier: Tier;
    model_tiers: Record<string, Tier>;
    shared_only: boolean;
    flex_server_timeout: number;
    flex_unsupported_models: string[];
    openai_api_version: string;
    /** OpenAI 格式的 Gemini 请求：auto 需要档位时转原生 / always 总是转 / never 从不转 */
    openai_native: "auto" | "always" | "never";
    /** 区域切换；平时区域就是上面的 region */
    region_switch: RegionSwitchSettings;
  };
  maas: { region: string; api_version: string };
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
  /** 事件附带的字段：kind 标识特定事件（档位降级为 tier_fallback / tier_mismatch），model 为上游模型 */
  fields: Record<string, string>;
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
  /** 实际发往的区域（换过区域时是最后一个） */
  region: string | null;
  /** 实际用的凭证号（从 1 数；换过凭证时是最后一个） */
  credential: number | null;
  status: number;
  duration_ms: number;
  /** 首字耗时：收到请求到发出第一段内容（流式含排队、重试、思考；非流式约等于总耗时）；没发出内容时为 null */
  first_byte_ms: number | null;
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

/** 检查更新的结果：latest 为 GitHub 上最新发布的版本（还没有发布过时为 null） */
export interface UpdateInfo {
  current: string;
  latest: string | null;
  url: string | null;
  newer: boolean;
}

export interface AppInfo {
  version: string;
  config_dir: string;
  config_path: string | null;
  /** 存在文件里时是文件路径；存在系统凭据库里时界面显示固定文字 */
  admin_token_location: string;
  admin_token_in_store: boolean;
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
