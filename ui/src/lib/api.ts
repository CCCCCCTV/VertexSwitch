// 界面与核心之间的接口。
// 在桌面程序里通过 Tauri 命令直接调用同进程的核心；在普通浏览器里（仅用于预览界面）
// 退回到内存里的示例数据，不连接任何服务。

import type {
  AppInfo,
  Backdrop,
  LogEntry,
  Metrics,
  Models,
  PatchResult,
  RequestRecord,
  Settings,
  Status,
  TestResult,
  Tier,
} from "./types";
import { mockApi } from "./mock";

export interface Api {
  readonly preview: boolean;
  status(): Promise<Status>;
  settings(): Promise<Settings>;
  patchSettings(patch: unknown): Promise<PatchResult>;
  setTier(tier: Tier): Promise<PatchResult>;
  restartServer(): Promise<unknown>;
  models(): Promise<Models>;
  reloadModels(): Promise<{ aliases: number }>;
  editModels(): Promise<{ path: string }>;
  refreshOllama(): Promise<{ ollama_models: number }>;
  logs(since: number, limit: number): Promise<LogEntry[]>;
  requests(since: number, limit: number): Promise<RequestRecord[]>;
  metrics(): Promise<Metrics>;
  sendTest(model: string): Promise<TestResult>;
  appInfo(): Promise<AppInfo>;
  openConfigDir(): Promise<void>;
  getAutostart(): Promise<{ enabled: boolean }>;
  setAutostart(enabled: boolean): Promise<{ enabled: boolean }>;
  adminToken(): Promise<{ token: string }>;
  /** Windows 系统强调色 #RRGGBB；取不到时为 null */
  systemAccent(): Promise<string | null>;
  /** 切换窗口背景材质（仅 Windows 11） */
  setBackdrop(kind: Backdrop): Promise<void>;
  pickKeyFile(): Promise<string | null>;
  onLog(cb: (e: LogEntry) => void): () => void;
  onRequest(cb: (r: RequestRecord) => void): () => void;
  onSettingsChanged(cb: () => void): () => void;
}

async function tauriApi(): Promise<Api> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const { open } = await import("@tauri-apps/plugin-dialog");

  // listen 是异步注册的；返回同步的取消函数
  const subscribe = <T>(event: string, cb: (payload: T) => void) => {
    const pending = listen<T>(event, (e) => cb(e.payload));
    return () => void pending.then((unlisten) => unlisten());
  };

  return {
    preview: false,
    status: () => invoke("get_status"),
    settings: () => invoke("get_settings"),
    patchSettings: (patch) => invoke("patch_settings", { patch }),
    setTier: (tier) => invoke("set_tier", { tier }),
    restartServer: () => invoke("restart_server"),
    models: () => invoke("get_models"),
    reloadModels: () => invoke("reload_models"),
    editModels: () => invoke("edit_models"),
    refreshOllama: () => invoke("refresh_ollama"),
    logs: (since, limit) => invoke("get_logs", { since, limit }),
    requests: (since, limit) => invoke("get_requests", { since, limit }),
    metrics: () => invoke("get_metrics"),
    sendTest: (model) => invoke("send_test", { model }),
    appInfo: () => invoke("get_app_info"),
    openConfigDir: () => invoke("open_config_dir"),
    getAutostart: () => invoke("get_autostart"),
    setAutostart: (enabled) => invoke("set_autostart", { enabled }),
    adminToken: () => invoke("get_admin_token"),
    systemAccent: () => invoke("get_system_accent"),
    setBackdrop: (kind) => invoke("set_backdrop", { kind }),
    pickKeyFile: async () => {
      const picked = await open({
        title: "选择服务账号密钥文件",
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      return typeof picked === "string" ? picked : null;
    },
    onLog: (cb) => subscribe("log", cb),
    onRequest: (cb) => subscribe("request", cb),
    onSettingsChanged: (cb) => subscribe("settings-changed", () => cb()),
  };
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api: Api = inTauri ? await tauriApi() : mockApi;

/** 把错误（Tauri 命令返回的字符串或 Error）转成可读文字。 */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
