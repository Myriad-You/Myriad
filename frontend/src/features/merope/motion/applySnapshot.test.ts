import type { SpeechArticulation } from '../rig/articulation'
import assert from 'node:assert/strict'
import test from 'node:test'
import { musicSignalAt } from '../singing/musicSignal.test-support'
import { applySingingWrite } from './applySnapshot'
import { resolveSingingApply } from './singingApply'

function recordingRig() {
  const singing: boolean[] = []
  const tracks: Array<string | null> = []
  const signal: Array<unknown> = []
  const articulation: SpeechArticulation[] = []
  const speechActive: boolean[] = []
  return {
    singing,
    tracks,
    signal,
    articulation,
    speechActive,
    rig: {
      setSinging: (value: boolean) => singing.push(value),
      setSingingTrack: (value: string | null) => tracks.push(value),
      setMusicSignal: (value: unknown) => signal.push(value),
      setSpeechArticulation: (value: SpeechArticulation) =>
        articulation.push(value),
      setSpeechActive: (value: boolean) => speechActive.push(value),
    },
  }
}

const rest: SpeechArticulation = { energy: 0, viseme: 'rest', amount: 0 }
const sung: SpeechArticulation = { energy: 0.6, viseme: 'open', amount: 0.8 }
const drive = {
  trackId: 'song-a',
  signal: musicSignalAt(1),
  articulation: sung,
}

test('speech-owned mouth does not clear visemes or speechActive', () => {
  const host = recordingRig()
  applySingingWrite(
    host.rig,
    resolveSingingApply({
      gap: 'active',
      holdExpired: false,
      audioPaused: false,
      mouthOwner: 'speech',
      headBodyOwner: 'music',
    }),
    drive,
  )
  assert.deepEqual(host.singing, [true])
  assert.deepEqual(host.tracks, ['song-a'])
  assert.deepEqual(host.signal, [drive.signal])
  assert.deepEqual(host.articulation, [])
  assert.deepEqual(host.speechActive, [])
})

test('music-owned mouth writes visemes without starting co-speech motion', () => {
  const host = recordingRig()
  applySingingWrite(
    host.rig,
    resolveSingingApply({
      gap: 'active',
      holdExpired: false,
      audioPaused: false,
      mouthOwner: 'music',
      headBodyOwner: 'music',
    }),
    drive,
  )
  assert.deepEqual(host.singing, [true])
  assert.deepEqual(host.articulation, [sung])
  assert.deepEqual(host.speechActive, [false])
})

test('yielding the body preserves singing evidence for weighted composition', () => {
  const host = recordingRig()
  applySingingWrite(
    host.rig,
    resolveSingingApply({
      gap: 'active',
      holdExpired: false,
      audioPaused: false,
      mouthOwner: 'music',
      headBodyOwner: 'performance',
    }),
    drive,
  )
  assert.deepEqual(host.singing, [true])
  assert.deepEqual(host.signal, [drive.signal])
})

test('stop releases groove and rests a music mouth', () => {
  const host = recordingRig()
  applySingingWrite(
    host.rig,
    resolveSingingApply({
      gap: 'stop',
      holdExpired: false,
      audioPaused: false,
      mouthOwner: 'music',
      headBodyOwner: 'music',
    }),
    { trackId: null, signal: drive.signal, articulation: rest },
  )
  assert.deepEqual(host.singing, [false])
  assert.deepEqual(host.signal, [null])
  assert.equal(host.articulation[0]?.viseme, 'rest')
  assert.deepEqual(host.speechActive, [false])
  assert.deepEqual(host.tracks, [null])
})

test('pause rests a music-owned mouth without leaving speech active', () => {
  const host = recordingRig()
  const write = applySingingWrite(
    host.rig,
    resolveSingingApply({
      gap: 'active',
      holdExpired: false,
      audioPaused: true,
      mouthOwner: 'music',
      headBodyOwner: 'music',
    }),
    drive,
  )

  assert.equal(write.speechActive, false)
  assert.equal(host.articulation[0]?.viseme, 'rest')
  assert.deepEqual(host.speechActive, [false])
  assert.deepEqual(host.singing, [true])
})
