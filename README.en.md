# VertexSwitch

[中文](README.md) | English

A desktop multi-protocol proxy for Vertex AI, with a GUI (Rust + Tauri).

- Serves the Anthropic Messages, Gemini generateContent and OpenAI Chat Completions APIs locally and forwards everything to Google Cloud Vertex AI
- Service account authentication with automatic token refresh
- Lives in the tray; configure models, service tiers (Standard / Flex / Priority), logs and usage visually
- Built for Windows 10/11 first; macOS / Linux work as a bonus

Implemented from scratch based on the official Google Cloud Vertex AI documentation. Not an official Google project.

## Status

Released: installers are on the [Releases](https://github.com/CCCCCCTV/VertexSwitch/releases/latest) page, and the app is still being updated. The behavior spec is `docs/SPEC.md`; background and milestones are in `docs/HANDOFF.md`.

The proxy core (`crates/vertexswitch-core`) implements:

- Anthropic Messages: `/v1/messages` (streaming and non-streaming) and `/v1/messages/count_tokens` → Claude on Vertex
- Native Gemini: `/v1beta/models/{model}:generateContent | streamGenerateContent | countTokens`
- Gemini service tiers: Standard / Flex / Priority, checked against `usageMetadata.trafficType`
- OpenAI Chat Completions: the Gemini compatible endpoint, MaaS open models, and OpenAI→Claude translation (including tool calls and streaming)
- Model lists: `/v1/models` (Anthropic / OpenAI format) and `/v1beta/models` (Gemini format)
- Admin API `/admin/api/*`: status, config read/write (applied immediately), model table, logs and request records (with live SSE streams), metrics and test requests; config and model table files reload automatically
- Automatic retries when upstream is busy (429 resource exhausted / rate limited, 503, Claude's 529 overloaded): up to 3 retries by default, waiting about 1, 2 and 4 s with random jitter and honouring `Retry-After`; under Settings → Proxy server you can turn it off, change the count, and set the wait as a number or a formula of the retry number `n` (e.g. `5`, `min(30, 3*n)`); stops as soon as the client disconnects
- `/metrics`: Prometheus metrics
- Desktop app (`src-tauri` + `ui`): stays in the tray (closing the window hides it; the tray menu switches the Gemini tier),
  with Overview / Requests / Logs / Models / Settings pages in Chinese or English; the proxy core runs in the same process
- Installer (per-user, no admin rights needed), single instance, start at login (tray only)

## Install

Download `VertexSwitch_x.y.z_x64-setup.exe` from [GitHub Releases](https://github.com/CCCCCCTV/VertexSwitch/releases/latest) and run it. It installs into your user profile and doesn't need admin rights.
Uninstalling keeps the config folder `%APPDATA%\VertexSwitch`.

Every release includes `SHA256SUMS.txt`, and the release notes have a PowerShell command you can paste to verify the download (it prints True when the file is intact).

After the first launch, pick your service account key file under Settings and you're ready to go.

The app checks GitHub for a newer version after startup and once a day (only the app name and version are sent; turn it off under Settings → System). Logs are also written to `%APPDATA%\VertexSwitch\logs`.

## Connect your clients

The proxy listens on `http://127.0.0.1:8787` by default (you can change the port under Settings). The Endpoints card on the Overview page lists these addresses; click one to copy it:

| Protocol | Environment variable | Address | Read by |
| --- | --- | --- | --- |
| Anthropic | `ANTHROPIC_BASE_URL` | `http://127.0.0.1:8787` | Claude Code, Anthropic SDK |
| OpenAI compatible | `OPENAI_BASE_URL` | `http://127.0.0.1:8787/v1` | OpenAI SDK |
| Gemini | `GOOGLE_GEMINI_BASE_URL` | `http://127.0.0.1:8787` | Gemini CLI, JS SDK (in the Python SDK, set `http_options.base_url` in code) |

When the API key under Settings (`server.api_key`) is empty, the proxy doesn't check client keys; if a client insists on a key, any value works. Once you set an API key, clients must send it as their API key (`Authorization: Bearer`, `x-api-key`, `x-goog-api-key` or the `key` query parameter all work).

## Build the desktop app from source

You need Node.js and the Rust (MSVC) toolchain:

```powershell
npm install
npx tauri dev                 # development: live-reloading UI
npx tauri build --no-bundle   # release build: target\release\VertexSwitch.exe
npx tauri build               # also builds the installer: target\release\bundle\nsis
```

`VertexSwitch.exe --minimized` starts without showing the window, in the tray only. To preview just the UI (sample data, no core): `npm run dev`.

## Run the proxy core from the command line

```powershell
cargo run --release -p vertexswitch-core                 # port from the config file (default 8787)
cargo run --release -p vertexswitch-core -- --port 8788  # another port while the desktop app is running
```

The config file is `%APPDATA%\VertexSwitch\config.toml` (defaults are used when it doesn't exist). Example:

```toml
[google]
credentials_file = 'C:\path\to\service-account.json'
project_id = ""        # empty: read it from the key file
region = "global"      # global / us / eu / a specific region

[gemini]
region = ""            # empty: same as google.region; Flex only works on global
tier = "standard"      # default tier: standard / flex / priority
shared_only = false    # true: PayGo only, skipping Provisioned Throughput
flex_server_timeout = 1800
openai_native = "auto" # OpenAI-format Gemini requests that need a tier are converted to the native format (or always / never)

[gemini.model_tiers]   # per upstream model, overrides the default tier (also editable on the Models page)
"gemini-3.5-flash" = "flex"

[server]
api_key = ""           # when set, clients must send this key

[retry]
busy = true            # retry when upstream is busy (429 / 503 / 529)
busy_attempts = 3      # max retries (1-10)
busy_wait = "2^(n-1)"  # seconds before retry n: a number or a formula with + - * / ^, brackets, min(), max()
```

The model table is built in. To change it, copy `crates/vertexswitch-core/data/models.toml` to
`%APPDATA%\VertexSwitch\models.toml` and edit it.

Tiers apply to both the native Gemini API and the OpenAI compatible API (`/v1/chat/completions`). Vertex's compatible endpoint doesn't accept tiers,
so when a tier is needed the proxy converts the OpenAI-format request to the native Gemini format and converts the response back.
A single request can override the tier with the `X-VertexSwitch-Tier: flex|priority|standard` header,
and the `X-VertexSwitch-Tier` response header reports the tier actually used.

## Admin API

Only accepts connections from this computer and requires the admin token. The token is stored in Windows Credential Manager; copy it under Settings → System in the app, or print it with `vertexswitch-core admin-token`:

```powershell
# First click "Copy" under Settings → System (or run vertexswitch-core admin-token from a source build)
$h = @{ Authorization = "Bearer " + (Get-Clipboard) }
$api = "http://127.0.0.1:8787/admin/api"

Invoke-RestMethod "$api/status" -Headers $h
# Change the tier: applied immediately and written back to the config file
Invoke-RestMethod "$api/config" -Method Patch -Headers $h -ContentType "application/json" `
  -Body '{"gemini":{"tier":"flex"}}'
Invoke-RestMethod "$api/requests?limit=20" -Headers $h
# Live logs (SSE)
curl.exe -N -H "Authorization: $($h.Authorization)" "$api/logs/stream"
```

All endpoints are listed in section 12 of `docs/SPEC.md` (in Chinese).

## Development

```powershell
npm run check                 # UI type check
npm run build                 # build the UI (embedded when the Tauri shell compiles)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

All tests use local fake upstreams (wiremock and raw TCP servers) and never call the real Vertex AI.

## License

[GNU AGPL-3.0](LICENSE) (version 3.0 only, `AGPL-3.0-only`). You may use, modify and distribute it freely, including commercially; but if you distribute a modified version, or offer a modified version to others as a network service, you must publish its complete source code under the same license.
