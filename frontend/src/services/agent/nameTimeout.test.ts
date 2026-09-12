import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

/** from_secs(2 * 60) → 120 */
function rustSeconds(rust: string, name: string): number {
  const match = rust.match(
    new RegExp(
      `${RegExp.escape(name)}: Duration = Duration::from_secs\\(([^)]+)\\)`,
    ),
  )
  assert.ok(match, `${name} not found`)
  const expression = match[1].trim()
  assert.match(expression, /^[\d\s*]+$/, `${name} 的表达式意外复杂：${expression}`)
  return expression
    .split('*')
    .map((part) => Number(part.trim()))
    .reduce((left, right) => left * right, 1)
}

/** 6 * 60 * 1000 → 360000 */
function tsMs(ts: string, name: string): number {
  const match = ts.match(new RegExp(`const ${RegExp.escape(name)} = ([\\d\\s*]+)`))
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

  // Must exceed backend NAME_CALL_TIMEOUT.
  assert.ok(
    frontend > backend * 1000,
    `前端 ${frontend}ms 必须大于后端 ${backend}s`,
  )
  // Slack only; not another long-task timeout.
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
  // Do not share the 15 min persona timeout.
  assert.doesNotMatch(nameCall, /PERSONA_GENERATION_TIMEOUT_MS/)
})

test('import persona uses the long onboarding timeout, including the dev proxy', () => {
  // /import needs the long timeout (else 30s proxy hop fails first).
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
