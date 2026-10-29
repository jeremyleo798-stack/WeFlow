// pnpm 安装时 koffi 的平台二进制不在依赖树里，electron-builder 不会收进包；
// koffi 会到 process.resourcesPath/koffi/<triplet>/koffi.node 兜底查找，这里补上。
const fs = require('node:fs')
const path = require('node:path')
const { createRequire } = require('node:module')

const ARCH_NAMES = { 0: 'ia32', 1: 'x64', 2: 'armv7l', 3: 'arm64', 4: 'universal' }

function resolvePlatformPackage(projectDir, platform, arch) {
  const koffiArch = arch === 'armv7l' ? 'arm' : arch
  const platformArch = `${platform}-${koffiArch}`
  const requireFromKoffi = createRequire(require.resolve('koffi', { paths: [projectDir] }))
  return path.dirname(requireFromKoffi.resolve(`@koromix/koffi-${platformArch}`))
}

exports.default = async function afterPack(context) {
  const arch = ARCH_NAMES[context.arch]
  if (!arch || arch === 'universal') return

  const projectDir = context.packager.projectDir
  const resourcesDir = context.electronPlatformName === 'darwin'
    ? path.join(context.appOutDir, `${context.packager.appInfo.productFilename}.app`, 'Contents', 'Resources')
    : path.join(context.appOutDir, 'resources')

  try {
    const packageDir = resolvePlatformPackage(projectDir, context.electronPlatformName, arch)
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
      console.log(`[after-pack-koffi] copied ${copied} koffi native binary to resources/koffi`)
    } else {
      console.warn('[after-pack-koffi] koffi native binary not found in platform package')
    }
  } catch (error) {
    console.warn(`[after-pack-koffi] fallback copy skipped: ${error.message}`)
  }
}
