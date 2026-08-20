import assert from 'node:assert/strict'
import test from 'node:test'
import { ApiError } from '../../../services/api'
import {
  generationFailureMessage,
  isGenerationTimeout,
} from './generationError'

test('isGenerationTimeout reads Abort and proxy wording', () => {
  assert.equal(isGenerationTimeout(new ApiError('Request timeout', 408, 'TIMEOUT')), true)
  assert.equal(isGenerationTimeout(new Error('Backend proxy timeout')), true)
  assert.equal(isGenerationTimeout(new Error('pro_unavailable')), false)
})

test('generationFailureMessage prefers the timeout copy', () => {
  assert.equal(
    generationFailureMessage(
      new Error('Backend proxy timeout'),
      'failed',
      'timed out',
    ),
    'timed out',
  )
  assert.equal(
    generationFailureMessage(new Error('boom'), 'failed', 'timed out'),
    'boom',
  )
  assert.equal(
    generationFailureMessage(
      new ApiError('Failed to distill report signals', 502, 'report_dna_failed'),
      '词条加载失败',
      'timed out',
    ),
    '词条加载失败',
  )
  assert.equal(generationFailureMessage('nope', 'failed', 'timed out'), 'failed')
})
