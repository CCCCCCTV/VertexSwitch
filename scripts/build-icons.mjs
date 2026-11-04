// 由 src-tauri/icons/source 下的两份 SVG 生成全部图标：npm run icons
//
// - icon.svg：完整版（十二道光芒），用于 32px 及以上
// - icon-small.svg：小尺寸版（八道更粗的光芒），用于 16–24px（包括托盘）
//
// 输出：Windows 多尺寸 icon.ico（小尺寸帧用小尺寸版）、macOS icon.icns、各尺寸 PNG、
// 托盘用的 tray/*.png，以及界面里的 ui/public/logo.svg。

import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Resvg } from "@resvg/resvg-js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const icons = join(root, "src-tauri", "icons");
const full = readFileSync(join(icons, "source", "icon.svg"), "utf8");
const small = readFileSync(join(icons, "source", "icon-small.svg"), "utf8");

const SMALL_MAX = 24;
const render = (size) =>
  new Resvg(size <= SMALL_MAX ? small : full, { fitTo: { mode: "width", value: size } }).render().asPng();

/** ICO：每帧直接存 PNG（Vista 起支持），宽高 256 记为 0 */
function ico(sizes) {
  const pngs = sizes.map(render);
  const header = Buffer.alloc(6 + 16 * sizes.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(sizes.length, 4);
  let offset = header.length;
  sizes.forEach((size, i) => {
    const at = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, at);
    header.writeUInt8(size >= 256 ? 0 : size, at + 1);
    header.writeUInt8(0, at + 2);
    header.writeUInt8(0, at + 3);
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(pngs[i].length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += pngs[i].length;
  });
  return Buffer.concat([header, ...pngs]);
}

/** ICNS：各条目直接存 PNG（macOS 10.7 起支持） */
function icns(entries) {
  const chunks = entries.map(([type, size]) => {
    const png = render(size);
    const head = Buffer.alloc(8);
    head.write(type, 0, "ascii");
    head.writeUInt32BE(png.length + 8, 4);
    return Buffer.concat([head, png]);
  });
  const body = Buffer.concat(chunks);
  const head = Buffer.alloc(8);
  head.write("icns", 0, "ascii");
  head.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([head, body]);
}

const out = (name, data) => {
  const path = join(icons, name);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
  console.log(`  ${name}`);
};

out("icon.ico", ico([16, 20, 24, 32, 40, 48, 64, 256]));
out(
  "icon.icns",
  icns([
    ["ic11", 32],
    ["ic12", 64],
    ["ic07", 128],
    ["ic13", 256],
    ["ic08", 256],
    ["ic14", 512],
    ["ic09", 512],
    ["ic10", 1024],
  ]),
);
out("32x32.png", render(32));
out("64x64.png", render(64));
out("128x128.png", render(128));
out("128x128@2x.png", render(256));
out("icon.png", render(512));
// 托盘：按屏幕缩放选一张，正好是系统托盘图标的像素尺寸，不再让系统缩放
for (const size of [16, 20, 24, 32]) out(`tray/${size}.png`, render(size));

copyFileSync(join(icons, "source", "icon.svg"), join(root, "ui", "public", "logo.svg"));
console.log("  ui/public/logo.svg");
