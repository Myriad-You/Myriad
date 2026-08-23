import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  classifyMusicLoadError,
  formatMusicError,
} from './musicError.ts'

describe('classifyMusicLoadError', () => {
  it('maps rate-limit and copyright wording to dedicated keys', () => {
    assert.deepEqual(
      classifyMusicLoadError(new Error('Rate limited')),
      { key: 'playlistRateLimited', detail: '' },
    )
    assert.deepEqual(
      classifyMusicLoadError(new Error('网易云API访问频率过高,请稍后再试或使用QQ音乐')),
      { key: 'playlistRateLimited', detail: '' },
    )
    assert.deepEqual(
      classifyMusicLoadError(
        new Error('该歌单因版权或地理位置限制无法播放,建议使用QQ音乐'),
      ),
      { key: 'playlistBlocked', detail: '' },
    )
    assert.deepEqual(
      classifyMusicLoadError(new Error('歌单为空或无可用歌曲')),
      { key: 'playlistEmpty', detail: '' },
    )
  })

  it('keeps a useful provider detail on generic load failure', () => {
    assert.deepEqual(
      classifyMusicLoadError(new Error('网易云API错误 (404)')),
      { key: 'loadPlaylistFailed', detail: '网易云API错误 (404)' },
    )
    assert.equal(
      classifyMusicLoadError(new Error('API Error: 502')).detail,
      '',
    )
  })
})

describe('formatMusicError', () => {
  it('joins the localized key with extra detail', () => {
    assert.equal(formatMusicError('加载歌单失败', '网易云API错误 (404)'), '加载歌单失败 · 网易云API错误 (404)')
    assert.equal(formatMusicError('加载歌单失败', '加载歌单失败'), '加载歌单失败')
  })
})
