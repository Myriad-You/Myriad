import type { PlaygroundErrorCopy } from './playgroundErrorMessages'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  mapPlaygroundGenerateError,
  mapPlaygroundRuntimeError,

} from './playgroundErrorMessages'

const copy: PlaygroundErrorCopy = {
  playgroundTimeoutHint: 'TIMEOUT',
  playgroundServerErrorHint: 'SERVER',
  playgroundGenerateFailed: 'GENERIC',
  playgroundCancelled: 'CANCELLED',
  playgroundAiNotConfiguredHint: 'AI_OFF',
  playgroundAiGenerationFailedHint: 'AI_FAIL',
  playgroundValidationFailedHint: 'VALIDATION',
  playgroundPayloadTooLargeHint: 'TOO_LARGE',
  playgroundAdminRequiredHint: 'ADMIN',
  playgroundAuthRequiredHint: 'AUTH',
  playgroundRateLimitHint: 'RATE',
  playgroundNetworkHint: 'NETWORK',
  playgroundStreamIncompleteHint: 'STREAM',
  playgroundAgentBusyHint: 'BUSY',
  playgroundBadRequestHint: 'BAD:{detail}',
  playgroundErrorDetail: 'Detail: {detail}',
  playgroundRuntimeError: 'Runtime: {message}',
}

function format(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

describe('mapPlaygroundGenerateError', () => {
  it('maps user cancel', () => {
    assert.equal(
      mapPlaygroundGenerateError('whatever', copy, {
        userCancelled: true,
        format,
      }),
      'CANCELLED',
    )
  })

  it('maps timeouts without misclassifying Pro AI agent failures', () => {
    assert.equal(
      mapPlaygroundGenerateError('TimeoutError', copy, { format }),
      'TIMEOUT',
    )
    assert.equal(
      mapPlaygroundGenerateError(
        'The operation was aborted due to timeout.',
        copy,
        { format },
      ),
      'TIMEOUT',
    )
    assert.equal(
      mapPlaygroundGenerateError('Pro AI agent generation failed', copy, {
        format,
      }),
      'AI_FAIL',
    )
    assert.equal(
      mapPlaygroundGenerateError(
        'Pro AI agent generation failed: upstream timeout',
        copy,
        { format },
      ),
      'AI_FAIL',
    )
  })

  it('maps auth and admin', () => {
    assert.equal(
      mapPlaygroundGenerateError(
        'Administrator access required. Only current admin users can perform this action.',
        copy,
        { format },
      ),
      'ADMIN',
    )
    assert.equal(
      mapPlaygroundGenerateError(
        'Please login before using administrator functions.',
        copy,
        { format },
      ),
      'AUTH',
    )
  })

  it('maps AI not configured and validation with detail', () => {
    assert.equal(
      mapPlaygroundGenerateError(
        'Pro AI model is not enabled or configured',
        copy,
        { format },
      ),
      'AI_OFF',
    )
    const msg = mapPlaygroundGenerateError(
      'Generated Tapp did not pass validation after 3 attempts: pageHtml contains forbidden HTML pattern: <script',
      copy,
      { format },
    )
    assert.ok(msg.startsWith('VALIDATION'))
    assert.ok(msg.includes('pageHtml contains forbidden'))
    assert.ok(msg.includes('Detail:'))
  })

  it('maps payload, network, stream, rate limit', () => {
    assert.ok(
      mapPlaygroundGenerateError(
        'Playground request body exceeds 8000000 bytes (history with full project snapshots is too large; reduce revisions)',
        copy,
        { format },
      ).startsWith('TOO_LARGE'),
    )
    assert.equal(
      mapPlaygroundGenerateError('Failed to fetch', copy, { format }),
      'NETWORK',
    )
    assert.equal(
      mapPlaygroundGenerateError(
        'Playground stream ended without a final response',
        copy,
        { format },
      ),
      'STREAM',
    )
    assert.equal(
      mapPlaygroundGenerateError('HTTP 429', copy, { format }),
      'RATE',
    )
  })

  it('maps bad request with detail', () => {
    assert.equal(
      mapPlaygroundGenerateError(
        'Instruction must contain 1-32680 characters',
        copy,
        { format },
      ),
      'BAD:Instruction must contain 1-32680 characters',
    )
  })

  it('maps agent busy and server 5xx', () => {
    assert.equal(
      mapPlaygroundGenerateError('Tapp Playground agent is shutting down', copy, {
        format,
      }),
      'BUSY',
    )
    assert.ok(
      mapPlaygroundGenerateError('HTTP 503', copy, { format }).startsWith(
        'SERVER',
      ),
    )
  })

  it('keeps descriptive Chinese messages', () => {
    assert.equal(
      mapPlaygroundGenerateError('包校验失败：缺少 main.js', copy, { format }),
      '包校验失败：缺少 main.js',
    )
  })

  it('prefers a stable backend code over English wording', () => {
    assert.equal(
      mapPlaygroundGenerateError('Pro AI model is not enabled or configured', copy, {
        format,
        code: 'playground_ai_unconfigured',
      }),
      'AI_OFF',
    )
    assert.equal(
      mapPlaygroundGenerateError(
        'Generated Tapp did not pass validation after 3 attempts: missing core.js',
        copy,
        { format, code: 'playground_validation_failed' },
      ),
      'VALIDATION\nDetail: missing core.js',
    )
  })
})

describe('mapPlaygroundRuntimeError', () => {
  it('prefixes message', () => {
    assert.equal(
      mapPlaygroundRuntimeError('TypeError: x is not a function', copy, format),
      'Runtime: TypeError: x is not a function',
    )
  })
})
