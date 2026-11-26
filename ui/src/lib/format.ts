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

/**
 * 取不到 token 的原因写短一点：去掉“获取 Google access token 失败：”开头（标题已经说了），
 * Google 的 JSON 错误体只留说明，例如 `HTTP 400 · Invalid grant: account not found`。
 */
export function tokenProblem(error: string): string {
  const text = error.replace(/^(获取 Google access token 失败：|Couldn't get a Google access token: )/, "");
  const http = /^HTTP (\d{3})[^{]*(\{.*\})\s*$/s.exec(text);
  if (http) {
    try {
      const body = JSON.parse(http[2]);
      const what = body.error_description || body.error;
      if (typeof what === "string" && what) return `HTTP ${http[1]} · ${what}`;
    } catch {
      // 错误体被截断或不是 JSON：原样显示
    }
  }
  return text;
}

/** 凭证的名字：“凭证 2（项目）”；还不知道项目时只写“凭证 2”。 */
export function credentialName(c: { credential: number; project: string | null }): string {
  return c.project
    ? t("overview.credentialName", { n: c.credential, project: c.project })
    : t("settings.credentialNo", { n: c.credential });
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
