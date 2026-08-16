import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  formatShareRoomId,
  gameMessageType,
  parseShareRoomId,
} from './GameBridge.ts'
import { generateFullSDK } from './sandbox/sdkGenerator.ts'

describe('Tapp.game share ids', () => {
  it('joins room_id and home_server', () => {
    assert.equal(
      formatShareRoomId('rm_abc', 'peer.example:8443'),
      'rm_abc@peer.example:8443',
    )
    assert.equal(formatShareRoomId('rm_abc@already', 'ignored'), 'rm_abc@already')
    assert.equal(formatShareRoomId('rm_abc', ''), 'rm_abc')
  })

  it('parses share ids', () => {
    assert.deepEqual(parseShareRoomId('rm_abc@peer.example:8443'), {
      roomId: 'rm_abc',
      homeServer: 'peer.example:8443',
    })
    assert.deepEqual(parseShareRoomId('rm_abc'), { roomId: 'rm_abc' })
    assert.deepEqual(
      parseShareRoomId('myriad:room:rm_abc@peer.example:8443'),
      { roomId: 'rm_abc', homeServer: 'peer.example:8443' },
    )
    assert.deepEqual(
      parseShareRoomId('https://peer.example:8443/api/federation/public/rooms/rm_abc'),
      { roomId: 'rm_abc', homeServer: 'peer.example:8443' },
    )
  })

  it('builds the game message type', () => {
    assert.equal(
      gameMessageType('com.example.chess', 'v1'),
      'game:com.example.chess:v1',
    )
  })

  it('exposes Tapp.game on the Page SDK', () => {
    const sdk = generateFullSDK(
      {
        id: 'com.example.chess',
        manifest: {
          id: 'com.example.chess',
          name: 'Chess',
          version: '1.0.0',
          main: 'main.js',
          permissions: ['game:session'],
          category: 'game',
          game: { protocol: 'v1' },
        },
        status: 'running',
        installedAt: '2026-08-14T00:00:00Z',
        grantedPermissions: ['game:session'],
        userRole: 'user',
      },
      'session',
      'page',
    )
    assert.match(sdk, /game:\s*\{/)
    assert.match(sdk, /sendIntent:/)
    assert.match(sdk, /sendState:/)
    assert.match(sdk, /type === "game:com.example.chess:v1"/)
    assert.doesNotMatch(sdk, /type\.indexOf\('game:'\)/)
  })
})
