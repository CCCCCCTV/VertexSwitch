<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "./lib/api";
  import { live, startLive } from "./lib/live.svelte";
  import Overview from "./pages/Overview.svelte";
  import Requests from "./pages/Requests.svelte";
  import Logs from "./pages/Logs.svelte";
  import SettingsPage from "./pages/Settings.svelte";
  import ModelsPage from "./pages/Models.svelte";

  const pages = [
    { id: "overview", label: "概览", icon: "M3 13h8V3H3zm0 8h8v-6H3zm10 0h8V11h-8zm0-18v6h8V3z" },
    { id: "requests", label: "请求", icon: "M4 6h16M4 12h16M4 18h10" },
    { id: "logs", label: "日志", icon: "M5 4h14v16H5zM8 8h8M8 12h8M8 16h5" },
    { id: "models", label: "模型", icon: "M12 3 3 8l9 5 9-5zM3 16l9 5 9-5M3 12l9 5 9-5" },
    { id: "settings", label: "设置", icon: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" },
  ] as const;

  type PageId = (typeof pages)[number]["id"];
  let page = $state<PageId>("overview");

  const server = $derived(live.status?.server);
  const healthy = $derived(live.status?.status === "ok" && server?.running);

  onMount(() => {
    void startLive();
  });
</script>

<div class="shell">
  <nav>
    <div class="brand">
      <img src="/logo.svg" alt="" width="28" height="28" />
      <span>VertexSwitch</span>
    </div>
    {#each pages as p}
      <button class="nav-item" class:active={page === p.id} onclick={() => (page = p.id)}>
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
          <path d={p.icon} fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        {p.label}
      </button>
    {/each}

    <div class="foot">
      {#if live.status}
        <div class="state">
          <span class="dot" class:ok={healthy} class:bad={!healthy}></span>
          {#if server?.running}
            <span class="mono">{server.addr}</span>
          {:else}
            <span>未在监听</span>
          {/if}
        </div>
        <div class="faint">v{live.status.version}{api.preview ? " · 界面预览" : ""}</div>
      {:else}
        <div class="faint">正在连接核心…</div>
      {/if}
    </div>
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
    grid-template-columns: 208px 1fr;
    height: 100%;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 12px;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    font-weight: 700;
    font-size: 16px;
    padding: 2px 8px 18px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    border: none;
    background: none;
    text-align: left;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--text-2);
    font-weight: 500;
  }

  .nav-item:hover:not(.active) {
    background: var(--surface-2);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .foot {
    margin-top: auto;
    padding: 10px 8px 0;
    border-top: 1px solid var(--border);
    font-size: 12px;
    display: grid;
    gap: 4px;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .dot.ok {
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
  }

  .dot.bad {
    background: var(--bad);
    box-shadow: 0 0 0 3px var(--bad-soft);
  }

  main {
    overflow: auto;
    padding: 24px 28px;
  }
</style>
