import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { formatDownloadCount } from './formatDownloadCount.ts'

describe('formatDownloadCount', () => {
  it('uses Traditional units for zh-TW', () => {
    assert.equal(formatDownloadCount(12_000, 'zh-TW'), '1.2萬')
    assert.equal(formatDownloadCount(200_000_000, 'zh-HK'), '2億')
    assert.equal(formatDownloadCount(12_000, 'zh-CN'), '1.2万')
  })

  it('uses compact Latin units for English', () => {
    assert.equal(formatDownloadCount(1500, 'en-US'), '1.5K')
    assert.equal(formatDownloadCount(2_000_000, 'en-GB'), '2M')
  })
})
