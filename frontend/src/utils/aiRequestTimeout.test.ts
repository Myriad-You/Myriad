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
      '/api/brewlia/items/1/annotations',
      '/api/agent/process',
      '/api/agent/clarify',
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
  })

  it('keeps the floor at 5 minutes', () => {
    assert.ok(AI_REQUEST_TIMEOUT_FLOOR_MS >= 5 * 60 * 1000)
    assert.ok(AI_IMAGE_REQUEST_TIMEOUT_MS >= AI_REQUEST_TIMEOUT_FLOOR_MS)
  })
})

describe('dev proxy uses the shared AI timeout table', () => {
  it('imports aiRequestTimeoutMs', () => {
    const astro = readFileSync(new URL('../../astro.config.mjs', import.meta.url), 'utf8')
    assert.match(astro, /from '\.\/src\/utils\/aiRequestTimeout\.mjs'/)
    assert.match(astro, /aiRequestTimeoutMs/)
    const timeoutPick = astro.slice(
      astro.indexOf('const aiTimeoutMs = aiRequestTimeoutMs'),
      astro.indexOf('const streamResponse'),
    )
    assert.ok(timeoutPick.length > 0, 'timeout picker not found')
    assert.match(timeoutPick, /aiTimeoutMs \?\? 30000/)
  })
})
