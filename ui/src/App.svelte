<script lang="ts">
  import { onMount } from "svelte";
  import { LayoutGrid, ListOrdered, ScrollText, Boxes, Settings2 } from "@lucide/svelte";
  import { api } from "./lib/api";
  import { live, startLive } from "./lib/live.svelte";
  import { PRESETS, previewBackground, setThemePref, setTint, theme } from "./lib/theme.svelte";
  import Overview from "./pages/Overview.svelte";
  import Requests from "./pages/Requests.svelte";
  import Logs from "./pages/Logs.svelte";
  import SettingsPage from "./pages/Settings.svelte";
  import ModelsPage from "./pages/Models.svelte";

  const pages = [
    { id: "overview", label: "概览", icon: LayoutGrid },
    { id: "requests", label: "请求", icon: ListOrdered },
    { id: "logs", label: "日志", icon: ScrollText },
    { id: "models", label: "模型", icon: Boxes },
    { id: "settings", label: "设置", icon: Settings2 },
  ] as const;

  type PageId = (typeof pages)[number]["id"];
  let page = $state<PageId>("overview");

  // 导航的选中药丸：一块滑到当前页，而不是每项各自亮起
  let navItems: HTMLButtonElement[] = $state([]);
  const gliderY = $derived.by(() => {
    const el = navItems[pages.findIndex((p) => p.id === page)];
    return el ? el.offsetTop + 4 : null;
  });
  // 首次定位不要动画，之后切页才滑动
  let gliderReady = $state(false);

  const failedRecent = $derived(
    live.requests.filter((r) => r.outcome !== "ok" && Date.now() - r.ts_ms < 10 * 60_000).length,
  );

  onMount(() => {
    // 仅浏览器预览：#page=logs&seed=rose 直接打开某页、某个种子色（用于截图与评审）
    if (api.preview) {
      const params = new URLSearchParams(location.hash.slice(1));
      const wanted = params.get("page");
      if (pages.some((p) => p.id === wanted)) page = wanted as PageId;
      const seed = params.get("seed");
      const preset = PRESETS.find((p) => p.id === seed);
      if (preset || /^[0-9a-f]{6}$/i.test(seed ?? "")) {
        setThemePref({ ...theme.pref, source: "custom", seed: preset?.seed ?? `#${seed}` });
      }
      // 背景图片：画一张模拟照片的渐变图，走和真图片一样的路径
      if (params.get("bg") === "1") {
        const canvas = document.createElement("canvas");
        canvas.width = 480;
        canvas.height = 300;
        const ctx = canvas.getContext("2d")!;
        const g = ctx.createLinearGradient(0, 0, 480, 300);
        g.addColorStop(0, "#f2c879");
        g.addColorStop(0.5, "#e07a8a");
        g.addColorStop(1, "#4b6fd1");
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, 480, 300);
        canvas.toBlob((blob) => blob && previewBackground(blob));
      }
      const liveScope = params.get("live");
      if (liveScope === "wallpaper" || liveScope === "behind") setThemePref({ ...theme.pref, source: liveScope });
      const tint = Number(params.get("tint"));
      if (params.has("tint") && tint >= 0 && tint <= 2) setTint(tint);
      // 浏览器里没有真正的 Mica：用一张模拟壁纸的渐变检查半透明效果
      if (params.get("mica") === "1") {
        document.documentElement.dataset.mica = "";
        document.documentElement.dataset.micaPreview = "";
      }
      const anchor = params.get("anchor");
      if (anchor) setTimeout(() => document.getElementById(anchor)?.scrollIntoView({ block: "center" }), 600);
    }
    void startLive();
    requestAnimationFrame(() => (gliderReady = true));
  });
</script>

<div class="shell">
  <nav aria-label="主导航">
    <img class="logo" src="/logo.svg" alt="VertexSwitch" width="30" height="30" />
    {#if gliderY !== null}
      <span class="glider" class:animated={gliderReady} style:transform="translateY({gliderY}px)" aria-hidden="true"
      ></span>
    {/if}
    {#each pages as p, i}
      <button
        bind:this={navItems[i]}
        class="item"
        class:active={page === p.id}
        aria-current={page === p.id ? "page" : undefined}
        onclick={() => (page = p.id)}
      >
        <span class="indicator">
          <p.icon size={20} strokeWidth={1.75} />
          {#if p.id === "requests" && failedRecent > 0}
            <span class="badge" aria-label="最近 10 分钟 {failedRecent} 个失败">{failedRecent}</span>
          {/if}
        </span>
        <span class="label">{p.label}</span>
      </button>
    {/each}
    {#if api.preview}<span class="preview" title="浏览器预览：示例数据，不连接核心">预览</span>{/if}
  </nav>

  <main>
    {#if page === "overview"}
      <Overview go={(p: PageId) => (page = p)} />
    {:else if page === "requests"}
      <Requests />
    {:else if page === "logs"}
      <Logs />
    {:else if page === "models"}
      <ModelsPage />
    {:else}
      <SettingsPage />
    {/if}
  </main>
</div>

<style>
  .shell {
    position: relative;
    isolation: isolate;
    overflow: hidden;
    display: grid;
    grid-template-columns: 76px 1fr;
    height: 100%;
  }

  /*
   * 实时取色的颜色场：一张几格大小的图拉伸铺满窗口再模糊，取到的颜色留在原来的位置。
   * 四周多铺一圈，模糊后边缘不会发灰；换图时 Chromium 会交叉淡入。
   */
  :global(:root[data-field]) .shell::before {
    content: "";
    position: absolute;
    inset: -60px;
    z-index: -1;
    pointer-events: none;
    background: var(--field) center / 100% 100% no-repeat;
    filter: blur(36px);
    transition: background-image var(--field-fade, 1.2s) ease;
  }

  /* 背景图片：盖在颜色场上、内容下面，不透明度在设置 → 外观里调 */
  :global(:root[data-bg-image]) .shell::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    pointer-events: none;
    background: var(--bg-image) center / cover no-repeat;
    opacity: var(--bg-image-opacity, 0.35);
  }

  /* 导航栏与内容区共用同一片背景，不再单独成一块 */
  nav {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 16px 0;
  }

  .glider {
    position: absolute;
    top: 0;
    left: 12px;
    width: 52px;
    height: 30px;
    border-radius: 999px;
    background: var(--accent-soft);
    pointer-events: none;
  }

  .glider.animated {
    transition: transform 0.38s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .logo {
    margin-bottom: 14px;
  }

  .item {
    position: relative;
    z-index: 1;
    flex-direction: column;
    gap: 3px;
    width: 64px;
    height: auto;
    padding: 4px 0 6px;
    border: none;
    background: none;
    color: var(--text-2);
  }

  .item:hover:not(:disabled),
  .item:active:not(:disabled) {
    background: none;
    color: var(--text);
    transform: none;
  }

  /* 药丸形选中指示（Material 导航栏） */
  .indicator {
    position: relative;
    display: grid;
    place-items: center;
    width: 52px;
    height: 30px;
    border-radius: 999px;
    transition:
      background-color 0.18s cubic-bezier(0.2, 0, 0, 1),
      transform 0.15s cubic-bezier(0.2, 0, 0, 1);
  }

  .item:hover:not(.active) .indicator {
    background: var(--hover);
  }

  .item:active .indicator {
    transform: scale(0.92);
  }

  .item:active:not(.active) .indicator {
    background: var(--press);
  }

  .item.active .indicator {
    color: var(--on-accent-soft);
  }

  .item.active {
    color: var(--text);
  }

  .label {
    font-size: 12px;
    font-weight: 500;
  }

  .item.active .label {
    font-weight: 600;
  }

  .badge {
    position: absolute;
    top: -3px;
    right: 6px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--bad);
    color: var(--on-bad);
    font-size: 10.5px;
    font-weight: 700;
    line-height: 16px;
  }

  .preview {
    margin-top: auto;
    font-size: 11px;
    color: var(--text-3);
    writing-mode: vertical-rl;
  }

  main {
    overflow: auto;
    padding: 22px 28px 28px;
  }
</style>
