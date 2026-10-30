// 校验更新说明渲染：任何 HTML/脚本输入都只能产生文本节点，不允许标签或事件属性进入产物。
const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { buildSync } = require('esbuild')

const root = path.resolve(__dirname, '..')
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'weflow-release-notes-'))
const outfile = path.join(tmpDir, 'harness.cjs')

buildSync({
  stdin: {
    contents: `
      const React = require('react')
      const { renderToStaticMarkup } = require('react-dom/server')
      const { renderReleaseNotes } = require(${JSON.stringify(path.join(root, 'src', 'components', 'releaseNotes.tsx'))})
      module.exports = (notes) => renderToStaticMarkup(React.createElement('div', null, renderReleaseNotes(notes)))
    `,
    resolveDir: root,
    loader: 'tsx'
  },
  bundle: true,
  platform: 'node',
  format: 'cjs',
  outfile,
  logLevel: 'silent'
})

try {
  const render = require(outfile)

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

  console.log('[release-notes] render OK')
} finally {
  fs.rmSync(tmpDir, { recursive: true, force: true })
}
