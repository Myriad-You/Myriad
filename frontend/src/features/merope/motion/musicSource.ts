import type { SpeechArticulation } from '../rig/articulation'
import type { BeatFrame } from '../singing/beatClock'
import type { SingingSpectrumDrive } from '../singing/singingGroove'
import type { SingingCue } from '../singing/singingTimeline'
import type { BehaviorPlan } from './behavior'
import type { MotionChannel } from './channels'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import type { SingingApply } from './singingApply'
import { BeatClock } from '../singing/beatClock'
import {
  restSingingArticulation,
  sampleSingingCue,
  singingArticulation,
  singingVocalEnergy,
} from '../singing/singingClock'
import { singingSpectrumDrive } from '../singing/singingGroove'
import { singingPlaybackGap } from '../singing/singingHold'
import { compileSingingTimeline } from '../singing/singingTimeline'
import { resolveSingingApply } from './singingApply'

export const SINGING_SAMPLE_INTERVAL_MS = 50
export const TRACK_SWITCH_HOLD_MS = 12_000
export const MUSIC_LEASE_TTL_MS = 250

/**
 * Music owns articulation and rhythmic head/body movement. Sparse facial and
 * gaze reactions belong to the reaction planner; a groove is not permanent
 * evidence of delight, agreement, or attention.
 */
const MUSIC_CHANNELS = [
  'mouth',
  'headBody',
] as const satisfies readonly MotionChannel[]

export interface SingingFrame {
  apply: SingingApply
  spectrum: SingingSpectrumDrive | null
  articulation: SpeechArticulation
  /** Stable media identity; a change invalidates tempo evidence immediately. */
  trackId: string | null
  behaviorPlan: BehaviorPlan | null
  /** Filled from the global scheduler by MotionRuntime. */
  behaviors: readonly import('./behavior').BehaviorSnapshot[]
}

export interface MusicMotionClock {
  now: () => number
  raf: (callback: (time: number) => void) => number
  caf: (id: number) => void
}

export interface MusicMotionAudio {
  getCurrentAudio: () => { paused: boolean; currentTime: number } | null
  getSpectrumBands: () => number[]
  connectAudioToAnalyser: (audio: { paused: boolean }) => boolean
}

export interface MusicMotionVisibility {
  isPageVisible: () => boolean
  onVisibility: (callback: (visible: boolean) => void) => () => void
}

export interface MusicTrackInput {
  trackId: string
  duration?: number
  verbatim?: Parameters<typeof compileSingingTimeline>[0]['verbatim']
  lines?: Parameters<typeof compileSingingTimeline>[0]['lines']
}

export type SingingFrameListener = (frame: SingingFrame) => void

type SingingTimelineCompiler = typeof compileSingingTimeline

function musicTrackInputFingerprint(track: MusicTrackInput): string {
  const verbatim =
    track.verbatim && track.verbatim.length > 0
      ? track.verbatim.map((line) =>
          (line.words ?? []).map((word) => [
            word.time,
            word.duration,
            word.text,
          ]),
        )
      : null
  if (verbatim) return JSON.stringify([track.trackId, 'verbatim', verbatim])
  const lines =
    track.lines && track.lines.length > 0
      ? track.lines.map((line) => [line.time, line.text])
      : null
  const duration =
    typeof track.duration === 'number' && Number.isFinite(track.duration)
      ? track.duration
      : null
  return JSON.stringify([track.trackId, 'lines', duration, lines])
}

/**
 * One sampler for every mounted face. Reuses the site audio analyser
 * (50ms cache) and publishes a channel-gated frame.
 */
export class MusicMotionSource {
  private readonly listeners = new Set<SingingFrameListener>()
  private readonly clock: MusicMotionClock
  private readonly audio: MusicMotionAudio
  private readonly visibility: MusicMotionVisibility
  private readonly coordinator: RigMotionCoordinator
  private playing = false
  private switching = false
  private playbackInput: readonly [boolean, boolean] | null = null
  private trackId = ''
  private trackInputFingerprint: string | null = null
  private cues: SingingCue[] = []
  private humming = true
  private compileGeneration = 0
  private connectedAudio: object | null = null
  private frame = 0
  private lastSample = 0
  private holdUntil = 0
  private pageVisible = true
  private unsubscribeVisibility: (() => void) | null = null
  private lastFrame: SingingFrame | null = null
  private musicLease: MotionLeaseHandle | null = null
  private readonly beatClock = new BeatClock()
  private behaviorPlan: BehaviorPlan | null = null
  private behaviorTrackId: string | null = null
  private behaviorSequence = 0
  private behaviorId: string | null = null
  private anticipationPegId: string | null = null
  private readonly compileTimeline: SingingTimelineCompiler

  constructor(
    coordinator: RigMotionCoordinator,
    clock: MusicMotionClock,
    audio: MusicMotionAudio,
    visibility: MusicMotionVisibility,
    compileTimeline: SingingTimelineCompiler = compileSingingTimeline,
  ) {
    this.coordinator = coordinator
    this.clock = clock
    this.audio = audio
    this.visibility = visibility
    this.compileTimeline = compileTimeline
  }

  subscribe(listener: SingingFrameListener): () => void {
    this.listeners.add(listener)
    if (this.lastFrame) listener(this.lastFrame)
    this.start()
    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) this.stop()
    }
  }

  listenerCount(): number {
    return this.listeners.size
  }

  setPlayback(playing: boolean, switching: boolean): void {
    const nextSwitching = playing ? false : switching
    if (
      this.playbackInput?.[0] === playing &&
      this.playbackInput[1] === nextSwitching
    ) {
      return
    }
    this.playbackInput = [playing, nextSwitching]
    this.playing = playing
    this.switching = nextSwitching
    this.start()
  }

  markSwitching(): void {
    if (this.playing) return
    this.switching = true
  }

  setTrack(track: MusicTrackInput): void {
    const inputFingerprint = musicTrackInputFingerprint(track)
    if (inputFingerprint === this.trackInputFingerprint) return
    this.trackInputFingerprint = inputFingerprint
    const trackId = track.trackId
    if (trackId !== this.trackId) {
      if (this.trackId) this.switching = true
      this.trackId = trackId
      this.beatClock.setTrack(trackId || null)
      this.cues = []
      this.humming = true
    }
    if (!trackId) {
      this.compileGeneration += 1
      this.cues = []
      this.humming = true
      return
    }
    const generation = ++this.compileGeneration
    void this.compileTimeline({
      verbatim: track.verbatim,
      lines: track.lines,
      songDuration: track.duration,
    }).then((cues) => {
      if (generation !== this.compileGeneration) return
      this.cues = cues
      this.humming = cues.length === 0
    })
  }

  sampleNow(timestamp: number = this.clock.now()): SingingFrame {
    const nowMs = timestamp
    const gap = singingPlaybackGap(this.playing, this.switching)
    let holdExpired = false
    if (gap === 'hold') {
      if (!this.holdUntil) this.holdUntil = timestamp + TRACK_SWITCH_HOLD_MS
      holdExpired = timestamp >= this.holdUntil
    } else {
      this.holdUntil = 0
    }

    const audio = this.audio.getCurrentAudio()
    const audioPaused = !audio || audio.paused
    // The player rebuilds its audio element across tracks. A boolean "already
    // connected" latched on the first one and never reconnected, leaving the
    // analyser wired to a dead element and every band reading zero.
    if (audio && this.connectedAudio !== audio) {
      this.connectedAudio = this.audio.connectAudioToAnalyser(audio)
        ? audio
        : null
    }

    if (gap === 'stop' || holdExpired) {
      this.releaseMusic()
      this.stopEntrainment(nowMs)
    } else {
      this.holdMusic(nowMs)
      this.holdEntrainment(nowMs)
    }

    const snapshot = this.coordinator.snapshot(nowMs)
    const apply = resolveSingingApply({
      gap,
      holdExpired,
      audioPaused,
      mouthOwner: snapshot.owners.mouth,
      headBodyOwner: snapshot.owners.headBody,
    })

    let spectrum: SingingSpectrumDrive | null = null
    let articulation = restSingingArticulation()
    if (!apply.release && audio && !audioPaused) {
      const time = Number.isFinite(audio.currentTime) ? audio.currentTime : 0
      const bands = this.audio.getSpectrumBands()
      const energy = singingVocalEnergy(bands)
      const hasSpectrum = bands.some((band) => band > 0.01)
      const baseDrive = singingSpectrumDrive(bands)
      const beatFrame = this.beatClock.sample(time, baseDrive.bass, true)
      if (hasSpectrum || beatFrame.confidence > 0) {
        spectrum = {
          ...baseDrive,
          sampleTimeSeconds: time,
          beatFrame: { ...beatFrame },
        }
      }
      this.updateBeatAnticipation(nowMs, beatFrame)
      const cue = this.humming ? null : sampleSingingCue(this.cues, time)
      articulation = singingArticulation({
        cue,
        energy: hasSpectrum ? energy : null,
        humming: this.humming,
      })
    } else {
      const beatFrame = this.beatClock.sample(
        audio && Number.isFinite(audio.currentTime) ? audio.currentTime : 0,
        0,
        false,
      )
      this.updateBeatAnticipation(nowMs, beatFrame)
    }

    const frame: SingingFrame = {
      apply,
      spectrum,
      articulation,
      trackId: this.trackId || null,
      behaviorPlan: this.behaviorPlan,
      behaviors: [],
    }
    this.lastFrame = frame
    for (const listener of this.listeners) listener(frame)
    return frame
  }

  private start(): void {
    if (this.frame || this.listeners.size === 0) return
    this.pageVisible = this.visibility.isPageVisible()
    if (!this.unsubscribeVisibility) {
      this.unsubscribeVisibility = this.visibility.onVisibility((visible) => {
        this.pageVisible = visible
        if (visible) this.start()
        else this.pauseLoop(true)
      })
    }
    if (!this.pageVisible) return
    const loop = (timestamp: number) => {
      if (!this.listeners.size) {
        this.frame = 0
        return
      }
      if (!this.pageVisible) {
        this.pauseLoop(true)
        return
      }
      if (timestamp - this.lastSample >= SINGING_SAMPLE_INTERVAL_MS) {
        this.lastSample = timestamp
        this.sampleNow(timestamp)
      }
      this.frame = this.clock.raf(loop)
    }
    this.frame = this.clock.raf(loop)
  }

  private pauseLoop(release: boolean): void {
    if (this.frame) {
      this.clock.caf(this.frame)
      this.frame = 0
    }
    this.holdUntil = 0
    if (release) {
      this.releaseMusic()
      const frame: SingingFrame = {
        apply: {
          release: true,
          writeGroove: false,
          writeMouth: false,
          restMouth: this.coordinator.owner('mouth') !== 'speech',
        },
        spectrum: null,
        articulation: restSingingArticulation(),
        trackId: this.trackId || null,
        behaviorPlan: null,
        behaviors: [],
      }
      this.stopEntrainment()
      this.lastFrame = frame
      for (const listener of this.listeners) listener(frame)
    }
  }

  private stop(): void {
    this.pauseLoop(true)
    this.unsubscribeVisibility?.()
    this.unsubscribeVisibility = null
    this.connectedAudio = null
    this.lastFrame = null
  }

  private holdMusic(nowMs: number): void {
    this.musicLease =
      this.coordinator.renew(this.musicLease, MUSIC_CHANNELS, {
        nowMs,
        ttlMs: MUSIC_LEASE_TTL_MS,
      }) ??
      this.coordinator.claim('music', MUSIC_CHANNELS, {
        nowMs,
        ttlMs: MUSIC_LEASE_TTL_MS,
      })
  }

  private releaseMusic(): void {
    this.coordinator.release(this.musicLease)
    this.musicLease = null
  }

  private holdEntrainment(nowMs: number): void {
    const trackId = this.trackId || null
    if (this.behaviorPlan && this.behaviorTrackId === trackId) return
    this.behaviorSequence += 1
    const prefix = `music-${this.behaviorSequence}`
    const start = `${prefix}:start`
    const ready = `${prefix}:ready`
    const strokeStart = `${prefix}:stroke-start`
    const strokePeak = `${prefix}:stroke-peak`
    const strokeEnd = `${prefix}:stroke-end`
    const anticipation = `${prefix}:next-beat`
    const behaviorId = `${prefix}:entrain`
    this.behaviorPlan = {
      id: prefix,
      originMs: nowMs,
      pegs: [
        { id: start, atMs: nowMs, revision: 0 },
        { id: ready, atMs: nowMs + 55, revision: 0 },
        { id: strokeStart, atMs: nowMs + 88, revision: 0 },
        { id: strokePeak, atMs: nowMs + 120, revision: 0 },
        { id: strokeEnd, atMs: nowMs + 180, revision: 0 },
        {
          id: anticipation,
          atMs: nowMs + 500,
          revision: 0,
          confidence: 0,
        },
      ],
      behaviors: [
        {
          id: behaviorId,
          function: 'entrain',
          kind: 'rhythmic',
          source: 'music',
          resources: [
            'body.head',
            'body.torso',
            'body.arm.left',
            'body.arm.right',
          ],
          channels: ['headBody'],
          timing: {
            start,
            ready,
            strokeStart,
            strokePeak,
            strokeEnd,
            relax: null,
            end: null,
          },
          anticipation,
          form: { family: 'music', id: 'groove' },
          intensity: 1,
          quality: {
            extent: 1.08,
            tempo: 0.82,
            power: 0.78,
            fluidity: 0.92,
            directness: 0.58,
            rebound: 0.54,
            asymmetry: 0.36,
            density: 0.7,
          },
          confidence: 0.8,
        },
      ],
    }
    this.behaviorTrackId = trackId
    this.behaviorId = behaviorId
    this.anticipationPegId = anticipation
  }

  private updateBeatAnticipation(
    nowMs: number,
    frame: Readonly<BeatFrame>,
  ): void {
    if (!this.behaviorId || !this.anticipationPegId) return
    const period = frame.bpm > 0 ? 60 / frame.bpm : 0
    const delayMs =
      period > 0 ? Math.max(0, (1 - frame.beatPhase) * period * 1_000) : 0
    if (!this.behaviorPlan) return
    const pegId = this.anticipationPegId
    this.behaviorPlan = {
      ...this.behaviorPlan,
      pegs: this.behaviorPlan.pegs.map((peg) =>
        peg.id === pegId
          ? {
              ...peg,
              atMs: nowMs + delayMs,
              revision: peg.revision + 1,
              confidence: frame.confidence,
            }
          : peg,
      ),
    }
  }

  private stopEntrainment(_nowMs?: number): void {
    this.behaviorPlan = null
    this.behaviorTrackId = null
    this.behaviorId = null
    this.anticipationPegId = null
  }
}
