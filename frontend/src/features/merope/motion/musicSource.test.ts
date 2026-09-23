import type {
  MusicMotionAudio,
  MusicMotionClock,
  MusicMotionVisibility,
} from './musicSource'
import assert from 'node:assert/strict'
import test from 'node:test'
import { AmbientMotionController } from '../anime25drig/ambientMotion'
import { Anime25DBehaviorMotionController } from '../anime25drig/behaviorMotion'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import {
  PoseGateController,
  resolvePoseGate,
} from '../anime25drig/poseArbitration'
import { PoseOccupancyController } from '../anime25drig/poseOccupancy'
import { RigMotionCoordinator } from './coordinator'
import { HumanPerformanceRuntime } from './humanPerformanceRuntime'
import {
  MUSIC_LEASE_TTL_MS,
  MusicMotionSource,
  TRACK_SWITCH_HOLD_MS,
} from './musicSource'

function fakeClock(): MusicMotionClock & {
  time: number
  frames: Array<(t: number) => void>
} {
  const frames: Array<(t: number) => void> = []
  return {
    time: 0,
    frames,
    now: () => 0,
    raf: (callback) => {
      frames.push(callback)
      return frames.length
    },
    caf: () => {
      frames.length = 0
    },
  }
}

function silentAudio(paused = false): MusicMotionAudio {
  return {
    getCurrentAudio: () => ({ paused, currentTime: 1.2 }),
    getMotionAudioFeatures: () => ({
      energy: 0.6,
      bass: 0.4,
      pulse: 0.5,
      presence: 0.3,
    }),
    connectAudioToAnalyser: () => true,
  }
}

const visible: MusicMotionVisibility = {
  isPageVisible: () => true,
  onVisibility: () => () => {},
}

test('media end releases music and restores moving idle even when playback UI remains playing', () => {
  const coordinator = new RigMotionCoordinator()
  const audio = { paused: false, ended: false, currentTime: 0 }
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    {
      ...silentAudio(),
      getCurrentAudio: () => audio,
    },
    visible,
  )
  source.setPlayback(true, false)
  let seed = 0x12345678
  const ambient = new AmbientMotionController(() => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    return seed / 0x100000000
  })
  const occupancy = new PoseOccupancyController()
  const gate = new PoseGateController()
  let low = Infinity
  let high = -Infinity
  for (let i = 0; i <= 60 * 20; i++) {
    const t = i / 60
    audio.currentTime = Math.min(t, 5)
    audio.ended = t >= 5
    audio.paused = audio.ended
    const frame = source.sampleNow(t * 1000)
    const policy = coordinator.snapshot(t * 1000).owners
    const pose = ambient.sample(t, true)
    const weights = gate.sample(
      1 / 60,
      resolvePoseGate(
        policy,
        occupancy.sample(1 / 60, {
          singing: frame.apply.writeGroove,
          speaking: false,
          thinking: false,
          pointerDriven: false,
          automation: true,
          sticker: 0,
        }),
        { performance: 1, stylized: 1, randomAmbient: 1 },
      ),
    )
    if (t >= 5) {
      assert.equal(frame.apply.release, true)
      assert.equal(frame.behaviorPlan, null)
      assert.equal(policy.headBody, 'idle')
    }
    if (t >= 6) {
      assert.ok(weights.ambient.headBody > 0.98)
      assert.ok(weights.random.headBody > 0.98)
      const value = pose.angleX * weights.ambient.headBody
      low = Math.min(low, value)
      high = Math.max(high, value)
    }
  }
  // This seed includes small inspections: restoration must not force a new
  // exaggerated gesture, but its normal idle motion must remain visible.
  assert.ok(high - low > 0.1, `post-song observation range ${high - low}`)
  audio.ended = false
  audio.paused = false
  audio.currentTime = 0
  assert.equal(
    source.sampleNow(21000).apply.writeGroove,
    true,
    'next song resumes without toggling the UI flag',
  )
})

test('phrase participation reaches the shared scheduler and body without restarting the lifecycle', async () => {
  const audio = { paused: false, currentTime: 1 }
  let energy = 0.6
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    {
      getCurrentAudio: () => audio,
      connectAudioToAnalyser: () => true,
      getMotionAudioFeatures: () => ({
        energy,
        bass: energy,
        pulse: energy,
        presence: energy,
      }),
    },
    visible,
    async () => [{ start: 2, end: 3, viseme: 'open', emphasis: false }],
  )
  source.setTrack({
    trackId: 'phrase-track',
    verbatim: [
      {
        time: 2,
        text: 'hello',
        words: [{ time: 2, duration: 1, text: 'hello' }],
      },
    ],
  })
  source.setPlayback(true, false)
  await Promise.resolve()
  const runtime = new HumanPerformanceRuntime()
  const body = new Anime25DBehaviorMotionController()
  let behaviorId = ''
  let startPeg = 0
  for (const [time, expected] of [
    [1, 'listen'],
    [1.8, 'sing'],
    [2.5, 'sing'],
    [3.5, 'listen'],
    [4, 'listen'],
    [4.4, 'settle'],
    [5, 'listen'],
  ] as const) {
    audio.currentTime = time
    energy = time >= 4 && time < 5 ? 0 : 0.6
    const frame = source.sampleNow(time * 1000)
    const scheduled = runtime.frame([frame.behaviorPlan], time * 1000)
    assert.ok(scheduled.plan)
    const realized = realizeAnime25DBehaviorPlan(scheduled.plan, time * 1000)
    assert.equal(realized.reports[0].result, 'accepted')
    const unit = realized.units[0]
    if (!behaviorId) {
      behaviorId = unit.behaviorId
      startPeg = unit.timing.startMs
    }
    assert.equal(unit.behaviorId, behaviorId)
    assert.equal(unit.timing.startMs, startPeg)
    body.replace(realized.units, time * 1000, time)
    assert.equal(body.sample(time + 0.3).musicMode, expected)
    if (expected === 'listen' || expected === 'settle')
      assert.equal(frame.articulation.viseme, 'rest')
  }
})

test('zero energy is published as measured silence, not absent analysis', () => {
  const audio = { paused: false, currentTime: 1 }
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    {
      getCurrentAudio: () => audio,
      connectAudioToAnalyser: () => true,
      getMotionAudioFeatures: () => ({
        energy: 0,
        bass: 0,
        pulse: 0,
        presence: 0,
      }),
    },
    visible,
  )
  source.setPlayback(true, false)
  source.sampleNow(1000)
  audio.currentTime = 1.4
  const frame = source.sampleNow(1400)
  assert.equal(frame.signal?.audio?.energy, 0)
  assert.equal(frame.behaviorPlan?.behaviors[0].form.id, 'settle')
  assert.equal(frame.articulation.viseme, 'rest')
})

test('one sampler fans out to every mounted rig', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  const seen: number[] = []
  const first = source.subscribe(() => {
    seen.push(1)
  })
  const second = source.subscribe(() => {
    seen.push(2)
  })
  assert.equal(source.listenerCount(), 2)
  source.sampleNow(80)
  assert.ok(seen.includes(1))
  assert.ok(seen.includes(2))
  first()
  second()
  assert.equal(source.listenerCount(), 0)
})

test('frames carry the media identity across a fast track switch', () => {
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  source.setTrack({ trackId: 'netease:song-a' })
  assert.equal(source.sampleNow(10).trackId, 'netease:song-a')

  source.setTrack({ trackId: 'qq:song-b' })
  assert.equal(source.sampleNow(20).trackId, 'qq:song-b')
})

test('duplicate face bindings compile one semantic lyric timeline', async () => {
  let compileCount = 0
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    silentAudio(),
    visible,
    async () => {
      compileCount += 1
      return []
    },
  )

  source.setTrack({
    trackId: 'netease:song-a',
    duration: 120,
    lines: [{ time: 0, text: 'first line' }],
  })
  source.setTrack({
    trackId: 'netease:song-a',
    duration: 120,
    lines: [{ time: 0, text: 'first line', translation: 'ignored' }],
  })
  await Promise.resolve()
  assert.equal(compileCount, 1)

  source.setTrack({
    trackId: 'netease:song-a',
    duration: 120,
    lines: [{ time: 0, text: 'changed line' }],
  })
  await Promise.resolve()
  assert.equal(compileCount, 2)
})

test('a duplicate face binding cannot cancel an active track-switch hold', () => {
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(false, false)
  source.markSwitching()

  // It must not overwrite the source's newer, internal switching state.
  source.setPlayback(false, false)
  const frame = source.sampleNow(10)
  assert.equal(frame.apply.release, false)
  assert.equal(frame.apply.writeGroove, true)
})

test('playing music claims mouth and body until speech takes the mouth', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  const frame = source.sampleNow(80)
  assert.equal(coordinator.owner('mouth', 80), 'music')
  assert.equal(coordinator.owner('headBody', 80), 'music')
  assert.equal(coordinator.owner('gaze', 80), 'idle')
  assert.equal(coordinator.owner('expression', 80), 'idle')
  assert.equal(frame.behaviorPlan?.behaviors[0]?.function, 'entrain')
  assert.equal(frame.behaviorPlan?.behaviors[0]?.kind, 'rhythmic')
  assert.equal(frame.apply.writeMouth, true)
  assert.equal(frame.apply.writeGroove, true)

  coordinator.claim('speech', ['mouth'], { nowMs: 90 })
  const yielded = source.sampleNow(90)
  assert.equal(yielded.apply.writeMouth, false)
  assert.equal(yielded.apply.writeGroove, true)
  assert.equal(coordinator.owner('headBody', 90), 'music')

  const takeover = coordinator.claim(
    'performance',
    ['headBody', 'expression'],
    { nowMs: 100 },
  )
  const shared = source.sampleNow(100)
  assert.equal(coordinator.owner('headBody', 100), 'performance')
  assert.equal(shared.apply.writeGroove, true)
  assert.ok(shared.signal, 'temporary ownership must not cut audio evidence')
  assert.equal(shared.apply.writeMouth, false, 'speech keeps its articulation')
  assert.equal(shared.behaviorPlan?.id, frame.behaviorPlan?.id)
  coordinator.release(takeover)
  assert.equal(source.sampleNow(110).apply.writeGroove, true)
  assert.equal(coordinator.owner('headBody', 110), 'music')
})

test('stopping music withdraws its candidate plan for global recovery', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  source.sampleNow(100)
  source.setPlayback(false, false)
  const stopped = source.sampleNow(200)
  assert.equal(stopped.behaviorPlan, null)
})

test('pause rests the mouth without dropping the music lease', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(true),
    visible,
  )
  source.setPlayback(true, false)
  const frame = source.sampleNow(80)
  assert.equal(frame.apply.restMouth, true)
  assert.equal(frame.apply.writeGroove, true)
  assert.equal(frame.apply.release, false)
  assert.equal(coordinator.owner('headBody', 80), 'music')
})

test('hiding the page releases music so a later sample can reclaim it', () => {
  const coordinator = new RigMotionCoordinator()
  let pageVisible = true
  const listeners = new Set<(visible: boolean) => void>()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    {
      isPageVisible: () => pageVisible,
      onVisibility: (callback) => {
        listeners.add(callback)
        return () => listeners.delete(callback)
      },
    },
  )
  source.setPlayback(true, false)
  source.subscribe(() => {})
  source.sampleNow(80)
  assert.equal(coordinator.owner('headBody', 80), 'music')
  pageVisible = false
  for (const listener of listeners) listener(false)
  assert.equal(coordinator.owner('headBody', 80), 'idle')
})

test('a track-switch hold then expires and drops the music lease', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  source.sampleNow(80)
  assert.equal(coordinator.owner('headBody', 80), 'music')
  source.setPlayback(false, true)
  const held = source.sampleNow(90)
  assert.equal(held.apply.release, false)
  assert.equal(held.apply.writeGroove, true)
  const expired = source.sampleNow(90 + TRACK_SWITCH_HOLD_MS)
  assert.equal(expired.apply.release, true)
  assert.equal(coordinator.owner('headBody', 90 + TRACK_SWITCH_HOLD_MS), 'idle')
})

test('music renews one lease instead of stacking a new claim every sample', () => {
  const coordinator = new RigMotionCoordinator()
  const source = new MusicMotionSource(
    coordinator,
    fakeClock(),
    silentAudio(),
    visible,
  )
  source.setPlayback(true, false)
  source.sampleNow(80)
  source.sampleNow(80 + MUSIC_LEASE_TTL_MS - 10)
  const leases = coordinator
    .snapshot(80 + MUSIC_LEASE_TTL_MS - 10)
    .leases.filter((lease) => lease.source === 'music')
  assert.equal(leases.length, 1)
})

test('reconnects the analyser when the player swaps its audio element', () => {
  const connected: object[] = []
  let current: { paused: boolean; currentTime: number } = {
    paused: false,
    currentTime: 1.2,
  }
  const audio: MusicMotionAudio = {
    getCurrentAudio: () => current,
    getMotionAudioFeatures: () => ({
      energy: 0.6,
      bass: 0.4,
      pulse: 0.5,
      presence: 0.3,
    }),
    connectAudioToAnalyser: (element) => {
      connected.push(element)
      return true
    },
  }
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    audio,
    visible,
  )
  source.setPlayback(true, false)
  source.sampleNow(10)
  source.sampleNow(20)
  assert.equal(connected.length, 1)

  current = { paused: false, currentTime: 0 }
  source.sampleNow(30)
  assert.equal(connected.length, 2)
  assert.equal(connected[1], current)
})

test('leaves a paused element unwired so no AudioContext starts before a gesture', () => {
  const connected: object[] = []
  const current = { paused: true, currentTime: 0 }
  const audio: MusicMotionAudio = {
    getCurrentAudio: () => current,
    getMotionAudioFeatures: () => ({
      energy: 0,
      bass: 0,
      pulse: 0,
      presence: 0,
    }),
    connectAudioToAnalyser: (element) => {
      connected.push(element)
      return true
    },
  }
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    audio,
    visible,
  )
  source.sampleNow(10)
  assert.equal(connected.length, 0)

  current.paused = false
  source.setPlayback(true, false)
  source.sampleNow(20)
  assert.equal(connected.length, 1)
})

test('publishes the next audio-clock beat as a mutable anticipator peg', () => {
  let currentTime = 0
  let bass = 0.05
  const audio: MusicMotionAudio = {
    getCurrentAudio: () => ({ paused: false, currentTime }),
    getMotionAudioFeatures: () => ({
      energy: 0.6,
      bass,
      pulse: bass,
      presence: 0.3,
    }),
    connectAudioToAnalyser: () => true,
  }
  const source = new MusicMotionSource(
    new RigMotionCoordinator(),
    fakeClock(),
    audio,
    visible,
  )
  source.setTrack({ trackId: 'steady-120bpm' })
  source.setPlayback(true, false)
  let latest = source.sampleNow(0)
  for (let beat = 0; beat < 6; beat += 1) {
    currentTime = beat * 0.5
    bass = 0.95
    latest = source.sampleNow(currentTime * 1_000)
    currentTime += 0.05
    bass = 0.05
    latest = source.sampleNow(currentTime * 1_000)
  }
  const entrainment = latest.behaviorPlan?.behaviors.find(
    (behavior) => behavior.function === 'entrain',
  )
  const anticipation = latest.behaviorPlan?.pegs.find(
    (peg) => peg.id === entrainment?.anticipation,
  )
  assert.ok((anticipation?.confidence ?? 0) > 0.8)
  assert.ok((anticipation?.atMs ?? 0) > currentTime * 1_000)
  assert.ok(
    (anticipation?.atMs ?? Number.POSITIVE_INFINITY) <=
      currentTime * 1_000 + 500,
  )
  assert.equal(latest.signal?.sampleTimeSeconds, currentTime)
})
