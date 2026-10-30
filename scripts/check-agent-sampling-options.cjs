// Exercise the production omit-temperature predicate without loading Electron or native modules.
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { test } = require('node:test')

const root = path.join(__dirname, '..')
const helperPath = path.join(root, 'electron/services/agentSamplingOptions.ts')
const ledPath = path.join(root, 'electron/services/agentModelLedService.ts')
const titlePath = path.join(root, 'electron/services/agentTitleService.ts')
const memoryPath = path.join(root, 'electron/services/agentMemorySynthesisService.ts')

function extractFunction(source, name) {
  const header = new RegExp(`function ${name}\\([^)]*\\)(?:\\s*:\\s*\\{[^}]*\\})?\\s*\\{`)
  const match = header.exec(source)
  assert.ok(match, `missing function ${name}`)
  const start = match.index
  const brace = start + match[0].length - 1
  let depth = 0
  for (let index = brace; index < source.length; index += 1) {
    const char = source[index]
    if (char === '{') depth += 1
    else if (char === '}') {
      depth -= 1
      if (depth === 0) return source.slice(start, index + 1)
    }
  }
  throw new Error(`unterminated function ${name}`)
}

function stripTypes(source) {
  return source
    .replace(/\bexport type [A-Za-z0-9_]+ = \{[\s\S]*?\r?\n\}\r?\n/, '')
    .replace(/\bexport /g, '')
    .replace(/: Agent(?:Sampling|Model)Config/g, '')
    .replace(/: \{ protocol\?: string; model\?: string \}/g, '')
    .replace(/temperature: number/g, 'temperature')
    .replace(/\): boolean/g, ')')
    .replace(/\): \{ temperature\?: number \}/g, ')')
}

function loadHelper() {
  const source = fs.readFileSync(helperPath, 'utf8')
  const js = stripTypes(source)
  const module = { exports: {} }
  vm.runInNewContext(`${js}\nmodule.exports = { omitsTemperature }`, {
    module,
    exports: module.exports,
  })
  return module.exports
}

function loadOmitsTemperature() {
  if (fs.existsSync(helperPath)) return loadHelper().omitsTemperature

  const led = extractFunction(fs.readFileSync(ledPath, 'utf8'), 'samplingOptions')
  const title = extractFunction(fs.readFileSync(titlePath, 'utf8'), 'titleSamplingOptions')
  const memory = extractFunction(fs.readFileSync(memoryPath, 'utf8'), 'memorySamplingOptions')
  const js = `
    function text(value) { return String(value ?? '').replace(/\\u0000/g, '').trim() }
    ${stripTypes(led)}
    ${stripTypes(title)}
    ${stripTypes(memory)}
    function omitsTemperature(config) {
      return !('temperature' in samplingOptions(config, 0.7))
        && !('temperature' in titleSamplingOptions(config))
        && !('temperature' in memorySamplingOptions(config))
    }
  `
  const module = { exports: {} }
  vm.runInNewContext(`${js}\nmodule.exports = { omitsTemperature }`, { module, exports: module.exports })
  return module.exports.omitsTemperature
}

function loadSamplingOptions() {
  const omits = loadOmitsTemperature()
  return (config, temperature) => (omits(config) ? {} : { temperature })
}

const omitsTemperature = loadOmitsTemperature()
const samplingOptions = loadSamplingOptions()
const plain = (value) => JSON.parse(JSON.stringify(value))

function responses(model) {
  return { protocol: 'openai-responses', model }
}

test('openai-responses gpt-6-luna omits temperature', () => {
  assert.equal(omitsTemperature(responses('gpt-6-luna')), true)
  assert.deepEqual(plain(samplingOptions(responses('gpt-6-luna'), 0.7)), {})
})

test('openai-responses gpt-6-* variants omit temperature', () => {
  for (const model of ['gpt-6', 'gpt-6-something', 'GPT-6-Luna']) {
    assert.equal(omitsTemperature(responses(model)), true, model)
    assert.deepEqual(plain(samplingOptions(responses(model), 0.2)), {})
  }
})

test('openai-responses gpt-5 and o-series still omit temperature', () => {
  for (const model of ['gpt-5', 'gpt-5.5', 'gpt-5.4-mini', 'o1', 'o1-preview', 'o3', 'o4-mini']) {
    assert.equal(omitsTemperature(responses(model)), true, model)
    assert.deepEqual(plain(samplingOptions(responses(model), 0.7)), {})
  }
})

test('non-reasoning openai-responses still includes temperature', () => {
  for (const model of ['gpt-4o', 'gpt-4.1', 'chatgpt-4o-latest']) {
    assert.equal(omitsTemperature(responses(model)), false, model)
    assert.deepEqual(plain(samplingOptions(responses(model), 0.7)), { temperature: 0.7 })
  }
})

test('openai-compatible gpt-6-luna still includes temperature', () => {
  const config = { protocol: 'openai-compatible', model: 'gpt-6-luna' }
  assert.equal(omitsTemperature(config), false)
  assert.deepEqual(plain(samplingOptions(config, 0.3)), { temperature: 0.3 })
})

test('anthropic path still omits temperature for any model', () => {
  for (const model of ['claude-sonnet-4', 'gpt-4o', 'gpt-6-luna']) {
    const config = { protocol: 'anthropic', model }
    assert.equal(omitsTemperature(config), true, model)
    assert.deepEqual(plain(samplingOptions(config, 0.7)), {})
  }
})

test('agent chat, title, and memory sampling share omitsTemperature', () => {
  if (!fs.existsSync(helperPath)) {
    assert.fail('shared helper electron/services/agentSamplingOptions.ts is required')
  }
  for (const file of [ledPath, titlePath, memoryPath]) {
    const source = fs.readFileSync(file, 'utf8')
    assert.match(source, /from '\.\/agentSamplingOptions'/)
    assert.match(source, /omitsTemperature\s*\(/)
  }
})
