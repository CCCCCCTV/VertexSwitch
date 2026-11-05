<script lang="ts">
  import { tick } from "svelte";
  import { live } from "../lib/live.svelte";
  import { time } from "../lib/format";
  import { t } from "../lib/i18n.svelte";

  type Level = "all" | "WARN" | "ERROR";
  let level = $state<Level>("all");
  let follow = $state(true);
  let box: HTMLDivElement | undefined = $state();

  const rank = { INFO: 0, WARN: 1, ERROR: 2 } as const;
  const rows = $derived(
    level === "all" ? live.logs : live.logs.filter((e) => rank[e.level] >= rank[level as "WARN" | "ERROR"]),
  );

  $effect(() => {
    // 有新日志且在跟随模式时滚到底部
    void rows.length;
    if (follow && box) void tick().then(() => box && (box.scrollTop = box.scrollHeight));
  });

  function onScroll() {
    if (!box) return;
    follow = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  }
</script>

<header>
  <h1>{t("nav.logs")}</h1>
  <div class="chips" role="radiogroup" aria-label={t("logs.level")}>
    <button class:on={level === "all"} onclick={() => (level = "all")}>{t("logs.all")}</button>
    <button class:on={level === "WARN"} onclick={() => (level = "WARN")}>{t("logs.warn")}</button>
    <button class:on={level === "ERROR"} onclick={() => (level = "ERROR")}>{t("logs.error")}</button>
  </div>
</header>

<div class="panel log" bind:this={box} onscroll={onScroll}>
  {#each rows as e (e.seq)}
    <div class="line {e.level.toLowerCase()}">
      <span class="ts">{time(e.ts_ms)}</span>
      <span class="lv">{e.level}</span>
      <span class="msg">{e.message}</span>
    </div>
  {:else}
    <div class="empty">{t("logs.empty")}</div>
  {/each}
</div>
{#if !follow}
  <button class="jump" onclick={() => { follow = true; if (box) box.scrollTop = box.scrollHeight; }}>{t("logs.jump")}</button>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  h1 {
    font-size: 22px;
  }

  .chips {
    display: flex;
    gap: 4px;
  }

  .chips button {
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 13px;
  }

  .chips button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }

  .log {
    height: calc(100vh - 140px);
    overflow: auto;
    padding: 10px 0;
    font-family: var(--mono);
    font-size: 12.5px;
  }

  .line {
    display: grid;
    grid-template-columns: 76px 52px 1fr;
    gap: 8px;
    padding: 2px 16px;
  }

  .line:hover {
    background: var(--hover);
  }

  .ts {
    color: var(--text-3);
  }

  .lv {
    font-weight: 600;
    color: var(--text-2);
  }

  .warn .lv,
  .warn .msg {
    color: var(--warn);
  }

  .error .lv,
  .error .msg {
    color: var(--bad);
  }

  .msg {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .jump {
    position: fixed;
    right: 44px;
    bottom: 32px;
    box-shadow: var(--shadow);
  }
</style>
