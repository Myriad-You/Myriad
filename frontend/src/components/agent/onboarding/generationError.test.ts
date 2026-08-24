import assert from 'node:assert/strict'
import test from 'node:test'
import { ApiError, parseApiErrorBody } from '../../../services/api'
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
    '词条加载失败 Failed to distill report signals',
  )
  assert.equal(generationFailureMessage('nope', 'failed', 'timed out'), 'failed')
})

test('parseApiErrorBody keeps code and prefers detail message', () => {
  const parsed = parseApiErrorBody(
    {
      error: 'Failed to suggest a name',
      code: 'name_suggest_failed',
      message: 'onboarding model returned empty text',
      hint: 'check Standard model',
    },
    502,
  )
  assert.equal(parsed.message, 'onboarding model returned empty text')
  assert.equal(parsed.code, 'name_suggest_failed')
  assert.equal(parsed.hint, 'check Standard model')
  assert.equal(
    parseApiErrorBody({ error: 'report_dna_failed' }, 502).code,
    'report_dna_failed',
  )
})

test('generationFailureMessage strips a code prefix from quota-style detail', () => {
  assert.equal(
    generationFailureMessage(
      new ApiError('QUEUE_FULL: Agent queue is full', 429, 'QUEUE_FULL'),
      '未知错误',
      'timed out',
      { QUEUE_FULL: '当前排队已满，请稍后再试。' },
    ),
    '当前排队已满，请稍后再试。 Agent queue is full',
  )
})

test('generationFailureMessage appends hint when it adds information', () => {
  assert.equal(
    generationFailureMessage(
      new ApiError(
        'The model returned an unusable visual design',
        502,
        'visual_design_unusable',
        undefined,
        'visual design failed style-lock check',
      ),
      '视觉设计失败',
      'timed out',
      { visual_design_unusable: '这次视觉设计不能用，请再生成一次。' },
    ),
    '这次视觉设计不能用，请再生成一次。 The model returned an unusable visual design visual design failed style-lock check',
  )
})

test('generationFailureMessage keeps provider detail on portrait failure', () => {
  class MeropeLikeError extends Error {
    constructor(
      message: string,
      readonly status: number,
      readonly code?: string,
    ) {
      super(message)
      this.name = 'MeropeApiError'
    }
  }
  assert.equal(
    generationFailureMessage(
      new MeropeLikeError(
        'OpenRouter image API returned HTTP 400: bad input_references',
        502,
        'image_provider_rejected',
      ),
      '主立绘生成失败，请重试',
      'timed out',
      { image_provider_rejected: '图片源拒绝了这次请求。' },
    ),
    '图片源拒绝了这次请求。 OpenRouter image API returned HTTP 400: bad input_references',
  )
  assert.equal(
    generationFailureMessage(
      new ApiError(
        'OpenRouter image API returned HTTP 402: {"error":{"message":"Insufficient credits"}}',
        502,
        'image_provider_credits',
      ),
      '主立绘生成失败，请重试',
      'timed out',
      { image_provider_credits: '图片源额度不足' },
    ),
    '图片源额度不足 OpenRouter image API returned HTTP 402: {"error":{"message":"Insufficient credits"}}',
  )
  assert.equal(
    generationFailureMessage(
      new ApiError('主立绘生成失败，请重试', 502, 'portrait_generation_failed'),
      '主立绘生成失败，请重试',
      'timed out',
      { portrait_generation_failed: '主立绘生成失败，请重试' },
    ),
    '主立绘生成失败，请重试',
  )
})
