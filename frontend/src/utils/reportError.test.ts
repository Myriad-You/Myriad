import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { reportUserFacingError } from './reportError.ts'

const copy = {
  generateNeedData: 'NEED_DATA',
  generateEmptySummary: 'EMPTY_SUMMARY',
}

describe('reportUserFacingError', () => {
  it('hides raw cache paths behind need-data copy', () => {
    assert.equal(
      reportUserFacingError(
        '平台数据未获取或处理失败：Raw data file not found: "./cache/raw/steam.json"',
        'FALLBACK',
        copy,
      ),
      'NEED_DATA',
    )
    assert.equal(
      reportUserFacingError('Failed to fetch data', 'FALLBACK', copy),
      'NEED_DATA',
    )
  })

  it('maps empty summary / missing stats', () => {
    assert.equal(
      reportUserFacingError('empty summary', 'FALLBACK', copy),
      'EMPTY_SUMMARY',
    )
  })

  it('keeps a useful platform hint', () => {
    const text = reportUserFacingError(
      'Steam 未返回游戏数据。请确认资料公开。',
      'FALLBACK',
      copy,
    )
    assert.match(text, /Steam/)
  })

  it('hides report persist dumps behind generate fallback', () => {
    assert.equal(
      reportUserFacingError(
        'insert report for steam: relation "platform_reports" does not exist',
        'FALLBACK',
        copy,
      ),
      'FALLBACK',
    )
    assert.equal(
      reportUserFacingError('Failed to save report', 'FALLBACK', copy),
      'FALLBACK',
    )
  })
})
