import type {
  PerformanceCue,
  RigMotionStyle,
} from '../../../services/agent/types'
import type { MeropeActivity } from '../types'
import type { BehaviorRealizerReport } from './behavior'
import type { MoodIntent, MotionFrame } from './intents'
import type { MusicMotionSource, SingingFrame } from './musicSource'
import { AmbientMotionSource } from './ambientSource'
import { RigMotionCoordinator } from './coordinator'
import { HumanPerformanceRuntime } from './humanPerformanceRuntime'
import { MoodMotionSource } from './moodSource'
import { PerformanceMotionSource } from './performanceSource'
import { SpeechMotionSource } from './speechSource'

export type MotionFrameListener = (frame: MotionFrame) => void

export interface RigSummaryFacts {
  capabilities: string[]
  recentIntents: PerformanceCue['intent'][]
  motionStyle: RigMotionStyle
  faceVisible: boolean
}

export interface LiveFaceConsumerState {
  ready: boolean
  mood: number
  arousal: number
  activity: MeropeActivity
  capabilities: readonly string[]
  priority?: number
}

export interface LiveFaceConsumer {
  update: (state: LiveFaceConsumerState) => void
  release: () => void
}

const MAX_RECENT = 6

/**
 * Single motion outlet for one coordinator. Sources publish intents here;
 * mounted rigs only subscribe.
 */
export class MotionRuntime {
  readonly coordinator: RigMotionCoordinator
  readonly speech: SpeechMotionSource
  readonly performance: PerformanceMotionSource
  readonly mood: MoodMotionSource
  readonly ambient: AmbientMotionSource
  private readonly humanPerformance = new HumanPerformanceRuntime()
  private readonly musicSource: MusicMotionSource | null
  private readonly listeners = new Set<MotionFrameListener>()
  private retains = 0
  private unsubMusic: (() => void) | null = null
  private musicFrame: SingingFrame | null = null
  private moodIntent: MoodIntent | null = null
  private capabilities: string[] = []
  private manualCapabilities: string[] = []
  private readonly faceConsumers = new Map<number, LiveFaceConsumerState>()
  private nextFaceConsumerId = 0
  private recentIntents: PerformanceCue['intent'][] = []
  private motionStyle: RigMotionStyle = 'even'
  private previewClock: ReturnType<typeof setTimeout> | null = null
  private previewClockActive = false

  constructor(
    coordinator: RigMotionCoordinator,
    musicSource: MusicMotionSource | null = null,
  ) {
    this.coordinator = coordinator
    this.musicSource = musicSource
    this.speech = new SpeechMotionSource(coordinator, () => this.emit())
    this.performance = new PerformanceMotionSource(
      coordinator,
      (intent) => {
        const style = intent.directive?.motionStyle
        if (style && style !== this.motionStyle) {
          this.motionStyle = style
          this.humanPerformance.setMotionStyle(style)
        }
        this.rememberIntents(
          intent.directive?.plan.cues.map((cue) => cue.intent) ?? [],
        )
        this.emit()
      },
      () => this.humanPerformance.snapshots(currentNow()),
    )
    this.mood = new MoodMotionSource(coordinator, (intent, bandChanged) => {
      this.moodIntent = intent
      if (bandChanged) this.performance.clearBearing()
      this.emit()
    })
    this.ambient = new AmbientMotionSource(coordinator)
  }

  retain(): () => void {
    this.retains += 1
    if (this.retains === 1) {
      this.speech.start()
      this.performance.start()
      this.ambient.claim()
      if (this.musicSource) {
        this.unsubMusic = this.musicSource.subscribe((frame) => {
          this.musicFrame = frame
          this.emit()
        })
      } else {
        this.startPreviewClock()
      }
    }
    return () => {
      this.retains -= 1
      if (this.retains > 0) return
      this.retains = 0
      this.stopPreviewClock()
      this.speech.stop()
      this.performance.stop()
      this.mood.release()
      this.ambient.release()
      this.unsubMusic?.()
      this.unsubMusic = null
      this.musicFrame = null
      this.humanPerformance.clear(currentNow())
    }
  }

  setCapabilities(capabilities: readonly string[]): void {
    this.manualCapabilities = uniqueCapabilities(capabilities)
    if (this.faceConsumers.size === 0) {
      this.capabilities = this.manualCapabilities
    }
  }

  attachLiveFaceConsumer(initial: LiveFaceConsumerState): LiveFaceConsumer {
    this.nextFaceConsumerId += 1
    const id = this.nextFaceConsumerId
    this.faceConsumers.set(id, normalizeFaceConsumer(initial))
    this.reconcileLiveFaces()
    let attached = true
    return {
      update: (state) => {
        if (!attached) return
        this.faceConsumers.set(id, normalizeFaceConsumer(state))
        this.reconcileLiveFaces()
      },
      release: () => {
        if (!attached) return
        attached = false
        this.faceConsumers.delete(id)
        this.reconcileLiveFaces()
      },
    }
  }

  summaryFacts(): RigSummaryFacts {
    return {
      capabilities: this.capabilities,
      recentIntents: this.recentIntents,
      motionStyle: this.motionStyle,
      faceVisible:
        this.faceConsumers.size > 0
          ? [...this.faceConsumers.values()].some((consumer) => consumer.ready)
          : this.retains > 0,
    }
  }

  frame(nowMs?: number): MotionFrame {
    const now = nowMs ?? currentNow()
    const speech = this.speech.current(now)
    const performance = this.performance.current(now)
    const human = this.humanPerformance.frame(
      [
        speech.behaviorPlan,
        performance.behaviorPlan,
        this.musicFrame?.behaviorPlan,
      ],
      now,
    )
    const speechBehaviors = human.behaviors.filter(
      (behavior) => behavior.source === 'coSpeech',
    )
    const performanceBehaviors = human.behaviors.filter(
      (behavior) => behavior.source === 'performance',
    )
    const musicBehaviors = human.behaviors.filter(
      (behavior) => behavior.source === 'music',
    )
    return {
      snapshot: this.coordinator.snapshot(now),
      bearing: this.performance.currentBearing() ?? this.mood.currentBearing(),
      speech: hasSpeechIntent(speech)
        ? { ...speech, behaviors: speechBehaviors }
        : null,
      performance:
        performance.directive || performance.behaviorPlan
          ? { ...performance, behaviors: performanceBehaviors }
          : null,
      music: this.musicFrame
        ? { ...this.musicFrame, behaviors: musicBehaviors }
        : null,
      mood: this.moodIntent,
      behaviorPlan: human.plan,
      behaviorRevision: human.revision,
      behaviors: human.behaviors,
    }
  }

  reportBehaviorRealizer(
    planId: string,
    behaviorId: string,
    result: 'accepted' | 'rejected',
    nowMs?: number,
    reason?: BehaviorRealizerReport['reason'],
  ): void {
    this.humanPerformance.reportRealizer(
      planId,
      behaviorId,
      result,
      nowMs ?? currentNow(),
      reason,
    )
  }

  subscribe(listener: MotionFrameListener): () => void {
    this.listeners.add(listener)
    listener(this.frame())
    return () => {
      this.listeners.delete(listener)
    }
  }

  private rememberIntents(intents: readonly PerformanceCue['intent'][]): void {
    if (intents.length === 0) return
    this.recentIntents = [...this.recentIntents, ...intents].slice(-MAX_RECENT)
  }

  private reconcileLiveFaces(): void {
    const ready = [...this.faceConsumers.entries()].filter(
      ([, consumer]) => consumer.ready,
    )
    this.capabilities = uniqueCapabilities(
      ready.flatMap(([, consumer]) => consumer.capabilities),
    )
    const authority = ready.sort(
      ([leftId, left], [rightId, right]) =>
        (right.priority ?? 0) - (left.priority ?? 0) || leftId - rightId,
    )[0]?.[1]
    if (authority) {
      this.mood.set(authority.mood, authority.activity, authority.arousal)
    }
  }

  private emit(): void {
    const frame = this.frame()
    for (const listener of this.listeners) listener(frame)
  }

  private startPreviewClock(): void {
    if (this.previewClockActive) return
    this.previewClockActive = true
    const step = () => {
      if (!this.previewClockActive) return
      const now =
        typeof performance !== 'undefined' ? performance.now() : Date.now()
      this.coordinator.tick(now)
      this.emit()
      const timer = setTimeout(step, 16)
      if (typeof timer === 'object' && 'unref' in timer) timer.unref()
      this.previewClock = timer
    }
    step()
  }

  private stopPreviewClock(): void {
    this.previewClockActive = false
    if (this.previewClock == null) return
    clearTimeout(this.previewClock)
    this.previewClock = null
  }
}

function hasSpeechIntent(
  intent: ReturnType<SpeechMotionSource['current']>,
): boolean {
  return Boolean(
    intent.active ||
    intent.autoSpeech ||
    intent.energy !== null ||
    intent.prosody ||
    intent.queuedText.length > 0 ||
    (intent.articulation && intent.articulation.amount > 0),
  )
}

/** Production faces: music may occupy the body; semantic reactions stay explicit. */
export function createLiveMotionRuntime(
  coordinator: RigMotionCoordinator,
  musicSource: MusicMotionSource | null,
): MotionRuntime {
  return new MotionRuntime(coordinator, musicSource)
}

export function createPreviewMotionRuntime(): MotionRuntime {
  return new MotionRuntime(new RigMotionCoordinator())
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}

function uniqueCapabilities(capabilities: readonly string[]): string[] {
  return [...new Set(capabilities)].slice(0, 12)
}

function normalizeFaceConsumer(
  state: LiveFaceConsumerState,
): LiveFaceConsumerState {
  return {
    ...state,
    capabilities: uniqueCapabilities(state.capabilities),
    priority: state.priority ?? 0,
  }
}
