// 检查更新：启动 10 秒后查一次 GitHub 上的最新版本，之后每天一次（设置里可以关掉自动检查）。
// 有新版本时设置页显示下载链接，导航的“设置”上出现一个小圆点。只提示，不自动下载安装。

import { api, errorText } from "./api";
import type { UpdateInfo } from "./types";

const AUTO_KEY = "vertexswitch.autoUpdateCheck";
const DAY = 24 * 3600_000;

function loadAuto(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) !== "off";
  } catch {
    return true;
  }
}

export const update = $state({
  auto: loadAuto(),
  info: null as UpdateInfo | null,
  checking: false,
  error: "",
});

export async function checkUpdate() {
  if (update.checking) return;
  update.checking = true;
  update.error = "";
  try {
    update.info = await api.checkUpdate();
  } catch (e) {
    update.error = errorText(e);
  } finally {
    update.checking = false;
  }
}

export function setAutoCheck(on: boolean) {
  update.auto = on;
  try {
    if (on) localStorage.removeItem(AUTO_KEY);
    else localStorage.setItem(AUTO_KEY, "off");
  } catch {
    // 记不住也不影响本次运行
  }
  if (on && !update.info) void checkUpdate();
}

export function startUpdateChecks() {
  const tick = () => {
    if (update.auto) void checkUpdate();
  };
  setTimeout(tick, 10_000);
  setInterval(tick, DAY);
}
