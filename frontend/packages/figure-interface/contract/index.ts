import type * as Anime25drig from '../src/anime25drig'
import type * as Events from '../src/events'
import type * as Root from '../src/index'
import type * as Interaction from '../src/interaction'
import type * as Motion from '../src/motion'
import type * as Performance from '../src/performance'
import type * as Rig from '../src/rig'
import type * as Singing from '../src/singing'
import type * as Speech from '../src/speech'
import type * as Workbench from '../src/workbench'
import assert from 'node:assert/strict'
import test from 'node:test'
import { bustManifest, lifecycleContract } from './lifecycle'

/** The entries of one @myriad/figure implementation, as the interface declares them. */
export interface FigureEntries {
  root: typeof Root
  anime25drig: typeof Anime25drig
  events: typeof Events
  interaction: typeof Interaction
  motion: typeof Motion
  performance: typeof Performance
  rig: typeof Rig
  singing: typeof Singing
  speech: typeof Speech
  workbench: typeof Workbench
}

function state(revision: number, after = 60, band = 'calm') {
  return {
    mood: { before: after, after, arousalBefore: 40, arousalAfter: 40, bandBefore: band, bandAfter: band, delta: 0, cause: 'test', revision },
    activity: 'reading',
  }
}

function directive(cues: unknown[]) {
  return { phase: 'delivery', moodRevision: 3, motionStyle: 'even', plan: { cues } }
}

function cue(intent: string, extra: Record<string, unknown> = {}) {
  return { intent, atMs: 0, intensity: 1, tempo: 1, fadeInMs: 120, fadeOutMs: 200, interrupt: 'queue', ...extra }
}

function touch(overrides: Partial<Parameters<InstanceType<typeof Interaction.TouchAppraisal>['observe']>[0]>) {
  return { id: 1, phase: 'update' as const, gesture: 'hold' as const, region: 'hair' as const, durationMs: 0, distance: 0, speed: 0, repeatCount: 1, x: 0.5, y: 0.3, ...overrides }
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await new Promise(resolve => setImmediate(resolve))
}

/** A tone at `hz` as an analyser shows it: its waveform, and decibels per bin up to half the rate. */
function tone(hz: number, amplitude: number, sampleRate = 48_000, bins = 1_024) {
  const waveform = Float32Array.from({ length: bins * 2 }, (_, i) => amplitude * Math.sin((2 * Math.PI * hz * i) / sampleRate))
  const binHz = sampleRate / (bins * 2)
  const decibels = Float32Array.from({ length: bins }, (_, i) => (Math.abs(i * binHz - hz) < binHz ? 20 * Math.log10(amplitude) : -120))
  return { waveform, decibels, sampleRate }
}

/**
 * Behaviour the site and the server rely on through the interface, the same in every
 * implementation: the wire vocabulary and its validation, the channel's turn and state
 * bookkeeping, speech events and mouth timelines, when the page plays the figure, the workbench's
 * looks and patch bounds, music features, touch, and the lifecycle of a runtime and a player.
 * An implementation's own tests call this once with its entries.
 */
export function figureContract(figure: FigureEntries): void {
  const { root, anime25drig, events, interaction, motion, performance, rig, singing, speech, workbench } = figure

  test('figure contract: a channel counts unstamped events as live and stamped ones only for its turn', () => {
    const channel = new root.FigureChannel()
    channel.setGeneration(4)
    assert.equal(channel.generation(), 4)
    assert.equal(channel.isLive(undefined), true)
    assert.equal(channel.isLive(0), true)
    assert.equal(channel.isLive(4), true)
    assert.equal(channel.isLive(3), false)
    assert.notEqual(channel.newIntentId(), channel.newIntentId())
  })

  test('figure contract: her state reaches the channel once per newer revision, bounded', () => {
    const channel = new root.FigureChannel()
    const seen: unknown[] = []
    channel.addEventListener(events.MEROPE_STATE_EVENT, (event) => {
      seen.push(events.meropeStateEventDetail((event as CustomEvent).detail))
    })
    events.dispatchMeropeState(channel, state(2))
    events.dispatchMeropeState(channel, state(2, 90))
    events.dispatchMeropeState(channel, state(1, 10))
    events.dispatchMeropeState(channel, { mood: { before: 'x' } })
    assert.equal(seen.length, 1)
    assert.deepEqual(seen[0], state(2))

    const parsed = events.meropeStateEventDetail({ mood: { before: -5, after: 140, bandBefore: 'low', bandAfter: 'high', delta: 30, revision: 7.9 } })
    assert.deepEqual(parsed, {
      mood: { before: 0, after: 100, bandBefore: 'sad', bandAfter: 'excited', delta: 10, cause: 'unknown', revision: 7 },
      activity: 'idle',
    })
  })

  test('figure contract: a loaded snapshot older than the live state does not undo it', () => {
    const channel = new root.FigureChannel()
    events.dispatchMeropeState(channel, state(9, 30, 'tense'))
    assert.deepEqual(events.resolveLoadedMeropeAffect(channel, { mood: 80, arousal: 70, moodRevision: 4 }), { mood: 30, arousal: 40 })
    channel.resetState()
    assert.deepEqual(events.resolveLoadedMeropeAffect(channel, {}), { mood: 70, arousal: 48 })
  })

  test('figure contract: mood bands follow the director\'s thresholds', () => {
    assert.equal(performance.moodBand(10, 90), 'floor')
    assert.equal(performance.moodBand(54, 54), 'sad')
    assert.equal(performance.moodBand(54, 55), 'tense')
    assert.equal(performance.moodBand(55, 54), 'calm')
    assert.equal(performance.moodBand(55, 55), 'excited')
    assert.equal(performance.moodBand(undefined), 'calm')
    assert.equal(performance.moodBand(Number.NaN, Number.NaN), 'calm')
  })

  test('figure contract: a performance directive keeps known words and bounded numbers only', () => {
    assert.equal(events.sanitizePerformanceDirective(null), null)
    assert.equal(events.sanitizePerformanceDirective(directive([])), null)
    assert.equal(events.sanitizePerformanceDirective({ ...directive([cue('greet')]), motionStyle: 'wild' }), null)
    assert.equal(events.sanitizePerformanceDirective({ ...directive([cue('greet')]), phase: 'never' }), null)

    const sanitized = events.sanitizePerformanceDirective(directive([
      cue('greet', { atMs: 9_000, intensity: 5, tempo: 0, fadeInMs: 1, fadeOutMs: 5_000 }),
      cue('juggle'),
      cue('think', { interrupt: 'shove' }),
      cue('listen'),
      cue('delight'),
      cue('notify'),
    ]))
    assert.deepEqual(sanitized, {
      phase: 'delivery',
      moodRevision: 3,
      motionStyle: 'even',
      plan: {
        cues: [
          { intent: 'greet', atMs: 5_000, intensity: 1.4, tempo: 0.5, fadeInMs: 40, fadeOutMs: 800, interrupt: 'queue' },
        ],
      },
    })
  })

  test('figure contract: an utterance reaches the channel as start, its text, end', () => {
    const channel = new root.FigureChannel()
    const phases: string[] = []
    channel.addEventListener(speech.MEROPE_SPEECH_EVENT, (event) => {
      const detail = speech.meropeSpeechEventDetail((event as CustomEvent).detail)
      if (detail) phases.push(detail.phase === 'chunk' ? `chunk:${detail.text}` : detail.phase)
    })
    speech.dispatchMeropeSpeechUtterance(channel, { messageId: 'm1', source: 'reply', utteranceId: 'u1', text: 'Hello there.' })
    speech.dispatchMeropeSpeechUtterance(channel, { messageId: 'm2', source: 'reply', utteranceId: 'u2', text: '   ' })
    speech.dispatchMeropeSpeech(channel, { phase: 'start' })
    assert.deepEqual(phases, ['start', 'chunk:Hello there.', 'end'])
  })

  test('figure contract: mouth shapes for a text fill the audio they are laid on', async () => {
    const cues = await speech.compileTextVisemes('Hello there, how are you?', 'en')
    assert.ok(cues.length > 0)
    const spans = speech.alignVisemeTimeline(cues, 2)
    assert.ok(spans.length > 0)
    assert.ok(Math.abs(spans.at(-1)!.endsAt - 2) < 1e-9)
    for (let i = 1; i < spans.length; i += 1) assert.ok(spans[i]!.endsAt >= spans[i - 1]!.endsAt)
    assert.equal(speech.visemeAt(spans, 0), spans[0])
    assert.equal(speech.visemeAt(spans, 99), spans.at(-1))
    assert.equal(speech.visemeAt(spans, -1), null)
    assert.deepEqual(speech.alignVisemeTimeline(cues, 0), [])
    assert.equal(speech.visemeAmount(2, false), 1)
    assert.equal(speech.visemeAmount(-1, true), 0)
    assert.ok(speech.estimateAutoSpeechDurationMs('Hello there, how are you today?') > speech.estimateAutoSpeechDurationMs('Hi.'))
  })

  test('figure contract: the page plays the 2.5D figure only with a package, an atlas and no failure for it', () => {
    const key = anime25drig.anime25DRuntimeKey('asset', 2, '/atlas.png')
    assert.equal(key, anime25drig.anime25DRuntimeKey('asset', 2, '/atlas.png'))
    assert.notEqual(key, anime25drig.anime25DRuntimeKey('asset', 3, '/atlas.png'))
    const ready = { hasManifest: true, hasPlayback: true, atlasUrl: '/atlas.png', runtimeKey: key, failedRuntimeKey: null }
    assert.equal(anime25drig.shouldUseAnime25DRuntime(ready), true)
    assert.equal(anime25drig.shouldUseAnime25DRuntime({ ...ready, failedRuntimeKey: key }), false)
    assert.equal(anime25drig.shouldUseAnime25DRuntime({ ...ready, atlasUrl: '' }), false)
    assert.equal(anime25drig.shouldUseAnime25DRuntime({ ...ready, hasPlayback: false }), false)
    const shown = { atlasReady: true, pageVisible: true, inViewport: true, cancelled: false }
    assert.equal(anime25drig.shouldAnimateAnime25D(shown), true)
    assert.equal(anime25drig.shouldAnimateAnime25D({ ...shown, pageVisible: false }), false)
    assert.equal(anime25drig.intersectionKeepsAnime25DVisible({ isIntersecting: false, boundingClientRect: { width: 0, height: 0 } }), true)
    assert.equal(anime25drig.intersectionKeepsAnime25DVisible({ isIntersecting: true, boundingClientRect: { width: 200, height: 300 } }), true)
    assert.equal(anime25drig.intersectionKeepsAnime25DVisible({ isIntersecting: false, boundingClientRect: { width: 200, height: 300 } }), false)
  })

  test('figure contract: a fresh coordinator holds no leases, and the figure\'s errors carry a code', () => {
    assert.equal(new motion.RigMotionCoordinator().leaseCount(), 0)
    const error = new root.FigureError('psdTooLarge')
    assert.ok(error instanceof Error)
    assert.ok(error instanceof root.FigureError)
    assert.equal(error.name, 'FigureError')
    assert.equal(error.code, 'psdTooLarge')
    assert.deepEqual(error.params, {})
  })

  test('figure contract: rig validators take a legal rig and turn away what is not one', () => {
    const manifest = bustManifest()
    assert.equal(rig.isRigManifest(manifest), true)
    assert.equal(rig.isLiveMeropeManifest(manifest), true)
    assert.equal(rig.isLiveMeropeManifest({ ...manifest, anime25dPlayback: undefined }), false)
    assert.equal(rig.isRigManifest({ ...manifest, textures: [{ ...manifest.textures[0], url: 'https://elsewhere.example/atlas.png' }] }), false)
    const keys = { canvas: [manifest.canvas.width, manifest.canvas.height], keyforms: manifest.anime25dPlayback.turnKeyforms }
    assert.equal(rig.isDocumentTurnKeyforms(keys), true)
    assert.equal(rig.isDocumentTurnKeyforms({ ...keys, canvas: [0, 10] }), false)
    for (const value of [null, undefined, 1, 'rig', [], {}]) {
      assert.equal(rig.isRigManifest(value), false)
      assert.equal(rig.isLiveMeropeManifest(value), false)
      assert.equal(rig.isDocumentTurnKeyforms(value), false)
    }
    assert.ok(rig.AUTHORED_EXPRESSION_KINDS.length > 0)
    assert.equal(new Set(rig.AUTHORED_EXPRESSION_KINDS).size, rig.AUTHORED_EXPRESSION_KINDS.length)
  })

  test('figure contract: every face preset reaches a driver at a known tier', () => {
    assert.ok(Object.keys(workbench.FACE_PRESETS).length > 0)
    for (const [name, preset] of Object.entries(workbench.FACE_PRESETS)) {
      for (const [control, tier] of Object.entries(preset.targets)) {
        assert.ok(control in workbench.BODY_CONTROL_DRIVERS, `${name}: ${control} drives nothing`)
        const extent = workbench.tierExtent(control as never, tier)
        assert.ok(extent !== null && Number.isFinite(extent), `${name}: ${control} at ${tier}`)
      }
    }
    assert.equal(workbench.tierExtent('headTurn', 'strong'), 1)
    assert.equal(workbench.tierExtent('headTurn', '-strong'), -1)
    assert.equal(workbench.tierExtent('headTurn', 'nope'), null)
    assert.equal(workbench.mappedControlValue('gazeVertical', 0.5), -0.5)
    assert.equal(workbench.mappedControlValue('headTurn', 0.4), 0.4)
  })

  test('figure contract: a pose correction patch is held to the bounds a package accepts', () => {
    assert.deepEqual(
      workbench.clampPoseCorrectionPatch({ x: 9, y: -9, radiusX: 0, radiusY: 5, dx: 1, dy: -1 }),
      { x: 2, y: -2, radiusX: 0.1, radiusY: 2, dx: 0.25, dy: -0.25 },
    )
    const inside = { x: 0.5, y: -0.25, radiusX: 0.4, radiusY: 0.3, dx: 0.1, dy: -0.05 }
    assert.deepEqual(workbench.clampPoseCorrectionPatch(inside), inside)
    assert.equal(workbench.countPoseCorrectionChanges([], []), 0)
  })

  test('figure contract: music features follow loudness and where the sound sits', () => {
    const features = () => ({ energy: 0, bass: 0, pulse: 0, presence: 0 })
    const silent = tone(100, 0)
    const quiet = singing.analyzeMotionAudio(silent.waveform, silent.decibels.fill(-120), silent.sampleRate, features())
    for (const value of Object.values(quiet)) assert.ok(value >= 0 && value < 1e-3, 'silence moves nothing')
    const low = tone(100, 0.3)
    const bass = singing.analyzeMotionAudio(low.waveform, low.decibels, low.sampleRate, features())
    const high = tone(2_000, 0.3)
    const voice = singing.analyzeMotionAudio(high.waveform, high.decibels, high.sampleRate, features())
    for (const value of [...Object.values(bass), ...Object.values(voice)]) assert.ok(value >= 0 && value <= 1)
    assert.ok(bass.energy > 0 && voice.energy > 0)
    assert.ok(bass.bass > voice.bass, 'a low tone is bass')
    assert.ok(voice.presence > bass.presence, 'a high tone is presence')
    const output = features()
    assert.equal(singing.analyzeMotionAudio(low.waveform, low.decibels, low.sampleRate, output), output, 'it fills the object it is given')
    const louder = singing.analyzeMotionAudio(low.waveform, low.decibels, low.sampleRate, features(), 0.5)
    assert.ok(louder.energy >= bass.energy, 'a turned-down player still reads the recording')
  })

  test('figure contract: a touch appraisal asks once per revision, applies only a known reaction, and stops when disposed', async () => {
    let clock = 10_000
    const asked: unknown[] = []
    const applied: [number, string][] = []
    let reply: unknown = { reaction: 'accept' }
    const appraisal = new interaction.TouchAppraisal({
      now: () => clock,
      request: async (summary) => { asked.push(summary); return reply },
      apply: (revision, reaction) => applied.push([revision, reaction]),
    })
    const held = touch({ phase: 'update', gesture: 'hold', durationMs: 900, repeatCount: 20 })
    appraisal.observe(touch({ phase: 'update', gesture: 'hold', durationMs: 100 }), 1)
    assert.equal(asked.length, 0, 'too short to ask about')
    appraisal.observe(held, 1)
    appraisal.observe(held, 1)
    await settle()
    assert.deepEqual(asked, [{ region: 'hair', gesture: 'hold', durationMs: 900, repeatCount: 8 }])
    assert.deepEqual(applied, [[1, 'accept']])
    clock += 6_000
    reply = { reaction: 'dance' }
    appraisal.observe(held, 2)
    await settle()
    assert.equal(asked.length, 2)
    assert.equal(applied.length, 1, 'an unknown reaction is not applied')
    appraisal.dispose()
    clock += 6_000
    appraisal.observe(held, 3)
    await settle()
    assert.equal(asked.length, 2)
  })

  test('figure contract: a touch encounter reports one completed long contact, not a short one', (t) => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const published: unknown[] = []
    const encounter = new interaction.TouchEncounter(summary => published.push(summary), () => 100_000)
    encounter.observe(touch({ id: 1, phase: 'start', gesture: 'contact', durationMs: 0 }))
    encounter.observe(touch({ id: 1, phase: 'end', gesture: 'hold', durationMs: 1_500 }), 'notice')
    t.mock.timers.tick(700)
    assert.deepEqual(published, [{ region: 'hair', displayedReaction: 'notice', gesture: 'hold', durationMs: 1_500, repeatCount: 1 }])
    const short: unknown[] = []
    const brief = new interaction.TouchEncounter(summary => short.push(summary), () => 100_000)
    brief.observe(touch({ id: 2, phase: 'start', gesture: 'contact', durationMs: 0 }))
    brief.observe(touch({ id: 2, phase: 'end', gesture: 'hold', durationMs: 300 }))
    brief.observe(touch({ id: 3, phase: 'start', gesture: 'contact', durationMs: 0 }))
    brief.observe(touch({ id: 3, phase: 'end', gesture: 'tap', durationMs: 80 }))
    t.mock.timers.tick(700)
    assert.deepEqual(short, [], 'a brief hold and a tap are not an encounter')
  })

  lifecycleContract(figure)
}
