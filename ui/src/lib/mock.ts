// 浏览器预览用的示例数据：界面开发时不需要运行桌面程序，也不会连接任何服务。

import type { Api } from "./api";
import type {
  LogEntry,
  Models,
  RequestRecord,
  Settings,
  Status,
  Tier,
} from "./types";

const settings: Settings = {
  server: { host: "127.0.0.1", port: 8788, api_key: "", keep_alive_seconds: 75 },
  google: {
    credentials_file: "C:\\Users\\me\\keys\\service-account.json",
    project_id: "",
    region: "global",
    token_refresh_minutes: 50,
  },
  timeouts: { connect: 10, read: 600, stream_idle: 900, write: 60, pool: 10 },
  retry: { attempts: 2, backoff_seconds: 0.4, quota_hang_seconds: 20 },
  claude: { anthropic_version: "vertex-2023-10-16", drop_body_fields: ["context_management"] },
  gemini: {
    region: "",
    tier: "standard",
    shared_only: false,
    flex_server_timeout: 1800,
    flex_unsupported_models: ["gemini-4-argon"],
    openai_api_version: "v1beta1",
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

const started = Date.now() - 3 * 3600_000 - 12 * 60_000;
let seq = 0;
const logs: LogEntry[] = [];
const requests: RequestRecord[] = [];
const logListeners = new Set<(e: LogEntry) => void>();
const requestListeners = new Set<(r: RequestRecord) => void>();
const settingsListeners = new Set<() => void>();

function log(level: LogEntry["level"], message: string, target = "vertexswitch_core") {
  const entry: LogEntry = { seq: ++seq, ts_ms: Date.now(), level, target, message };
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
    tier: s.backend === "gemini" && s.protocol === "gemini" ? settings.gemini.tier : null,
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

log("INFO", "VertexSwitch 0.1.0 监听 http://127.0.0.1:8788");
log("INFO", "Google access token 已刷新", "vertexswitch_core::auth");
for (let i = 0; i < 14; i++) request(i, Date.now() - (14 - i) * 47_000);
log("WARN", "gemini-3.5-flash：请求了 priority 档位，但上游实际按 ON_DEMAND 处理（期望 ON_DEMAND_PRIORITY）", "vertexswitch_core::frontends::gemini");

let tick = 0;
setInterval(() => {
  request(tick++);
  if (tick % 4 === 0) log("INFO", "Google access token 已刷新", "vertexswitch_core::auth");
}, 5000);

function status(): Status {
  return {
    status: "ok",
    version: "0.1.0",
    region: settings.google.region,
    gemini_region: settings.gemini.region || settings.google.region,
    gemini_tier: settings.gemini.tier,
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

function merge(target: any, patch: any) {
  for (const [k, v] of Object.entries(patch)) {
    if (v && typeof v === "object" && !Array.isArray(v)) merge(target[k], v);
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
    log("INFO", "配置已更新", "vertexswitch_core::app");
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
  logs: (since, limit) => delay(logs.filter((e) => e.seq > since).slice(-limit)),
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
  appInfo: () =>
    delay({
      version: "0.1.0",
      config_dir: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch",
      config_path: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch\\config.toml",
      admin_token_path: "C:\\Users\\me\\AppData\\Roaming\\VertexSwitch\\admin_token",
    }),
  openConfigDir: () => delay(undefined),
  pickKeyFile: () => delay("C:\\Users\\me\\keys\\service-account.json"),
  onLog: (cb) => (logListeners.add(cb), () => logListeners.delete(cb)),
  onRequest: (cb) => (requestListeners.add(cb), () => requestListeners.delete(cb)),
  onSettingsChanged: (cb) => (settingsListeners.add(cb), () => settingsListeners.delete(cb)),
};
