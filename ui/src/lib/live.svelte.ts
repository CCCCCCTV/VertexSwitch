// 全局实时数据：启动时拉一次历史，之后靠事件追加。切换页面不丢数据。

import { api } from "./api";
import type { LogEntry, RequestRecord, Status, Tier } from "./types";

const MAX_LOGS = 1000;
const MAX_REQUESTS = 500;

export const live = $state({
  logs: [] as LogEntry[],
  requests: [] as RequestRecord[],
  status: null as Status | null,
  statusError: "" as string,
  /** 本次运行中观察到的档位切换（时间线上画竖线） */
  tierMarks: [] as { ts_ms: number; tier: Tier }[],
});

/** 按序号有序插入并去重：历史拉取与实时事件可能重叠、先后不定。 */
function push<T extends { seq: number }>(list: T[], item: T, max: number) {
  let i = list.length;
  while (i > 0 && list[i - 1].seq > item.seq) i--;
  if (i > 0 && list[i - 1].seq === item.seq) return;
  list.splice(i, 0, item);
  if (list.length > max) list.splice(0, list.length - max);
}

export async function refreshStatus() {
  try {
    const next = await api.status();
    const prev = live.status?.gemini_tier;
    if (prev && prev !== next.gemini_tier) {
      live.tierMarks.push({ ts_ms: Date.now(), tier: next.gemini_tier });
    }
    live.status = next;
    live.statusError = "";
  } catch (e) {
    live.statusError = String(e);
  }
}

let started = false;

export async function startLive() {
  if (started) return;
  started = true;
  api.onLog((e) => push(live.logs, e, MAX_LOGS));
  api.onRequest((r) => push(live.requests, r, MAX_REQUESTS));
  api.onSettingsChanged(() => void refreshStatus());
  const [logs, requests] = await Promise.all([api.logs(0, MAX_LOGS), api.requests(0, MAX_REQUESTS)]);
  for (const e of logs) push(live.logs, e, MAX_LOGS);
  for (const r of requests) push(live.requests, r, MAX_REQUESTS);
  await refreshStatus();
  setInterval(refreshStatus, 5000);
  // 浏览器预览：放两次档位切换，让时间线上的标记能被看到
  if (api.preview) {
    live.tierMarks.push(
      { ts_ms: Date.now() - 12 * 60_000, tier: "flex" },
      { ts_ms: Date.now() - 4 * 60_000, tier: "standard" },
    );
  }
}
