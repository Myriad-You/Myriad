import type { BehaviorQuality } from '../motion/behavior'
import type { MusicMode, MusicMotionSignal } from './musicSignal'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from '../anime25drig/driver'
import { PoseResponseController } from '../anime25drig/poseResponse'
import { MUSIC_QUALITY } from '../motion/musicReaction'
import { musicSignalAt } from './musicSignal.test-support'
import {
  ENTRY_HELD_BACK_SHARE,
  ENTRY_OFF_TEMPO_BEYOND,
  MAX_PHASE_CATCHUP,
  MIN_SINGING_NOD_INTERVAL_SECONDS,
  SingingGrooveController,
  singingNodBeatStride,
} from './singingGroove'

function play(
  options: {
    fps?: number
    duration?: number
    track?: string
    signal?: (time: number) => MusicMotionSignal | null
    mode?: (time: number) => MusicMode
    quality?: Readonly<BehaviorQuality>
  } = {},
) {
  const fps = options.fps ?? 60
  const controller = new SingingGrooveController()
  controller.setTrack(options.track ?? 'music-test')
  controller.setArmMotion(true)
  const response = new PoseResponseController()
  const current = { ...IDENTITY_DRIVER }
  const samples = []
  for (let frame = 0; frame <= (options.duration ?? 30) * fps; frame++) {
    const t = frame / fps
    const mode = options.mode?.(t) ?? 'listen'
    const signal = options.signal ? options.signal(t) : musicSignalAt(t)
    const raw = {
      ...controller.sample(
        t,
        true,
        signal,
        options.quality ?? MUSIC_QUALITY[mode],
        mode,
      ),
    }
    response.step(current, { ...IDENTITY_DRIVER, ...raw }, frame ? 1 / fps : 0)
    samples.push({ t, ...current, raw })
  }
  return samples
}

test('full-body listening spans several beats with smooth shoulder and arm crossings', () => {
  for (const bpm of [60, 100, 120, 180, 200]) {
    const samples = play({
      fps: 120,
      duration: 40,
      signal: (t) => musicSignalAt(t, { bpm }),
    }).slice(600)
    for (const [key, minimumRange] of [
      ['body', 1],
      ['armPos', 0.55],
    ] as const) {
      const values = samples.map((s) => s.raw[key])
      assert.ok(
        Math.max(...values) - Math.min(...values) > minimumRange,
        `${bpm} ${key} excursion`,
      )
    }
    for (const key of ['body', 'armY', 'armPos'] as const) {
      let acceleration = 0
      for (let i = 2; i < samples.length; i++) {
        assert.ok(Math.abs(samples[i].raw[key]) < 1.2)
        acceleration = Math.max(
          acceleration,
          Math.abs(
            samples[i].raw[key] -
              2 * samples[i - 1].raw[key] +
              samples[i - 2].raw[key],
          ) *
            120 ** 2,
        )
      }
      assert.ok(acceleration < 22, `${bpm} ${key} acceleration ${acceleration}`)
    }
  }
})

test('changing motif mid-transition carries its velocity and acceleration', () => {
  const controller = new SingingGrooveController()
  const state = controller as unknown as {
    chooseMotif: (now: number, bpm: number) => void
    motifMotion: Record<
      string,
      {
        sample: (now: number) => number
        value: number
        velocity: number
        acceleration: number
      }
    >
  }
  state.chooseMotif(0, 100)
  for (const time of [0.3, 0.5, 0.8]) {
    const before = Object.fromEntries(
      Object.entries(state.motifMotion).map(([key, motion]) => {
        motion.sample(time)
        return [key, [motion.value, motion.velocity, motion.acceleration]]
      }),
    )
    state.chooseMotif(time, 180)
    for (const [key, motion] of Object.entries(state.motifMotion)) {
      motion.sample(time)
      for (const [i, value] of [
        motion.value,
        motion.velocity,
        motion.acceleration,
      ].entries()) {
        assert.ok(
          Math.abs(value - before[key][i]) < 1e-9,
          `${key} derivative ${i}`,
        )
}
    }
  }
})

test('drum envelopes and repeated tempo corrections do not jerk the filtered head', () => {
  for (const scenario of ['pulses', 'tempo'] as const) {
    const samples = play({
      fps: 120,
      duration: 30,
      mode: () => 'sing',
      signal: (t) =>
        musicSignalAt(t, {
          bpm: scenario === 'tempo' ? (Math.floor(t / 3) % 2 ? 120 : 100) : 120,
          audio: {
            energy: scenario === 'pulses' ? (t % 0.5 < 0.1 ? 0.65 : 0.12) : 0.6,
            bass: 0.5,
            pulse: 0.6,
            presence: 0.5,
          },
        }),
    })
    for (const key of ['angleX', 'angleY', 'angleZ'] as const) {
      let maxJerk = 0
      for (let i = 361; i < samples.length; i++) {
        const jerk =
          (samples[i][key] -
            3 * samples[i - 1][key] +
            3 * samples[i - 2][key] -
            samples[i - 3][key]) *
          120 ** 3
        maxJerk = Math.max(maxJerk, Math.abs(jerk))
      }
      assert.ok(maxJerk < 180, `${scenario} ${key}: jerk ${maxJerk}`)
    }
    const settled = samples.slice(600)
    const range =
      Math.max(...settled.map((s) => s.angleZ)) -
      Math.min(...settled.map((s) => s.angleZ))
    assert.ok(
      range > 0.9,
      `${scenario}: cannot pass smoothness by shrinking the performance (${range})`,
    )
  }
})

test('fast music plans a full preparation ahead of the selected metrical beat', () => {
  for (const bpm of [120, 180, 200]) {
    const controller = new SingingGrooveController()
    controller.setTrack('music-test')
    const internals = controller as unknown as {
      lastNodAt: number
      nodReleaseAt: number
      amplitude: number
    }
    let previous = -Infinity
    let count = 0
    for (let f = 0; f < 30 * 60; f++) {
      const t = f / 60
      controller.sample(
        t,
        true,
        musicSignalAt(t, { bpm }),
        MUSIC_QUALITY.sing,
        'sing',
      )
      if (internals.lastNodAt === previous) continue
      previous = internals.lastNodAt
      const preparation = internals.nodReleaseAt - t
      assert.ok(
        preparation >= 0.32 && preparation <= 0.46,
        `${bpm}: ${preparation}`,
      )
      const arrival = internals.nodReleaseAt + 0.045
      const position = (arrival * bpm) / 60
      assert.ok(Math.abs(position - Math.round(position)) < 1e-6)
      count++
    }
    assert.ok(count >= 3, `${bpm}: no longer follows music (${count})`)
  }
})

test('head accents use a slower metrical level at all supported tempi', () => {
  for (const bpm of [60, 72, 86, 100, 120, 150, 180, 200]) {
    assert.ok(
      (60 / bpm) * singingNodBeatStride(bpm) >=
        MIN_SINGING_NOD_INTERVAL_SECONDS,
    )
  }
})

test('body and head retain visible range without allocating a face reaction', () => {
  const samples = play().slice(180)
  const span = (key: 'body' | 'angleZ' | 'angleX') =>
    Math.max(...samples.map((s) => s[key])) -
    Math.min(...samples.map((s) => s[key]))
  assert.ok(span('body') > 0.8)
  assert.ok(span('angleZ') > 0.9)
  assert.ok(span('angleX') > 0.3)
  for (const sample of samples) {
    assert.equal(sample.raw.eyeX, 0)
    assert.equal(sample.raw.brow, 0)
    assert.ok(sample.angleY > -0.21 && sample.angleY < 0.18)
    assert.ok(Math.abs(sample.angleZ) < 1)
  }
})

test('sway responds to music tempo, not only amplitude or an independent timer', () => {
  const slow = play({ signal: (t) => musicSignalAt(t, { bpm: 80 }) })
  const fast = play({ signal: (t) => musicSignalAt(t, { bpm: 110 }) })
  const difference =
    slow
      .slice(180)
      .reduce((sum, s, i) => sum + Math.abs(s.body - fast[i + 180].body), 0) /
    (slow.length - 180)
  assert.ok(difference > 0.15, `tempo has no effect: ${difference}`)
})

test('director extent reaches the actual body, not only the planned quality', () => {
  const compact = play({ quality: { ...MUSIC_QUALITY.listen, extent: 0.8 } })
  const expansive = play({ quality: { ...MUSIC_QUALITY.listen, extent: 1.3 } })
  const range = (samples: ReturnType<typeof play>) =>
    Math.max(...samples.map((s) => s.body)) -
    Math.min(...samples.map((s) => s.body))
  assert.ok(range(expansive) > range(compact) * 1.5)
})

test('measured silence settles but unavailable audio remains quiet listening', () => {
  const silent = play({
    duration: 15,
    signal: (t) =>
      musicSignalAt(
        t,
        t > 7
          ? {
              audio: { energy: 0, bass: 0, pulse: 0, presence: 0 },
              bpm: 0,
            }
          : {},
      ),
  })
  const unknown = play({
    duration: 15,
    signal: (t) => musicSignalAt(t, { audio: null, bpm: 0 }),
  })
  assert.ok(
    Math.max(...silent.slice(11 * 60).map((s) => Math.abs(s.body))) < 0.001,
  )
  assert.ok(
    Math.max(...unknown.slice(11 * 60).map((s) => Math.abs(s.body))) > 0.003,
  )
})

test('stale audio evidence fades instead of predicting forever', () => {
  const frozen = musicSignalAt(5)
  const samples = play({
    duration: 10,
    signal: (t) => (t < 5 ? musicSignalAt(t) : frozen),
  })
  assert.ok(Math.abs(samples.at(-1)!.body) < 0.001)
})

test('phrase preparation starts before the vocal and releases after it', () => {
  const phrase = { start: 2, end: 4, confidence: 0.95 }
  const sung = play({
    duration: 6,
    mode: () => 'sing',
    signal: (t) => musicSignalAt(t, { phrase }),
  })
  const listening = play({ duration: 6 })
  assert.ok(
    sung[Math.round(1.96 * 60)].angleY >
      listening[Math.round(1.96 * 60)].angleY + 0.02,
  )
  assert.ok(sung[4 * 60].raw.armY > listening[4 * 60].raw.armY + 0.1)
  assert.ok(sung[5 * 60].angleY < sung[4 * 60].angleY)
})

test('actual head accents are sparse, modest, and anticipate the audible pulse', () => {
  const samples = play({ duration: 60 })
  const peaks: number[] = []
  for (let i = 181; i < samples.length - 1; i++) {
    const s = samples[i]
    if (
      s.angleY < -0.05 &&
      s.angleY < samples[i - 1].angleY &&
      s.angleY <= samples[i + 1].angleY
    ) {
      peaks.push(s.t)
    }
  }
  assert.ok(peaks.length >= 5 && peaks.length < 26, `nod count ${peaks.length}`)
  for (let i = 1; i < peaks.length; i++) assert.ok(peaks[i] - peaks[i - 1] > 1)
  const errors = peaks.map((t) => Math.abs(t - Math.round(t * 2) / 2))
  assert.ok(
    Math.max(...errors) < 0.14,
    `actual nod phase error ${Math.max(...errors)}`,
  )
})

test('silence, resuming and mode replacement preserve actual pose continuity', () => {
  const samples = play({
    duration: 18,
    mode: (t) => (t < 6 ? 'sing' : t < 10 ? 'settle' : 'listen'),
  })
  let maxStep = 0
  for (let i = 1; i < samples.length; i++) {
    for (const key of ['angleX', 'angleY', 'angleZ', 'body'] as const) {
      maxStep = Math.max(
        maxStep,
        Math.abs(samples[i][key] - samples[i - 1][key]),
      )
    }
  }
  assert.ok(maxStep < 0.07, `pose step ${maxStep}`)
  assert.ok(Math.abs(samples[9 * 60].body) < 0.005)
  assert.ok(
    Math.abs(samples[6 * 60].body) > 0.01,
    'does not reset to origin on handoff',
  )
})

test('track replacement keeps the physical pose rather than resetting the oscillator', () => {
  const controller = new SingingGrooveController()
  let pose = { ...controller.sample(0, true, musicSignalAt(0)) }
  for (let i = 1; i <= 151; i++)
    pose = { ...controller.sample(i / 60, true, musicSignalAt(i / 60)) }
  controller.setTrack('other-song')
  const next = controller.sample(152 / 60, true, musicSignalAt(0, { bpm: 80 }))
  assert.ok(Math.abs(next.body - pose.body) < 0.05)
  assert.ok(Math.abs(next.angleZ - pose.angleZ) < 0.05)
})

test('30, 60, and 120 fps keep the same slow musical movement', () => {
  const sample = (t: number) =>
    musicSignalAt(Math.floor((t + 1e-9) / 0.025) * 0.025)
  const reference = play({ fps: 120, duration: 24, signal: sample })
  for (const fps of [30, 60]) {
    const actual = play({ fps, duration: 24, signal: sample })
    const error =
      actual.reduce(
        (sum, s, i) => sum + Math.abs(s.body - reference[(i * 120) / fps].body),
        0,
      ) / actual.length
    assert.ok(error < 0.045, `${fps} fps differs ${error}`)
  }
})

test('a participation change moves the pose no faster than the groove itself', () => {
  const groove = new SingingGrooveController()
  const at = (time: number): MusicMotionSignal => musicSignalAt(time)
  let time = 0
  const step = (quality: BehaviorQuality, mode: MusicMode) => {
    time += 1 / 60
    return { ...groove.sample(time, true, at(time), quality, mode) }
  }
  for (let frame = 0; frame < 180; frame += 1) {
    step(MUSIC_QUALITY.listen, 'listen')
  }

  const before = step(MUSIC_QUALITY.listen, 'listen')
  const ordinary = step(MUSIC_QUALITY.listen, 'listen')
  const walk = Math.max(
    ...(['angleX', 'angleZ', 'body'] as const).map((key) =>
      Math.abs(ordinary[key] - before[key]),
    ),
  )

  const changed = step(MUSIC_QUALITY.hum, 'hum')
  for (const key of ['angleX', 'angleZ', 'body'] as const) {
    const jump = Math.abs(changed[key] - ordinary[key])
    assert.ok(
      jump <= walk * 1.6,
      `${key} moved ${jump} on a mode change against an ordinary ${walk}`,
    )
  }
})

test('a participation change still arrives, it only stops stepping', () => {
  const groove = new SingingGrooveController()
  let time = 0
  const run = (frames: number, quality: BehaviorQuality, mode: MusicMode) => {
    let pose = groove.sample(time, true, musicSignalAt(time), quality, mode)
    for (let frame = 0; frame < frames; frame += 1) {
      time += 1 / 60
      pose = groove.sample(time, true, musicSignalAt(time), quality, mode)
    }
    return { ...pose }
  }
  run(180, MUSIC_QUALITY.listen, 'listen')
  const listening = run(1, MUSIC_QUALITY.listen, 'listen')
  const singing = run(30, MUSIC_QUALITY.sing, 'sing')
  assert.notDeepEqual(singing, listening)
})

test('losing the lock is not losing the music', () => {
  const BPM = 150
  const lockedPeriod = (60 * 8) / BPM
  const fallbackPeriod = 1 / 0.18

  const uncertain = (time: number) =>
    musicSignalAt(time, {
      bpm: BPM,
      confidence: time < 10 ? 0.6 : 0.2 + 0.05 * Math.sin(time * Math.PI * 2),
    })

  const samples = play({ duration: 50, signal: uncertain }).filter(
    (sample) => sample.t > 20,
  )
  const crossings: number[] = []
  for (let i = 1; i < samples.length; i += 1) {
    if (samples[i - 1]!.raw.angleZ <= 0 && samples[i]!.raw.angleZ > 0) {
      crossings.push(samples[i]!.t)
    }
  }
  assert.ok(crossings.length > 2, 'no roll oscillation to measure')
  const period = (crossings.at(-1)! - crossings[0]!) / (crossings.length - 1)

  assert.ok(
    Math.abs(period - lockedPeriod) < Math.abs(period - fallbackPeriod),
    `roll period ${period}s sits nearer the fallback ${fallbackPeriod}s than the tempo ${lockedPeriod}s it had agreed on`,
  )
})

test('catching the beat is a transition, not a jump', () => {
  const BPM = 150
  const controller = new SingingGrooveController()
  controller.setTrack('catch-up')

  let previousPhase: number | null = null
  let worst = 0
  for (let frame = 0; frame <= 30 * 60; frame += 1) {
    const time = frame / 60
    controller.sample(
      time,
      true,
      musicSignalAt(time, { bpm: BPM, confidence: time < 12 ? 0 : 0.9 }),
      MUSIC_QUALITY.listen,
      'listen',
    )
    const internals = controller as unknown as {
      phase: number
      frequency: number
    }
    if (previousPhase !== null && internals.frequency > 1e-6) {
      let advance = internals.phase - previousPhase
      if (advance < -0.5) advance += 1
      if (advance > 0.5) advance -= 1
      const speed = advance * 60
      const off = Math.abs(speed - internals.frequency) / internals.frequency
      if (time > 1 && off > worst) worst = off
    }
    previousPhase = internals.phase
  }

  assert.ok(
    worst <= MAX_PHASE_CATCHUP + 0.02,
    `the beat dragged the sway ${(worst * 100).toFixed(0)}% off its own speed`,
  )
})

test('a body still swaying keeps the tempo it was swaying to', () => {
  const BPM = 128
  const swayTarget = BPM / (60 * 8)
  const controller = new SingingGrooveController()
  controller.setTrack('leaving')
  const internals = controller as unknown as {
    amplitude: number
    frequency: number
  }

  for (let frame = 0; frame <= 10 * 60; frame += 1) {
    const time = frame / 60
    controller.sample(
      time,
      true,
      musicSignalAt(time, { bpm: BPM, confidence: 0.85 }),
      MUSIC_QUALITY.listen,
      'listen',
    )
  }
  assert.ok(internals.amplitude > 0.9, 'expected a settled sway to wind down')

  let heldWhileMoving = true
  for (let frame = 1; frame <= 45; frame += 1) {
    controller.sample(10 + frame / 60, false, null, undefined, 'listen')
    if (internals.amplitude > 0.2) {
      const off = Math.abs(internals.frequency - swayTarget) / swayTarget
      if (off > 0.05) heldWhileMoving = false
    }
  }
  assert.ok(
    heldWhileMoving,
    'the sway changed speed on the way out instead of finishing at its own tempo',
  )
})

test('the body joins in small until it has the timing', () => {
  const BPM = 128
  const swayTarget = BPM / (60 * 8)
  const controller = new SingingGrooveController()
  controller.setTrack('joining')
  const internals = controller as unknown as {
    amplitude: number
    frequency: number
  }

  for (let frame = 0; frame <= 3 * 60; frame += 1) {
    controller.sample(frame / 60, false, null, undefined, 'listen')
  }

  let whileOffTempo = 0
  let settled = 0
  for (let frame = 1; frame <= 8 * 60; frame += 1) {
    const time = 3 + frame / 60
    controller.sample(
      time,
      true,
      musicSignalAt(time, { bpm: BPM, confidence: 0.85 }),
      MUSIC_QUALITY.listen,
      'listen',
    )
    const off = Math.abs(internals.frequency - swayTarget) / swayTarget
    if (off > ENTRY_OFF_TEMPO_BEYOND) {
      whileOffTempo = Math.max(whileOffTempo, internals.amplitude)
    }
    if (time > 6) settled = internals.amplitude
  }

  assert.ok(
    whileOffTempo <= ENTRY_HELD_BACK_SHARE + 0.05,
    `swayed to ${whileOffTempo} while still off the tempo it had been given`,
  )
  assert.ok(settled > 0.9, `settled sway is only ${settled}`)
})

test('a fast drum pattern is not a reason to nod faster', () => {
  const nodsOver = (onsetEverySeconds: number): number => {
    const controller = new SingingGrooveController()
    controller.setTrack(`onsets-${onsetEverySeconds}`)
    const internals = controller as unknown as { lastNodAt: number }
    let previous = internals.lastNodAt
    let nods = 0
    for (let frame = 1; frame <= 90 * 60; frame += 1) {
      const time = frame / 60
      const phase = time % onsetEverySeconds
      controller.sample(
        time,
        true,
        {
          ...musicSignalAt(time, { bpm: 0, confidence: 0 }),
          audio: { energy: 0.7, bass: 0.6, pulse: 0.6, presence: 0.4 },
          beatFrame: {
            bpm: 0,
            confidence: 0,
            beatCount: 0,
            beatPhase: 0,
            beatCrossed: false,
            onset: phase < 1 / 60,
          },
        },
        MUSIC_QUALITY.listen,
        'listen',
      )
      if (internals.lastNodAt !== previous) {
        previous = internals.lastNodAt
        nods += 1
      }
    }
    return nods
  }

  const dense = nodsOver(0.15)
  const spaced = nodsOver(0.6)
  assert.ok(spaced > 0, 'beat-spaced onsets should still earn accents')
  assert.ok(
    dense <= spaced,
    `a hi-hat pattern earned ${dense} nods against ${spaced} for beat-spaced hits`,
  )
})

test('an accent placed without a lock is a smaller one', () => {
  const peakDip = (confidence: number): number => {
    const controller = new SingingGrooveController()
    controller.setTrack(`accent-${confidence}`)
    let deepest = 0
    for (let frame = 1; frame <= 40 * 60; frame += 1) {
      const time = frame / 60
      const pose = controller.sample(
        time,
        true,
        {
          ...musicSignalAt(time, { bpm: 120, confidence }),
          audio: { energy: 0.7, bass: 0.6, pulse: 0.6, presence: 0.4 },
        },
        MUSIC_QUALITY.listen,
        'listen',
      )
      deepest = Math.min(deepest, pose.angleY)
    }
    return deepest
  }

  const known = peakDip(0.9)
  const guessed = peakDip(0.2)
  assert.ok(known < -0.01, 'a confident beat should still earn a real accent')
  assert.ok(
    guessed > known * 0.75,
    `a guessed accent dipped to ${guessed} against ${known} for a known beat`,
  )
})
