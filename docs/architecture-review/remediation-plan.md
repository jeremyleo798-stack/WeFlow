# 修复计划（Remediation Plan）

基线：`main` @ `e66f89c` + 工作区未提交修复。排期按“发布阻断 → 信任链 → 安全纵深 → 工程门禁”排序，P0 未完成前不建议对外发布。

## 执行状态总览（2026-10-30）

| 事项 | 状态 | 证据 |
| --- | --- | --- |
| R-01 afterPack 路径 | 已完成（工作区） | 假 context 运行复制 DLL + koffi 成功 |
| R-02 DLL 分发 | 已完成 | DLL 入库 `native/wcdb-native/prebuilt/`；CI 缺失即失败 |
| R-03 WCDB_DLL_PATH 校验 | 已完成（工作区） | typecheck 0 错误 |
| R-04 发布源对齐 | 已澄清 + 已加检查 | `check-release-config.cjs` OK |
| R-05 签名 | 已决策：不做（自用/朋友分发） | portable 分发不支持自动更新；保留来源与 SHA-512 提示 |
| R-06 CI 质量门禁 | 已完成（工作区） | release.yml 4 个 job 均接入 |
| R-07 webSecurity/CSP | 已完成 | `weflow-media://` 特权协议 + `webSecurity: true` + 构建期 CSP + 权限默认拒绝 |
| R-08 IPC 路径能力 | 已完成（工作区） | 删除 writeBase64；openPath 目录约束 |
| R-09 更新说明渲染 | 已完成（工作区） | `test:release-notes` 通过 |
| R-10 HTTP 边界 | 风险接受 | 0.0.0.0 为文档化选项 |
| R-11 单一包管理器 | 未执行 | 需工具链决策 |
| R-12 IPC 契约化 | 未执行 | 重构项 |
| R-13 发布 smoke | 已完成（工作区） | `check-release-artifacts.cjs` OK |
| R-14 AGENTS.md | 已完成（工作区） | 新增 `AGENTS.md` |

## P0 — 发布阻断（当天）

### R-01 提交 afterPack 路径修复（F-01）· 已完成

- 改动：`package.json:115` → `"afterPack": "scripts/after-pack.cjs"`
- 验收：
  - `node -e "require('./scripts/after-pack.cjs')"` 退出码 0；
  - `npm run build:portable` 完成且产物内存在 `resources/resources/native/wcdb-native/dist/wcdb_api.dll` 与 `resources/koffi/<triplet>/koffi.node`。
- 风险：`build:portable` 耗时较长；可先只跑 `electron-builder --dir` 验证 afterPack。

### R-02 决策并落实 DLL 分发方式（F-02）· 已完成

- 已做：采用“提交入库”。DLL 移至 `native/wcdb-native/prebuilt/wcdb_api.dll`（`prebuilt/README.md` 记录构建方式、rustc commit、大小与 sha256），`after-pack.cjs` 默认源改为该路径；`WCDB_DLL_SOURCE` 仍可覆盖；CI 缺失或 DLL < 1 MB 直接抛错并打印 sha256。
- 验收：假 context 默认源复制成功（12,839,447 bytes，sha256 `55120c34…`）；`CI=true` + 缺失源退出码 1。

### R-03 提交 WCDB_DLL_PATH 存在性校验（F-03）· 已完成

- 改动：`electron/services/wcdbCore.ts:410-411` 使用 `trim()` + `existsSync`，失效 env 回退到内置候选。
- 验收：`npm run typecheck:electron` 0 错误；手工设置 `WCDB_DLL_PATH=C:\not-exist.dll` 启动，日志显示回退到 bundled 路径且初始化成功。

## P1 — 发布信任链（发布前必须闭环）

### R-04 对齐更新/发布仓库（F-04）· 已澄清

- 结论：发布源为上游 `hicccc77/WeFlow`；本地 remote 是 fork，预期差异，不构成缺陷。
- 已做：`scripts/check-release-config.cjs` 校验 `package.json.repository`、`build.publish.owner/repo`、`main.ts` 内 `repoBase`、README 指向同一仓库；已接入 release CI 的 `release-config-check` job。
- 剩余：真实升级联调受分发形态限制（portable 不支持 electron-updater，上游暂无 release），改为手动替换 exe 验证；启动与手动检查的失败分支均已静默处理。

### R-05 更新包完整性与签名（F-05）· 已决策：不做证书（自用/朋友分发）

- 已做：`app:checkForUpdates` 与 `app:updateAvailable` 载荷附带 `sha512`、下载地址与文件名；更新弹窗展示“未签名，请核对来源与 SHA-512”。
- 分发形态结论（2026-10-30）：发布产物是 `portable` 单文件 exe，而 `electron-updater@6.8.11` 不含 portable 安装支持（无 `PORTABLE_EXECUTABLE` 处理，安装走 NSIS）；上游 `hicccc77/WeFlow` 目前 0 个 release。因此现有分发形态下自动更新不可用，朋友使用为手动下载替换 exe，签名校验无实际作用。
- 决策：不购买 Authenticode 证书；保留“未签名 + SHA-512 + 下载来源”提示作为完整性核对手段。若未来改为 NSIS 安装版分发，再配置 `win.publisherName` 并恢复强制签名校验。
- 参考：`node_modules/electron-updater/out/NsisUpdater.js` 的 `verifySignature` 在 `publisherName == null` 时直接返回。

### R-06 发布 CI 质量门禁（F-06）· 已完成

- 改动：`.github/workflows/release.yml` 各构建任务在 `npx vite build` 前执行：
  ```bash
  npm run typecheck   # 同时覆盖 src 与 electron
  npm run test:wcdb
  npm run test:release-notes
  ```
- 验收：故意在 `electron/main.ts` 插入类型错误，release 工作流失败；正常提交通过。

## P2 — 安全纵深（一个迭代内）

### R-07 恢复渲染进程同源策略 + CSP（F-07）· 已完成

- 实际动作：新增 `electron/mediaProtocol.ts` / `electron/mediaUrl.ts` / `src/utils/mediaUrl.ts`。`weflow-media://` 注册为 standard/secure/CORS/stream 特权 scheme，只放行 userData、temp、`Documents/WeFlow`、配置 `cachePath`/`dbPath` 内的文件，每次请求 realpath（`realpathSync.native`，展开 8.3 短名）归一后做目录包含校验；支持 Range/206、HEAD 与 CORS 预检。主进程媒体 URL 生产点、渲染进程全部 `file://` 消费点迁移到该协议；三个窗口 `webSecurity: true`；`vite.config.ts` 仅在构建期注入 CSP meta（开发模式 HMR 不受影响），其中 `script-src` 为 `'self' 'wasm-unsafe-eval'`：Shiki 代码高亮引擎经 WebAssembly 实例化，缺少该项时打包版会拒绝 `WebAssembly.instantiate`（2026-10-30 整体审查在真实打包版中复现并修复）；`setPermissionRequestHandler` / `setPermissionCheckHandler` 默认拒绝，仅放行 `clipboard-sanitized-write`。
- 验收（真实构建 + CDP）：图片、视频经协议加载成功（naturalWidth/视频元数据正常）；Range 请求返回 206 与正确 `Content-Range`；`C:/Windows/win.ini` → 403，缺失文件 → 404；`fetch('file:///C:/Windows/win.ini')` 被 CSP 阻断；权限状态 granted 且用户手势下 `navigator.clipboard.writeText` 成功；首页与侧边栏正常渲染；Shiki 的 wasm chunk 在打包版内可编译（`WebAssembly.compile`）。`npm run typecheck` 0 错误。

### R-08 收紧 IPC 路径能力（F-08）· 已完成

- 实际动作：删除无调用方的 `file:writeBase64`（连同 preload 与 `electron.d.ts` 声明）；`shell:openPath` 改为 realpath 规范化 + `relative()` 包含校验，允许根为 userData/downloads/Documents\WeFlow/`exportPath`，以及用户经 `dialog` 选择的目录（`approveOpenRoot`）。
- 验收：非白名单路径返回“不允许打开该路径”；导出目录选择后其子路径可打开。类型检查 0 错误。

### R-09 更新说明安全渲染（F-09）· 已完成

- 实际动作：抽出 `src/components/releaseNotes.tsx`，HTML 先转纯文本再渲染为 `p/h2-h6/ul/li` React 节点，不使用 `dangerouslySetInnerHTML`；`UpdateDialog` 改为调用该渲染器。
- 验收：`npm run test:release-notes` 注入 `<script>`/`<img onerror>`/`javascript:` 链接后断言不产生可执行 HTML。

### R-10 HTTP API 边界（F-15）· 风险接受

- 结论：`0.0.0.0` 绑定是文档化选项（Docker/N8N 场景），SSE 依赖 query token；默认仅本机 + 无 token 全拒绝已足够。保留现状，发布说明中提示暴露面。

## P3 — 工程可持续性（持续）

### R-11 单一包管理器（F-10、F-11）

- 动作：选定 npm 或 pnpm；将所选包管理器的锁文件与构建策略文件（`pnpm-workspace.yaml`）入库；删除另一套 overrides；CI 使用 frozen lockfile。
- 验收：干净 clone + CI 同命令可复现 `node_modules` 与 Electron 版本；`npm ci --dry-run` 或 `pnpm install --frozen-lockfile` 通过。

### R-12 IPC 契约化（F-12）

- 动作：按域拆分 `electron/ipc/*.ts`，统一 `registerHandler(channel, schema, fn)`；preload 从通道清单生成。
- 验收：新增通道只改一处；`tsc -p tsconfig.node.json` 能发现 main/preload 类型不一致。

### R-13 发布 smoke 测试（F-13）· 已完成

- 实际动作：`scripts/check-release-artifacts.cjs` 检查产物资源目录内 `app.asar`、`koffi/<platform>_<arch>/koffi.node`、Windows 下 DLL > 1 MB；已接入 release CI 四个产物任务。另加 `check-release-config.cjs`。

### R-14 Agent/领域指引（F-14）· 已完成（本地）

- 实际动作：新增仓库根 `AGENTS.md`，含命令、包管理器现状、发布关键路径、安全不变量、文档索引。
- 注意：`.gitignore:69` 显式忽略 `AGENTS.md`/`CLAUDE.md`，文件仅在本机生效；要让新 clone 也获得指引，需维护者移除该忽略规则。

## 排期与依赖

| 阶段 | 事项 | 依赖 | 预估 | 状态 |
| --- | --- | --- | --- | --- |
| P0 | R-01 → R-02 → R-03 | 无 | 0.5–1 天 | 已完成 2 项，R-02 待决策 |
| P1 | R-04 → R-05 → R-06 | R-02 决策 | 1–2 天 | R-04/R-06 完成，R-05 已决策不做（portable 无自动更新） |
| P2 | R-07 → R-08 → R-09 → R-10 | R-06 门禁生效 | 2–3 天 | R-07/R-08/R-09 完成，R-10 接受 |
| P3 | R-11 → R-12 → R-13 → R-14 | 无 | 持续 | R-13/R-14 完成，R-11/R-12 未启 |

## 验收总门禁（发布前）

1. `npm run typecheck` 0 错误；`npm run test:wcdb`、`npm run test:release-notes` 全绿。
2. 干净 clone + CI 同命令构建成功，产物含 DLL/koffi，且 afterPack 缺失即失败（DLL 已入库，满足）。
3. 旧版本可检测并完成一次真实更新；签名未配置时弹窗展示来源与 SHA-512（已实现，待真实升级联调）。
4. 三个窗口 `webSecurity: true` 且 CSP 生效；任意路径写入/打开被拒（已满足：媒体协议 containment 真实构建验证、路径 IPC 已约束）。
5. 更新源与 `git remote` 一致，或有书面镜像说明（已文档化上游关系并有 CI 检查）。
