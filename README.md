# check-pan-link

一个基于 Rust + Axum 的网盘分享链接检查服务。  
A cloud-drive share link checking service built with Rust and Axum.

当前项目的重点已经放在 115 分享链接上，既支持基础可用性检查，也支持直接拉取分享文件列表，并提供了一个内置 demo 页面方便手工测试。  
The current focus of this project is 115 share links. It supports both basic availability checks and file list retrieval, and includes a built-in demo page for manual testing.

## Current Capabilities / 当前能力

- `POST /api/check`
  用来检查分享链接是否可用，返回 `valid / invalid / unknown` 三态结果。  
  Checks whether a share link is usable and returns one of `valid / invalid / unknown`.
- `POST /api/115/share/list`
  用来获取 115 分享链接的文件列表。  
  Fetches the file list for a 115 share link.
- `GET /demo`
  内置一个简单的测试页面，可以直接在浏览器里测试 `/api/check` 和 `/api/115/share/list`。  
  Serves a built-in demo page for testing `/api/check` and `/api/115/share/list` in the browser.
- `POST /telegram/webhook`
  支持 Telegram webhook 调用检查能力。  
  Supports link checking through a Telegram webhook.

更完整的接口说明见 [docs/api.md](docs/api.md)。  
For full API details, see [docs/api.md](docs/api.md).

## Done / 已完成

- [x] 115 分享链接状态检查  
  支持提取码缺失、提取码错误、链接失效、违规内容、过期等状态识别。  
  115 share status checks, including missing receive code, invalid receive code, invalid share, violation content, expired links, and more.
- [x] 115 分享文件列表接口  
  支持从分享链接递归拉取文件内容。  
  115 share file listing with recursive traversal.
- [x] `list_type=files`  
  返回纯文件列表，并带完整路径。  
  Returns a flat file list with full paths.
- [x] `list_type=tree`  
  保留文件夹结构，返回树形结果。  
  Preserves folder structure and returns a tree.
- [x] 115 多域名兼容  
  支持 `115.com`、`115cdn.com`、`anxia.com`，其中 `anxia.com` 会在内部归一化成 `115cdn.com` 再处理。  
  Supports `115.com`, `115cdn.com`, and `anxia.com`, with `anxia.com` normalized internally to `115cdn.com`.
- [x] 内置 demo 页面  
  方便直接拿项目里的示例 115 链接做测试。  
  Includes a built-in demo page with sample 115 links for quick testing.
- [x] 115 实际返回兼容  
  兼容 115 接口里 `cid / pid / fid` 混用数字和字符串的情况。  
  Handles live 115 API responses where `cid / pid / fid` may be returned as either numbers or strings.

## 115 Examples / 115 使用示例

### Check a 115 Share / 检查 115 分享链接

```bash
curl -X POST http://127.0.0.1:8080/api/check \
  -H "content-type: application/json" \
  -d "{\"url\":\"https://115cdn.com/s/swfsfjg3h7i?password=l3a6\"}"
```

### List 115 Files / 获取 115 文件列表

```bash
curl -X POST http://127.0.0.1:8080/api/115/share/list \
  -H "content-type: application/json" \
  -d "{\"url\":\"https://115cdn.com/s/swfsfjg3h7i?password=l3a6\",\"list_type\":\"files\"}"
```

### Get 115 Tree Structure / 获取 115 树形目录

```bash
curl -X POST http://127.0.0.1:8080/api/115/share/list \
  -H "content-type: application/json" \
  -d "{\"url\":\"https://115cdn.com/s/swfsfjg3h7i?password=l3a6\",\"list_type\":\"tree\"}"
```

## Local Development / 本地运行

### Run Directly / 直接运行

```bash
cargo run
```

默认监听：  
Default bind address:

- `APP_HOST=127.0.0.1`
- `APP_PORT=8080`

启动后可以打开：  
After startup, you can open:

- `http://127.0.0.1:8080/demo`

### Run Tests / 运行测试

```bash
cargo test -- --nocapture
```

### Docker

```bash
docker build -t check-pan-link .
docker run --rm -p 8080:8080 check-pan-link
```

## Project Structure / 项目结构

```text
src/
  app.rs
  config.rs
  error.rs
  checker/
  providers/
  routes/
  telegram/
docs/
  api.md
```

主要目录说明：  
Main directories:

- `checker/`
  服务层与请求/响应模型。  
  Service layer and request/response models.
- `providers/`
  各网盘 provider 逻辑，当前 115 支持最完整。  
  Provider implementations, with 115 currently having the richest support.
- `routes/`
  HTTP 路由，包括 API、healthz、telegram、demo。  
  HTTP routes, including API, healthz, Telegram, and demo handlers.
- `docs/api.md`
  详细 API 文档。  
  Detailed API documentation.

## TODO

### Near Term / 近期计划

- [ ] 补充 README 里的更多请求/响应示例，覆盖异常场景。  
  Add more request/response examples to the README, including error scenarios.
- [ ] 把 `share_state = 0` 这类 115 审核中状态细化成更贴近业务语义的字段命名。  
  Refine `share_state = 0` and similar 115 review states with clearer business-oriented naming.
- [ ] 在 demo 页面里加入更多示例链接和错误场景按钮。  
  Add more sample links and error-case shortcuts to the demo page.
- [ ] 给 115 文件列表接口增加更清晰的错误提示和状态展示。  
  Improve error reporting and status presentation for the 115 file listing API.

### Later / 后续计划

- [ ] 为 `pan189` 和 `pan123` 增加像 115 一样的 provider 级状态识别，而不是只做基础 HTTP 检查。  
  Add provider-specific status detection for `pan189` and `pan123`, instead of relying only on basic HTTP checks.
- [ ] 增加批量检查接口，支持一次提交多条链接。  
  Add batch check APIs for multiple links in one request.
- [ ] 增加结果缓存或轻量历史记录，避免重复请求上游接口。  
  Add result caching or lightweight history to avoid repeated upstream requests.
- [ ] 增加更完整的 Web UI，而不只是当前 demo 页。  
  Build a fuller web UI beyond the current demo page.
- [ ] 支持更细粒度的状态分类，比如审核中、处理中、被取消、需要人工确认等。  
  Support finer-grained states such as reviewing, processing, cancelled, or manual confirmation required.
- [ ] 补充更多真实返回样例测试，持续提高兼容性。  
  Add more live-response fixture tests to keep improving compatibility.

## Environment Variables / 环境变量

| Name | Default | Description |
| --- | --- | --- |
| `APP_HOST` | `127.0.0.1` | HTTP bind host |
| `APP_PORT` | `8080` | HTTP bind port |
| `CHECK_TIMEOUT_SECS` | `10` | Outbound request timeout |
| `CORS_ALLOWED_ORIGIN` | unset | Optional CORS configuration |
| `TELOXIDE_TOKEN` | unset | Telegram bot token |
| `TELEGRAM_WEBHOOK_SECRET` | unset | Telegram webhook secret |

## Notes / 说明

这个项目目前已经可以拿来做 115 分享链接的日常检查和文件树测试，但整体还在持续演进阶段。  
This project is already usable for day-to-day 115 share checking and file tree inspection, but it is still evolving.

如果你现在主要关注 115：  
If your main focus right now is 115:

- `/api/check` 适合做链接状态判断。  
  `/api/check` is best for link status checks.
- `/api/115/share/list` 适合做目录抓取。  
  `/api/115/share/list` is best for file and folder listing.
- `/demo` 适合做手工联调和快速验证。  
  `/demo` is useful for manual integration testing and quick verification.
