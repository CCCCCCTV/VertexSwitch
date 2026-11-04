<script lang="ts">
  import { onMount } from "svelte";
  import { LayoutGrid, ListOrdered, ScrollText, Boxes, Settings2 } from "@lucide/svelte";
  import { api } from "./lib/api";
  import { live, startLive } from "./lib/live.svelte";
  import { PRESETS, setThemePref } from "./lib/theme.svelte";
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
        setThemePref({ source: "custom", seed: preset?.seed ?? `#${seed}` });
      }
      const anchor = params.get("anchor");
      if (anchor) setTimeout(() => document.getElementById(anchor)?.scrollIntoView({ block: "center" }), 600);
    }
    void startLive();
  });
</script>

<div class="shell">
  <nav aria-label="主导航">
    <img class="logo" src="/logo.svg" alt="VertexSwitch" width="30" height="30" />
    {#each pages as p}
      <button
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
    display: grid;
    grid-template-columns: 76px 1fr;
    height: 100%;
  }

  nav {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 16px 0;
    background: var(--rail);
    border-right: 1px solid var(--border);
  }

  .logo {
    margin-bottom: 14px;
  }

  .item {
    flex-direction: column;
    gap: 3px;
    width: 64px;
    height: auto;
    padding: 4px 0 6px;
    border: none;
    background: none;
    color: var(--text-2);
  }

  .item:hover:not(:disabled) {
    background: none;
    color: var(--text);
  }

  /* 药丸形选中指示（Material 导航栏） */
  .indicator {
    position: relative;
    display: grid;
    place-items: center;
    width: 52px;
    height: 30px;
    border-radius: 999px;
    transition: background 0.18s cubic-bezier(0.2, 0, 0, 1);
  }

  .item:hover .indicator {
    background: var(--surface-3);
  }

  .item.active .indicator {
    background: var(--accent-soft);
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
    color: #fff;
    font-size: 10.5px;
    font-weight: 700;
    line-height: 16px;
  }

  .preview {
    margin-top: auto;
    font-size: 11px;
    color: var(--text-3);
    writing-mode: vertical-rl;
    letter-spacing: 0.1em;
  }

  main {
    overflow: auto;
    padding: 22px 28px 28px;
  }
</style>
