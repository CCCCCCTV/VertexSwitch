<script lang="ts">
  import { live } from "../lib/live.svelte";
  import { OUTCOME_TONE, duration, time, tokens } from "../lib/format";
  import { t } from "../lib/i18n.svelte";

  let filter = $state<"all" | "failed">("all");
  let query = $state("");

  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return live.requests
      .filter((r) => filter === "all" || r.outcome !== "ok")
      .filter((r) => !q || `${r.model ?? ""} ${r.path} ${r.backend ?? ""}`.toLowerCase().includes(q))
      .slice()
      .reverse();
  });
  const failed = $derived(live.requests.filter((r) => r.outcome !== "ok").length);
</script>

<header>
  <h1>{t("nav.requests")}</h1>
  <div class="tools">
    <div class="chips" role="radiogroup" aria-label={t("requests.filter")}>
      <button class:on={filter === "all"} onclick={() => (filter = "all")}
        >{t("requests.all", { n: live.requests.length })}</button
      >
      <button class:on={filter === "failed"} onclick={() => (filter = "failed")}
        >{t("requests.failed", { n: failed })}</button
      >
    </div>
    <input type="text" placeholder={t("requests.search")} bind:value={query} aria-label={t("requests.search")} />
  </div>
</header>

<div class="panel table-card">
  {#if rows.length}
    <table>
      <thead>
        <tr>
          <th>{t("requests.time")}</th>
          <th>{t("requests.protocol")}</th>
          <th>{t("requests.model")}</th>
          <th class="tier-col">{t("requests.tier")}</th>
          <th>{t("requests.status")}</th>
          <th class="num">{t("requests.duration")}</th>
          <th class="num">{t("common.input")}</th>
          <th class="num">{t("common.output")}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.seq)}
          <tr>
            <td class="mono faint nowrap">{time(r.ts_ms)}</td>
            <td class="nowrap">
              <span class="proto">{r.protocol}</span>{#if r.stream}<span class="pill">{t("requests.stream")}</span>{/if}
            </td>
            <td class="model">
              <div class="mono ellipsis" title={r.model ?? undefined}>{r.model ?? "—"}</div>
              {#if r.error}<div class="err" title={r.error}>{r.error}</div>{/if}
            </td>
            <td class="tier-col">{#if r.tier}<span class="pill accent">{r.tier}</span>{:else}<span class="faint">—</span>{/if}</td>
            <td>
              <span class="pill {OUTCOME_TONE[r.outcome]}">
                {r.status} {t(`outcome.${r.outcome}`)}
              </span>
            </td>
            <td class="num nowrap">{duration(r.duration_ms)}</td>
            <td class="num nowrap">{tokens(r.input_tokens)}</td>
            <td class="num nowrap">{tokens(r.output_tokens)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="empty">{t(live.requests.length ? "requests.noMatch" : "requests.empty")}</div>
  {/if}
</div>
<p class="faint hint">{t("requests.hint")}</p>

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
    gap: 16px;
  }

  h1 {
    font-size: 22px;
  }

  .tools {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .tools input {
    width: 240px;
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

  .table-card {
    padding: 0;
    overflow: hidden;
  }

  .proto {
    margin-right: 6px;
  }

  .err {
    color: var(--bad);
    font-size: 12px;
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .nowrap {
    white-space: nowrap;
  }

  /* 模型列吃掉剩余宽度，过长时截断（完整 ID 在提示里） */
  td.model {
    max-width: 0;
    width: 100%;
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 900px) {
    .tier-col {
      display: none;
    }

    .tools input {
      width: 160px;
    }
  }

  .hint {
    font-size: 12px;
    margin-top: 10px;
  }
</style>
