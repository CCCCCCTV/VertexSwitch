// 显示用的格式化函数。

export function time(ms: number): string {
  return new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60_000)} 分 ${Math.round((ms % 60_000) / 1000)} 秒`;
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d) return `${d} 天 ${h} 小时`;
  if (h) return `${h} 小时 ${m} 分`;
  return `${m} 分钟`;
}

export function tokens(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return n.toLocaleString("zh-CN");
}

export const TIER_INFO = {
  standard: { label: "Standard", hint: "标准按量计费" },
  flex: { label: "Flex", hint: "价格更低，可能排队；仅 global 区域" },
  priority: { label: "Priority", hint: "优先处理，价格更高；global / us / eu" },
} as const;

export const OUTCOME_INFO = {
  ok: { label: "成功", tone: "ok" },
  error: { label: "失败", tone: "bad" },
  stream_error: { label: "流中断", tone: "bad" },
  cancelled: { label: "已取消", tone: "muted" },
} as const;
