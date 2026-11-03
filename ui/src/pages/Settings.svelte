<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../lib/api";
  import { refreshStatus } from "../lib/live.svelte";
  import type { AppInfo, Settings } from "../lib/types";

  let original = $state<Settings | null>(null);
  let draft = $state<Settings | null>(null);
  let info = $state<AppInfo | null>(null);
  let saving = $state(false);
  let message = $state<{ tone: "ok" | "bad" | "warn"; text: string } | null>(null);
  let showKey = $state(false);
  let dropFields = $state("");

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
    <section class="card">
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

    <section class="card">
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

    <section class="card">
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

    <section class="card">
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

    <details class="card">
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

    {#if info}
      <p class="faint paths">
        配置文件 <span class="mono">{info.config_path}</span> · 管理 token <span class="mono">{info.admin_token_path}</span>
        <button class="link" onclick={() => api.openConfigDir()}>打开配置目录</button>
      </p>
    {/if}
  </div>
{:else}
  <div class="empty">正在加载…</div>
{/if}

<style>
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
    position: sticky;
    top: -24px;
    background: var(--bg);
    padding: 8px 0;
    z-index: 2;
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

  details summary::before {
    content: "▸";
    display: inline-block;
    width: 16px;
    color: var(--text-3);
  }

  details[open] summary::before {
    content: "▾";
  }

  .paths {
    font-size: 12px;
    margin: 0;
  }
</style>
