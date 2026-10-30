# 修复计划（Remediation Plan）

基线：`main` @ `e66f89c` + 工作区未提交修复。排期按“发布阻断 → 信任链 → 安全纵深 → 工程门禁”排序，P0 未完成前不建议对外发布。

## 执行状态总览（2026-10-30）

| 事项 | 状态 | 证据 |
| --- | --- | --- |
| R-01 afterPack 路径 | 已完成（工作区） | 假 context 运行复制 DLL + koffi 成功 |
| R-02 DLL 分发 | 已加固，待决策 | CI 缺失即失败；分发方式未定 |
| R-03 WCDB_DLL_PATH 校验 | 已完成（工作区） | typecheck 0 错误 |
| R-04 发布源对齐 | 已澄清 + 已加检查 | `check-release-config.cjs` OK |
| R-05 签名 | 未执行 | 需证书决策 |
| R-06 CI 质量门禁 | 已完成（工作区） | release.yml 4 个 job 均接入 |
| R-07 webSecurity/CSP | 缓期 | 需媒体协议迁移 |
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

### R-02 决策并落实 DLL 分发方式（F-02）· 已加固，待决策

- 已做：`after-pack.cjs` 在 `CI=true` 或 `WCDB_DLL_REQUIRED=1` 时，DLL/koffi 缺失或 DLL < 1 MB 直接抛错；支持 `WCDB_DLL_SOURCE` 指向外部构建产物；打印 sha256。本地构建仍仅告警。
- 剩余：从 A/B/C 中选定一种并落到 CI（当前 CI 会因 `native/wcdb-native/dist/wcdb_api.dll` 未入库而失败，需先提供二进制）。
- 选项：
  - **A（推荐）** CI 内构建 Rust DLL（`cargo build --release -p wcdb_api`），`after-pack.cjs` 找不到 DLL 时 **抛错**；
  - **B** 将 `wcdb_api.dll` 作为 Release asset，CI 构建前下载并校验 sha256，缺失同样抛错；
  - **C** 明确不内置：删除 `copyWcdb` 与 bundled 候选路径，启动时提示配置 `wcdbLibPath`。
- 验收：从干净 clone 执行与 release 相同的命令，产物内 DLL 存在（A/B）或应用给出明确可操作提示（C）；`after-pack.cjs` 不再出现“缺失仍成功”的分支。
- 注意：DLL 为 12.8 MB 二进制，若选 A/B，需在文档中记录来源、版本与校验值。

### R-03 提交 WCDB_DLL_PATH 存在性校验（F-03）· 已完成

- 改动：`electron/services/wcdbCore.ts:410-411` 使用 `trim()` + `existsSync`，失效 env 回退到内置候选。
- 验收：`npm run typecheck:electron` 0 错误；手工设置 `WCDB_DLL_PATH=C:\not-exist.dll` 启动，日志显示回退到 bundled 路径且初始化成功。

## P1 — 发布信任链（发布前必须闭环）

### R-04 对齐更新/发布仓库（F-04）· 已澄清

- 结论：发布源为上游 `hicccc77/WeFlow`；本地 remote 是 fork，预期差异，不构成缺陷。
- 已做：`scripts/check-release-config.cjs` 校验 `package.json.repository`、`build.publish.owner/repo`、`main.ts` 内 `repoBase`、README 指向同一仓库；已接入 release CI 的 `release-config-check` job。
- 剩余：安装旧版做一次真实升级联调。

### R-05 更新包完整性与签名（F-05）· 未执行

- 最低要求：构建机配置 Authenticode 证书 + `win.publisherName`，使 `electron-updater` 执行签名校验；无证书时在发布说明中显式标注风险，并在应用内展示更新包哈希与来源 URL。
- 验收：篡改 `latest.yml` 或替换安装包后，更新被拒绝并记录日志；正常更新可完成。
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

### R-07 恢复渲染进程同源策略 + CSP（F-07）· 缓期

- 缓期原因：本地媒体（视频/语音/图片）以 `file://` 直接加载，恢复 `webSecurity` 需先完成自定义协议迁移，属独立工作项。

- 动作：本地媒体改用自定义协议（`protocol.handle('weflow-media', ...)` 并做路径包含校验），三个窗口恢复 `webSecurity: true`；`index.html` 或 `session.defaultSession.webRequest.onHeadersReceived` 注入 CSP；补 `setPermissionRequestHandler` 默认拒绝。
- 验收：视频/图片/语音播放与拖拽导入回归通过；DevTools 中 `fetch('file:///C:/Windows/win.ini')` 被拒；CSP 无 `unsafe-eval`。

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
| P1 | R-04 → R-05 → R-06 | R-02 决策 | 1–2 天 | R-04/R-06 完成，R-05 未启 |
| P2 | R-07 → R-08 → R-09 → R-10 | R-06 门禁生效 | 2–3 天 | R-08/R-09 完成，R-07 缓期，R-10 接受 |
| P3 | R-11 → R-12 → R-13 → R-14 | 无 | 持续 | R-13/R-14 完成，R-11/R-12 未启 |

## 验收总门禁（发布前）

1. `npm run typecheck` 0 错误；`npm run test:wcdb`、`npm run test:release-notes` 全绿。
2. 干净 clone + CI 同命令构建成功，产物含 DLL/koffi，且 afterPack 缺失即失败（R-02 决策未落地前不满足）。
3. 旧版本可检测并完成一次真实更新；篡改更新包被拒绝（或已记录风险并展示哈希）。
4. 三个窗口 `webSecurity: true` 且 CSP 生效；任意路径写入/打开被拒（路径部分已满足，`webSecurity` 部分未满足）。
5. 更新源与 `git remote` 一致，或有书面镜像说明（已文档化上游关系并有 CI 检查）。
