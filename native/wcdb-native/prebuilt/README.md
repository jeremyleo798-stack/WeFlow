# prebuilt/wcdb_api.dll

内置的 WCDB C ABI 实现，由本仓库 `native/wcdb-native`（Rust `cdylib`）构建，随安装包分发。

| 项 | 值 |
| --- | --- |
| 构建命令 | `cd native/wcdb-native && cargo build --release`（产物 `target/release/wcdb_api.dll`） |
| 工具链 | Rust `x86_64-pc-windows-gnu`（见 `docs/third-party-components.md`），内嵌 rustc commit `58cf8f9c35b355199727627d07ae34a4e2846d67` |
| 依赖方式 | `rusqlite` + `bundled-sqlcipher-vendored-openssl`，静态链接，仅依赖 Windows 系统 DLL |
| 大小 | 12,839,447 bytes |
| sha256 | `55120c347c7093a239de04d978c24a346fae85b9416683cc14c92b2b4926606d` |

更新流程：重新构建后覆盖本文件，更新上表的 sha256 与大小。`scripts/after-pack.cjs` 打包时会打印实际使用的 sha256，`WCDB_DLL_SOURCE` 可临时指向其他构建产物。

## 为什么入库

`.gitignore` 的 `dist` 规则会忽略 `native/wcdb-native/dist/`，导致干净克隆与 CI 拿不到 DLL；发布构建在 CI 下缺 DLL 会直接失败。把可执行产物放在 `prebuilt/`（不受忽略规则影响）使发布可复现，同时保持 `dist/` 作为本地构建输出目录。
