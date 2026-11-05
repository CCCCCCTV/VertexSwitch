// 界面语言：中文 / English。偏好只存在这台电脑上，“跟随系统”按 WebView 报告的系统语言决定。
// t() 读的是响应式状态，模板和 $derived 里调用会随语言切换自动更新。
// 核心（代理）产生的日志与错误信息保持原样，不在这里翻译。

import { zh, type MessageKey } from "./locales/zh";
import { en } from "./locales/en";

export type Lang = "zh" | "en";
export type LangPref = "system" | Lang;
export type { MessageKey };

const KEY = "vertexswitch.lang";
const dicts: Record<Lang, Record<MessageKey, string>> = { zh, en };

function systemLang(): Lang {
  return (navigator.languages?.[0] ?? navigator.language ?? "").toLowerCase().startsWith("zh") ? "zh" : "en";
}

const resolve = (pref: LangPref): Lang => (pref === "system" ? systemLang() : pref);

function loadPref(): LangPref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "zh" || v === "en" ? v : "system";
  } catch {
    return "system";
  }
}

const initial = loadPref();
export const i18n = $state({ pref: initial, lang: resolve(initial) });

function applyLang() {
  document.documentElement.lang = i18n.lang === "zh" ? "zh-CN" : "en";
}
applyLang();

/** save 为 false 时只在本次运行生效（浏览器预览用） */
export function setLangPref(pref: LangPref, save = true) {
  i18n.pref = pref;
  i18n.lang = resolve(pref);
  applyLang();
  if (!save) return;
  try {
    if (pref === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, pref);
  } catch {
    // 存储不可用时只在本次运行生效
  }
}

// 跟随系统时，系统语言变了也跟着变
window.addEventListener("languagechange", () => {
  if (i18n.pref === "system") setLangPref("system");
});

/**
 * 取当前语言的文字。{name} 换成 vars 里的值；
 * 英文需要单复数时写成 "单数|复数"，按 vars.n 是否为 1 选一个。
 */
export function t(key: MessageKey, vars?: Record<string, string | number>): string {
  let text: string = dicts[i18n.lang][key];
  if (vars) {
    if (typeof vars.n === "number" && text.includes("|")) text = text.split("|")[vars.n === 1 ? 0 : 1];
    text = text.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m));
  }
  return text;
}
