import type * as Anime25drig from '../src/anime25drig'
import type * as Events from '../src/events'
import type * as Root from '../src/index'
import type * as Motion from '../src/motion'
import type * as Performance from '../src/performance'
import type * as Rig from '../src/rig'
import type * as Speech from '../src/speech'
import assert from 'node:assert/strict'
import test from 'node:test'

/** The entries of one @myriad/figure implementation, as the interface declares them. */
export interface FigureEntries {
  root: typeof Root
  anime25drig: typeof Anime25drig
  events: typeof Events
  motion: typeof Motion
  performance: typeof Performance
  rig: typeof Rig
  speech: typeof Speech
}

function state(revision: number, after = 60, band = 'calm') {
  return {
  mood: { before: after, after, arousalBefore: 40, arousalAfter: 40, bandBefore: band, bandAfter: band, delta: 0, cause: 'test', revision },
  activity: 'reading',
}
}

function directive(cues: unknown[]) {
  return {
  phase: 'delivery',
  moodRevision: 3,
  motionStyle: 'even',
  plan: { cues },
}
}

function cue(intent: string, extra: Record<string, unknown> = {}) {
  return {
  intent,
  atMs: 0,
  intensity: 1,
  tempo: 1,
  fadeInMs: 120,
  fadeOutMs: 200,
  interrupt: 'queue',
  ...extra,
}
}

/**
 * Behaviour the site and the server rely on through the interface, the same in every
 * implementation: the wire vocabulary and its validation, the channel's turn and state
 * bookkeeping, speech events and mouth timelines, and when the page plays the figure.
 * An implementation's own tests call this once with its entries.
 */
export function figureContract(figure: FigureEntries): void {
  const { root, anime25drig, events, motion, performance, rig, speech } = figure

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

  test('figure contract: rig validators turn away what is not a rig', () => {
    for (const value of [null, undefined, 1, 'rig', [], {}]) {
      assert.equal(rig.isRigManifest(value), false)
      assert.equal(rig.isLiveMeropeManifest(value), false)
      assert.equal(rig.isDocumentTurnKeyforms(value), false)
    }
    assert.ok(rig.AUTHORED_EXPRESSION_KINDS.length > 0)
    assert.equal(new Set(rig.AUTHORED_EXPRESSION_KINDS).size, rig.AUTHORED_EXPRESSION_KINDS.length)
  })
}
