/**
 * media.control handler granted-layer rejection tests.
 *
 * 沙箱 media.control handler 在派发任何播放器事件前调用
 * `mediaControlGrantedDenial`（granted 层判定），随后回打
 * runtime-grants/authorize。本文件锁定 granted 层的跨域拒绝：只持有
 * media:playback 的 grant 不得改音量或队列，反之亦然。高层 bridge action
 * （playTrack/jumpToIndex/setSkipVip/loadNeteasePlaylist）走同一映射。
 *
 *   pnpm exec tsx --test src/tapp/runtime/mediaControlAuth.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  MEDIA_ACTION_PERMISSIONS,
  mediaControlGrantedDenial,
} from './permissionConfig.ts'

const ALL_ACTIONS = Object.keys(MEDIA_ACTION_PERMISSIONS)

describe('media.control granted-layer rejection (mediaControlGrantedDenial)', () => {
  it('playback-only grant rejects every volume/queue action', () => {
    const granted = ['media:playback']
    for (const action of ['volume', 'mute', 'unmute', 'mode']) {
      assert.equal(
        mediaControlGrantedDenial(granted, action),
        `Permission denied: ${MEDIA_ACTION_PERMISSIONS[action]} required`,
        `playback-only grant must reject ${action}`,
      )
    }
    for (const action of ['media.setSkipVip', 'media.loadNeteasePlaylist']) {
      assert.ok(
        mediaControlGrantedDenial(granted, action)?.startsWith(
          'Permission denied:',
        ),
        `playback-only grant must reject ${action}`,
      )
    }
  })

  it('volume-only grant rejects playback/queue actions', () => {
    const granted = ['media:volume']
    for (const action of [
      'play',
      'pause',
      'next',
      'prev',
      'seek',
      'mode',
      'media.playTrack',
      'media.jumpToIndex',
      'media.setSkipVip',
      'media.loadNeteasePlaylist',
    ]) {
      assert.ok(
        mediaControlGrantedDenial(granted, action)?.startsWith(
          'Permission denied:',
        ),
        `volume-only grant must reject ${action}`,
      )
    }
  })

  it('queue-only grant rejects playback/volume actions', () => {
    const granted = ['media:queue']
    for (const action of [
      'play',
      'pause',
      'next',
      'prev',
      'seek',
      'volume',
      'mute',
      'unmute',
      'media.playTrack',
      'media.jumpToIndex',
    ]) {
      assert.ok(
        mediaControlGrantedDenial(granted, action)?.startsWith(
          'Permission denied:',
        ),
        `queue-only grant must reject ${action}`,
      )
    }
  })

  it('grants containing the narrowest permission pass the granted layer', () => {
    const full = ['media:playback', 'media:volume', 'media:queue']
    for (const action of ALL_ACTIONS) {
      assert.equal(
        mediaControlGrantedDenial(full, action),
        null,
        `full media grant must pass granted layer for ${action}`,
      )
    }
    assert.equal(mediaControlGrantedDenial(['media:volume'], 'mute'), null)
    assert.equal(mediaControlGrantedDenial(['media:volume'], 'volume'), null)
    assert.equal(mediaControlGrantedDenial(['media:queue'], 'mode'), null)
    assert.equal(mediaControlGrantedDenial(['media:queue'], 'media.setSkipVip'), null)
    assert.equal(mediaControlGrantedDenial(['media:playback'], 'seek'), null)
    assert.equal(mediaControlGrantedDenial(['media:playback'], 'media.playTrack'), null)
  })

  it('unknown action is always rejected', () => {
    const granted = ['media:playback', 'media:volume', 'media:queue']
    assert.equal(
      mediaControlGrantedDenial(granted, 'skip'),
      'Unknown media action: skip',
    )
    assert.equal(
      mediaControlGrantedDenial(granted, ''),
      'Unknown media action: ',
    )
  })

  it('empty or missing granted permissions deny every write action', () => {
    for (const granted of [undefined, null, []]) {
      for (const action of ALL_ACTIONS) {
        assert.ok(
          mediaControlGrantedDenial(granted, action)?.startsWith(
            'Permission denied:',
          ),
          `missing grant must reject ${action}`,
        )
      }
    }
  })

  it('denial reasons always name the narrowest permission, never the removed coarse name', () => {
    const granted: string[] = []
    for (const action of ALL_ACTIONS) {
      const denial = mediaControlGrantedDenial(granted, action)
      assert.ok(denial, `expected denial for ${action}`)
      assert.ok(
        !denial.includes('media:control'),
        `denial for ${action} must not reference removed media:control`,
      )
    }
  })
})
