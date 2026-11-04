<script lang="ts">
  // 请求时间线：最近 30 分钟每分钟的请求数，按结果堆叠；降级与档位切换标在时间轴上。
  import { onMount } from "svelte";
  import { OctagonX } from "@lucide/svelte";
  import { live } from "../lib/live.svelte";
  import { time } from "../lib/format";

  const MINUTES = 30;
  const H = 168;
  const PAD = { top: 10, right: 8, bottom: 22, left: 30 };

  let width = $state(600);
  let now = $state(Date.now());
  let hover = $state<number | null>(null);

  onMount(() => {
    const t = setInterval(() => (now = Date.now()), 15_000);
    return () => clearInterval(t);
  });

  const start = $derived(Math.floor(now / 60_000) * 60_000 - (MINUTES - 1) * 60_000);

  const buckets = $derived.by(() => {
    const b = Array.from({ length: MINUTES }, (_, i) => ({
      ts: start + i * 60_000,
      ok: 0,
      interrupted: 0,
      failed: 0,
      downgraded: 0,
    }));
    for (const r of live.requests) {
      const i = Math.floor((r.ts_ms - start) / 60_000);
      if (i < 0 || i >= MINUTES) continue;
      if (r.outcome === "ok") b[i].ok++;
      else if (r.outcome === "error") b[i].failed++;
      else b[i].interrupted++;
    }
    // 降级来自核心的告警日志（trafficType 与请求的档位不符）
    for (const e of live.logs) {
      if (e.level !== "WARN" || !e.message.includes("实际按")) continue;
      const i = Math.floor((e.ts_ms - start) / 60_000);
      if (i >= 0 && i < MINUTES) b[i].downgraded++;
    }
    return b;
  });

  const total = $derived(buckets.reduce((s, b) => s + b.ok + b.interrupted + b.failed, 0));
  const failed = $derived(buckets.reduce((s, b) => s + b.failed + b.interrupted, 0));
  const max = $derived(Math.max(4, ...buckets.map((b) => b.ok + b.interrupted + b.failed)));
  const step = $derived(max <= 4 ? 1 : Math.ceil(max / 4));
  const ticks = $derived(Array.from({ length: Math.floor(max / step) + 1 }, (_, i) => i * step));

  const plotW = $derived(Math.max(0, width - PAD.left - PAD.right));
  const plotH = H - PAD.top - PAD.bottom;
  const slot = $derived(plotW / MINUTES);
  const barW = $derived(Math.max(2, slot - 3));
  const y = (v: number) => PAD.top + plotH - (v / max) * plotH;
  const x = (i: number) => PAD.left + i * slot + (slot - barW) / 2;

  const marks = $derived(
    live.tierMarks
      .filter((m) => m.ts_ms >= start)
      .map((m) => ({ ...m, x: PAD.left + ((m.ts_ms - start) / 60_000) * slot })),
  );

  const hovered = $derived(hover === null ? null : buckets[hover]);
</script>

<div class="timeline" bind:clientWidth={width}>
  <div class="summary">
    <strong>{total}</strong> 个请求
    {#if failed}<span class="bad"><OctagonX size={14} />{failed} 个失败或中断</span>{/if}
    <span class="legend">
      <span><i class="sw ok"></i>成功</span>
      <span><i class="sw warn"></i>中断</span>
      <span><i class="sw bad hatch"></i>失败</span>
      <span><i class="tri"></i>档位降级</span>
    </span>
  </div>
  <svg
    width={width}
    height={H}
    role="img"
    aria-label="最近 30 分钟共 {total} 个请求，其中 {failed} 个失败或中断"
    onmouseleave={() => (hover = null)}
  >
    <defs>
      <!-- 失败柱加斜纹：状态不只靠颜色区分（种子色相近红色时也读得出） -->
      <pattern id="fail-hatch" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
        <rect width="5" height="5" class="bad" />
        <line x1="0" y1="0" x2="0" y2="5" class="hatch-line" />
      </pattern>
    </defs>
    {#each ticks as t}
      <line class="grid" x1={PAD.left} x2={width - PAD.right} y1={y(t)} y2={y(t)} />
      <text class="axis" x={PAD.left - 8} y={y(t) + 4} text-anchor="end">{t}</text>
    {/each}

    {#each buckets as b, i}
      {@const okH = (b.ok / max) * plotH}
      {@const intH = (b.interrupted / max) * plotH}
      {@const badH = (b.failed / max) * plotH}
      <g class:dim={hover !== null && hover !== i}>
        <!-- 每分钟一个浅底座：没有请求的时段读作“0”，而不是图没画出来 -->
        <rect class="slot" x={x(i)} width={barW} y={PAD.top + plotH - 3} height="3" rx="1.5" />
        <rect class="ok" x={x(i)} width={barW} y={PAD.top + plotH - okH} height={okH} rx="2" />
        <rect class="warn" x={x(i)} width={barW} y={PAD.top + plotH - okH - intH} height={intH} rx="2" />
        <rect class="fail" x={x(i)} width={barW} y={PAD.top + plotH - okH - intH - badH} height={badH} rx="2" />
        {#if b.downgraded}
          {@const top = PAD.top + plotH - okH - intH - badH - 5}
          <path class="tri" d="M{x(i) + barW / 2 - 4} {top - 6} h8 l-4 6 z" />
        {/if}
      </g>
      <rect
        class="hit"
        x={PAD.left + i * slot}
        y={PAD.top}
        width={slot}
        height={plotH}
        onmouseenter={() => (hover = i)}
        role="presentation"
      />
      {#if i % 10 === 0 || i === MINUTES - 1}
        <text class="axis" x={x(i) + barW / 2} y={H - 6} text-anchor="middle">
          {i === MINUTES - 1 ? "现在" : time(b.ts).slice(0, 5)}
        </text>
      {/if}
    {/each}

    {#each marks as m}
      <line class="mark" x1={m.x} x2={m.x} y1={PAD.top - 4} y2={PAD.top + plotH} />
      <text class="mark-label" x={m.x + 4} y={PAD.top + 8}>切到 {m.tier}</text>
    {/each}
  </svg>

  {#if hovered}
    <div class="tip" style:left="{Math.min(width - 170, Math.max(0, x(hover!) - 60))}px">
      <b>{time(hovered.ts).slice(0, 5)}</b>
      成功 {hovered.ok} · 中断 {hovered.interrupted} · 失败 {hovered.failed}
      {#if hovered.downgraded}<br /><span class="warn-t">档位降级 {hovered.downgraded} 次</span>{/if}
    </div>
  {/if}
</div>

<style>
  .timeline {
    position: relative;
  }

  .summary {
    display: flex;
    align-items: baseline;
    gap: 6px;
    margin-bottom: 6px;
    color: var(--text-2);
    font-size: 13px;
  }

  .summary strong {
    font: 600 20px var(--font-display);
    color: var(--text);
  }

  .summary .bad {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--bad);
  }

  .legend {
    margin-left: auto;
    display: flex;
    gap: 12px;
    font-size: 12px;
  }

  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .sw {
    width: 9px;
    height: 9px;
    border-radius: 2px;
  }

  .sw.ok {
    background: var(--series-ok);
  }

  .sw.warn {
    background: var(--series-warn);
  }

  .sw.bad {
    background: repeating-linear-gradient(45deg, var(--series-bad) 0 2px, var(--surface) 2px 3px);
  }

  i.tri {
    width: 0;
    height: 0;
    border-left: 4.5px solid transparent;
    border-right: 4.5px solid transparent;
    border-top: 7px solid var(--warn);
  }

  svg {
    display: block;
    overflow: visible;
  }

  .grid {
    stroke: var(--border);
    stroke-dasharray: 2 4;
  }

  .axis {
    fill: var(--text-3);
    font-size: 11px;
  }

  rect.slot {
    fill: var(--border);
  }

  rect.ok {
    fill: var(--series-ok);
  }

  rect.warn {
    fill: var(--series-warn);
  }

  rect.bad {
    fill: var(--series-bad);
  }

  /* 失败柱用斜纹图案；CSS 的 fill 会覆盖 SVG 的 fill 属性，所以图案也写在 CSS 里 */
  rect.fail {
    fill: url(#fail-hatch);
  }

  .hatch-line {
    stroke: var(--surface);
    stroke-width: 1.6;
  }

  path.tri {
    fill: var(--warn);
  }

  g {
    transition: opacity 0.12s;
  }

  g.dim {
    opacity: 0.4;
  }

  .hit {
    fill: transparent;
  }

  /* 档位切换标记：出现时自上而下画出来 */
  .mark {
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-dasharray: 4 3;
    transform-box: fill-box;
    transform-origin: top;
    animation: draw 0.7s cubic-bezier(0.16, 1, 0.3, 1) both;
  }

  @keyframes draw {
    from {
      transform: scaleY(0);
    }
  }

  .mark-label {
    fill: var(--accent);
    font-size: 11px;
    font-weight: 600;
  }

  .tip {
    position: absolute;
    top: 30px;
    width: 170px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--bg);
    font-size: 12px;
    pointer-events: none;
    box-shadow: 0 4px 12px oklch(0 0 0 / 0.18);
  }

  .tip b {
    display: block;
  }

  .warn-t {
    color: var(--warn-soft);
  }
</style>
