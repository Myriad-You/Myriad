import type { FaceLife } from '../events/performanceContract'
import type { ResolvedScore } from '../motion/scoreTimeline'
import type { BodyControl, ScoreMove } from '../performance/types'
import { BODY_CONTROLS, SCORE_MOVES } from '../events/performanceContract'
import { angleZForHeadRoll, HEAD_TILT_RADIANS } from './poseScale'
import { scoreMoveOffsets, scoreMoveSeconds } from './scoreMoves'

/**
 * Everything the director has asked of the body, laid over time. Nothing in
 * here takes a control and gives it back: each beat rises, stays and settles
 * by its own envelope, and at any moment a control goes where the beats on it
 * pull, each by how strongly it is playing. A newer beat on the same control
 * fades in as an older one fades out.
 *
 * Her standing acting (what she does while nothing else is going on) plays
 * as strongly as she is free for it: it fades back as she speaks or as the
 * director's reply beats play, and comes up again after, by itself.
 */

/** A beat without its own length stays this long. */
const DEFAULT_HOLD_SECONDS = 3
const MIN_RELEASE_SECONDS = 0.4
/** How quickly she becomes taken up by speech or a reply, and free again (rad/s). */
const ENGAGE_RATE = 5
const RETURN_RATE = 1.6
/** How quickly one standing score gives way to the next (rad/s). */
const STANDING_RATE = 2.5

interface PoseItem {
  id: string
  scoreId: number
  standing: boolean
  /** Part of the life of the look her face holds: it neither takes her up nor waits for her. */
  face?: boolean
  control: BodyControl
  value: number
  at: number
  attack: number
  hold: number
  release: number
}

interface MoveItem {
  id: string
  scoreId: number
  standing: boolean
  face?: boolean
  at: number
  move: ScoreMove
}

interface Loop {
  scoreId: number
  /** The score's beats relative to its start, to play again each period. */
  beats: ResolvedScore['beats']
  startNowMs: number
  periodSeconds: number
  /** Player time of the cycle most recently laid out. */
  cycleAt: number
  cycle: number
}

export interface ActingSample {
  value: number
  weight: number
}

/**
 * Semantic up is positive; the renderer's image-space Y points down. A
 * directed head tilt says how much it should show, as a share of the full
 * tilt (HEAD_TILT_RADIANS): the director's slight (0.35) is about 8°, clear
 * (0.65) about 15° and strong the full 24°. Everyday motion keeps the low,
 * slow end of angleZ to itself.
 */
export function mappedControlValue(key: BodyControl, value: number): number {
  if (key === 'gazeVertical') return -value
  if (key === 'headTilt') return angleZForHeadRoll(value * HEAD_TILT_RADIANS)
  if (key === 'hairSway') return value * 1.5
  if (key === 'chestSway') return value * 4
  return value
}

export class ActingField {
  /** `random` varies each time round of a look's life; tests pass their own. */
  constructor(private readonly random: () => number = Math.random) {}

  private poses: PoseItem[] = []
  private moves: MoveItem[] = []
  private replyScoreId = 0
  private standingScoreId = 0
  private loop: Loop | null = null
  /** The look her face holds, and its life playing again, a little differently, each time round. */
  private face: { name: string; loop: Loop; length: number } | null = null
  private faceScoreId = 0
  private speaking = false
  private engaged = 0
  /** Per standing score: how much it is still the one playing. */
  private readonly standingShares = new Map<number, number>()
  readonly values = new Map<BodyControl, ActingSample>()
  readonly offsets = new Map<BodyControl, number>()
  readonly moveWeights = new Map<BodyControl, number>()

  setSpeaking(speaking: boolean): void {
    this.speaking = speaking
  }

  /**
   * A score from the director, moved from the clock (`nowMs`) onto the
   * player's (`time`). A revised score moves its beats that have not started;
   * a new one drops the unstarted beats of the one it follows, and what is
   * under way plays out.
   */
  setScore(score: Readonly<ResolvedScore>, time: number, nowMs: number): void {
    const standing = score.standing !== undefined
    const current = standing ? this.standingScoreId : this.replyScoreId
    if (score.id !== current) {
      this.poses = this.poses.filter((item) => item.face || item.standing !== standing || item.at <= time)
      this.moves = this.moves.filter((item) => item.face || item.standing !== standing || item.at <= time)
      if (standing) {
        this.standingScoreId = score.id
        this.standingShares.set(score.id, this.standingShares.get(score.id) ?? 0)
        this.loop = score.standing!.loopMs > 0
          ? { scoreId: score.id, beats: score.beats, startNowMs: nowMs, periodSeconds: score.standing!.loopMs / 1000, cycleAt: time, cycle: 0 }
          : null
      } else {
        this.replyScoreId = score.id
      }
    }
    this.lay(score.id, standing, score.beats, (atMs) => time + (atMs - nowMs) / 1000, '', time)
  }

  /**
   * The look her face holds (null for none). Its life plays from now on,
   * over and over until the look changes; what is under way of the last
   * look's life plays out.
   */
  setFace(name: string | null, life: Readonly<FaceLife> | null, time: number): BodyControl[] {
    if ((this.face?.name ?? null) === (life ? name : null)) return []
    // What the last look's life was doing is left where it is, for the caller
    // to carry on from (DirectedPoseController's shift), not played out or faded.
    const dropped = new Set<BodyControl>()
    for (const item of this.poses) { if (item.face && item.at <= time) dropped.add(item.control)
}
    for (const item of this.moves) {
      if (item.face && item.at <= time) { for (const control of SCORE_MOVES[item.move.kind].controls) dropped.add(control)
}
    }
    this.poses = this.poses.filter((item) => !item.face)
    this.moves = this.moves.filter((item) => !item.face)
    if (!name || !life) {
      this.face = null
      return [...dropped]
    }
    this.faceScoreId -= 1
    const beats = life.beats.map((beat, index) => ({ id: `face:${name}:${index}`, atMs: beat.atMs, ...(beat.pose ? { pose: beat.pose } : {}), ...(beat.move ? { move: beat.move } : {}) }))
    const periodSeconds = life.periodMs / 1000
    this.face = { name, loop: { scoreId: this.faceScoreId, beats, startNowMs: 0, periodSeconds, cycleAt: time, cycle: 0 }, length: periodSeconds }
    this.lay(this.faceScoreId, false, varyLife(beats, periodSeconds, this.random, true), (atMs) => time + atMs / 1000, '', time, true)
    return [...dropped]
  }

  /** A single move from now, such as the start of a change of face. */
  addMove(id: string, move: Readonly<ScoreMove>, time: number): void {
    this.moves.push({ id, scoreId: 0, standing: false, at: time, move })
  }

  /** Advances to `time`; `drivable` says which controls this body has. */
  step(dt: number, time: number, drivable: (control: BodyControl) => boolean): void {
    this.continueLoop(time)
    this.continueFace(time)
    this.poses = this.poses.filter((item) => time < item.at + item.attack + item.hold + item.release)
    this.moves = this.moves.filter((item) => time < item.at + scoreMoveSeconds(item.move))
    // How taken up she is: speaking, or the director's reply beats playing.
    let reply = 0
    for (const item of this.poses) { if (!item.standing && !item.face) reply = Math.max(reply, envelope(item, time))
}
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    const busy = Math.max(this.speaking ? 1 : 0, reply)
    this.engaged += (busy - this.engaged) * (1 - Math.exp(-(busy > this.engaged ? ENGAGE_RATE : RETURN_RATE) * step))
    for (const [id, share] of this.standingShares) {
      const goal = id === this.standingScoreId ? 1 : 0
      const next = share + (goal - share) * (1 - Math.exp(-STANDING_RATE * step))
      if (goal === 0 && next < 0.001 && !this.poses.some((item) => item.scoreId === id)) this.standingShares.delete(id)
      else this.standingShares.set(id, next)
    }
    const gain = (standing: boolean, scoreId: number) =>
      standing ? (this.standingShares.get(scoreId) ?? 0) * (1 - this.engaged) : 1
    this.values.clear()
    const sums = new Map<BodyControl, { weight: number; weighted: number }>()
    for (const item of this.poses) {
      if (!drivable(item.control)) continue
      const strength = envelope(item, time) * gain(item.standing, item.scoreId)
      if (strength <= 0) continue
      const sum = sums.get(item.control) ?? { weight: 0, weighted: 0 }
      sum.weight += strength
      sum.weighted += strength * item.value
      sums.set(item.control, sum)
    }
    for (const [control, sum] of sums) {
      this.values.set(control, { value: sum.weighted / sum.weight, weight: Math.min(1, sum.weight) })
    }
    this.offsets.clear()
    this.moveWeights.clear()
    const offsets: Partial<Record<BodyControl, number>> = {}
    for (const item of this.moves) {
      if (time < item.at) continue
      for (const key of Object.keys(offsets) as BodyControl[]) delete offsets[key]
      const shape = scoreMoveOffsets(item.move, time - item.at, offsets)
      const strength = gain(item.standing, item.scoreId)
      for (const [control, offset] of Object.entries(offsets) as Array<[BodyControl, number]>) {
        if (!drivable(control)) continue
        this.offsets.set(control, (this.offsets.get(control) ?? 0) + offset * strength)
        this.moveWeights.set(control, Math.max(this.moveWeights.get(control) ?? 0, shape * strength))
      }
    }
  }

  /** A looping standing score lays out its next period before the last one ends. */
  private continueLoop(time: number): void {
    const loop = this.loop
    if (!loop || loop.scoreId !== this.standingScoreId) return
    while (time >= loop.cycleAt + loop.periodSeconds - 1) {
      loop.cycle += 1
      loop.cycleAt += loop.periodSeconds
      const cycleAt = loop.cycleAt
      this.lay(loop.scoreId, true, loop.beats, (atMs) => cycleAt + (atMs - loop.startNowMs) / 1000, `#${loop.cycle}`, time)
    }
  }

  private continueFace(time: number): void {
    const face = this.face
    if (!face) return
    const loop = face.loop
    while (time >= loop.cycleAt + face.length - 1) {
      loop.cycle += 1
      loop.cycleAt += face.length
      face.length = loop.periodSeconds * (1 - LIFE_PERIOD_SPREAD + 2 * LIFE_PERIOD_SPREAD * this.random())
      const cycleAt = loop.cycleAt
      this.lay(loop.scoreId, false, varyLife(loop.beats, loop.periodSeconds, this.random, false), (atMs) => cycleAt + atMs / 1000, `#${loop.cycle}`, time, true)
    }
  }

  private lay(
    scoreId: number,
    standing: boolean,
    beats: ResolvedScore['beats'],
    at: (atMs: number) => number,
    suffix: string,
    time: number,
    face = false,
  ): void {
    for (const beat of beats) {
      const id = beat.id + suffix
      const start = at(beat.atMs)
      if (beat.pose) {
        const attack = beat.pose.transitionMs / 1000
        const hold = beat.pose.holdMs > 0 ? beat.pose.holdMs / 1000 : DEFAULT_HOLD_SECONDS
        for (const [control, value] of Object.entries(beat.pose.targets) as Array<[BodyControl, number]>) {
          const known = this.poses.find((item) => item.id === id && item.control === control)
          if (known) {
            if (known.at > time) known.at = start
            continue
          }
          this.poses.push({
            id, scoreId, standing, ...(face ? { face } : {}), control, value: mappedControlValue(control, value),
            at: start, attack, hold, release: Math.max(MIN_RELEASE_SECONDS, attack),
          })
        }
      }
      if (beat.move) {
        const known = this.moves.find((item) => item.id === id)
        if (known) {
          if (known.at > time) known.at = start
        } else {
          this.moves.push({ id, scoreId, standing, ...(face ? { face } : {}), at: start, move: beat.move })
        }
      }
    }
  }
}

/** Each time round a look's life comes a little early or late, longer or shorter... */
const LIFE_PERIOD_SPREAD = 0.25
const LIFE_SHIFT = 0.12
const LIFE_SIZE_SPREAD = 0.3
/** ...and now and then leaves a beat out, so it is never the same twice. */
const LIFE_SKIP = 0.25

/**
 * One time round of a look's life: each beat shifted, its pose or move
 * larger or smaller, quicker or slower, sometimes fewer repeats, sometimes
 * left out. The first time round plays every beat.
 */
function varyLife(
  beats: ResolvedScore['beats'],
  periodSeconds: number,
  random: () => number,
  whole: boolean,
): ResolvedScore['beats'] {
  const around = (spread: number) => 1 - spread + 2 * spread * random()
  return beats.flatMap((beat) => {
    if (!whole && random() < LIFE_SKIP) return []
    const atMs = Math.max(0, beat.atMs + (random() * 2 - 1) * LIFE_SHIFT * periodSeconds * 1000)
    const pose = beat.pose && (() => {
      const size = around(LIFE_SIZE_SPREAD)
      const targets = Object.fromEntries(Object.entries(beat.pose.targets).map(([control, value]) => {
        const rest = (BODY_CONTROLS[control as BodyControl] as { rest?: number }).rest ?? 0
        return [control, rest + ((value as number) - rest) * size]
      }))
      return { ...beat.pose, targets, transitionMs: beat.pose.transitionMs * around(0.25), holdMs: beat.pose.holdMs * around(0.4) }
    })()
    const move = beat.move && {
      ...beat.move,
      amount: Math.max(0.1, Math.min(1, beat.move.amount * around(LIFE_SIZE_SPREAD))),
      tempo: beat.move.tempo * around(0.15),
      count: beat.move.count > 1 ? 1 + Math.floor(random() * beat.move.count) : 1,
    }
    return [{ ...beat, atMs, ...(pose ? { pose } : {}), ...(move ? { move } : {}) }]
  })
}

function envelope(item: Readonly<PoseItem>, time: number): number {
  const t = time - item.at
  if (t < 0) return 0
  if (t < item.attack) {
    const share = t / Math.max(1e-6, item.attack)
    // A look or a turn of the head sets off at once and settles in; lids and mouth ease both ways.
    return swings(item.control) ? 1 - (1 - share) ** 3 : smooth(share)
  }
  if (t < item.attack + item.hold) return 1
  return 1 - smooth((t - item.attack - item.hold) / item.release)
}

/** Where she looks and how she holds her head and body: moves that set off at once. */
function swings(control: BodyControl): boolean {
  return control.startsWith('gaze') || control.startsWith('head') || control.startsWith('torso') || control.includes('Arm')
}

function smooth(t: number): number {
  const x = Math.max(0, Math.min(1, t))
  return x * x * (3 - 2 * x)
}
