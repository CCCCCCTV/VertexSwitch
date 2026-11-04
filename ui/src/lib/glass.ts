// 液态玻璃的折射：给每块玻璃（.panel、.glass）按它自己的尺寸和圆角生成一张位移贴图，
// 做成 SVG feDisplacementMap 滤镜交给 backdrop-filter，让玻璃边缘把背后的画面往里弯、像凸透镜一样放大，
// 中间保持平直。CSS 在这之后再叠模糊、提饱和度和高光（见 app.css 的 data-glass）。
//
// 只有 Chromium 内核支持 backdrop-filter 引用 SVG 滤镜（桌面程序的 WebView2 正是）；
// 其他浏览器里 --glass-filter 不生效，只剩模糊玻璃。

const NS = "http://www.w3.org/2000/svg";
const SELECTOR = ".panel, .glass";
/** 边缘弯折带的宽度（px） */
const BEZEL = 24;
/** 位移贴图满幅时的最大偏移（px），实际偏移最多是它的一半 */
const SCALE = 40;
/** 贴图按一半分辨率生成，feImage 拉伸回去，足够平滑 */
const MAP_SCALE = 0.5;

let defs: SVGSVGElement | null = null;
let resize: ResizeObserver | null = null;
let mutation: MutationObserver | null = null;
let seq = 0;
/** 正在照看的玻璃 → 它的滤镜 id */
const tracked = new Map<HTMLElement, string>();
const pending = new Set<HTMLElement>();
let frame = 0;

/**
 * 圆角矩形的位移贴图：R、G 两个通道记横、纵偏移（128 为不动）。
 * 离边越近偏移越大（(1 - t)² 递减），方向朝向内部：取样点往里挪，边缘的画面就被放大、弯过来。
 */
function lensMap(w: number, h: number, radius: number): string {
  const mw = Math.max(1, Math.round(w * MAP_SCALE));
  const mh = Math.max(1, Math.round(h * MAP_SCALE));
  const canvas = document.createElement("canvas");
  canvas.width = mw;
  canvas.height = mh;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  const image = ctx.createImageData(mw, mh);
  const data = image.data;
  const r = Math.min(radius, w / 2, h / 2);
  const [cx, cy] = [w / 2, h / 2];
  for (let y = 0; y < mh; y++) {
    for (let x = 0; x < mw; x++) {
      const px = (x + 0.5) / MAP_SCALE;
      const py = (y + 0.5) / MAP_SCALE;
      // 圆角矩形的有向距离（内部为负）
      const qx = Math.abs(px - cx) - (cx - r);
      const qy = Math.abs(py - cy) - (cy - r);
      const sdf = Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
      const depth = -sdf;
      let dx = 0;
      let dy = 0;
      if (depth < BEZEL) {
        // 外法线：圆角处沿对角，直边处垂直于边
        let nx: number;
        let ny: number;
        if (qx > 0 && qy > 0) {
          const len = Math.hypot(qx, qy) || 1;
          [nx, ny] = [qx / len, qy / len];
        } else if (qx > qy) {
          [nx, ny] = [1, 0];
        } else {
          [nx, ny] = [0, 1];
        }
        nx *= Math.sign(px - cx) || 1;
        ny *= Math.sign(py - cy) || 1;
        const m = (1 - Math.max(0, depth) / BEZEL) ** 2;
        [dx, dy] = [-nx * m, -ny * m];
      }
      const i = (y * mw + x) * 4;
      data[i] = 128 + dx * 127;
      data[i + 1] = 128 + dy * 127;
      data[i + 2] = 128;
      data[i + 3] = 255;
    }
  }
  ctx.putImageData(image, 0, 0);
  return canvas.toDataURL();
}

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
}

/** 按元素当前的尺寸（重新）生成它的滤镜 */
function install(el: HTMLElement) {
  if (!defs || !el.isConnected) return;
  const w = el.offsetWidth;
  const h = el.offsetHeight;
  if (w < 8 || h < 8) return;
  let id = tracked.get(el);
  if (!id) {
    id = `vs-glass-${++seq}`;
    tracked.set(el, id);
  }
  const radius = parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;
  const filter = svg("filter", {
    id,
    x: 0,
    y: 0,
    width: w,
    height: h,
    filterUnits: "userSpaceOnUse",
    "color-interpolation-filters": "sRGB",
  });
  filter.append(
    svg("feImage", {
      href: lensMap(w, h, radius),
      x: 0,
      y: 0,
      width: w,
      height: h,
      preserveAspectRatio: "none",
      result: "lens",
    }),
    svg("feDisplacementMap", {
      in: "SourceGraphic",
      in2: "lens",
      scale: SCALE,
      xChannelSelector: "R",
      yChannelSelector: "G",
    }),
  );
  defs.querySelector(`#${id}`)?.remove();
  defs.append(filter);
  el.style.setProperty("--glass-filter", `url(#${id})`);
}

/** 尺寸变化合并到下一帧统一重做 */
function schedule(el: HTMLElement) {
  pending.add(el);
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    for (const target of pending) install(target);
    pending.clear();
  });
}

/** 换页后：照看新出现的玻璃，丢掉已经离开页面的 */
function scan() {
  for (const [el, id] of tracked) {
    if (!el.isConnected) {
      resize?.unobserve(el);
      defs?.querySelector(`#${id}`)?.remove();
      tracked.delete(el);
    }
  }
  for (const el of document.querySelectorAll<HTMLElement>(SELECTOR)) {
    if (!tracked.has(el)) {
      tracked.set(el, `vs-glass-${++seq}`);
      resize?.observe(el);
    }
  }
}

export function startGlass() {
  if (defs) return;
  defs = svg("svg", { width: 0, height: 0, "aria-hidden": "true" });
  defs.style.position = "absolute";
  document.body.append(defs);
  resize = new ResizeObserver((entries) => {
    for (const entry of entries) schedule(entry.target as HTMLElement);
  });
  mutation = new MutationObserver(scan);
  // 只看界面本身（滤镜放在 body 下，避免自己触发自己）
  mutation.observe(document.getElementById("app") ?? document.body, { childList: true, subtree: true });
  scan();
}

export function stopGlass() {
  if (!defs) return;
  mutation?.disconnect();
  resize?.disconnect();
  cancelAnimationFrame(frame);
  frame = 0;
  pending.clear();
  for (const el of tracked.keys()) el.style.removeProperty("--glass-filter");
  tracked.clear();
  defs.remove();
  defs = null;
  resize = null;
  mutation = null;
}
