import type { PerformanceDirective } from '../../../services/agent/types'
import type { TouchObservation } from '../interaction/touchGesture'
import type { PresentedTouchReaction, TouchReaction } from '../interaction/touchReaction'
import type { BehaviorPlan } from './behavior'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import { moodBand } from '../../../components/agent/meropeVitals'
import { selectTouchReaction } from '../interaction/touchReaction'

/** A separate candidate plan, not a replacement of the reply director plan. */
export class TouchMotionSource {
  private plan: BehaviorPlan | null = null
  private lease: MotionLeaseHandle | null = null
  private owner: string | null = null
  private contactId = 0
  private gesture = ''
  private releaseTimer: ReturnType<typeof setTimeout> | null = null
  private lastTouch: TouchObservation | null = null
  private mood = 70
  private arousal = 48
  private semantic = ''
  private revision = 0
  private refinement: TouchReaction | null = null
  private spatialKey = ''
  private strokeX = 0
  private strokeY = 0
  private caress = 0
  private hairTailAtMs: number | null = null
  private resumedHair = false
  private presented: { owner: string; contactId: number; reaction: TouchReaction } | null = null
  private remembered: { owner: string; region: TouchObservation['region']; reaction: TouchReaction;
    band: ReturnType<typeof moodBand>; expiresAt: number; plan: BehaviorPlan } | null = null

  private speech: { messageId: string; plan: BehaviorPlan; expiresAt: number; started: boolean } | null = null
  private speechLease: MotionLeaseHandle | null = null
  private completedSpeech: { owner: string; reaction: TouchReaction; plan: BehaviorPlan; expiresAt: number } | null = null

  /** Freeze the evidence actually sent for this completion, not a later contact. */
  expectSpeech(owner: string, reaction: TouchReaction | null, nowMs: number): void {
    this.completedSpeech = null
    const remembered = this.remembered
    if (this.lastTouch || !remembered || remembered.owner !== owner
      || remembered.reaction !== reaction || nowMs >= remembered.expiresAt || reaction === 'notice') { return
}
    this.completedSpeech = { owner, reaction: remembered.reaction, plan: remembered.plan, expiresAt: nowMs + 20_000 }
  }

  cancelExpectedSpeech(owner: string): void {
    if (this.completedSpeech?.owner === owner) this.completedSpeech = null
  }

  private clearSpeech(): void {
    this.speech = null
    this.coordinator.release(this.speechLease)
    this.speechLease = null
  }

  private continuedReaction: TouchReaction | null = null

  notePresented(owner: string, receipt: PresentedTouchReaction | null, nowMs: number): void {
    if (!receipt || !this.lastTouch || owner !== this.owner
      || receipt.behaviorId !== this.plan?.id || receipt.reaction !== this.plan.behaviors[0]?.form.id
      || nowMs < receipt.atMs || nowMs - receipt.atMs > 250) {
      return
    }
    this.presented = { owner, contactId: this.contactId, reaction: receipt.reaction }
    this.remembered = { owner, region: this.lastTouch.region, reaction: receipt.reaction,
      band: moodBand(this.mood, this.arousal), expiresAt: nowMs + 4000, plan: this.plan! }
  }

  displayedReaction(owner: string, contactId: number): TouchReaction | null {
    return this.presented?.owner === owner && this.presented.contactId === contactId
      ? this.presented.reaction : null
  }

  constructor(private readonly coordinator: RigMotionCoordinator, private readonly changed: () => void) {}

  current(): BehaviorPlan | null { return this.plan }

  /** Continue a sampled attitude, never an unrendered model result or a new caress. */
  accompanySpeech(messageId: string, nowMs: number): void {
    this.clearSpeech()
    const remembered = this.completedSpeech
    this.completedSpeech = null
    if (this.lastTouch || !remembered || nowMs >= remembered.expiresAt || remembered.reaction === 'notice') return
    const id = `${remembered.plan.id}:speech:${messageId}`
    this.speech = { messageId, expiresAt: nowMs + 12_000, started: false,
      plan: { ...remembered.plan, id, behaviors: remembered.plan.behaviors.map(behavior => ({
        ...behavior, id, intensity: behavior.intensity * 0.7,
        // Keep the settled phase.
        form: { ...behavior.form, parameters: { ...behavior.form.parameters, strokeX: 0, strokeY: 0, caress: 0 } },
      })) } }
  }

  speechContinuation(nowMs: number, playing: (id: string) => boolean): BehaviorPlan | null {
    const speech = this.speech
    if (!speech) return null
    const active = playing(speech.messageId)
    if (nowMs >= speech.expiresAt || (speech.started && !active)) {
      this.clearSpeech()
      return null
    }
    if (!active) return null
    speech.started = true
    if (!this.plan) { this.speechLease = this.coordinator.renew(this.speechLease, ['expression', 'gaze', 'headBody'], { nowMs })
      ?? this.coordinator.claim('performance', ['expression', 'gaze', 'headBody'], { nowMs })
}
    return this.plan ? null : speech.plan
  }

  version(): number { return this.revision }

  /** A delayed co-speech decoration cannot turn this same refusal into a joke. */
  acceptsSpeechRefinement(messageId: string, directive: PerformanceDirective, nowMs: number): boolean {
    const speech = this.speech
    if (!speech || speech.messageId !== messageId || nowMs >= speech.expiresAt) return true
    const reaction = speech.plan.behaviors[0]?.form.id
    if (reaction !== 'withdraw' && reaction !== 'hesitate') return true
    return !directive.plan.cues?.some(cue => cue.intent === 'silly' || cue.intent === 'maniac')
      && !directive.phrases?.some(phrase => phrase.intent === 'laugh' || phrase.intent === 'tease')
  }

  refine(owner: string, revision: number, reaction: TouchReaction, nowMs: number): void {
    if (this.owner !== owner || this.revision !== revision || !this.lastTouch) return
    this.refinement = reaction
    this.update(owner, this.lastTouch, nowMs)
  }

  setAffect(mood: number, arousal: number, nowMs: number): void {
    if (moodBand(mood, arousal) !== moodBand(this.mood, this.arousal)) {
      this.remembered = null
      this.completedSpeech = null
      this.continuedReaction = null
      this.clearSpeech()
    }
    this.mood = mood
    this.arousal = arousal
    if (this.lastTouch && this.owner) this.update(this.owner, { ...this.lastTouch, phase: 'update' }, nowMs)
  }

  update(owner: string, touch: TouchObservation, nowMs: number): void {
    if (touch.phase === 'start') { this.clearSpeech(); this.completedSpeech = null }
    // A visible tap tail is not an active contact.
    if (touch.phase !== 'start' && touch.phase !== 'cancel' && !this.lastTouch) return
    if (touch.phase === 'end' || touch.phase === 'cancel') {
      if (this.owner === owner && this.contactId === touch.id) {
        if (touch.phase === 'end' && (touch.gesture === 'tap'
          || touch.region === 'hair' && ['hold', 'stroke'].includes(touch.gesture))) {
          if (touch.gesture === 'tap') {
            this.update(owner, { ...touch, phase: 'update' }, nowMs)
          } else if (this.plan) {
            // Preserve the face/phrase during a short lift, not the last movement command.
            this.strokeX = 0
            this.strokeY = 0
            this.spatialKey = ''
            this.plan = { ...this.plan, behaviors: this.plan.behaviors.map(behavior => ({
              ...behavior,
              form: { ...behavior.form, parameters: { ...behavior.form.parameters, strokeX: 0, strokeY: 0 } },
            })) }
            this.changed()
          }
          this.hairTailAtMs = touch.gesture === 'tap' ? null : nowMs
          this.lastTouch = null
          if (this.releaseTimer) clearTimeout(this.releaseTimer)
          this.releaseTimer = setTimeout(() => {
            if (this.owner === owner && this.contactId === touch.id) this.release(owner, true)
          }, touch.gesture !== 'tap' ? 300 : touch.repeatCount >= 3 ? 1450 : 1100)
        } else {
          this.release(owner, touch.phase === 'end')
        }
      }
      return
    }
    if (touch.phase !== 'start' && (this.owner !== owner || this.contactId !== touch.id)) return
    const same = this.owner === owner && this.contactId === touch.id
    const continuation = this.remembered?.owner === owner && this.remembered.region === touch.region
      && nowMs < this.remembered.expiresAt && this.remembered.band === moodBand(this.mood, this.arousal)
      && this.remembered.reaction !== 'notice' ? this.remembered.reaction : null
    if ((this.lastTouch && this.lastTouch.region !== touch.region)
      || (this.remembered && (this.remembered.region !== touch.region || this.remembered.owner !== owner))) {
      this.remembered = null
      this.continuedReaction = null
      this.presented = null
    }
    const continuedReaction = same ? this.continuedReaction : continuation
    const resumeHair = !same && touch.phase === 'start' && this.owner === owner
      && touch.region === 'hair' && this.hairTailAtMs !== null
      && nowMs >= this.hairTailAtMs && nowMs - this.hairTailAtMs <= 300
    const continuing = !same && touch.phase === 'start' && this.owner === owner
      && this.plan !== null && this.releaseTimer !== null && (touch.repeatCount > 0 || resumeHair)
      && this.semantic.startsWith(`${touch.region}:`)
    const continuousPlan = same || continuing
    const resumedHair = resumeHair || same && this.resumedHair && touch.region === 'hair'
    if (continuing) {
      clearTimeout(this.releaseTimer!)
      this.releaseTimer = null
    }
    const elapsed = same && this.lastTouch ? touch.durationMs - this.lastTouch.durationMs : 0
    const bound = (value: number) => Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0
    if (!same) { this.strokeX = 0; this.strokeY = 0; if (!resumeHair) this.caress = 0 }
    if (elapsed > 0 && this.lastTouch) {
      const blend = -Math.expm1(-elapsed / 160)
      this.strokeX += (bound((touch.x - this.lastTouch.x) * 1000 / elapsed) - this.strokeX) * blend
      this.strokeY += (bound((touch.y - this.lastTouch.y) * 1000 / elapsed) - this.strokeY) * blend
      const speed = Number.isFinite(touch.speed) ? Math.max(0, touch.speed) : 0
      const target = touch.region === 'hair' && touch.gesture === 'stroke'
        ? Math.min(1, speed * 4) * Math.max(0, 1 - speed / 1.2) : 0
      this.caress += (target - this.caress) * -Math.expm1(-elapsed / 800)
    }
    if (touch.region !== 'hair') this.caress = 0
    const quantize = (value: number) => Math.round(bound(value) * 50) / 50
    const parameters = {
      x: quantize(touch.position?.x ?? 0), y: quantize(touch.position?.y ?? 0),
      strokeX: quantize(this.strokeX), strokeY: quantize(this.strokeY),
      caress: quantize(this.caress),
    }
    const spatialKey = Object.values(parameters).join(':')
    const local = selectTouchReaction(resumedHair && touch.gesture === 'contact'
      ? { ...touch, gesture: 'hold' } : touch, this.mood, this.arousal)
    const semantic = `${touch.region}:${touch.gesture}:${touch.repeatCount}:${local}`
    if (!same || semantic !== this.semantic) {
      const sustained = same && this.lastTouch
        && ['hold', 'stroke'].includes(this.lastTouch.gesture)
        && ['hold', 'stroke'].includes(touch.gesture)
        && this.semantic === `${touch.region}:${this.lastTouch.gesture}:${touch.repeatCount}:${local}`
      const continuedJudgment = resumedHair && this.semantic.split(':')[3] === local
      if (!sustained && !continuedJudgment) this.refinement = null
      this.revision += 1
    }
    const reaction = this.refinement ?? (local === 'withdraw' ? local : continuedReaction ?? local)
    const key = `${reaction}:${touch.gesture}`
    this.lastTouch = touch
    this.semantic = semantic
    this.resumedHair = resumedHair
    if (same && this.gesture === key && this.spatialKey === spatialKey) return
    this.spatialKey = spatialKey
    if (!continuousPlan) this.release(undefined, true)
    this.continuedReaction = continuedReaction
    this.resumedHair = resumedHair
    this.owner = owner
    this.contactId = touch.id
    this.lastTouch = touch
    this.gesture = key
    const origin = continuousPlan ? this.plan!.originMs : nowMs
    const id = continuousPlan ? this.plan!.id : `touch:${owner}:${touch.id}`
    this.plan = {
      id, originMs: origin,
      pegs: [0, 55, 85, 120, 160].map((offset, index) => ({ id: `${id}:${index}`, atMs: origin + offset, revision: 0 })),
      behaviors: [{
        id, function: 'attend', kind: 'state', source: 'performance',
        resources: ['face.expression', 'face.gaze', 'body.head', 'body.torso'], channels: ['expression', 'gaze', 'headBody'],
        form: { family: 'touch', id: reaction, parameters },
        intensity: touch.repeatCount > 0 ? 1 : touch.gesture === 'contact' && !resumedHair && !continuedReaction ? 0.7 : 0.95,
        timing: { start: `${id}:0`, ready: `${id}:1`, strokeStart: `${id}:2`, strokePeak: `${id}:3`, strokeEnd: `${id}:4`, relax: null, end: null },
        quality: { fluidity: 0.9, rebound: 0.08 },
      }],
    }
    const channels = this.plan.behaviors.flatMap((behavior) => behavior.channels)
    this.lease = this.coordinator.renew(this.lease, channels, { nowMs })
      ?? this.coordinator.claim('performance', channels, { nowMs })
    this.changed()
  }

  release(owner?: string, preserveEncounter = false): void {
    if (owner !== undefined && this.owner !== null && owner !== this.owner) return
    if (owner !== undefined && owner !== this.owner
      && owner !== this.presented?.owner && owner !== this.remembered?.owner) { return
}
    const changed = this.plan !== null || (!preserveEncounter && this.speech !== null)
    if (!preserveEncounter) { this.presented = null; this.remembered = null; this.completedSpeech = null; this.clearSpeech() }
    this.continuedReaction = null
    if (this.releaseTimer) clearTimeout(this.releaseTimer)
    this.releaseTimer = null
    this.coordinator.release(this.lease)
    this.lease = null
    this.plan = null
    this.owner = null
    this.lastTouch = null
    this.refinement = null
    this.hairTailAtMs = null
    this.resumedHair = false
    this.caress = 0
    this.revision += 1
    if (changed) this.changed()
  }
}
