import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  AI_IMAGE_REQUEST_TIMEOUT_MS,
  AI_REQUEST_TIMEOUT_FLOOR_MS,
  aiRequestTimeoutMs,
} from './aiRequestTimeout.mjs'

describe('aiRequestTimeoutMs', () => {
  it('gives image-class paths 15 minutes', () => {
    assert.equal(
      aiRequestTimeoutMs('/api/home/stickers/generate'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
    assert.equal(
      aiRequestTimeoutMs('/api/merope/rig/portrait'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
    assert.equal(
      aiRequestTimeoutMs('/api/model3d/tasks/abc'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
    assert.equal(
      aiRequestTimeoutMs('/api/tapp/3d/tasks'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
    assert.equal(
      aiRequestTimeoutMs('/api/agent/persona/draft'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
    assert.equal(
      aiRequestTimeoutMs('/api/media/12/edit-preview'),
      AI_IMAGE_REQUEST_TIMEOUT_MS,
    )
  })

  it('gives other AI paths at least 5 minutes', () => {
    const paths = [
      '/api/speech/tts',
      '/api/tapp/ai/v2/tasks',
      '/api/reports/generate-all',
      '/api/reports/platform',
      '/api/prompt/generate',
      '/api/seo/generate-copy',
      '/api/ai/recommend-icon',
      '/api/phantasiai/items/1/annotations',
      '/api/agent/process',
      '/api/agent/process/stream',
      '/api/agent/tasks/x/answer',
    ]
    for (const path of paths) {
      const ms = aiRequestTimeoutMs(path)
      assert.ok(
        (ms ?? 0) >= AI_REQUEST_TIMEOUT_FLOOR_MS,
        `${path} → ${ms} below 5 min floor`,
      )
    }
  })

  it('does not treat ordinary CRUD as AI', () => {
    assert.equal(aiRequestTimeoutMs('/api/config/ui'), undefined)
    assert.equal(aiRequestTimeoutMs('/api/auth/me'), undefined)
    assert.equal(aiRequestTimeoutMs('/api/agent/health'), undefined)
    assert.equal(aiRequestTimeoutMs('/api/agent/persona'), undefined)
    assert.equal(aiRequestTimeoutMs('/api/media/12/edits'), undefined)
  })

  it('keeps the floor at 5 minutes', () => {
    assert.ok(AI_REQUEST_TIMEOUT_FLOOR_MS >= 5 * 60 * 1000)
    assert.ok(AI_IMAGE_REQUEST_TIMEOUT_MS >= AI_REQUEST_TIMEOUT_FLOOR_MS)
  })
})

describe('dev proxy uses the shared AI timeout table', () => {
  it('imports aiRequestTimeoutMs', () => {
    const vite = readFileSync(
      new URL('../../scripts/vite/backendDevProxy.mjs', import.meta.url),
      'utf8',
    )
    assert.match(vite, /from '\.\.\/\.\.\/src\/utils\/aiRequestTimeout\.mjs'/)
    assert.match(vite, /aiRequestTimeoutMs/)
    const timeoutPick = vite.slice(
      vite.indexOf('const aiTimeoutMs = aiRequestTimeoutMs'),
      vite.indexOf('const streamResponse'),
    )
    assert.ok(timeoutPick.length > 0, 'timeout picker not found')
    assert.match(timeoutPick, /aiTimeoutMs \?\? 30000/)
  })
})

describe('production proxy and persona worker outwait the browser', () => {
  const minutes = (source: string, name: string) => {
    const found = source.match(
      new RegExp(`const ${name}: Duration = Duration::from_secs\\((\\d+) \\* 60\\)`),
    )
    assert.ok(found, `${name} not found`)
    return Number(found[1]) * 60 * 1000
  }
  const read = (path: string) =>
    readFileSync(new URL(path, import.meta.url), 'utf8')

  it('waits longer for long persona paths than the browser does', () => {
    const proxy = read('../../../proxy/src/main.rs')
    const worker = read('../../../backend/src/persona/worker.rs')
    const proxyLong = minutes(proxy, 'PERSONA_LONG_REQUEST_TIMEOUT')
    const workerLong = minutes(worker, 'LONG_WORK_TIMEOUT')
    assert.ok(workerLong > AI_IMAGE_REQUEST_TIMEOUT_MS)
    assert.ok(proxyLong > workerLong)
    assert.ok(
      minutes(proxy, 'PERSONA_REQUEST_TIMEOUT') > minutes(worker, 'WORK_TIMEOUT'),
    )
    for (const path of ['/api/merope/rig', '/api/agent/persona']) {
      assert.equal(aiRequestTimeoutMs(`${path}/x`), AI_IMAGE_REQUEST_TIMEOUT_MS)
      assert.ok(proxy.includes(`"${path}"`), `proxy misses ${path}`)
      assert.ok(worker.includes(`"${path}"`), `worker misses ${path}`)
    }
  })
})
