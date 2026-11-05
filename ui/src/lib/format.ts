// 显示用的格式化函数。

import { t } from "./i18n.svelte";
import type { Tier } from "./types";

export function time(ms: number): string {
  return new Date(ms).toLocaleTimeString("en-GB", { hour12: false });
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return t("fmt.minSec", { m: Math.floor(ms / 60_000), s: Math.round((ms % 60_000) / 1000) });
}

export function uptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d) return t("fmt.dayHour", { d, h });
  if (h) return t("fmt.hourMin", { h, m });
  return t("fmt.min", { m });
}

export function tokens(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return n.toLocaleString("en-US");
}

/** 档位名在两种语言里一样，说明文字见词典 tier.<id>.hint */
export const TIERS = ["standard", "flex", "priority"] as const satisfies readonly Tier[];
export const TIER_LABEL: Record<Tier, string> = { standard: "Standard", flex: "Flex", priority: "Priority" };

export const OUTCOME_TONE = {
  ok: "ok",
  error: "bad",
  stream_error: "bad",
  cancelled: "muted",
} as const;
