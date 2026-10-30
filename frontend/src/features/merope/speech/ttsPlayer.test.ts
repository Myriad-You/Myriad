import type { SpeechProsodyTimeline } from '@myriad/figure/speech/prosody'
import type { TextVisemeCue } from '@myriad/figure/speech/textVisemes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TtsPipeline } from './ttsPipeline'
import {
  pcmSamples,
  playPcmStream,
  playTtsBuffer,
  sampleDecodedMouth,
  sampleMouth,
  speechOutputLatencySeconds,
} from './ttsPlayer'

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

// Loudness is not a shape.
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

interface FakeClock {
  currentTime: number
  outputLatency?: number
  baseLatency?: number
}

function fakeContext(
  decoded: Promise<unknown>,
  options: {
    state?: AudioContextState
    resume?: () => Promise<void>
    outputLatency?: number
    baseLatency?: number
  } = {},
): {
  context: AudioContext
  sources: FakeSource[]
  clock: FakeClock
} {
  const sources: FakeSource[] = []
  const context = {
    state: options.state ?? 'running',
    currentTime: 0,
    outputLatency: options.outputLatency,
    baseLatency: options.baseLatency,
    destination: {},
    resume: options.resume ?? (() => Promise.resolve()),
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
    createBuffer: (_channels: number, length: number, sampleRate: number) => {
      const samples = new Float32Array(length)
      return {
        duration: length / sampleRate,
        numberOfChannels: 1,
        sampleRate,
        getChannelData: () => samples,
      }
    },
  }
  return {
    context: context as unknown as AudioContext,
    sources,
    clock: context,
  }
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

  // It must not count as the segment ending.
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

test('cancelling without an audio device suppresses the deferred completion', async () => {
  const savedContext = globalThis.AudioContext
  Reflect.deleteProperty(globalThis, 'AudioContext')
  try {
    const state = hooks()
    const handle = playTtsBuffer(new ArrayBuffer(8), segment, state)
    handle.stop()
    await settle()
    assert.equal(state.ended, 0)
  } finally {
    if (savedContext) globalThis.AudioContext = savedContext
  }
})

test('suspended audio waits for resume before publishing mouth or phrase timing', async () => {
  const { promise: resumed, resolve: resume } = Promise.withResolvers<void>()
  const { context, sources } = fakeContext(Promise.resolve({ duration: 1 }), {
    state: 'suspended',
    resume: () => resumed,
  })
  let now = 100
  const timings: number[] = []
  let mouthFrames = 0
  const handle = playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: () => {
        mouthFrames += 1
      },
      onProsody: (_timeline, timing) => timings.push(timing.startedAtMs),
      onEnded: () => {},
    },
    context,
    { compileVisemes: async () => [], now: () => now },
  )
  try {
    await settle()
    assert.equal(
      sources.reduce((sum, source) => sum + source.started, 0),
      0,
    )
    assert.equal(mouthFrames, 0)
    assert.deepEqual(timings, [])
    now = 900
    resume()
    await settle()
    assert.equal(sources[0]?.started, 1)
    assert.equal(mouthFrames, 1)
    assert.ok(timings.length > 0)
    assert.ok(timings.every((timing) => timing === 900))
  } finally {
    handle.stop()
    resume()
  }
})

test('cancelling during resume prevents late playback and completion', async () => {
  const { promise: resumed, resolve: resume } = Promise.withResolvers<void>()
  const state = hooks()
  const { context, sources } = fakeContext(Promise.resolve({ duration: 1 }), {
    state: 'suspended',
    resume: () => resumed,
  })
  const handle = playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  await settle()
  handle.stop()
  resume()
  await settle()
  assert.equal(
    sources.reduce((sum, source) => sum + source.started, 0),
    0,
  )
  assert.equal(state.ended, 0)
})

test('resume rejection ends the segment without starting its mouth or audio', async () => {
  const state = hooks()
  const { context, sources } = fakeContext(Promise.resolve({ duration: 1 }), {
    state: 'suspended',
    resume: async () => {
      throw new Error('audio unavailable')
    },
  })
  playTtsBuffer(new ArrayBuffer(8), segment, state, context)
  await settle()
  assert.equal(state.ended, 1)
  assert.equal(
    sources.reduce((sum, source) => sum + source.started, 0),
    0,
  )
})

test('a resume failure releases the real queue so the next segment can play', async () => {
  let attempts = 0
  const { context, sources } = fakeContext(Promise.resolve({ duration: 1 }), {
    state: 'suspended',
    resume: async () => {
      attempts += 1
      if (attempts === 1) throw new Error('audio unavailable')
    },
  })
  const played: string[] = []
  const pipeline = new TtsPipeline({
    synthesize: async () => new ArrayBuffer(8),
    play: (audio, item, onEnded) =>
      playTtsBuffer(
        audio,
        item,
        {
          onEnergy: () => {},
          onStarted: () => {
            played.push(item.messageId)
          },
          onEnded,
        },
        context,
        { compileVisemes: async () => [] },
      ),
  })
  try {
    pipeline.enqueue([
      segment,
      { ...segment, messageId: 'next', segmentId: 'next:1' },
    ])
    await settle()
    assert.deepEqual(played, ['next'])
    assert.equal(pipeline.isBusyWith(segment.messageId), false)
    assert.equal(pipeline.isBusyWith('next'), true)
    sources[0]!.onended?.()
    assert.equal(pipeline.playing, false)
    assert.equal(pipeline.queueLength, 0)
  } finally {
    pipeline.cancel()
  }
})

test('cold viseme compilation never delays decoded audio playback', async () => {
  const { promise: compilation, resolve: finishCompilation } = Promise.withResolvers<
    TextVisemeCue[]
  >()
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
  const { promise: compilation, resolve: finishCompilation } = Promise.withResolvers<
    TextVisemeCue[]
  >()
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
  assert.deepEqual(timings, [1_234, 1_234])
})

test('text beats reach playback before cold visemes and survive their later refinement', async () => {
  const { promise: compilation, resolve } = Promise.withResolvers<
    TextVisemeCue[]
  >()
  const timelines: SpeechProsodyTimeline[] = []
  const { context, sources } = fakeContext(Promise.resolve({ duration: 4 }))
  const handle = playTtsBuffer(
    new ArrayBuffer(8),
    { ...segment, text: '不过我们可以试试。你觉得呢？' },
    {
      onEnergy: () => {},
      onEnded: () => {},
      onProsody: (timeline) => timelines.push(timeline),
    },
    context,
    { compileVisemes: () => compilation, now: () => 1_000 },
  )
  await settle()
  assert.equal(sources[0]?.started, 1)
  assert.equal(timelines.length, 1)
  assert.ok(timelines[0]!.accents.length >= 2)
  resolve([])
  await settle()
  assert.deepEqual(timelines[1], timelines[0])
  handle.stop()
})

test('her own voice is read sample by sample, an odd byte waiting for the next piece', () => {
  // 0x4000 = 16384 → 0.5; 0xC000 = -16384 → -0.5, split across pieces.
  const first = pcmSamples(null, new Uint8Array([0x00, 0x40, 0x00]))
  assert.deepEqual(Array.from(first.samples), [0.5])
  assert.deepEqual(Array.from(first.carry ?? []), [0x00])
  const second = pcmSamples(first.carry, new Uint8Array([0xC0]))
  assert.deepEqual(Array.from(second.samples), [-0.5])
  assert.equal(second.carry, null)
})

test('the output delay counts only when the browser reports one', () => {
  assert.equal(speechOutputLatencySeconds({}), 0)
  assert.equal(speechOutputLatencySeconds({ outputLatency: 0, baseLatency: 0.01 }), 0)
  assert.equal(speechOutputLatencySeconds({ outputLatency: Number.NaN }), 0)
  assert.equal(
    speechOutputLatencySeconds({ outputLatency: 0.182, baseLatency: 0.01 }),
    0.192,
  )
  assert.equal(speechOutputLatencySeconds({ outputLatency: 30 }), 1)
})

test('a voice still on its way to the ear leaves the mouth shut', () => {
  const samples = new Float32Array(1_000).fill(0.4)
  const buffer = {
    duration: 1,
    numberOfChannels: 1,
    sampleRate: 1_000,
    getChannelData: () => samples,
  }
  assert.equal(sampleDecodedMouth(buffer, [], -0.1)?.viseme, 'rest')
  assert.equal(sampleDecodedMouth(buffer, [], -0.1)?.energy, 0)
  assert.notEqual(sampleDecodedMouth(buffer, [], -0.01)?.viseme, 'rest')
})

/** Animation frames run by hand, one at a time. */
function frames(): { next: () => void; restore: () => void } {
  const saved = globalThis.requestAnimationFrame
  let pending: FrameRequestCallback | null = null
  globalThis.requestAnimationFrame = ((callback: FrameRequestCallback) => {
    pending = callback
    return 1
  }) as typeof requestAnimationFrame
  return {
    next: () => {
      const callback = pending
      pending = null
      callback?.(0)
    },
    restore: () => {
      globalThis.requestAnimationFrame = saved
    },
  }
}

function loudBuffer(): unknown {
  return {
    duration: 1,
    numberOfChannels: 1,
    sampleRate: 1_000,
    getChannelData: () => new Float32Array(1_000).fill(0.4),
  }
}

test('the mouth follows what is heard, the output delay behind the audio clock', async () => {
  const frame = frames()
  const mouths: string[] = []
  const { context, clock } = fakeContext(Promise.resolve(loudBuffer()), {
    outputLatency: 0.19,
    baseLatency: 0.01,
  })
  const handle = playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: (_energy, articulation) => mouths.push(articulation.viseme),
      onEnded: () => {},
    },
    context,
    { compileVisemes: async () => [] },
  )
  try {
    await settle()
    clock.currentTime = 0.1
    frame.next()
    assert.deepEqual(mouths, ['rest', 'rest'], 'shut while it is on its way')
    clock.currentTime = 0.2
    frame.next()
    assert.notEqual(mouths.at(-1), 'rest', 'open once it is heard')
    assert.equal(handle.progress?.(), 0, 'only what was heard counts as said')
    clock.currentTime = 0.7
    assert.ok(Math.abs((handle.progress?.() ?? 0) - 0.5) < 1e-9)
  } finally {
    handle.stop()
    frame.restore()
  }
})

test('without a reported output delay the mouth keeps to the audio clock', async () => {
  const mouths: string[] = []
  const { context } = fakeContext(Promise.resolve(loudBuffer()))
  const handle = playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: (_energy, articulation) => mouths.push(articulation.viseme),
      onEnded: () => {},
    },
    context,
    { compileVisemes: async () => [] },
  )
  await settle()
  assert.notEqual(mouths[0], 'rest')
  handle.stop()
})

test('gestures start when the voice is heard and move with a new output delay', async () => {
  const frame = frames()
  const timings: number[] = []
  const { context, clock } = fakeContext(Promise.resolve(loudBuffer()), {
    outputLatency: 0.2,
  })
  const handle = playTtsBuffer(
    new ArrayBuffer(8),
    segment,
    {
      onEnergy: () => {},
      onProsody: (_timeline, timing) => timings.push(timing.startedAtMs),
      onEnded: () => {},
    },
    context,
    { compileVisemes: async () => [], now: () => 1_000 },
  )
  try {
    await settle()
    assert.ok(timings.length > 0)
    assert.ok(timings.every((timing) => timing === 1_200))

    // Jitter in the reading does not move them.
    clock.outputLatency = 0.204
    frame.next()
    assert.ok(timings.every((timing) => timing === 1_200))

    // A headset connected mid-line: moved, not started over.
    const before = timings.length
    clock.outputLatency = 0.25
    frame.next()
    assert.deepEqual(timings.slice(before), [1_250])
    frame.next()
    assert.equal(timings.length, before + 1)
  } finally {
    handle.stop()
    frame.restore()
  }
})

test('her own voice moves the mouth when it reaches the ear', () => {
  const frame = frames()
  const mouths: string[] = []
  const { context, clock } = fakeContext(Promise.resolve({}), {
    outputLatency: 0.2,
  })
  const stream = playPcmStream(
    {
      onEnergy: (_energy, articulation) => mouths.push(articulation.viseme),
      onEnded: () => {},
    },
    context,
  )
  try {
    // A tenth of a second, loud, scheduled to play at 0.12 s.
    const bytes = new Uint8Array(4_800)
    const view = new DataView(bytes.buffer)
    for (let i = 0; i < 2_400; i += 1)
      view.setInt16(i * 2, i % 2 === 0 ? 12_000 : -12_000, true)
    stream.push(bytes)
    assert.equal(mouths.at(-1), 'rest')
    clock.currentTime = 0.15
    frame.next()
    assert.equal(mouths.at(-1), 'rest', 'played but not yet heard')
    clock.currentTime = 0.33
    frame.next()
    assert.notEqual(mouths.at(-1), 'rest', 'heard')
    clock.currentTime = 0.6
    frame.next()
    assert.equal(mouths.at(-1), 'rest', 'all of it heard')
  } finally {
    stream.stop()
    frame.restore()
  }
})
