// 发布产物检查：确认解包目录内含 asar、koffi 原生二进制，Windows 还必须有内置 WCDB DLL。
// 用法：node scripts/check-release-artifacts.cjs [platform] [arch]
// 默认取当前平台/架构；在 package.json build.directories.output（release）下自动发现含 app.asar 的解包目录。
const fs = require('node:fs')
const path = require('node:path')

const platform = process.argv[2] || process.platform
const arch = process.argv[3] || process.arch
const koffiArch = arch === 'armv7l' ? 'arm' : arch
const triplet = `${platform}_${koffiArch}`
const distDir = process.env.RELEASE_DIST_DIR
  ? path.resolve(process.env.RELEASE_DIST_DIR)
  : path.resolve(__dirname, '..', 'release')

if (!fs.existsSync(distDir)) {
  console.error(`[release-artifacts] directory not found: ${distDir}`)
  process.exit(1)
}

const resourceDirs = []
const walk = (dir, depth) => {
  if (depth > 6) return
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    const full = path.join(dir, entry.name)
    if (fs.existsSync(path.join(full, 'app.asar'))) {
      resourceDirs.push(full)
      continue
    }
    walk(full, depth + 1)
  }
}
walk(distDir, 0)

if (resourceDirs.length === 0) {
  console.error(`[release-artifacts] no unpacked resources/app.asar found under ${distDir}`)
  process.exit(1)
}

const candidates = resourceDirs.filter((dir) => {
  const nativeDir = path.join(dir, 'koffi', triplet)
  return fs.existsSync(path.join(nativeDir, 'koffi.node'))
})

const errors = []
if (candidates.length === 0) {
  errors.push(`no packaged resources contain koffi/${triplet}/koffi.node (found: ${resourceDirs.join(', ')})`)
} else if (candidates.length > 1) {
  errors.push(`multiple packaged resources match ${triplet}: ${candidates.join(', ')}`)
}

if (platform === 'win32') {
  const dll = candidates.length === 1
    ? path.join(candidates[0], 'resources', 'native', 'wcdb-native', 'dist', 'wcdb_api.dll')
    : ''
  if (!dll || !fs.existsSync(dll)) {
    errors.push(`bundled wcdb_api.dll missing at ${dll || '<no candidate>'}`)
  } else if (fs.statSync(dll).size < 1024 * 1024) {
    errors.push(`bundled wcdb_api.dll is unexpectedly small: ${fs.statSync(dll).size} bytes`)
  } else {
    console.log(`[release-artifacts] wcdb_api.dll ${fs.statSync(dll).size} bytes`)
  }
}

if (errors.length > 0) {
  console.error('[release-artifacts] failed:')
  for (const message of errors) console.error(`  - ${message}`)
  process.exit(1)
}

const koffiDir = path.join(candidates[0], 'koffi', triplet)
console.log(`[release-artifacts] OK: ${candidates[0]}`)
console.log(`[release-artifacts] koffi: ${fs.statSync(path.join(koffiDir, 'koffi.node')).size} bytes (${triplet})`)
