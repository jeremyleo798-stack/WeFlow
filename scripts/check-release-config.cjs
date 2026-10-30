// 发布源一致性检查：package.json repository / build.publish / 主进程 feed / README 必须指向同一仓库。
// 用法：node scripts/check-release-config.cjs
const fs = require('node:fs')
const path = require('node:path')

const root = path.resolve(__dirname, '..')
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'))
const mainSource = fs.readFileSync(path.join(root, 'electron', 'main.ts'), 'utf8')
const readme = fs.readFileSync(path.join(root, 'README.md'), 'utf8')

const errors = []

const repoUrl = String(pkg.repository?.url || '').replace(/\.git$/, '')
const repoMatch = repoUrl.match(/github\.com[/:]([^/]+)\/([^/]+)$/i)
if (!repoMatch) {
  errors.push(`package.json repository.url is not a GitHub repo: ${pkg.repository?.url || '(missing)'}`)
}
const [, repoOwner, repoName] = repoMatch || []

const publishOwner = String(pkg.build?.publish?.owner || '')
const publishRepo = String(pkg.build?.publish?.repo || '')
if (!publishOwner || !publishRepo) {
  errors.push('package.json build.publish.owner/repo is missing')
} else if (repoOwner && (publishOwner.toLowerCase() !== repoOwner.toLowerCase() || publishRepo.toLowerCase() !== repoName.toLowerCase())) {
  errors.push(`build.publish points to ${publishOwner}/${publishRepo}, repository.url points to ${repoOwner}/${repoName}`)
}

const feedMatch = mainSource.match(/const repoBase = 'https:\/\/github\.com\/([^/]+)\/([^/]+)\/releases'/)
if (!feedMatch) {
  errors.push('electron/main.ts: repoBase not found or not in the expected format')
} else if (repoOwner && (feedMatch[1].toLowerCase() !== repoOwner.toLowerCase() || feedMatch[2].toLowerCase() !== repoName.toLowerCase())) {
  errors.push(`update feed points to ${feedMatch[1]}/${feedMatch[2]}, repository.url points to ${repoOwner}/${repoName}`)
}

if (repoOwner && !readme.toLowerCase().includes(`github.com/${repoOwner}/${repoName}`.toLowerCase())) {
  errors.push(`README.md does not link to github.com/${repoOwner}/${repoName}`)
}

if (errors.length > 0) {
  console.error('[release-config] inconsistent release target:')
  for (const message of errors) console.error(`  - ${message}`)
  process.exit(1)
}

console.log(`[release-config] OK: repository, publish config, update feed and README all target ${repoOwner}/${repoName}`)
