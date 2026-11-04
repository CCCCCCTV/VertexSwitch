// 莫奈式取色：整个界面的色调由一个种子色生成。
// CSS 里所有颜色都写成 oklch(L C H)，H 与 C 来自 --seed-h / --seed-c 两个变量，
// 这里只负责把种子色换算成 OKLCH 并写进这两个变量。默认种子是 Windows 系统强调色。

import { api } from "./api";

export const PRESETS = [
  { id: "blue", label: "靛蓝", seed: "#3B5BDB" },
  { id: "teal", label: "青绿", seed: "#0CA678" },
  { id: "violet", label: "紫罗兰", seed: "#7048E8" },
  { id: "rose", label: "玫瑰", seed: "#D6336C" },
  { id: "amber", label: "琥珀", seed: "#E67700" },
  { id: "graphite", label: "石墨", seed: "#5C677D" },
] as const;

export type ThemeSource = "system" | "custom";
export interface ThemePref {
  source: ThemeSource;
  /** source 为 custom 时使用 */
  seed: string;
}

const KEY = "vertexswitch.theme";
/** 上次读到的系统强调色：首帧直接用它，避免先画成后备色再跳变 */
const ACCENT_KEY = "vertexswitch.systemAccent";
const FALLBACK_SEED = "#3B5BDB";

function cachedAccent(): string | null {
  // 桌面程序在页面加载前注入的系统强调色优先
  const injected = (window as { __VS_ACCENT__?: string }).__VS_ACCENT__;
  if (injected && /^#[0-9a-f]{6}$/i.test(injected)) return injected;
  try {
    const v = localStorage.getItem(ACCENT_KEY);
    return v && /^#[0-9a-f]{6}$/i.test(v) ? v : null;
  } catch {
    return null;
  }
}

export function loadPref(): ThemePref {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = JSON.parse(raw);
      if ((p.source === "system" || p.source === "custom") && /^#[0-9a-f]{6}$/i.test(p.seed)) return p;
    }
  } catch {
    // 存储不可用时用默认值
  }
  return { source: "system", seed: FALLBACK_SEED };
}

function savePref(pref: ThemePref) {
  try {
    localStorage.setItem(KEY, JSON.stringify(pref));
  } catch {
    // 忽略：下次启动回到默认
  }
}

/** sRGB 十六进制 → OKLCH（L 0–1，C，H 角度）。 */
export function hexToOklch(hex: string): { l: number; c: number; h: number } {
  const n = parseInt(hex.slice(1), 16);
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const r = lin((n >> 16) & 255);
  const g = lin((n >> 8) & 255);
  const b = lin(n & 255);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const C = Math.hypot(A, B);
  const H = (Math.atan2(B, A) * 180) / Math.PI;
  return { l: L, c: C, h: (H + 360) % 360 };
}

/** 固定状态色的色相（与 app.css 一致）：失败 27°、降级 70°、成功 155° */
const STATUS_HUES = { bad: 27, warn: 70, ok: 155 } as const;

function hueDistance(a: number, b: number) {
  const d = Math.abs(a - b) % 360;
  return d > 180 ? 360 - d : d;
}

/**
 * 把种子色写进 CSS 变量。彩度夹在 0.04–0.16：太灰的种子仍保留一点色调，太艳的不刺眼。
 * 种子色相与某个状态色相近时在根元素上标记，CSS 据此让链接等不再用主色，避免和状态色混淆。
 */
export function applySeed(hex: string) {
  const { c, h } = hexToOklch(hex);
  const root = document.documentElement;
  root.style.setProperty("--seed-h", h.toFixed(1));
  root.style.setProperty("--seed-c", Math.min(0.16, Math.max(0.04, c)).toFixed(3));
  const near = Object.entries(STATUS_HUES).find(([, hue]) => hueDistance(h, hue) < 35);
  if (near) root.dataset.seedNear = near[0];
  else delete root.dataset.seedNear;
}

export const theme = $state({
  pref: loadPref(),
  systemAccent: cachedAccent(),
  /** 当前实际使用的种子色 */
  seed: FALLBACK_SEED,
});

function resolve() {
  theme.seed =
    theme.pref.source === "system" ? (theme.systemAccent ?? FALLBACK_SEED) : theme.pref.seed;
  applySeed(theme.seed);
}

export function setThemePref(pref: ThemePref) {
  theme.pref = pref;
  savePref(pref);
  resolve();
}

async function refreshSystemAccent() {
  try {
    const accent = await api.systemAccent();
    if (accent) {
      theme.systemAccent = accent;
      try {
        localStorage.setItem(ACCENT_KEY, accent);
      } catch {
        // 忽略：只是首帧优化
      }
    }
  } catch {
    // 读不到就沿用缓存或后备色
  }
  resolve();
}

/** 启动时调用；窗口重新获得焦点时重读系统强调色（用户可能刚在 Windows 设置里改过）。 */
export function startTheme() {
  resolve();
  void refreshSystemAccent();
  window.addEventListener("focus", () => void refreshSystemAccent());
}
