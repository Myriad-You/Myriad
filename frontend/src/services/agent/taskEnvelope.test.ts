import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  imageUrlFromStepOutput,
  imageUrlsFromAgentPayload,
  messageFromStepOutput,
  taskInnerValue,
} from './taskEnvelope.ts'

describe('taskInnerValue', () => {
  it('unwraps Tapp envelopes and leaves flat output alone', () => {
    assert.deepEqual(
      taskInnerValue({
        format: 'json',
        value: { analysis: '分析正文' },
        contextProvenance: [],
      }),
      { analysis: '分析正文' },
    )
    assert.deepEqual(taskInnerValue({ analysis: '旧格式' }), {
      analysis: '旧格式',
    })
  })
})

describe('messageFromStepOutput', () => {
  it('unwraps Tapp envelopes for analyze, chat, and summarize', () => {
    assert.equal(
      messageFromStepOutput({
        format: 'json',
        value: { analysis: '分析正文', type: 'custom' },
        contextProvenance: [],
      }),
      '分析正文',
    )
    assert.equal(
      messageFromStepOutput({
        format: 'text',
        value: '回复正文',
        contextProvenance: [],
      }),
      '回复正文',
    )
    assert.equal(
      messageFromStepOutput({
        format: 'json',
        value: { summary: '摘要正文', style: 'brief' },
        contextProvenance: [],
      }),
      '摘要正文',
    )
  })

  it('still reads flat step output including search aiSummary', () => {
    assert.equal(messageFromStepOutput({ analysis: '旧格式' }), '旧格式')
    assert.equal(
      messageFromStepOutput({ aiSummary: '搜索综述' }),
      '搜索综述',
    )
  })
})

describe('imageUrlFromStepOutput', () => {
  it('reads envelope url and legacy imageUrl', () => {
    assert.equal(
      imageUrlFromStepOutput({
        format: 'image',
        value: {
          url: 'https://example.invalid/a.png',
          width: 1024,
          height: 768,
        },
        contextProvenance: [],
      }),
      'https://example.invalid/a.png',
    )
    assert.equal(
      imageUrlFromStepOutput({ imageUrl: '/api/brew/image-cache/aa/abcd.png' }),
      '/api/brew/image-cache/aa/abcd.png',
    )
  })
})

describe('imageUrlsFromAgentPayload', () => {
  it('merges envelope data with stepHistory imageUrl', () => {
    assert.deepEqual(
      imageUrlsFromAgentPayload(
        {
          format: 'image',
          value: { url: 'https://example.invalid/a.png' },
          contextProvenance: [],
        },
        [{ imageUrl: 'https://example.invalid/b.png' }],
      ),
      ['https://example.invalid/a.png', 'https://example.invalid/b.png'],
    )
  })
})
