// pnpm 安装时 koffi 的平台二进制不在依赖树里，electron-builder 不会收进包；
// koffi 会到 process.resourcesPath/koffi/<triplet>/koffi.node 兜底查找，这里补上。
// 同时补齐 WCDB 原生库（native/wcdb-native/dist/wcdb_api.dll），
// 供未配置 wcdbLibPath 的机器直接使用内置实现。
const fs = require('node:fs')
const path = require('node:path')
const { createHash } = require('node:crypto')
const { createRequire } = require('node:module')

const ARCH_NAMES = { 0: 'ia32', 1: 'x64', 2: 'armv7l', 3: 'arm64', 4: 'universal' }
// 发布构建缺少原生库时应直接失败，避免产出静默降级的安装包。
const requireNativeArtifacts = () => process.env.CI === 'true' || process.env.WCDB_DLL_REQUIRED === '1'

function resolvePlatformPackage(projectDir, platform, arch) {
  const koffiArch = arch === 'armv7l' ? 'arm' : arch
  const platformArch = `${platform}-${koffiArch}`
  const requireFromKoffi = createRequire(require.resolve('koffi', { paths: [projectDir] }))
  return path.dirname(requireFromKoffi.resolve(`@koromix/koffi-${platformArch}`))
}

function copyKoffi(context, resourcesDir, arch) {
  try {
    const packageDir = resolvePlatformPackage(context.packager.projectDir, context.electronPlatformName, arch)
    let copied = 0
    for (const entry of fs.readdirSync(packageDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const source = path.join(packageDir, entry.name, 'koffi.node')
      if (!fs.existsSync(source)) continue
      const targetDir = path.join(resourcesDir, 'koffi', entry.name)
      fs.mkdirSync(targetDir, { recursive: true })
      fs.copyFileSync(source, path.join(targetDir, 'koffi.node'))
      copied += 1
    }
    if (copied > 0) {
      console.log(`[after-pack] copied ${copied} koffi native binary to resources/koffi`)
    } else if (requireNativeArtifacts()) {
      throw new Error('[after-pack] koffi native binary not found in platform package')
    } else {
      console.warn('[after-pack] koffi native binary not found in platform package')
    }
  } catch (error) {
    if (requireNativeArtifacts()) throw error
    console.warn(`[after-pack] koffi fallback copy skipped: ${error.message}`)
  }
}

function copyWcdb(context, resourcesDir) {
  if (context.electronPlatformName !== 'win32') return
  const source = process.env.WCDB_DLL_SOURCE
    ? path.resolve(process.env.WCDB_DLL_SOURCE)
    : path.join(context.packager.projectDir, 'native', 'wcdb-native', 'dist', 'wcdb_api.dll')
  const missingMessage = `[after-pack] wcdb_api.dll not found at ${source}; set WCDB_DLL_SOURCE to a built DLL or commit the bundled binary`
  if (!fs.existsSync(source)) {
    if (requireNativeArtifacts()) throw new Error(missingMessage)
    console.warn(`${missingMessage}; release requires a configured wcdbLibPath`)
    return
  }
  const size = fs.statSync(source).size
  if (size < 1024 * 1024) {
    throw new Error(`[after-pack] wcdb_api.dll at ${source} is only ${size} bytes; expected a compiled DLL`)
  }
  const sha256 = createHash('sha256').update(fs.readFileSync(source)).digest('hex')
  // 与主进程 resourcesPath（<resources>/resources）保持一致
  const targetDir = path.join(resourcesDir, 'resources', 'native', 'wcdb-native', 'dist')
  fs.mkdirSync(targetDir, { recursive: true })
  fs.copyFileSync(source, path.join(targetDir, 'wcdb_api.dll'))
  console.log(`[after-pack] copied wcdb_api.dll to resources/resources/native/wcdb-native/dist (sha256=${sha256})`)
}

exports.default = async function afterPack(context) {
  const arch = ARCH_NAMES[context.arch]
  if (!arch || arch === 'universal') return

  const resourcesDir = context.electronPlatformName === 'darwin'
    ? path.join(context.appOutDir, `${context.packager.appInfo.productFilename}.app`, 'Contents', 'Resources')
    : path.join(context.appOutDir, 'resources')

  copyKoffi(context, resourcesDir, arch)
  copyWcdb(context, resourcesDir)
}
