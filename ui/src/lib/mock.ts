// 浏览器预览用的示例数据：界面开发时不需要运行桌面程序，也不会连接任何服务。

import type { Api } from "./api";
import { i18n } from "./i18n.svelte";
import type {
  LogEntry,
  Models,
  RequestRecord,
  Settings,
  Status,
  Tier,
  UpdateInfo,
} from "./types";

const settings: Settings = {
  server: { host: "127.0.0.1", port: 8787, api_key: "", keep_alive_seconds: 75 },
  google: {
    credentials_file: "C:\\Users\\me\\keys\\service-account.json",
    project_id: "",
    region: "global",
    token_refresh_minutes: 50,
  },
  timeouts: { connect: 10, read: 600, stream_idle: 900, write: 60, pool: 10 },
  retry: {
    attempts: 2,
    backoff_seconds: 0.4,
    quota_hang_seconds: 20,
    busy: true,
    busy_attempts: 3,
    busy_wait: "2^(n-1)",
  },
  claude: { anthropic_version: "vertex-2023-10-16", drop_body_fields: ["context_management"] },
  gemini: {
    region: "",
    tier: "standard",
    model_tiers: { "gemini-3.5-flash-lite": "flex" },
    shared_only: false,
    flex_server_timeout: 1800,
    flex_unsupported_models: ["gemini-4-argon"],
    openai_api_version: "v1beta1",
    openai_native: "auto",
  },
  maas: { region: "", api_version: "v1" },
  ollama: { enabled: false, base_url: "http://127.0.0.1:11434" },
  openai: { default_max_tokens: 8192 },
};

const models: Models = {
  claude: {
    "claude-opus-5-5": "claude-opus-5-5",
    "claude-sonnet-4-6": "claude-sonnet-4-6",
    "claude-haiku-4-5": "claude-haiku-4-5@20251001",
  },
  gemini: {
    "gemini-4-argon": "gemini-4-argon",
    "gemini-3.8-flash": "gemini-3.8-flash",
    "gemini-3.5-flash": "gemini-3.5-flash",
    "gemini-3.5-flash-lite": "gemini-3.5-flash-lite",
    "gemini-3-flash": "gemini-3-flash-preview",
  },
  maas: {},
  ollama: [],
  source: "builtin",
  user_file: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch\\models.toml",
};

let autostart = false;
let mockBackground: Blob | null = null;
const started = Date.now() - 3 * 3600_000 - 12 * 60_000;
let seq = 0;
const logs: LogEntry[] = [];
const requests: RequestRecord[] = [];
const logListeners = new Set<(e: LogEntry) => void>();
const requestListeners = new Set<(r: RequestRecord) => void>();
const settingsListeners = new Set<() => void>();

// 示例日志模拟核心：按写下时的界面语言
const tr = (zh: string, en: string) => (i18n.lang === "en" ? en : zh);

function log(level: LogEntry["level"], message: string, target = "vertexswitch_core", fields: Record<string, string> = {}) {
  const entry: LogEntry = { seq: ++seq, ts_ms: Date.now(), level, target, message, fields };
  logs.push(entry);
  logListeners.forEach((cb) => cb(entry));
}

const samples: Array<Partial<RequestRecord>> = [
  { protocol: "gemini", model: "gemini-3.5-flash-lite", backend: "gemini", path: "/v1beta/models/gemini-3.5-flash-lite:generateContent", input_tokens: 412, output_tokens: 96 },
  { protocol: "anthropic", model: "claude-sonnet-4-6", backend: "claude", path: "/v1/messages", stream: true, input_tokens: 18230, output_tokens: 1204 },
  { protocol: "openai", model: "gemini-3.8-flash", backend: "gemini", path: "/v1/chat/completions", stream: true, input_tokens: 980, output_tokens: 311 },
  { protocol: "gemini", model: "gemini-4-argon", backend: "gemini", path: "/v1beta/models/gemini-4-argon:streamGenerateContent", stream: true, input_tokens: 5120, output_tokens: 2048 },
  { protocol: "openai", model: null, backend: null, path: "/v1/chat/completions", status: 404, outcome: "error", error: "未知模型 \"gpt-4o\"" },
];

function request(i: number, ts = Date.now()) {
  const s = samples[i % samples.length];
  const record: RequestRecord = {
    seq: ++seq,
    ts_ms: ts,
    method: "POST",
    path: s.path ?? "/",
    protocol: s.protocol ?? "openai",
    model: s.model ?? null,
    backend: s.backend ?? null,
    tier: s.backend === "gemini" ? (settings.gemini.model_tiers[s.model ?? ""] ?? settings.gemini.tier) : null,
    stream: s.stream ?? false,
    status: s.status ?? 200,
    duration_ms: s.status ? 1 : 400 + ((i * 937) % 9000),
    input_tokens: s.input_tokens ?? null,
    output_tokens: s.output_tokens ?? null,
    outcome: s.outcome ?? "ok",
    error: s.error ?? null,
  };
  requests.push(record);
  requestListeners.forEach((cb) => cb(record));
}

const tokenRefreshed = () =>
  log("INFO", tr("Google access token 已刷新", "Google access token refreshed"), "vertexswitch_core::auth");

// #empty=1：刚启动、还没有任何请求的样子（检查空状态用）
const emptyPreview = new URLSearchParams(location.hash.slice(1)).get("empty") === "1";
if (!emptyPreview) for (let i = 0; i < 14; i++) request(i, Date.now() - (14 - i) * 47_000);

// 开头几条日志等第一次读取时再写：那时预览参数 #lang 已生效
let seeded = false;
function seedLogs() {
  if (seeded) return;
  seeded = true;
  log("INFO", tr("VertexSwitch 0.1.0 监听 http://127.0.0.1:8787", "VertexSwitch 0.1.0 listening on http://127.0.0.1:8787"));
  tokenRefreshed();
  if (emptyPreview) return;
  log(
    "WARN",
    tr(
      "请求了 priority 档位，但上游实际按 ON_DEMAND 处理（期望 ON_DEMAND_PRIORITY）",
      "Requested the priority tier, but upstream served it as ON_DEMAND (expected ON_DEMAND_PRIORITY)",
    ),
    "vertexswitch_core::frontends::gemini",
    { kind: "tier_mismatch", model: "gemini-3.5-flash" },
  );
  log(
    "WARN",
    tr(
      "Vertex AI 返回 429（繁忙），1.1 秒后第 1 次重试（最多 3 次）",
      "Vertex AI returned 429 (busy); retry 1 of 3 in 1.1s",
    ),
    "vertexswitch_core::transport",
    { kind: "upstream_retry", model: "gemini-3.5-pro", status: "429" },
  );
  log(
    "INFO",
    tr("Vertex AI 在第 1 次重试后成功", "Vertex AI succeeded on retry 1"),
    "vertexswitch_core::transport",
    { kind: "upstream_retry_ok", model: "gemini-3.5-pro", retries: "1" },
  );
}

let tick = 0;
setInterval(() => {
  if (emptyPreview) return;
  request(tick++);
  if (tick % 4 === 0) tokenRefreshed();
}, 5000);

function status(): Status {
  return {
    status: "ok",
    version: "0.1.0",
    region: settings.google.region,
    gemini_region: settings.gemini.region || settings.google.region,
    gemini_tier: settings.gemini.tier,
    gemini_model_tiers: { ...settings.gemini.model_tiers },
    google_token: "ok",
    project_id: "my-project-123",
    listen: `${settings.server.host}:${settings.server.port}`,
    uptime_secs: Math.floor((Date.now() - started) / 1000),
    in_flight: 0,
    models: { claude: 3, gemini: 5, maas: 0, ollama: 0 },
    config_path: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch\\config.toml",
    server: { running: true, addr: `${settings.server.host}:${settings.server.port}`, error: null },
  };
}

// JSON Merge Patch：null 表示删掉该项
function merge(target: any, patch: any) {
  for (const [k, v] of Object.entries(patch)) {
    if (v === null) delete target[k];
    else if (typeof v === "object" && !Array.isArray(v)) merge((target[k] ??= {}), v);
    else target[k] = v;
  }
}

const delay = <T>(value: T) => new Promise<T>((r) => setTimeout(() => r(structuredClone(value)), 120));

export const mockApi: Api = {
  preview: true,
  status: () => delay(status()),
  settings: () => delay(settings),
  patchSettings: (patch) => {
    const oldAddr = `${settings.server.host}:${settings.server.port}`;
    merge(settings, patch);
    log("INFO", tr("配置已更新", "Config updated"), "vertexswitch_core::app");
    settingsListeners.forEach((cb) => cb());
    const restart = oldAddr !== `${settings.server.host}:${settings.server.port}`;
    return delay({ settings, restart_required: restart, server: status().server });
  },
  setTier: (tier: Tier) => mockApi.patchSettings({ gemini: { tier } }),
  restartServer: () => delay(status().server),
  models: () => delay(models),
  reloadModels: () => delay({ aliases: 8 }),
  editModels: () => delay({ path: models.user_file ?? "" }),
  refreshOllama: () => delay({ ollama_models: 0 }),
  logs: (since, limit) => (seedLogs(), delay(logs.filter((e) => e.seq > since).slice(-limit))),
  requests: (since, limit) => delay(requests.filter((r) => r.seq > since).slice(-limit)),
  metrics: () => {
    const totals = new Map<string, any>();
    for (const r of requests) {
      const key = `${r.protocol}|${r.backend ?? "none"}|${r.status}`;
      const t = totals.get(key) ?? {
        protocol: r.protocol, backend: r.backend ?? "none", status: r.status,
        requests: 0, duration_ms: 0, input_tokens: 0, output_tokens: 0,
      };
      t.requests++;
      t.duration_ms += r.duration_ms;
      t.input_tokens += r.input_tokens ?? 0;
      t.output_tokens += r.output_tokens ?? 0;
      totals.set(key, t);
    }
    return delay({ uptime_secs: status().uptime_secs, in_flight: 0, totals: [...totals.values()] });
  },
  sendTest: (model) =>
    new Promise((r) =>
      setTimeout(() => r({ ok: true, status: 200, latency_ms: 812, reply: `pong（预览：${model}）`, error: null }), 800),
    ),
  // 预览用的简化版，真实程序由核心解析（规则见 crates/vertexswitch-core/src/wait_formula.rs）
  previewBusyWait: (expr, count) => {
    const src = expr
      .replace(/（/g, "(")
      .replace(/）/g, ")")
      .replace(/，/g, ",")
      .replace(/×/g, "*")
      .replace(/÷/g, "/")
      .replace(/＋/g, "+")
      .replace(/－/g, "-")
      .replace(/\*\*/g, "^")
      .toLowerCase();
    if (!src.trim()) return Promise.reject(tr("等待时间是空的", "The wait time is empty"));
    if (!/^(?:[\d\s.+\-*/^(),n]|min|max)*$/.test(src))
      return Promise.reject(tr("有不认识的字符或名字", "Contains characters or names that aren't allowed"));
    // 两个值之间缺运算符（如 3n、2(n+1)）
    if (/(\d|n|\))\s*(n|\()|(n|\))\s*\d|\d\s+\d/.test(src.replace(/min|max/g, "f")))
      return Promise.reject(tr("两个数之间要写运算符，比如 3*n", "Put an operator between values, e.g. 3*n"));
    try {
      const f = new Function("n", "min", "max", `return (${src.replace(/\^/g, "**")});`);
      const waits = Array.from({ length: Math.min(Math.max(count, 1), 10) }, (_, i) =>
        Math.min(Math.max(Number(f(i + 1, Math.min, Math.max)), 0), 300),
      );
      if (waits.some((w) => !Number.isFinite(w))) throw new Error();
      return delay(waits);
    } catch {
      return Promise.reject(tr("等待时间写得不对", "The wait time isn't valid"));
    }
  },
  appInfo: () =>
    delay({
      version: "0.1.0",
      config_dir: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch",
      config_path: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch\\config.toml",
      admin_token_location: "系统凭据库（VertexSwitch / admin-token）",
      admin_token_in_store: true,
    }),
  openConfigDir: () => delay(undefined),
  getAutostart: () => delay({ enabled: autostart }),
  setAutostart: (enabled) => {
    autostart = enabled;
    return delay({ enabled });
  },
  adminToken: () => delay({ token: "0123456789abcdef0123456789abcdef0123456789abcdef" }),
  systemAccent: () => delay("#0078D4"),
  setBackdrop: () => delay(undefined),
  setTitleBar: () => delay(undefined),
  // 预览：左边偏红、右边偏蓝的颜色场，相当于窗口后面左右各有一个应用
  sampleBackdrop: () =>
    delay({
      seed: "#C8424A",
      cols: 8,
      rows: 5,
      field: Array.from({ length: 40 }, (_, i) => (i % 8 < 4 ? "#D2484E" : "#3A6FD8")),
    }),
  onWindowMoved: () => () => {},
  pickKeyFile: () => delay("C:\\Users\\me\\keys\\service-account.json"),
  // 预览：用浏览器的文件选择框，图片只留在内存里
  chooseBackgroundImage: () =>
    new Promise((resolve) => {
      const input = document.createElement("input");
      input.type = "file";
      input.accept = "image/*";
      input.onchange = () => {
        mockBackground = input.files?.[0] ?? mockBackground;
        resolve(Boolean(input.files?.length));
      };
      input.click();
    }),
  backgroundImage: () => Promise.resolve(mockBackground),
  clearBackgroundImage: () => {
    mockBackground = null;
    return delay(undefined);
  },
  setLanguage: () => delay(undefined),
  openLogDir: () => delay(undefined),
  // #update=1：预览“有新版本”的样子
  checkUpdate: () =>
    delay<UpdateInfo>(
      new URLSearchParams(location.hash.slice(1)).get("update") === "1"
        ? { current: "0.1.0", latest: "0.2.0", url: "https://github.com/CCCCCCTV/VertexSwitch/releases/tag/v0.2.0", newer: true }
        : { current: "0.1.0", latest: "0.1.0", url: "https://github.com/CCCCCCTV/VertexSwitch/releases/tag/v0.1.0", newer: false },
    ),
  openReleasePage: () => delay(undefined),
  onLog: (cb) => (logListeners.add(cb), () => logListeners.delete(cb)),
  onRequest: (cb) => (requestListeners.add(cb), () => requestListeners.delete(cb)),
  onSettingsChanged: (cb) => (settingsListeners.add(cb), () => settingsListeners.delete(cb)),
};
