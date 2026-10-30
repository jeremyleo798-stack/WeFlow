// 校验更新界面渲染：说明文字只能是文本节点；未签名提示必须展示来源与 SHA-512。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { build } = require('esbuild')

const root = path.resolve(__dirname, '..')
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'weflow-release-notes-'))
const outfile = path.join(tmpDir, 'harness.cjs')

const bundle = () => build({
  stdin: {
    contents: `
      const React = require('react')
      const { renderToStaticMarkup } = require('react-dom/server')
      const { renderReleaseNotes } = require(${JSON.stringify(path.join(root, 'src', 'components', 'releaseNotes.tsx'))})
      const UpdateDialog = require(${JSON.stringify(path.join(root, 'src', 'components', 'UpdateDialog.tsx'))}).default
      module.exports = {
        notes: (notes) => renderToStaticMarkup(React.createElement('div', null, renderReleaseNotes(notes))),
        dialog: (info) => renderToStaticMarkup(React.createElement(UpdateDialog, {
          open: true,
          updateInfo: info,
          onClose: () => {},
          onUpdate: () => {},
          isDownloading: false,
          progress: 0
        }))
      }
    `,
    resolveDir: root,
    loader: 'tsx'
  },
  bundle: true,
  platform: 'node',
  format: 'cjs',
  outfile,
  loader: { '.scss': 'empty' },
  plugins: [{
    name: 'stub-liquid-glass',
    setup(build) {
      build.onResolve({ filter: /LiquidGlass$/ }, () => ({ path: 'liquid-glass-stub', namespace: 'stub' }))
      build.onLoad({ filter: /.*/, namespace: 'stub' }, () => ({
        contents: 'export default function LiquidGlass(props) { return props.children }',
        loader: 'jsx'
      }))
    }
  }],
  logLevel: 'silent'
})

const run = async () => {
  await bundle()
  try {
    const { notes: render, dialog: renderDialog } = require(outfile)

  const injected = render(
    '<h2>更新内容</h2><script>alert(1)</script><img src=x onerror="alert(2)">' +
    '<p>修复 <strong>问题</strong></p><ul><li>条目 A</li></ul><a href="javascript:alert(3)">点我</a>'
  )
  assert.ok(!/<script/i.test(injected), 'script tag must not survive')
  assert.ok(!/onerror/i.test(injected), 'event handler must not survive')
  assert.ok(!/<img/i.test(injected), 'img tag must not survive')
  assert.ok(!/javascript:/i.test(injected), 'javascript URL must not survive')
  assert.ok(/<h3>更新内容<\/h3>/.test(injected), 'html heading becomes a text heading')
  assert.ok(/<ul><li>条目 A<\/li><\/ul>/.test(injected), 'html list becomes a react list')
  assert.ok(/修复 问题/.test(injected), 'html text content is preserved')

  const markdown = render('## 新功能\n- 支持 **批量导出**\n- 修复 [问题](https://example.com/a)\n\n普通段落')
  assert.ok(/<h3>新功能<\/h3>/.test(markdown), 'markdown heading renders')
  assert.ok(/支持 批量导出/.test(markdown), 'bold markers are removed')
  assert.ok(/问题 \(https:\/\/example.com\/a\)/.test(markdown), 'http link renders as text')
  assert.ok(/<p>普通段落<\/p>/.test(markdown), 'paragraph renders')

  const unsafeLink = render('[x](javascript:alert(1))')
  assert.ok(!/<a\b/i.test(unsafeLink), 'no anchor element is produced')
  assert.ok(!/href=["']?javascript:/i.test(unsafeLink), 'non-http links never become href attributes')

  const signedInfo = renderDialog({
    version: '5.1.0',
    releaseNotes: '<p>修复问题</p><img src=x onerror="alert(1)">',
    sha512: 'q83vEjRWeJA1b2NlbnRlcg==',
    downloadUrl: 'https://github.com/hicccc77/WeFlow/releases/latest/download/WeFlow-5.1.0-x64-Setup.exe'
  })
  assert.ok(/此更新包未做代码签名/.test(signedInfo), 'unsigned notice is shown')
  assert.ok(signedInfo.includes('q83vEjRWeJA1b2NlbnRlcg=='), 'sha512 is rendered')
  assert.ok(signedInfo.includes('WeFlow-5.1.0-x64-Setup.exe'), 'download url is rendered')
  assert.ok(/修复问题/.test(signedInfo), 'release notes still render inside the dialog')
  assert.ok(!/onerror/i.test(signedInfo), 'dialog never renders injected event handlers')

  const plainInfo = renderDialog({ version: '5.1.0', releaseNotes: '修复问题' })
  assert.ok(!/此更新包未做代码签名/.test(plainInfo), 'verification block is hidden without hash data')

  console.log('[release-notes] render OK')
  console.log('[update-dialog] render OK')
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
}

run().catch((error) => {
  console.error(error)
  process.exit(1)
})
