# 质量属性场景（Quality Scenarios）

每个场景遵循：刺激 → 响应 → 度量 → 当前证据 → 验证方式。未实现或未验证的场景标注“缺口”。

## QS-01 可重复发布：干净环境构建内置 WCDB 产物

- 刺激：CI 从干净 clone 执行发布构建（无缓存、无本地 DLL）。
- 响应：构建成功，产物包含 `wcdb_api.dll` 与 koffi 原生二进制；缺失任一即构建失败。
- 度量：连续 2 次构建产物路径与 sha256 一致；DLL 缺失时退出码非 0。
- 当前证据：`scripts/after-pack.cjs` 在 `CI=true` / `WCDB_DLL_REQUIRED=1` 时对缺失 DLL 或 koffi 抛错，并校验 DLL 体积、打印 sha256（F-02 已加固）；分发方式（入库/Release asset/CI 编译）待决策。
- 验证：`scripts/check-release-artifacts.cjs` 解包断言 <1 MB DLL 或缺失 koffi 时失败（已实现）。

## QS-02 可重复发布：本地与 CI 依赖树一致

- 刺激：开发者本地安装依赖并构建，CI 同时构建。
- 响应：两者使用同一包管理器、同一锁文件、同一 Electron 版本与同一组安全 override。
- 度量：`electron --version` 一致；锁文件 diff 为空；`npm ci`/`pnpm install --frozen-lockfile` 均通过。
- 当前证据：本地 Electron 43.8.3，`package-lock.json` 锁 43.5.0；`pnpm.overrides` 对 npm 不生效（缺口 F-10/F-11）。
- 验证：在干净容器分别执行两条安装命令并比对 `node_modules` 顶层版本。

## QS-03 供应链完整性：恶意更新被拒绝

- 刺激：攻击者篡改 `latest.yml` 或替换安装包。
- 响应：客户端拒绝安装并记录日志/提示用户。
- 度量：篡改后更新失败率 100%；正常更新成功率 100%。
- 当前证据：`NsisUpdater.verifySignature` 在 `publisherName == null` 时返回 null，签名校验被跳过；无 Authenticode 配置（缺口 F-05）。
- 验证：本地搭建 generic feed，替换包后触发更新，观察日志与安装行为。

## QS-04 供应链完整性：更新源唯一且可解释

- 刺激：应用检查更新。
- 响应：请求命中正式发布仓库的 `latest.yml`，不得命中第三方仓库。
- 度量：feed URL 的 owner/repo 与 `git remote`（或文档声明的镜像）一致。
- 当前证据：`main.ts` 内 `repoBase`、`build.publish.owner` 均指向 `hicccc77/WeFlow`，README 一致；remote 为 fork（预期）。`scripts/check-release-config.cjs` 已在 CI 校验四处一致（F-04 已澄清）。
- 验证：`node scripts/check-release-config.cjs`；发版后抓包比对 feed URL。

## QS-05 运行安全：渲染进程无法读取任意本地文件

- 刺激：渲染进程执行 `fetch('file:///C:/Windows/win.ini')` 或加载恶意远程资源。
- 响应：请求被同源策略/CSP 拒绝，控制台报错。
- 度量：三类窗口均拒绝；正常本地媒体播放不受影响。
- 当前证据：三窗口 `webSecurity: false`，无 CSP（缺口 F-07）。
- 验证：DevTools 手动执行 + Playwright/Electron 自动化断言。

## QS-06 运行安全：渲染进程无法任意写文件

- 刺激：渲染进程调用 `window.electronAPI.file.writeBase64('C:\\Windows\\Temp\\x.bat', ...)` 或 `shell.openPath` 指向任意路径。
- 响应：写入通道不存在；打开非白名单路径被拒。
- 度量：白名单目录内打开成功；目录外 100% 拒绝。
- 当前证据：`file:writeBase64` 已删除（main/preload/`electron.d.ts` 同步移除）；`shell:openPath` 使用 realpath + `relative()` 包含校验，允许根含用户 dialog 选择目录（F-08 已修复）。
- 验证：类型检查覆盖移除残链；手工调用 `shell.openPath('C:\\Windows\\System32\\cmd.exe')` 应返回拒绝。

## QS-07 运行安全：恶意 Release 正文不执行

- 刺激：`releaseNotes` 含 `<img src=x onerror=alert(1)>`。
- 响应：仅作为文本/安全 HTML 渲染，无脚本执行。
- 度量：无弹窗、无网络请求；正常 Markdown 正常显示。
- 当前证据：`src/components/releaseNotes.tsx` 将 HTML 转纯文本后渲染 React 节点，`dangerouslySetInnerHTML` 已移除（F-09 已修复）。
- 验证：`npm run test:release-notes` 对恶意输入断言无可执行 HTML（已实现）。

## QS-08 可靠性：DLL 加载失败可诊断且可回退

- 刺激：`WCDB_DLL_PATH` 指向不存在的文件，或未配置该变量。
- 响应：失效 env 被忽略，回退到内置 DLL；若全部失败，日志写明尝试过的路径与错误码。
- 度量：回退成功率 100%；日志包含候选路径列表。
- 当前证据：`wcdbCore.getDllPath()` 已加 `existsSync` 校验，失效 env 回退内置候选（F-03 已修复）；`wcdbCore.ts:862` 记录 `dllPath` 与 `resourcesPath`。
- 验证：手工设置失效 env 启动，检查 `userData/logs/wcdb.log`。

## QS-09 可维护性：新增 IPC 通道不遗漏三处同步

- 刺激：开发者新增一个主进程通道。
- 响应：类型检查能发现 preload/渲染类型未同步。
- 度量：故意遗漏 preload 映射时 `npm run typecheck` 失败。
- 当前证据：三处手写（main/preload/electron.d.ts），无生成器（缺口 F-12）。
- 验证：构造一次故意遗漏的 PR，观察 CI。

## QS-10 可测试性：发布关键路径有自动化守卫

- 刺激：提交引入 afterPack 路径错误或移除 DLL 拷贝。
- 响应：CI 在合并前失败。
- 度量：注入上述缺陷后 CI 失败率 100%；正常提交通过。
- 当前证据：release.yml 已加 `npm run typecheck`（含 electron）、`test:wcdb`、`test:release-notes`、产物检查与配置检查；仍无 PR 级门禁（F-13 部分修复）。
- 验证：在分支上制造缺陷，观察 release 工作流（或 PR 门禁）结果。

## QS-11 隐私：密钥不落盘、不落日志

- 刺激：执行数据库解密与图片密钥获取。
- 响应：密钥仅存在于内存与 safeStorage 加密配置；日志仅记录布尔/计数。
- 度量：`wcdb.log` 与 `traceKeyFlow` 输出中检索不到密钥字符串；配置文件密钥字段为密文（safeStorage 可用时）。
- 当前证据：`traceKeyFlow` 仅记录 `hasKey` 等布尔值；`config.ts` 对 `decryptKey/imageAesKey/imageXorKey/aiModelApiKey` 使用 safeStorage；safeStorage 不可用时回退明文（需在文档中标注）。
- 验证：grep 日志与配置；模拟 safeStorage 不可用场景确认提示。

## 场景优先级

| 优先级 | 场景 | 关联发现 |
| --- | --- | --- |
| P0 | QS-01、QS-08 | F-01、F-02、F-03 |
| P1 | QS-03、QS-04、QS-10 | F-04、F-05、F-06、F-13 |
| P2 | QS-05、QS-06、QS-07 | F-07、F-08、F-09 |
| P3 | QS-02、QS-09、QS-11 | F-10、F-11、F-12 |
