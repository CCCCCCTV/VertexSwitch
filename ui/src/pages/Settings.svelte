<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { refreshStatus } from "../lib/live.svelte";
  import {
    BACKDROPS,
    LIVE_SCOPES,
    PRESETS,
    TINT_DEFAULT,
    setBackdrop,
    setThemePref,
    setTint,
    theme,
  } from "../lib/theme.svelte";
  import type { AppInfo, Backdrop, Settings } from "../lib/types";

  let original = $state<Settings | null>(null);
  let draft = $state<Settings | null>(null);
  let info = $state<AppInfo | null>(null);
  const liveScope = $derived(LIVE_SCOPES.find((s) => s.id === theme.pref.source));
  let saving = $state(false);
  let message = $state<{ tone: "ok" | "bad" | "warn"; text: string } | null>(null);
  let showKey = $state(false);
  let dropFields = $state("");
  let autostart = $state<boolean | null>(null);
  let systemMessage = $state<{ tone: "ok" | "bad"; text: string } | null>(null);

  /** 只包含改动字段的合并补丁（RFC 7396）。数组整体替换。 */
  function diff(a: any, b: any): any {
    const out: any = {};
    for (const key of Object.keys(b)) {
      const x = a?.[key];
      const y = b[key];
      if (y && typeof y === "object" && !Array.isArray(y)) {
        const sub = diff(x, y);
        if (Object.keys(sub).length) out[key] = sub;
      } else if (JSON.stringify(x) !== JSON.stringify(y)) {
        out[key] = y;
      }
    }
    return out;
  }

  const patch = $derived.by(() => {
    if (!original || !draft) return {};
    const next = $state.snapshot(draft) as Settings;
    next.claude.drop_body_fields = dropFields.split(",").map((s) => s.trim()).filter(Boolean);
    return diff(original, next);
  });
  const dirty = $derived(Object.keys(patch).length > 0);

  async function load() {
    const s = await api.settings();
    original = s;
    draft = structuredClone(s);
    dropFields = s.claude.drop_body_fields.join(", ");
  }

  onMount(() => {
    void load();
    void api.appInfo().then((i) => (info = i));
    void api.getAutostart().then((a) => (autostart = a.enabled)).catch(() => (autostart = null));
    return api.onSettingsChanged(() => {
      if (!dirty) void load();
    });
  });

  async function save() {
    saving = true;
    message = null;
    try {
      const result = await api.patchSettings(patch);
      original = result.settings;
      draft = structuredClone(result.settings);
      dropFields = result.settings.claude.drop_body_fields.join(", ");
      if (result.restart_error) {
        message = { tone: "bad", text: `已保存，但${result.restart_error}` };
      } else if (result.restart_required) {
        message = { tone: "ok", text: `已保存，代理已改为监听 ${result.server?.addr ?? ""}` };
      } else {
        message = { tone: "ok", text: "已保存并立即生效" };
      }
      await refreshStatus();
    } catch (e) {
      message = { tone: "bad", text: errorText(e) };
    } finally {
      saving = false;
    }
  }

  function revert() {
    if (original) {
      draft = structuredClone($state.snapshot(original) as Settings);
      dropFields = original.claude.drop_body_fields.join(", ");
    }
    message = null;
  }

  async function pickKey() {
    const path = await api.pickKeyFile();
    if (path && draft) draft.google.credentials_file = path;
  }

  // 开机自启与配置文件无关，切换后立即生效
  async function toggleAutostart(enabled: boolean) {
    systemMessage = null;
    try {
      autostart = (await api.setAutostart(enabled)).enabled;
      systemMessage = { tone: "ok", text: enabled ? "已开启开机自启（启动后只在托盘运行）" : "已关闭开机自启" };
    } catch (e) {
      systemMessage = { tone: "bad", text: errorText(e) };
    }
  }

  async function copyAdminToken() {
    systemMessage = null;
    try {
      await navigator.clipboard.writeText((await api.adminToken()).token);
      systemMessage = { tone: "ok", text: "管理 token 已复制到剪贴板" };
    } catch (e) {
      systemMessage = { tone: "bad", text: errorText(e) };
    }
  }

  function generateKey() {
    const bytes = crypto.getRandomValues(new Uint8Array(24));
    if (draft) draft.server.api_key = "vs-" + [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
    showKey = true;
  }
</script>

<header>
  <h1>设置</h1>
  <div class="actions">
    {#if message}<span class="msg {message.tone}">{message.text}</span>{/if}
    <button onclick={revert} disabled={!dirty || saving}>撤销</button>
    <button class="primary" onclick={save} disabled={!dirty || saving}>{saving ? "保存中…" : "保存"}</button>
  </div>
</header>

<datalist id="regions">
  <option value="global">global（推荐）</option>
  <option value="us">us（美国多区域）</option>
  <option value="eu">eu（欧盟多区域）</option>
  <option value="us-central1"></option>
  <option value="us-east5"></option>
  <option value="europe-west1"></option>
  <option value="asia-northeast1"></option>
</datalist>

{#if draft}
  <div class="sections">
    <section class="panel">
      <h2>Google Cloud</h2>
      <label class="field">
        <span>服务账号密钥文件</span>
        <div class="with-btn">
          <input type="text" bind:value={draft.google.credentials_file} placeholder="留空则使用环境变量 GOOGLE_APPLICATION_CREDENTIALS" />
          <button onclick={pickKey}>选择…</button>
        </div>
      </label>
      <div class="two">
        <label class="field">
          <span>项目 ID</span>
          <input type="text" bind:value={draft.google.project_id} placeholder="留空则从密钥文件读取" />
        </label>
        <label class="field">
          <span>默认区域</span>
          <input type="text" list="regions" bind:value={draft.google.region} />
          <small>Claude 使用此区域；Gemini 未单独设置时也用它</small>
        </label>
      </div>
    </section>

    <section class="panel">
      <h2>Gemini</h2>
      <div class="two">
        <label class="field">
          <span>区域</span>
          <input type="text" list="regions" bind:value={draft.gemini.region} placeholder={`沿用默认区域（${draft.google.region}）`} />
          <small>Flex 只支持 global；Priority 支持 global / us / eu</small>
        </label>
        <label class="field">
          <span>服务档位</span>
          <select bind:value={draft.gemini.tier}>
            <option value="standard">Standard（标准）</option>
            <option value="flex">Flex（低价，排队）</option>
            <option value="priority">Priority（优先）</option>
          </select>
        </label>
        <label class="field">
          <span>Flex 排队超时（秒）</span>
          <input type="number" min="1" max="1800" bind:value={draft.gemini.flex_server_timeout} />
          <small>最长 1800 秒（30 分钟）</small>
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={draft.gemini.shared_only} />
          <span>只走 PayGo<small>不使用预配吞吐量（发送 X-Vertex-AI-LLM-Request-Type: shared）</small></span>
        </label>
      </div>
    </section>

    <section class="panel">
      <h2>代理服务</h2>
      <div class="two">
        <label class="field">
          <span>监听地址</span>
          <input type="text" bind:value={draft.server.host} />
          <small>127.0.0.1 只允许本机访问</small>
        </label>
        <label class="field">
          <span>端口</span>
          <input type="number" min="1" max="65535" bind:value={draft.server.port} />
          <small>修改后自动在新端口重新监听</small>
        </label>
      </div>
      <label class="field">
        <span>API key</span>
        <div class="with-btn">
          <input type={showKey ? "text" : "password"} bind:value={draft.server.api_key} placeholder="留空则不鉴权" autocomplete="off" />
          <button onclick={() => (showKey = !showKey)}>{showKey ? "隐藏" : "显示"}</button>
          <button onclick={generateKey}>生成</button>
        </div>
        <small>设置后客户端需携带：Authorization: Bearer、x-api-key 或 x-goog-api-key</small>
      </label>
    </section>

    <section class="panel">
      <h2>Ollama（本地模型）</h2>
      <div class="two">
        <label class="check">
          <input type="checkbox" bind:checked={draft.ollama.enabled} />
          <span>启用<small>通过 OpenAI 接口转发到本地 Ollama，不需要 Google 认证</small></span>
        </label>
        <label class="field">
          <span>Ollama 地址</span>
          <input type="text" bind:value={draft.ollama.base_url} disabled={!draft.ollama.enabled} />
        </label>
      </div>
    </section>

    <details class="panel">
      <summary><h2>高级</h2></summary>
      <div class="three">
        <label class="field"><span>连接超时（秒）</span><input type="number" min="1" bind:value={draft.timeouts.connect} /></label>
        <label class="field"><span>非流式读超时（秒）</span><input type="number" min="1" bind:value={draft.timeouts.read} /></label>
        <label class="field"><span>流式空闲上限（秒）</span><input type="number" min="1" bind:value={draft.timeouts.stream_idle} /></label>
        <label class="field"><span>连接错误重试次数</span><input type="number" min="0" max="5" bind:value={draft.retry.attempts} /></label>
        <label class="field">
          <span>配额耗尽判定（秒）</span><input type="number" min="1" bind:value={draft.retry.quota_hang_seconds} />
          <small>上游挂起超过这么久后无响应断开，按配额耗尽处理</small>
        </label>
        <label class="field"><span>OpenAI→Claude 默认 max_tokens</span><input type="number" min="1" bind:value={draft.openai.default_max_tokens} /></label>
      </div>
      <label class="field">
        <span>转发给 Claude 前删除的字段</span>
        <input type="text" bind:value={dropFields} placeholder="逗号分隔" />
      </label>
    </details>

    <section class="panel" id="appearance">
      <div class="row-title"><h2>外观</h2><span class="faint small">立即生效，只影响这台电脑</span></div>
      <div class="seeds" role="radiogroup" aria-label="主题色">
        <button
          role="radio"
          aria-checked={theme.pref.source === "system"}
          class="seed system"
          class:on={theme.pref.source === "system"}
          onclick={() => setThemePref({ ...theme.pref, source: "system" })}
        >
          <span class="swatch" style:background={theme.systemAccent ?? "var(--accent)"}></span>
          跟随 Windows 强调色
        </button>
        {#each PRESETS as p}
          <button
            role="radio"
            aria-checked={theme.pref.source === "custom" && theme.pref.seed.toLowerCase() === p.seed.toLowerCase()}
            aria-label={p.label}
            title={p.label}
            class="seed"
            class:on={theme.pref.source === "custom" && theme.pref.seed.toLowerCase() === p.seed.toLowerCase()}
            onclick={() => setThemePref({ ...theme.pref, source: "custom", seed: p.seed })}
          >
            <span class="swatch" style:background={p.seed}></span>
          </button>
        {/each}
        <label class="seed custom" title="自定义颜色">
          <input
            type="color"
            value={theme.seed}
            oninput={(e) => setThemePref({ ...theme.pref, source: "custom", seed: e.currentTarget.value })}
            aria-label="自定义主题色"
          />
          自定义
        </label>
      </div>
      <div class="live">
        <span class="live-label" id="live-label">实时取色</span>
        <div class="segmented" role="radiogroup" aria-labelledby="live-label">
          <button
            role="radio"
            aria-checked={!liveScope}
            class:on={!liveScope}
            onclick={() => liveScope && setThemePref({ ...theme.pref, source: "system" })}>关闭</button
          >
          {#each LIVE_SCOPES as scope}
            <button
              role="radio"
              aria-checked={liveScope?.id === scope.id}
              class:on={liveScope?.id === scope.id}
              onclick={() => setThemePref({ ...theme.pref, source: scope.id })}>{scope.label}</button
            >
          {/each}
        </div>
      </div>
      <p class="faint small live-hint">
        {liveScope ? liveScope.hint : "关闭时颜色来自上面选的颜色；打开后从桌面背景取色，背景按取到的颜色和位置渐变"}
      </p>
      <div class="tint">
        <label for="tint">色调强度</label>
        <span class="faint small">中性</span>
        <input
          id="tint"
          type="range"
          min="0"
          max="2"
          step="0.05"
          value={theme.pref.tint}
          oninput={(e) => setTint(Number(e.currentTarget.value))}
        />
        <span class="faint small">浓</span>
        <button class="ghost" onclick={() => setTint(TINT_DEFAULT)} disabled={theme.pref.tint === TINT_DEFAULT}>恢复默认</button>
      </div>
      {#if theme.backdropSupported || api.preview}
        <div class="backdrop">
          <label for="backdrop">窗口材质</label>
          <select
            id="backdrop"
            value={theme.pref.backdrop}
            onchange={(e) => setBackdrop(e.currentTarget.value as Backdrop)}
          >
            {#each BACKDROPS as b}<option value={b.id}>{b.label}</option>{/each}
          </select>
          <span class="faint small"
            >{BACKDROPS.find((b) => b.id === theme.pref.backdrop)?.hint}{liveScope
              ? "；实时取色打开时背景由取到的颜色铺满，材质看不到"
              : ""}</span
          >
        </div>
      {/if}
      <p class="faint small">整个界面的色调都由这个颜色生成，强度只影响底色和面板；浅色 / 深色跟随系统设置。</p>
    </section>

    <section class="panel">
      <div class="row-title">
        <h2>系统</h2>
        {#if systemMessage}<span class="msg {systemMessage.tone}">{systemMessage.text}</span>{/if}
      </div>
      <label class="check inline">
        <input
          type="checkbox"
          checked={autostart ?? false}
          disabled={autostart === null}
          onchange={(e) => toggleAutostart(e.currentTarget.checked)}
        />
        <span>开机时自动启动<small>登录 Windows 后在托盘里运行，不弹出窗口；立即生效，不需要点保存</small></span>
      </label>
      {#if info}
        <dl class="facts">
          <dt>配置文件</dt>
          <dd><span class="mono">{info.config_path}</span> <button class="link" onclick={() => api.openConfigDir()}>打开目录</button></dd>
          <dt>管理 token</dt>
          <dd>{info.admin_token_location} <button class="link" onclick={copyAdminToken}>复制</button></dd>
          <dt>版本</dt>
          <dd>{info.version}</dd>
        </dl>
      {/if}
    </section>
  </div>
{:else}
  <div class="empty">正在加载…</div>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    /* 吸顶：铺满内容区宽度并带一条分隔线，滚动时下面的内容不会从边缘露出 */
    position: sticky;
    top: -22px;
    z-index: 2;
    margin: -22px -28px 16px;
    padding: 22px 28px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }

  /* 半透明窗口材质下底色很薄：吸顶栏用更不透明的同色底再加模糊，滚过的内容不会透出来 */
  :global(:root[data-mica]) header {
    background: oklch(from var(--bg) l c h / 0.92);
    backdrop-filter: blur(24px) saturate(1.2);
  }

  /* 实时取色时背后是颜色场：吸顶栏半透明加模糊，颜色连贯，滚过的内容也看不清 */
  :global(:root[data-field]) header {
    background: oklch(from var(--bg) l c h / 0.35);
    backdrop-filter: blur(28px) saturate(1.2);
  }

  h1 {
    font-size: 22px;
  }

  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .msg {
    font-size: 13px;
    margin-right: 6px;
  }

  .msg.ok {
    color: var(--ok);
  }

  .msg.bad {
    color: var(--bad);
  }

  .sections {
    display: grid;
    gap: 14px;
    max-width: 920px;
  }

  .field {
    display: grid;
    gap: 5px;
    margin-bottom: 12px;
    align-content: start;
  }

  .field > span {
    font-size: 13px;
    font-weight: 500;
  }

  small {
    font-size: 12px;
    color: var(--text-3);
  }

  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 16px;
    align-items: start;
  }

  .three {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0 16px;
    margin-top: 12px;
    align-items: start;
  }

  .with-btn {
    display: flex;
    gap: 8px;
  }

  .with-btn input {
    flex: 1;
  }

  .check {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding-top: 24px;
    margin-bottom: 12px;
  }

  .check input {
    margin-top: 4px;
    accent-color: var(--accent);
  }

  .check span {
    display: grid;
    font-size: 13px;
    font-weight: 500;
  }

  details summary {
    cursor: pointer;
    list-style: none;
  }

  details summary h2 {
    display: inline;
    font-size: 15px;
  }

  /* 用描边画的折叠箭头，不用字符图标 */
  details summary::before {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    margin: 0 10px 2px 2px;
    border: solid var(--text-3);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(-45deg);
    transition: transform 0.18s cubic-bezier(0.2, 0, 0, 1);
  }

  details[open] summary::before {
    transform: rotate(45deg);
  }

  .small {
    font-size: 12px;
  }

  .seeds {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
    margin-bottom: 10px;
  }

  .seed {
    height: 36px;
    padding: 0 10px;
    gap: 8px;
    border-radius: 999px;
  }

  .seed:not(.system):not(.custom) {
    width: 36px;
    padding: 0;
  }

  .seed.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--ring);
  }

  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.12);
  }

  .seed.custom {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    background: var(--surface);
    cursor: pointer;
    font-weight: 500;
  }

  .tint {
    display: grid;
    grid-template-columns: auto auto minmax(160px, 320px) auto auto;
    justify-content: start;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
  }

  .backdrop {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-bottom: 10px;
  }

  .backdrop label {
    font-weight: 500;
    margin-right: 6px;
  }

  .backdrop select {
    width: auto;
  }

  .tint label {
    font-weight: 500;
    margin-right: 6px;
  }

  .tint input[type="range"] {
    accent-color: var(--accent);
    width: 100%;
  }

  .seed.custom input {
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
  }

  .row-title {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 8px;
  }

  .row-title h2 {
    font-size: 15px;
  }

  .check.inline {
    padding-top: 0;
  }

  .live {
    display: flex;
    gap: 16px;
    align-items: center;
    margin-top: 16px;
  }

  .live-label {
    font-weight: 500;
  }

  .live-hint {
    margin: 6px 0 14px;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 4px 0 0;
    font-size: 13px;
  }

  .facts dt {
    color: var(--text-2);
  }

  .facts dd {
    margin: 0;
  }
</style>
