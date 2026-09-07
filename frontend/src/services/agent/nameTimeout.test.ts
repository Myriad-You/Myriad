import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

/** `const X: Duration = Duration::from_secs(2 * 60);` → 120 */
function rustSeconds(rust: string, name: string): number {
  const match = rust.match(
    new RegExp(`${name}: Duration = Duration::from_secs\\(([^)]+)\\)`),
  )
  assert.ok(match, `${name} not found`)
  const expression = match[1].trim()
  assert.match(expression, /^[\d\s*]+$/, `${name} 的表达式意外复杂：${expression}`)
  return expression
    .split('*')
    .map((part) => Number(part.trim()))
    .reduce((left, right) => left * right, 1)
}

/** `const X = 6 * 60 * 1000` → 360000 */
function tsMs(ts: string, name: string): number {
  const match = ts.match(new RegExp(`const ${name} = ([\\d\\s*]+)`))
  assert.ok(match, `${name} not found`)
  return match[1]
    .split('*')
    .map((part) => Number(part.trim()))
    .reduce((left, right) => left * right, 1)
}

test('the browser waits longer for a name than the backend waits upstream', () => {
  const backend = rustSeconds(
    source('../../../../backend/src/services/agent/merope/onboarding_ai.rs'),
    'NAME_CALL_TIMEOUT',
  )
  const frontend = tsMs(source('./agentApi.ts'), 'NAME_SUGGEST_TIMEOUT_MS')

  // 掐得比后端小，浏览器会先断开，用户看到的是空泛的网络错误而不是后端
  // 整理好的失败原因（模型没配、被网关拒、返回不可用等等）。
  assert.ok(
    frontend > backend * 1000,
    `前端 ${frontend}ms 必须大于后端 ${backend}s`,
  )
  // 也不该大太多：留的是余量，不是又一档长任务超时。
  assert.ok(
    frontend <= backend * 1000 * 2,
    `前端 ${frontend}ms 相对后端 ${backend}s 留得过宽`,
  )
})

test('a name roll is not filed under the long-running persona timeout', () => {
  const ts = source('./agentApi.ts')
  const nameCall = ts.slice(
    ts.indexOf('async suggestPersonaName('),
    ts.indexOf('async suggestPersonaVisualDesign('),
  )
  assert.ok(nameCall.length > 0, 'suggestPersonaName not found')
  assert.match(nameCall, /NAME_SUGGEST_TIMEOUT_MS/)
  // 起名和「起草人设 / 出视觉设定」不是一回事，别再共用那 15 分钟。
  assert.doesNotMatch(nameCall, /PERSONA_GENERATION_TIMEOUT_MS/)
})

test('import persona uses the long onboarding timeout, including the dev proxy', () => {
  // 导入和起草一样要等 Pro 整理完。代理名单漏了 /import 的话，浏览器还在等
  // 15 分钟，中间那一跳 30 秒就报 Backend proxy timeout。
  const api = source('./agentApi.ts')
  const importCall = api.slice(
    api.indexOf('async importPersona('),
    api.indexOf('async draftPersona('),
  )
  assert.ok(importCall.length > 0, 'importPersona not found')
  assert.match(importCall, /PERSONA_GENERATION_TIMEOUT_MS/)

  const table = readFileSync(
    new URL('../../utils/aiRequestTimeout.mjs', import.meta.url),
    'utf8',
  )
  assert.match(table, /'\/api\/agent\/persona\/'/)
})

test('dev proxy long-timeouts cover model3d downloads, agent process, and SSE', () => {
  const astro = readFileSync(
    new URL('../../../astro.config.mjs', import.meta.url),
    'utf8',
  )
  assert.match(astro, /aiRequestTimeoutMs/)
  assert.match(astro, /AGENT_PROCESS_PROXY_TIMEOUT_MS/)
  const timeoutPick = astro.slice(
    astro.indexOf('const aiTimeoutMs = aiRequestTimeoutMs'),
    astro.indexOf('const streamResponse'),
  )
  assert.ok(timeoutPick.length > 0, 'timeout picker not found')
  assert.match(timeoutPick, /isAgentSsePath/)
  assert.match(timeoutPick, /aiTimeoutMs/)
})

test('TapSDK AI host round-trip is at least 5 minutes', () => {
  const sdk = source('../../tapp/runtime/sandbox/sdkShared.ts')
  const aiMs = tsMs(sdk, 'SDK_AI_REQUEST_TIMEOUT_MS')
  assert.ok(
    aiMs >= 5 * 60 * 1000,
    `Tapp.ai sendRequest ${aiMs}ms 低于 5 分钟保底`,
  )
})
