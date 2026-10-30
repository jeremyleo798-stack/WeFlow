# WeFlow 架构风险与质量审查（2026-10-30）

## 0. 审查范围

- 审查对象：`C:\Users\Administrator\Documents\开发\WeFlow\WeFlow_tree0`（Electron 桌面应用，分支 `main`）
- 提交范围：`213a56f`..`e66f89c`（Windows 便携打包加固、WCDB DLL 内置、依赖与类型安全清理），并包含当前工作区未提交改动
- 重点：打包链路、WCDB 原生库加载、依赖/锁文件、类型安全与发布可信度
- 证据来源：源码、构建脚本、CI 工作流、锁文件、`node_modules` 中 electron-updater 实现、git 历史
- 非目标：逐行业务逻辑正确性、微信协议/数据库 schema 正确性、UI 视觉回归

置信度约定：**high** = 直接代码/配置/依赖实现证据；**medium** = 多个间接信号一致但无直接证据；**low** = 命名或目录推断；**unknown** = 明确未知。

## 1. 结论摘要

这次改动方向正确：便携包改为单文件 exe、启用 asar + Electron fuses、WCDB 原生库内置、类型错误清零。审查发现的发布链路问题已按 P0→P3 处理，当前状态：

1. **发布构建阻断（已修复）**：`HEAD` 的 `package.json:115` 曾指向已删除的 `scripts/after-pack-koffi.cjs`；现已改为 `scripts/after-pack.cjs`，并用假 context 验证 DLL/koffi 均能复制。
2. **内置 DLL 缺失不再静默（已加固）**：`native/wcdb-native/dist/wcdb_api.dll`（12.8 MB）仍被 `.gitignore:12` 忽略，但 CI 构建缺失时会直接失败，打包后还有产物检查兜底。**DLL 最终分发方式仍待决策**（提交入库 / Release asset / CI 编译 Rust）。
3. **更新供应链（部分处理）**：feed、`build.publish`、`repository.url` 与 README 实际都指向 `hicccc77/WeFlow`（上游正式仓库），本地 remote 是 fork，并非配置漂移；已新增一致性检查。未签名与 `publisherName` 缺失仍使 `electron-updater` 跳过签名校验，需证书才能闭环。
4. **安全纵深（部分修复）**：更新说明已改为纯 React 节点渲染并加回归测试；未使用的 `file:writeBase64` 已删除，`shell:openPath` 已加目录/realpath 约束。`webSecurity:false` 与 CSP 仍需先完成本地媒体自定义协议迁移，未强行翻转。

其余为中等优先级的可维护性、安全纵深与质量门禁问题，详见发现清单。

## 2. 变更基线与架构快照

| 层 | 组成 | 规模证据 |
| --- | --- | --- |
| 渲染进程 | React 19 + Vite 8，`src/**` | `index.html` 无 CSP |
| 预加载桥 | `electron/preload.ts` | 802 行，约 299 处 `ipcRenderer.*` 调用 |
| 主进程 | `electron/main.ts` + 84 个 service | 5724 行，251 个 `ipcMain.handle` |
| 原生数据库 | `wcdbService`（worker_threads）→ `wcdbCore`（koffi）→ `wcdb_api.dll` | `wcdbService.ts` 914 行，`wcdbCore.ts` 5311 行 |
| 打包 | electron-builder 26，asar + 4 项 fuses，afterPack 补 koffi/DLL | `package.json:build` |
| 更新 | `electron-updater` generic feed | `electron/main.ts:492-528` |
| 本地 HTTP API | `httpService`，默认 `127.0.0.1:5031` | token 鉴权 + `timingSafeEqual` |

打包路径要点：`extraResources` 把仓库 `resources/` 复制到 `<app>/resources/resources/`，因此便携版解包后 `process.resourcesPath` 下的内置 DLL 实际位于 `<resources>/resources/native/wcdb-native/dist/wcdb_api.dll`；`after-pack.cjs` 与 `wcdbCore.getDllPath()` 的第二候选路径一致，这部分设计是自洽的。

## 3. 发现清单

| ID | 严重度 | 可能性 | 影响 | 置信度 | 问题 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| F-01 | Critical | 确定 | 高 | high | afterPack 指向已删除脚本，发布构建直接失败 | 已修复（待提交） |
| F-02 | High | 确定（CI） | 高 | high | 内置 DLL 被 gitignore，缺失时打包只告警 | 已加固；DLL 分发方式待决策 |
| F-03 | Medium | 中 | 中 | high | `WCDB_DLL_PATH` 未校验存在性即短路返回 | 已修复（待提交） |
| F-04 | High→Info | 确定 | 高 | high | 更新/发布目标仓库与 origin 不一致 | 已澄清：hicccc77 为上游正式仓库；新增一致性检查 |
| F-05 | High | 中 | 高 | high | 未签名 + 更新签名校验被跳过 | 待证书；风险已记录 |
| F-06 | Medium | 高 | 中高 | high | 发布 CI 不跑 electron 类型检查与测试 | 已修复 |
| F-07 | High | 中 | 高 | high | 三个窗口 `webSecurity:false` 且无 CSP | 待协议迁移，未翻转 |
| F-08 | High | 低-中 | 高 | high | 渲染进程可写任意路径、打开任意本地路径 | 已修复 |
| F-09 | Medium | 低-中 | 中 | high | 更新说明以 HTML 直接注入渲染进程 | 已修复 + 回归测试 |
| F-10 | Medium | 高 | 中 | high | npm/pnpm 双包管理器、锁文件与 overrides 分叉 | 未处理 |
| F-11 | Medium | 高 | 中 | high | `pnpm-workspace.yaml`（allowBuilds）未入库 | 未处理 |
| F-12 | Medium | 高 | 中 | high | 主进程单文件 + 251 个 IPC 通道，无契约层 | 未处理 |
| F-13 | Medium | 高 | 高 | high | 自动化验证极薄，发布路径无测试 | 部分修复：新增发布配置/产物/渲染检查 |
| F-14 | Low-Med | 中 | 中 | high | 无 AGENTS.md/CLAUDE.md 等领域与回归指引 | 已修复：新增 AGENTS.md（`.gitignore` 忽略，仅本地生效） |
| F-15 | Low-Med→Low | 中 | 中 | high | `http:start` 绑定地址由渲染进程决定，token 可走 query | 风险接受：非回环为文档化显式选项且强制 token |

### F-01 发布构建阻断：afterPack 指向已删除脚本

- 证据：`HEAD:package.json:115` → `"afterPack": "scripts/after-pack-koffi.cjs"`；该文件已在 `394895e` 重命名为 `scripts/after-pack.cjs`。
- 影响：任何从 `HEAD` 执行的 `npm run build` / `npm run build:portable` 都会在打包阶段因钩子模块不存在而失败；即发布提交本身不可构建。
- 现状：已改为 `"afterPack": "scripts/after-pack.cjs"`（`package.json:115`）。
- 验证：`node -e "require('./scripts/after-pack.cjs')"` 通过；用假 context 在临时目录执行 afterPack，DLL（12,839,447 字节）与 koffi（`win32_x64`）均成功复制；完整 `build:portable` 未重跑（构建耗时长）。

### F-02 内置 DLL 不入库，缺失时仅告警

- 证据：`.gitignore:12` 的 `dist` 规则命中 `native/wcdb-native/dist/wcdb_api.dll`；`native/.gitignore` 忽略 `*.node`、`target/`；`scripts/after-pack.cjs:45` 在源文件缺失时只 `console.warn('[after-pack] wcdb_api.dll not found; release requires a configured wcdbLibPath')` 并 `return`。
- 影响：GitHub Actions 从干净 clone 构建时没有 DLL，产物仍会成功生成但核心 WCDB 功能退化；用户必须自行配置 `wcdbLibPath`，而 UI 不会在构建期暴露该缺陷。这是“静默降级”，比构建失败更难发现。
- 建议（二选一，需产品决策）：
  - A. 将 DLL 作为发布资产/构建输入管理（提交到仓库、Release asset 下载或 CI 编译 Rust），并在 `afterPack` 缺失时 **抛错终止**；
  - B. 明确“不内置”，删除 `after-pack.cjs` 的拷贝逻辑与 `wcdbCore` 的 bundled 候选，改为强制配置，避免误导。
- 不建议保持现状：代码声称内置，仓库却无法复现内置。
- 现状（2026-10-30）：已消除“静默降级”。`after-pack.cjs` 在 `CI=true` 或 `WCDB_DLL_REQUIRED=1` 时缺失 DLL/koffi 直接抛错；DLL 小于 1 MB（如 LFS 指针）始终失败；支持 `WCDB_DLL_SOURCE` 指向外部 DLL；成功后打印 sha256。新增 `scripts/check-release-artifacts.cjs` 在打包后再次校验产物。**仍待决策**：DLL 最终以“提交入库 / Release asset / CI 编译 Rust”哪种方式进入构建。

### F-03 WCDB_DLL_PATH 未校验存在性

- 证据：`HEAD:electron/services/wcdbCore.ts:410-411`
  ```ts
  const envDllPath = process.env.WCDB_DLL_PATH
  if (envDllPath && envDllPath.length > 0) return envDllPath
  ```
  只要环境变量非空就直接返回，即使文件不存在，也不会回退到内置 DLL 候选。
- 影响：用户机器上残留的失效环境变量会让应用报出误导性的加载错误，且内置 DLL 明明存在却不用。
- 现状：已改为 `envDllPath && existsSync(envDllPath)`（`wcdbCore.ts:410-411`），并加了 `trim()`。

### F-04 更新与发布目标仓库和 origin 不一致（已澄清）

- 证据：`electron/main.ts:493` `const repoBase = 'https://github.com/hicccc77/WeFlow/releases'`；`package.json` `build.publish = { provider: github, owner: hicccc77, repo: WeFlow }`；`package.json.repository.url` 与 README 徽章/克隆命令同样指向 `hicccc77/WeFlow`；`git remote -v` → `jeremyleo798-stack/WeFlow`。
- 结论：`hicccc77/WeFlow` 是项目上游正式仓库，本地是 fork。四处发布目标一致，因此不是配置漂移；风险仅剩“fork 构建若误用本仓 token 发布到上游”这一运维场景。
- 处理：新增 `scripts/check-release-config.cjs`，在 release 工作流入口校验 repository / publish / feed / README 一致性；上游与 fork 的关系写入 `AGENTS.md`。

### F-05 未签名发布 + 更新签名校验被跳过

- 证据：
  - `package.json` 的 `build.win` 无 `publisherName`、无 `signingHashAlgorithms`/证书配置，也无 `forceCodeSigning`；
  - `electron-updater` 的 `NsisUpdater.verifySignature` 在 `appUpdateConfig.publisherName == null` 时直接 `return null`（已读 `node_modules/electron-updater/out/NsisUpdater.js`）；
  - 更新走 `setFeedURL({ provider: 'generic', ... })`（`main.ts:518-522`）。
- 影响：Windows 用户首次运行会有 SmartScreen 警告；更新包的真实性只依赖 HTTPS 与目标仓库控制权，一旦仓库/CI 被攻破，恶意更新会被静默安装。
- 建议：至少配置 Authenticode 签名 + `win.publisherName`；短期无法签名时，在发布文档中显式记录该风险，并考虑在应用内展示更新包哈希。

### F-06 发布 CI 不跑 electron 类型检查与测试

- 证据：`.github/workflows/release.yml` 四个构建任务均执行 `npm install` → `npx tsc`（如 `:172`）→ `npx vite build`（如 `:173`）→ `electron-builder`。裸 `tsc` 读取根 `tsconfig.json`，其 `include` 仅 `src/**`，**不包含 `electron/**`**；`tsconfig.node.json` 从未在 CI 执行。`npm run test:wcdb` 也未在 release 工作流出现。
- 影响：主进程类型错误、WCDB 可选导出缺失可以随发布溜过去；本地 `npm run build` 反而更严格，形成“本地过、CI 不过/反之”的错位。
- 建议：release 工作流改为 `npm run typecheck && npm run test:wcdb`；`build:portable` 脚本同样补 `typecheck:electron`。
- 现状：已执行。四个构建任务改为 `npm run typecheck` + `npm run test:wcdb` + `npm run test:release-notes` + `vite build`；`build:portable` 使用 `npm run typecheck`。

### F-07 `webSecurity:false` + 无 CSP

- 证据：`electron/main.ts:1129`（主窗口）、`1555`（视频窗口）、`1614`（图片查看器）均 `webSecurity: false`；`index.html` 无 CSP meta；`main.ts` 无 `onHeadersReceived` 注入 CSP、无 `setPermissionRequestHandler`。
- 影响：关闭同源策略后，渲染进程中的任何注入点（见 F-09）都能读取任意 `file://` 内容并发起跨域请求；配合 F-08 的任意路径写入，可升级为本地文件窃取/覆盖。
- 建议：用自定义协议（`protocol.handle`）服务本地媒体，恢复 `webSecurity: true`；加 CSP（至少 `default-src 'self'`、限制 `img-src`/`media-src` 到自定义协议）；补权限处理器。

### F-08 渲染进程可达的任意路径写入/打开

- 证据：`electron/main.ts:2950` `file:writeBase64` 直接 `writeFile(filePath, ...)`；`3061` `shell:openPath` 直接 `shell.openPath(path)`。两者均无路径白名单或目录约束；`preload.ts` 已把它们暴露给页面。
- 影响：渲染进程被 XSS/恶意内容控制后，可写启动目录、覆盖用户文件，或用系统默认程序打开任意本地文件（可被用作执行链的一环）。
- 建议：写入限制在用户选择的导出目录内（`dialog.showSaveDialog` 返回路径 + realpath 包含校验）；`shell:openPath` 限定允许的扩展名与目录，或整体删除该通道。
- 现状：已执行。`file:writeBase64` 全仓无调用，已从 main/preload/类型声明中删除；`shell:openPath` 现在只允许 userData、downloads、`Documents/WeFlow`、配置的导出目录，以及用户经 `dialog:openDirectory` / `sns:selectExportDir` / `dialog:saveFile` 选定过的目录，并做 realpath 包含校验。

### F-09 更新说明 HTML 注入

- 证据：`src/components/UpdateDialog.tsx:102` `dangerouslySetInnerHTML={{ __html: updateInfo.releaseNotes }}`；`releaseNotes` 来源于 GitHub Release 正文。
- 影响：能修改 Release 正文的一方（仓库协作者、被盗 token、镜像源）可在应用渲染进程执行 HTML/脚本；在 `contextIsolation` 下拿不到 Node，但可调用 preload 暴露的全部 IPC（含 F-08）。
- 建议：用 `DOMPurify.sanitize` 或把 Markdown 渲染为 React 节点；禁止 `script`/`on*`/`iframe`。
- 现状：已执行。渲染逻辑抽到 `src/components/releaseNotes.tsx`，只产出 `p/h/ul/li` 文本节点，HTML 标签被剥离、非 http(s) 链接不生成 `href`；`scripts/check-release-notes-render.cjs` 用恶意输入做回归断言。

### F-10 双包管理器与锁文件/overrides 分叉

- 证据：
  - `package-lock.json` 入库，release CI 用 `npm install`，WCDB CI 用 `npm ci --legacy-peer-deps`；
  - `pnpm-lock.yaml`、`pnpm-workspace.yaml` 均被忽略（`.gitignore:76-77`）；
  - 本地 `node_modules` 的 Electron 为 43.8.3，`package-lock.json` 锁 43.5.0；
  - `package.json` 同时有 npm `overrides` 与 `pnpm.overrides`，但 npm 不读 `pnpm.overrides`、pnpm 不读 `overrides`，两套安全 pin 各自只在一种包管理器生效。
- 影响：本地验证过的产物与 CI 产物依赖树不同；安全公告的 override 覆盖在两条链路上不一致，容易出现“本地已修、发布仍受影响”。
- 建议：选定单一包管理器（发布链用 npm 就删 pnpm 相关配置，或反之并让 CI 用 pnpm + frozen lockfile），把锁文件与 allowBuilds 一并入库。

### F-11 allowBuilds 未入库导致新环境装不出 Electron

- 证据：`pnpm-workspace.yaml` 被 `.gitignore:77` 忽略；其内容（`allowBuilds` 白名单）决定 pnpm 12 是否执行 `electron`/`koffi` 等安装脚本。本次会话中正是因缺少 `electron` 白名单，`node_modules/electron/dist` 为空，只能手工解压缓存 zip 修复。
- 影响：任何新机器/新 agent 克隆后 `pnpm install` 会得到一个“装了但跑不起来”的 Electron；构建失败点隐蔽。
- 建议：`pnpm-workspace.yaml` 入库（它不再只是 workspace 配置，而是 pnpm 12 的构建策略文件），并补 `electron: true`。

### F-12 主进程单体与 IPC 面无契约

- 证据：`electron/main.ts` 5724 行、251 个 `ipcMain.handle`、84 个 service 文件；`preload.ts` 802 行手工映射；无共享 schema/类型化通道注册。
- 影响：新增/修改通道靠人工同步三处（main、preload、`src/types/electron.d.ts`），遗漏不会在编译期暴露；安全审查也难以覆盖全部通道（本次只能抽样高危项）。
- 建议：按域拆分 IPC 注册（`ipc/chat.ts`、`ipc/media.ts`…），用统一 `registerHandler(channel, schema, fn)` 包装，preload 由通道清单生成。

### F-13 自动化验证极薄

- 证据：仓库内仅 `scripts/check-wcdb-optional-exports.cjs`（46 项断言）与 `scripts/check-agent-sampling-options.cjs`（7 项）；`native/wcdb-native/tests/abi-smoke.cjs` 未被默认 CI 执行；无主进程/更新器/打包产物测试。
- 影响：F-01、F-02、F-03 这类“打包即坏”的问题没有任何自动化门禁能拦住，本次全靠人工与审查发现。
- 建议：至少增加三类 smoke：① 打包后解包校验 DLL/koffi 存在；② 更新配置一致性（owner/repo/feed 与 remote 对齐）；③ `tsc -p tsconfig.node.json` 纳入 CI。

### F-14 缺少 agent/领域指引

- 证据：仓库无 `AGENTS.md`、`CLAUDE.md`、`.github/copilot-instructions.md`；无架构/领域文档描述“哪些路径是发布关键路径、哪些行为不可回归”。
- 影响：AI 辅助改动缺少业务与发布风险上下文，容易再次出现“改了脚本名没改配置”这类跨文件不一致。
- 建议：补一份简短 `AGENTS.md`：包管理器约定、类型检查命令、发布关键路径清单（afterPack、fuses、feed、DLL）、禁止事项。

### F-15 HTTP API 绑定与 token 传递

- 证据：`electron/main.ts:5446-5449` `http:start` 接受渲染进程传入的 `host`/`port` 并直接 `listen`；`httpService.ts:420` 接受 `access_token` query 参数；默认 `127.0.0.1` 且无 token 时拒绝所有请求（`httpService.ts:406-411`），CORS 仅回显 localhost 来源（`httpService.ts:433-437`）。
- 影响：若用户把 host 设为 `0.0.0.0`，服务会暴露到局域网；query token 可能进入日志/浏览器历史。默认配置下风险低。
- 建议：host 仅允许 `127.0.0.1`/`localhost`（或加显式二次确认）；token 只接受 `Authorization` 头，query 形式标记为 deprecated。
- 现状：风险接受，不做行为修改。`SettingsPage` 明确把 `0.0.0.0` 作为 Docker/N8N 的文档化选项，且无 token 时服务拒绝所有非 `/health` 请求；SSE 客户端无法设置请求头，query token 是必要能力。后续若要收紧，应先提供 SSE 的 header 替代方案。

## 4. 质量属性评估

| 质量属性 | 现状 | 主要风险 |
| --- | --- | --- |
| 发布可重复性 | 双包管理器 + 未入库构建输入（DLL、allowBuilds） | F-02、F-10、F-11 |
| 供应链完整性 | 无签名、feed 指向他仓、更新说明可注入 | F-04、F-05、F-09 |
| 安全边界 | contextIsolation/fuses 到位；但 webSecurity 全关、IPC 路径无约束、无 CSP | F-07、F-08、F-15 |
| 可维护性 | 主进程单体、IPC 无契约 | F-12 |
| 可测试性 | 仅两个 smoke 脚本，发布路径零覆盖 | F-13 |
| 可观测性 | WCDB 有 `userData/logs/wcdb.log` 与 `traceKeyFlow`（仅记录布尔值，无密钥材料） | 正向；但无更新/打包诊断日志 |

## 5. Business Application Fitness

WeFlow 是本地优先的微信数据归档与分析桌面应用，涉及聊天数据库解密密钥、媒体文件与 AI API Key，属于高隐私敏感度业务应用。

| 维度 | 评估 | 证据 |
| --- | --- | --- |
| 业务模型清晰度 | 中：核心实体（账号、会话、消息、媒体、导出任务）可从 service 层读出，但无领域文档 | `electron/services/` 84 个文件；无 `docs/domain` |
| 数据与状态理解 | 中：配置经 safeStorage 加密（可用性不足时回退明文，`config.ts`），密钥仅内存传递；但缺少数据生命周期/保留策略文档 | `electron/services/config.ts` |
| 端到端路径落地 | 中：连接 DB → 拉消息 → 导出/HTTP API 路径存在；发布路径（打包→更新）证据充分但存在 F-01/F-02 | 本次审查重点 |
| Agent 指引覆盖 | 中：已补 `AGENTS.md`，但被 `.gitignore:69` 忽略，仅对本机 clone 生效 | F-14 |
| 业务验证准备度 | 低：无业务规则/迁移/端到端测试 | F-13 |

## 6. 未知与假设

| 项 | 类型 | 需要的验证 |
| --- | --- | --- |
| ~~实际发布仓库~~ | 已澄清 | `repository.url`、README、`build.publish`、feed 均为 `hicccc77/WeFlow`；本地 remote 为 fork |
| 是否有代码签名证书计划 | 未知 | 产品/发布决策 |
| DLL 的正式分发方式（入库/Release asset/CI 编译） | 未决 | 需用户决策，见 F-02 |
| 用户是否依赖 `WCDB_DLL_PATH` 指向自编译实现 | 假设：少量高级用户 | 查文档/Issue |
| 仓库可见性（public/private） | 未知（`gh` 查询被权限拦截） | 人工确认；影响 F-05 严重度 |

## 7. 已确认的正向控制

- 三个窗口均 `contextIsolation: true`、`nodeIntegration: false`（`main.ts:1127-1129` 等）。
- Electron fuses 已启用：禁用 `--inspect`、禁用 `NODE_OPTIONS`、asar 完整性校验、仅从 asar 加载（`package.json:build.electronFuses`）。
- 导航与新窗口有协议白名单（`main.ts:334-370`）。
- HTTP API 默认仅本机、无 token 拒绝全部请求、`timingSafeEqual` 比较、CORS 仅回显 localhost。
- `traceKeyFlow` 仅记录布尔/计数，不写密钥材料（`main.ts` key IPC 包装）。
- asar 打包 + `files` 仅含 `dist/**`、`dist-electron/**`，源码不随包分发（与发布目标一致）。

## 8. 建议的下一步

1. 提交本轮修复（工作区尚未提交），然后在发版机跑一次 `build:portable` 做完整产物级验证。
2. 就 F-02 做决策：内置 DLL 必须在 CI 可复现（提交入库 / Release asset / CI 编译），否则改为显式外部依赖。当前 CI 会因 DLL 缺失而失败，这是有意为之。
3. 评估签名方案（F-05），这是上线前的信任链底线。
4. 规划 F-07：把渲染进程的 `file://` 媒体加载迁移到自定义协议后，再恢复 `webSecurity: true` 并加 CSP；单独翻转会破坏图片/视频/语音播放。
5. 持续推进 F-10/F-11/F-12：统一包管理器与锁文件、把 IPC 注册契约化。
6. 如需要，重新加入 `AGENTS.md` 版本控制（先移除 `.gitignore:69` 的忽略规则）。

## 9. 修复执行记录（2026-10-30）

| 项 | 改动 | 验证 |
| --- | --- | --- |
| F-01 | `package.json` afterPack 改回 `scripts/after-pack.cjs` | 假 context 执行 afterPack 成功 |
| F-02 | `after-pack.cjs`：CI 缺失 DLL/koffi 抛错；支持 `WCDB_DLL_SOURCE`；DLL <1 MB 拒绝；打印 sha256 | CI 模拟缺 DLL 失败、本地仅告警均符合预期 |
| F-03 | `wcdbCore.getDllPath()` 校验 `WCDB_DLL_PATH` 存在性 | `npm run typecheck` 通过 |
| F-04 | 新增 `scripts/check-release-config.cjs`，release 工作流入口校验 | 脚本输出 OK |
| F-06 | release.yml 四个构建任务改跑 `typecheck` + `test:wcdb` + `test:release-notes`；`build:portable` 补 electron 类型检查 | 本地等价命令通过 |
| F-08 | 删除未使用的 `file:writeBase64`；`shell:openPath` 增加目录/realpath 约束 | 类型检查通过；渲染层调用点均在允许目录 |
| F-09 | 新增 `releaseNotes.tsx`，移除 `dangerouslySetInnerHTML` | `test:release-notes` 恶意输入断言通过 |
| F-13 | 新增 release config / artifacts / release-notes 三项检查并接入 CI | artifacts 检查对现有 `release-out` 通过，对残缺产物失败 |
| F-14 | 新增 `AGENTS.md`（本地生效；仓库 `.gitignore:69` 显式忽略该文件，是否入库需维护者决定） | — |
| F-15 | 风险接受，保留文档化 `0.0.0.0` 选项与 query token | 代码核对 |

验证汇总：`npm run typecheck` 0 错误；`test:wcdb` 46/46；agent sampling 7/7；`test:release-notes` 通过；release-config 通过；release-artifacts 对 `release-out/win-unpacked` 通过；`npx vite build` 通过（含新 `releaseNotes.tsx`）。

尚未执行：完整 `electron-builder` 打包（仅用假 context 验证 afterPack）、应用内更新弹窗真实渲染验证（仅组件级渲染检查）、`risk-map.dot` 的 Graphviz 渲染（本机未安装 graphviz，仅做结构校验）。

配套文件：`risk-map.dot`（风险关系图）、`remediation-plan.md`（修复排期与验收）、`quality-scenarios.md`（质量场景）。
