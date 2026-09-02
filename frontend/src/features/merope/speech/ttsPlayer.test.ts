import type { TextVisemeCue } from '../anime25drig/textVisemes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { playTtsBuffer, sampleDecodedMouth, sampleMouth } from './ttsPlayer'

test('audio energy maps to a rest viseme when the buffer is silence', () => {
  const bins = new Uint8Array(32)
  bins.fill(128)
  const sample = sampleMouth(bins)
  assert.equal(sample.viseme, 'rest')
  assert.ok((sample.energy ?? 0) < 0.06)
})

test('louder audio opens the mouth instead of staying at rest', () => {
  const sample = sampleMouth(loud())
  assert.notEqual(sample.viseme, 'rest')
  assert.ok((sample.energy ?? 0) > 0.2)
})

function loud(): Uint8Array {
  const bins = new Uint8Array(32)
  for (let i = 0; i < bins.length; i++) bins[i] = i % 2 === 0 ? 20 : 230
  return bins
}

// Loudness is not a shape. On the old energy table this frame was 'wide'; a
// shut-lip consonant that happens to be loud must still read as shut.
test('the phoneme timeline decides the shape and the audio decides the amount', () => {
  const spans = [
    { viseme: 'closed' as const, endsAt: 0.3, emphasis: false },
    { viseme: 'narrow' as const, endsAt: 0.6, emphasis: true },
  ]
  const shut = sampleMouth(loud(), spans, 0.1)
  assert.equal(shut.viseme, 'closed')
  assert.ok(shut.amount > 0)

  const stressed = sampleMouth(loud(), spans, 0.4)
  assert.equal(stressed.viseme, 'narrow')
  assert.ok(stressed.amount >= shut.amount)

  // Same audio, no timeline: the loudness shape still stands in.
  assert.equal(sampleMouth(loud()).viseme, 'wide')
})

test('a pause inside a segment closes the mouth whatever the text says next', () => {
  const silence = new Uint8Array(32)
  silence.fill(128)
  const sample = sampleMouth(
    silence,
    [{ viseme: 'wide', endsAt: 1, emphasis: false }],
    0.5,
  )
  assert.equal(sample.viseme, 'rest')
  assert.equal(sample.amount, 0)
})

test('decoded audio predicts an upcoming mouth opening before the current sample', () => {
  const samples = new Float32Array(200)
  samples.fill(0.5, 45, 70)
  const buffer = {
    duration: 0.2,
    numberOfChannels: 1,
    sampleRate: 1_000,
    getChannelData: () => samples,
  }
  const current = sampleDecodedMouth(buffer, [], 0, 0)
  const predicted = sampleDecodedMouth(buffer, [], 0, 0.045)
  assert.equal(current?.viseme, 'rest')
  assert.notEqual(predicted?.viseme, 'rest')
  assert.ok((predicted?.energy ?? 0) > 0.5)
})

class FakeSource {
  buffer: unknown = null
  onended: (() => void) | null = null
  started = 0
  stopped = 0
  disconnected = 0
  connect(): void {}
  disconnect(): void {
    this.disconnected += 1
  }

  start(): void {
    this.started += 1
  }

  stop(): void {
    this.stopped += 1
  }
}

function fakeContext(decoded: Promise<unknown>): {
  context: AudioContext
  sources: FakeSource[]
} {
  const sources: FakeSource[] = []
  const context = {
    state: 'running',
    currentTime: 0,
    destination: {},
    resume: () => {},
    decodeAudioData: () => decoded,
    createAnalyser: () => ({
      fftSize: 256,
      getByteTimeDomainData: (bins: Uint8Array) => bins.fill(128),
      connect: () => {},
      disconnect: () => {},
    }),
    createBufferSource: () => {
      const source = new FakeSource()
      sources.push(source)
      return source
    },
  }
  return { context: context as unknown as AudioContext, sources }
}

const segment = {
  segmentId: 'msg:1',
  sequence: 1,
  text: '你好',
  messageId: 'msg',
  generation: 0,
  interrupt: 'queue',
} as const

function hooks(): { ended: number; onEnergy: () => void; onEnded: () => void } {
  const state = {
    ended: 0,
    onEnergy: () => {},
    onEnded: () => {
      state.ended += 1
    },
  }
  return state
}

/** Lets the decode promise and its continuations run. */
function settle(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve))
}

const raf = (): number => 1
globalThis.requestAnimationFrame = raf as typeof requestAnimationFrame
globalThis.cancelAnimationFrame = (() => {}) as typeof cancelAnimationFrame

test('a stale onended after cancel does not report the segment finished', async () => {
  const state = hooks()
  const { context, sources } = fakeContext(Promise.resolve({}))
  const handle = playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  await settle()
  assert.equal(sources.length, 1)

  handle.stop()
  assert.equal(sources[0]!.stopped, 1)

  // WebAudio dispatches onended asynchronously, so it still arrives after the
  // cancel that stopped the source. It must not count as the segment ending.
  sources[0]!.onended?.()
  assert.equal(state.ended, 0)
})

test('playback that runs to the end reports the segment finished once', async () => {
  const state = hooks()
  const { context, sources } = fakeContext(Promise.resolve({}))
  playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  await settle()

  sources[0]!.onended?.()
  assert.equal(state.ended, 1)
  sources[0]!.onended?.()
  assert.equal(state.ended, 1)
})

test('cancelling before decode resolves never starts playback', async () => {
  const state = hooks()
  const { context, sources } = fakeContext(Promise.resolve({}))
  const handle = playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  handle.stop()
  await settle()

  assert.deepEqual(sources, [])
  assert.equal(state.ended, 0)
})

test('a decode failure reports the segment finished so the queue moves on', async () => {
  const state = hooks()
  const { context } = fakeContext(Promise.reject(new Error('bad audio')))
  playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  await settle()

  assert.equal(state.ended, 1)
})

test('cold viseme compilation never delays decoded audio playback', async () => {
  let finishCompilation: (cues: TextVisemeCue[]) => void = () => {}
  const compilation = new Promise<TextVisemeCue[]>((resolve) => {
    finishCompilation = resolve
  })
  const { context, sources } = fakeContext(Promise.resolve({}))
  playTtsBuffer(new ArrayBuffer(8), segment, hooks(), context, {
    compileVisemes: () => compilation,
  })
  await settle()
  assert.equal(sources[0]?.started, 1)
  finishCompilation([])
})

test('predictive mouth target is published before audio enters the render timeline', async () => {
  const order: string[] = []
  const { context } = fakeContext(
    Promise.resolve({
      duration: 1,
      numberOfChannels: 1,
      sampleRate: 1_000,
      getChannelData: () => new Float32Array(1_000).fill(0.4),
    }),
  )
  playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: () => order.push('mouth'),
      onStarted: () => order.push('audio'),
      onEnded: () => {},
    },
    context,
    { compileVisemes: async () => [] },
  )
  await settle()
  assert.deepEqual(order.slice(0, 2), ['mouth', 'audio'])
})

test('late prosody retains the actual audio origin for predictive scheduling', async () => {
  let finishCompilation: (cues: TextVisemeCue[]) => void = () => {}
  const compilation = new Promise<TextVisemeCue[]>((resolve) => {
    finishCompilation = resolve
  })
  const timings: number[] = []
  const { context } = fakeContext(
    Promise.resolve({
      duration: 1,
      numberOfChannels: 1,
      sampleRate: 1_000,
      getChannelData: () => new Float32Array(1_000),
    }),
  )
  playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: () => {},
      onProsody: (_timeline, timing) => timings.push(timing.startedAtMs),
      onEnded: () => {},
    },
    context,
    { compileVisemes: () => compilation, now: () => 1_234 },
  )
  await settle()
  finishCompilation([])
  await settle()
  assert.deepEqual(timings, [1_234])
})
