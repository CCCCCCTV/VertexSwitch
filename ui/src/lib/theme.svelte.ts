// 莫奈式取色：整个界面的色调由一个种子色生成。
// CSS 里所有颜色都写成 oklch(L C H)，H 与 C 来自 --seed-h / --seed-c 两个变量，
// 这里只负责把种子色换算成 OKLCH 并写进这两个变量。默认种子是 Windows 系统强调色。
// 实时取色时还会生成一张“颜色场”（按位置排列的背景色），模糊后铺在窗口背景上；
// 液态玻璃则把窗口后方的整幅画面画在最底层，板块做成折射它的玻璃（见 glass.ts）。

import { api } from "./api";
import { startGlass, stopGlass } from "./glass";
import type { Backdrop, BackdropSample, GlassFrame, LiveScope } from "./types";

export const PRESETS = [
  { id: "blue", label: "靛蓝", seed: "#3B5BDB" },
  { id: "teal", label: "青绿", seed: "#0CA678" },
  { id: "violet", label: "紫罗兰", seed: "#7048E8" },
  { id: "rose", label: "玫瑰", seed: "#D6336C" },
  { id: "amber", label: "琥珀", seed: "#E67700" },
  { id: "graphite", label: "石墨", seed: "#5C677D" },
] as const;

/**
 * system：Windows 强调色；custom：用户选的颜色；
 * wallpaper / behind：实时取色（整张壁纸 / 窗口后方，包括后面的其他应用）
 */
export type ThemeSource = "system" | "custom" | LiveScope;
export interface ThemePref {
  source: ThemeSource;
  /** source 为 custom 时使用 */
  seed: string;
  /** 色调强度 0–2：0 接近中性灰，1 为默认，2 最浓 */
  tint: number;
  /** 窗口背景材质，只在支持的系统（Windows 11）上起作用 */
  backdrop: Backdrop;
  /** 实时取色的间隔（毫秒）；0 表示取完马上取下一次 */
  liveInterval: number;
  /** 背景图片的不透明度 0–1 */
  bgOpacity: number;
  /** 液态玻璃：窗口后方的画面透过玻璃板块显示，主题色也取自后方 */
  glass: boolean;
}

export const LIVE_SCOPES: { id: LiveScope; label: string; hint: string }[] = [
  {
    id: "wallpaper",
    label: "整张壁纸",
    hint: "取整张壁纸的颜色（Wallpaper Engine 动态壁纸取当前画面），被窗口挡住也不影响",
  },
  {
    id: "behind",
    label: "窗口后方",
    hint: "取窗口正后方的画面，包括后面的其他应用；左边是红色应用、右边是蓝色应用，背景也左红右蓝。移动窗口时立即重取",
  },
];

export const LIVE_INTERVALS = [
  { ms: 0, label: "无间隔" },
  { ms: 500, label: "0.5 秒" },
  { ms: 1000, label: "1 秒" },
  { ms: 2000, label: "2 秒" },
  { ms: 5000, label: "5 秒" },
  { ms: 10_000, label: "10 秒" },
  { ms: 30_000, label: "30 秒" },
] as const;
export const LIVE_INTERVAL_DEFAULT = 2000;
export const BG_OPACITY_DEFAULT = 0.35;

export const BACKDROPS: { id: Backdrop; label: string; hint: string }[] = [
  { id: "mica", label: "Mica", hint: "系统静态壁纸的模糊色调（不认动态壁纸），最安静" },
  { id: "tabbed", label: "Mica Alt", hint: "同样取静态壁纸色调，颜色更明显" },
  { id: "acrylic", label: "亚克力", hint: "透出窗口后面的内容（模糊），最明显" },
  { id: "none", label: "不透明", hint: "关闭半透明" },
];

export const TINT_DEFAULT = 1;

const KEY = "vertexswitch.theme";
/** 上次读到的系统强调色：首帧直接用它，避免先画成后备色再跳变 */
const ACCENT_KEY = "vertexswitch.systemAccent";
const FALLBACK_SEED = "#3B5BDB";
/** 上次实时取色的结果：首帧直接用它 */
const LIVE_KEY = "vertexswitch.liveSample";
const HEX = /^#[0-9a-f]{6}$/i;

const isLive = (source: ThemeSource): source is LiveScope => source === "wallpaper" || source === "behind";

type LiveState = BackdropSample & { scope: LiveScope };

function validSample(s: unknown): s is BackdropSample {
  const v = s as BackdropSample;
  return (
    !!v &&
    HEX.test(v.seed) &&
    Number.isInteger(v.cols) &&
    Number.isInteger(v.rows) &&
    Array.isArray(v.field) &&
    v.field.length === v.cols * v.rows &&
    v.field.every((c) => HEX.test(c))
  );
}

function cachedLive(): LiveState | null {
  try {
    const v = JSON.parse(localStorage.getItem(LIVE_KEY) ?? "null");
    return validSample(v) && isLive((v as LiveState).scope) ? (v as LiveState) : null;
  } catch {
    return null;
  }
}

function cachedAccent(): string | null {
  // 桌面程序在页面加载前注入的系统强调色优先
  const injected = (window as { __VS_ACCENT__?: string }).__VS_ACCENT__;
  if (injected && HEX.test(injected)) return injected;
  try {
    const v = localStorage.getItem(ACCENT_KEY);
    return v && HEX.test(v) ? v : null;
  } catch {
    return null;
  }
}

export function loadPref(): ThemePref {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const p = JSON.parse(raw);
      if (["system", "custom", "wallpaper", "behind"].includes(p.source) && HEX.test(p.seed)) {
        const tint = typeof p.tint === "number" && p.tint >= 0 && p.tint <= 2 ? p.tint : TINT_DEFAULT;
        const backdrop = BACKDROPS.some((b) => b.id === p.backdrop) ? (p.backdrop as Backdrop) : "mica";
        const liveInterval = LIVE_INTERVALS.some((i) => i.ms === p.liveInterval)
          ? (p.liveInterval as number)
          : LIVE_INTERVAL_DEFAULT;
        const bgOpacity =
          typeof p.bgOpacity === "number" && p.bgOpacity >= 0 && p.bgOpacity <= 1 ? p.bgOpacity : BG_OPACITY_DEFAULT;
        return { source: p.source, seed: p.seed, tint, backdrop, liveInterval, bgOpacity, glass: p.glass === true };
      }
    }
  } catch {
    // 存储不可用时用默认值
  }
  return {
    source: "system",
    seed: FALLBACK_SEED,
    tint: TINT_DEFAULT,
    backdrop: "mica",
    liveInterval: LIVE_INTERVAL_DEFAULT,
    bgOpacity: BG_OPACITY_DEFAULT,
    glass: false,
  };
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
  // Material 3 色彩调和：时间线的“成功”绿往种子色相偏一点（差值的一半，最多 15°），仍一眼是绿色
  const toward = ((h - STATUS_HUES.ok + 540) % 360) - 180;
  const okHue = STATUS_HUES.ok + Math.max(-15, Math.min(15, toward / 2));
  root.style.setProperty("--ok-h", okHue.toFixed(1));
}

export const theme = $state({
  pref: loadPref(),
  systemAccent: cachedAccent(),
  /** 实时取色最近采用的结果 */
  live: cachedLive() as LiveState | null,
  /** 当前实际使用的种子色 */
  seed: FALLBACK_SEED,
  /** 系统支持半透明窗口材质（Windows 11）；桌面程序在页面加载前注入 */
  backdropSupported: Boolean((window as { __VS_MICA__?: boolean }).__VS_MICA__),
  /** 背景图片（blob 地址）；没有设置时为 null */
  bgImage: null as string | null,
});

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

/**
 * 把颜色场画成一张 cols × rows 的小图：保留每格的色相，亮度统一到当前主题的底色亮度，
 * 彩度随色调强度缩放。CSS 把它拉伸铺满窗口再模糊，就是一片按位置渐变的背景。
 */
function renderField(live: LiveState, tint: number): string {
  const canvas = document.createElement("canvas");
  canvas.width = live.cols;
  canvas.height = live.rows;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  // 深色底上同样的彩度显得更浓，上限压低一些
  const [lightness, maxChroma] = darkQuery.matches ? [0.25, 0.075] : [0.9, 0.11];
  live.field.forEach((hex, i) => {
    const { c, h } = hexToOklch(hex);
    const chroma = Math.min(0.2, Math.min(c * 0.85, maxChroma) * tint);
    ctx.fillStyle = `oklch(${lightness} ${chroma.toFixed(3)} ${h.toFixed(1)})`;
    ctx.fillRect(i % live.cols, Math.floor(i / live.cols), 1, 1);
  });
  return canvas.toDataURL();
}

/** 种子色从哪种实时取色来：液态玻璃总是取窗口后方，否则看颜色来源 */
function colorScope(): LiveScope | null {
  if (theme.pref.glass) return "behind";
  return isLive(theme.pref.source) ? theme.pref.source : null;
}

/** 颜色场的平均亮度（0 黑 – 1 白），液态玻璃据此调玻璃的浓淡，背后再花也看得清字 */
function fieldLuminance(field: string[]) {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const total = field.reduce((sum, hex) => {
    const n = parseInt(hex.slice(1), 16);
    return sum + 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
  }, 0);
  return field.length ? total / field.length : 0.5;
}

let glassCanvas: HTMLCanvasElement | null = null;

/** App 把铺在最底层的画布交给这里，液态玻璃把窗口后方的画面画进去 */
export function registerGlassCanvas(canvas: HTMLCanvasElement | null) {
  glassCanvas = canvas;
  if (canvas && theme.pref.glass) void sampleGlass();
}

function drawGlass(frame: GlassFrame) {
  const canvas = glassCanvas;
  if (!canvas) return;
  if (canvas.width !== frame.width || canvas.height !== frame.height) {
    canvas.width = frame.width;
    canvas.height = frame.height;
  }
  canvas.getContext("2d")?.putImageData(new ImageData(frame.pixels, frame.width, frame.height), 0, 0);
  canvas.dataset.ready = "";
}

function applyGlass() {
  const root = document.documentElement;
  if (theme.pref.glass) {
    root.dataset.glass = "";
    const live = theme.live?.scope === "behind" ? theme.live : null;
    root.style.setProperty("--glass-lum", (live ? fieldLuminance(live.field) : 0.5).toFixed(3));
    startGlass();
  } else {
    delete root.dataset.glass;
    root.style.removeProperty("--glass-lum");
    delete glassCanvas?.dataset.ready;
    stopGlass();
  }
}

function applyField() {
  const root = document.documentElement;
  const live = theme.live;
  if (!theme.pref.glass && isLive(theme.pref.source) && live && live.scope === theme.pref.source) {
    const url = renderField(live, theme.pref.tint);
    if (url) {
      root.style.setProperty("--field", `url("${url}")`);
      root.dataset.field = "";
      return;
    }
  }
  delete root.dataset.field;
  root.style.removeProperty("--field");
}

/** 背景图片铺在颜色场之上、内容之下，不透明度由用户调 */
function applyBackgroundImage() {
  const root = document.documentElement;
  root.style.setProperty("--bg-image-opacity", String(theme.pref.bgOpacity));
  if (theme.bgImage) {
    root.style.setProperty("--bg-image", `url("${theme.bgImage}")`);
    root.dataset.bgImage = "";
  } else {
    root.style.removeProperty("--bg-image");
    delete root.dataset.bgImage;
  }
}

function showBackground(blob: Blob | null) {
  if (theme.bgImage) URL.revokeObjectURL(theme.bgImage);
  theme.bgImage = blob ? URL.createObjectURL(blob) : null;
  applyBackgroundImage();
}

/** 仅浏览器预览：直接用一张生成的图当背景 */
export const previewBackground = (blob: Blob) => showBackground(blob);

/** 选图片设为背景；取消选择时什么都不变 */
export async function chooseBackground() {
  if (await api.chooseBackgroundImage()) showBackground(await api.backgroundImage());
}

export async function clearBackground() {
  await api.clearBackgroundImage();
  showBackground(null);
}

export function setBackgroundOpacity(bgOpacity: number) {
  setThemePref({ ...theme.pref, bgOpacity });
}

function resolve() {
  const system = theme.systemAccent ?? FALLBACK_SEED;
  const scope = colorScope();
  const live = theme.live && theme.live.scope === scope ? theme.live : null;
  theme.seed = scope ? (live?.seed ?? system) : theme.pref.source === "custom" ? theme.pref.seed : system;
  applySeed(theme.seed);
  const root = document.documentElement;
  root.style.setProperty("--tint", String(theme.pref.tint));
  // 取色越频繁，颜色场换图的淡入越短，跟得上画面
  const interval = theme.pref.liveInterval;
  root.style.setProperty("--field-fade", interval === 0 ? "0.25s" : interval < 2000 ? "0.6s" : "1.2s");
  applyField();
  applyBackgroundImage();
  applyGlass();
}

/** 底色是否半透明，跟着窗口材质走 */
function applyBackdropAttr() {
  const root = document.documentElement;
  if (theme.backdropSupported && theme.pref.backdrop !== "none") root.dataset.mica = "";
  else delete root.dataset.mica;
}

export function setBackdrop(backdrop: Backdrop) {
  setThemePref({ ...theme.pref, backdrop });
  applyBackdropAttr();
  if (theme.backdropSupported) void api.setBackdrop(backdrop).catch(() => {});
}

/** 只改色调强度，保留颜色来源 */
export function setTint(tint: number) {
  setThemePref({ ...theme.pref, tint });
}

export function setThemePref(pref: ThemePref) {
  theme.pref = pref;
  savePref(pref);
  resolve();
  syncLive();
}

/** 两个种子色看起来是否明显不同（只比较界面实际用到的色相与彩度；都接近灰色时色相不算数） */
function differs(a: string, b: string) {
  const x = hexToOklch(a);
  const y = hexToOklch(b);
  if (x.c < 0.03 && y.c < 0.03) return false;
  return hueDistance(x.h, y.h) > 12 || Math.abs(x.c - y.c) > 0.03;
}

/** 两张颜色场的平均差异（0–255），小于阈值就不重画背景 */
function fieldDistance(a: string[], b: string[]) {
  if (a.length !== b.length) return Infinity;
  let sum = 0;
  a.forEach((hex, i) => {
    const x = parseInt(hex.slice(1), 16);
    const y = parseInt(b[i].slice(1), 16);
    for (const shift of [16, 8, 0]) sum += Math.abs(((x >> shift) & 255) - ((y >> shift) & 255));
  });
  return sum / (a.length * 3);
}

/** 当前采样循环对应的“范围:间隔”；变了就换一个循环（旧循环看到代号变化后自己退出） */
let liveKey: string | null = null;
let liveGeneration = 0;
let stopMoveWatch: (() => void) | undefined;
let moveDebounce: ReturnType<typeof setTimeout> | undefined;
/** 与当前种子色不同、等待下一次采样确认的新颜色：画面在变，连续两次取到相近的颜色才换，避免闪烁 */
let candidate: string | null = null;
let sampling = false;

async function sampleLive() {
  const scope = theme.pref.source;
  if (theme.pref.glass || !isLive(scope) || sampling || document.visibilityState !== "visible") return;
  sampling = true;
  let sample: BackdropSample | null = null;
  try {
    sample = await api.sampleBackdrop(scope);
  } catch {
    // 读屏失败：沿用上一次的颜色
  } finally {
    sampling = false;
  }
  if (colorScope() === scope) acceptSample(sample, scope);
}

/** 窗口移动时又来一次读屏请求：正在读就记下，读完马上再读一次 */
let glassAgain = false;

async function sampleGlass() {
  if (!theme.pref.glass || document.visibilityState !== "visible") return;
  if (sampling) {
    glassAgain = true;
    return;
  }
  sampling = true;
  let frame: GlassFrame | null = null;
  try {
    frame = await api.captureGlassFrame(Math.round(innerWidth));
  } catch {
    // 读屏失败：保留上一帧
  } finally {
    sampling = false;
  }
  if (frame && theme.pref.glass) {
    drawGlass(frame);
    acceptSample(frame.sample, "behind");
  }
  if (glassAgain) {
    glassAgain = false;
    void sampleGlass();
  }
}

/** 采用一次取色：种子色连续两次相近才换，颜色场变化明显才重画 */
function acceptSample(sample: BackdropSample | null, scope: LiveScope) {
  if (!validSample(sample)) return;
  const prev = theme.live && theme.live.scope === scope ? theme.live : null;
  let seed = sample.seed;
  if (prev) {
    if (!differs(sample.seed, prev.seed)) {
      seed = prev.seed;
      candidate = null;
    } else if (candidate && !differs(sample.seed, candidate)) {
      candidate = null;
    } else {
      candidate = sample.seed;
      seed = prev.seed;
    }
  }
  const field = prev && fieldDistance(prev.field, sample.field) < 6 ? prev.field : sample.field;
  if (prev && seed === prev.seed && field === prev.field) return;

  theme.live = { ...sample, seed, field, scope };
  try {
    localStorage.setItem(LIVE_KEY, JSON.stringify(theme.live));
  } catch {
    // 忽略：只是首帧优化
  }
  resolve();
}

/** 等到下一次采样：按间隔等；无间隔时等下一帧（窗口隐藏时浏览器暂停动画帧，循环也就停下） */
function nextTick(interval: number) {
  return new Promise<void>((done) =>
    interval > 0 ? setTimeout(done, interval) : requestAnimationFrame(() => done()),
  );
}

/**
 * 打开实时取色或液态玻璃就循环读屏（上一次读完才等下一次），都关掉就停。
 * 窗口后方要在窗口移动后重读：取色等移动停下 0.2 秒再取；液态玻璃要背景跟手，移动中就不停地读。
 */
function syncLive() {
  const mode = theme.pref.glass ? "glass" : isLive(theme.pref.source) ? theme.pref.source : null;
  const key = mode && `${mode}:${theme.pref.liveInterval}`;
  if (key === liveKey) return;
  const modeChanged = mode !== liveKey?.split(":")[0];
  liveKey = key;
  const generation = ++liveGeneration;
  candidate = null;
  if (modeChanged) {
    stopMoveWatch?.();
    stopMoveWatch = undefined;
  }
  if (!mode) return;
  const sample = mode === "glass" ? sampleGlass : sampleLive;
  void (async () => {
    while (generation === liveGeneration) {
      await sample();
      await nextTick(theme.pref.liveInterval);
    }
  })();
  if (mode === "glass" && !stopMoveWatch) {
    stopMoveWatch = api.onWindowMoved(() => void sampleGlass());
  } else if (mode === "behind" && !stopMoveWatch) {
    stopMoveWatch = api.onWindowMoved(() => {
      clearTimeout(moveDebounce);
      moveDebounce = setTimeout(() => void sampleLive(), 200);
    });
  }
}

/** 液态玻璃开关 */
export function setGlass(glass: boolean) {
  setThemePref({ ...theme.pref, glass });
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
  // 桌面程序在 Windows 11 上默认以 Mica 创建窗口；用户选了别的材质就在这里切过去
  applyBackdropAttr();
  if (theme.backdropSupported && theme.pref.backdrop !== "mica") {
    void api.setBackdrop(theme.pref.backdrop).catch(() => {});
  }
  resolve();
  syncLive();
  void refreshSystemAccent();
  window.addEventListener("focus", () => void refreshSystemAccent());
  // 浅色 / 深色切换时颜色场要按新的底色亮度重画
  darkQuery.addEventListener("change", () => applyField());
  // 窗口从托盘重新显示时立即取一次
  document.addEventListener("visibilitychange", () => void (theme.pref.glass ? sampleGlass() : sampleLive()));
  void api
    .backgroundImage()
    .then(showBackground)
    .catch(() => {});
}

/** 设置实时取色的间隔（毫秒，0 为无间隔） */
export function setLiveInterval(liveInterval: number) {
  setThemePref({ ...theme.pref, liveInterval });
}
