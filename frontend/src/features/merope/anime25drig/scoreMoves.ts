import type { BodyControl, ScoreMove } from '../../../services/agent/types'

/**
 * How each of the score's there-and-back moves unfolds, in the body
 * controls' own semantic units, before any mapping to drivers. Every move
 * starts and ends at rest, so it can be laid over whatever the body holds.
 */

type Offsets = Partial<Record<BodyControl, number>>

const DIRECTIONS: Record<NonNullable<ScoreMove['direction']>, readonly [number, number]> = {
  'left': [-1, 0], 'right': [1, 0], 'up': [0, 1], 'down': [0, -1],
  'up-left': [-0.75, 0.75], 'up-right': [0.75, 0.75], 'down-left': [-0.75, -0.75], 'down-right': [0.75, -0.75],
}

/** One go and back: rise over `attack`, stay for `hold`, settle over `release` (seconds). */
interface Pulse {
  attack: number
  hold: number
  release: number
  /** Rest before the next repetition. */
  gap: number
}

const PULSES: Record<ScoreMove['kind'], Pulse> = {
  nod: { attack: 0.2, hold: 0, release: 0.24, gap: 0.02 },
  shake: { attack: 0.5, hold: 0, release: 0, gap: 0 },
  glance: { attack: 0.12, hold: 0.5, release: 0.3, gap: 0.15 },
  blink: { attack: 0.07, hold: 0.05, release: 0.14, gap: 0.1 },
  wink: { attack: 0.1, hold: 0.28, release: 0.18, gap: 0.15 },
  beat: { attack: 0.1, hold: 0.04, release: 0.28, gap: 0.08 },
  shrug: { attack: 0.2, hold: 0.35, release: 0.4, gap: 0.1 },
  sigh: { attack: 0.5, hold: 1, release: 0.8, gap: 0.2 },
  bounce: { attack: 0.16, hold: 0, release: 0.16, gap: 0.02 },
  startle: { attack: 0.07, hold: 0.25, release: 0.7, gap: 0.15 },
}

function period(move: Readonly<ScoreMove>): number {
  const pulse = PULSES[move.kind]
  return (pulse.attack + pulse.hold + pulse.release + pulse.gap) / move.tempo
}

/** How long the move takes, in seconds. */
export function scoreMoveSeconds(move: Readonly<ScoreMove>): number {
  const pulse = PULSES[move.kind]
  return period(move) * move.count - pulse.gap / move.tempo
}

/**
 * Writes the move's offsets at `elapsed` seconds into `out` and returns how
 * much of the body it is moving now, 0 to 1.
 */
export function scoreMoveOffsets(move: Readonly<ScoreMove>, elapsed: number, out: Offsets): number {
  if (!(elapsed >= 0) || elapsed >= scoreMoveSeconds(move)) return 0
  const pulse = PULSES[move.kind]
  const cycle = period(move)
  const repetition = Math.min(move.count - 1, Math.floor(elapsed / cycle))
  const local = (elapsed - repetition * cycle) * move.tempo
  // Repeated nods and shakes are each a little smaller than the one before.
  const fading = move.kind === 'nod' || move.kind === 'shake' || move.kind === 'bounce' ? 0.8 ** repetition : 1
  const amount = move.amount * fading
  const shape = pulseShape(pulse, local)
  const add = (control: BodyControl, value: number) => {
    out[control] = (out[control] ?? 0) + value
  }
  const sides = (fallback: 'left' | 'right'): Array<'left' | 'right'> =>
    move.side === 'both' ? ['left', 'right'] : [move.side ?? fallback]
  switch (move.kind) {
    case 'nod':
      add('headNod', -0.5 * amount * shape)
      return 1
    case 'shake':
      add('headTurn', 0.35 * amount * Math.sin((2 * Math.PI * local) / pulse.attack))
      return 1
    case 'glance': {
      const [x, y] = DIRECTIONS[move.direction ?? 'right']
      add('gazeHorizontal', 0.9 * amount * x * shape)
      add('gazeVertical', 0.9 * amount * y * shape)
      return shape
    }
    case 'blink':
      add('eyeOpenLeft', -Math.min(1, amount * 1.6) * shape)
      add('eyeOpenRight', -Math.min(1, amount * 1.6) * shape)
      return shape
    case 'wink':
      for (const side of sides('right')) {
        add(side === 'left' ? 'eyeOpenLeft' : 'eyeOpenRight', -Math.min(1, amount * 1.6) * shape)
      }
      return shape
    case 'beat':
      for (const side of sides('right')) {
        add(side === 'left' ? 'leftArmRaise' : 'rightArmRaise', 0.4 * amount * shape)
      }
      return shape
    case 'shrug':
      add('leftArmRaise', 0.35 * amount * shape)
      add('rightArmRaise', 0.35 * amount * shape)
      add('torsoRise', 0.5 * amount * shape)
      add('browLift', 0.6 * amount * shape)
      return shape
    case 'sigh': {
      // A breath in, then a long settle down and out, then back.
      const inhale = local < pulse.attack ? smooth(local / pulse.attack) : 0
      const exhale = local < pulse.attack ? 0 : shape
      add('torsoRise', amount * (0.3 * inhale - 0.5 * exhale))
      add('headNod', -0.25 * amount * exhale)
      add('eyeOpenLeft', -0.35 * amount * exhale)
      add('eyeOpenRight', -0.35 * amount * exhale)
      return Math.max(inhale, exhale)
    }
    case 'bounce':
      add('torsoRise', 0.45 * amount * shape)
      add('headNod', 0.12 * amount * shape)
      return 1
    case 'startle':
      add('torsoRise', 0.55 * amount * shape)
      add('eyeWide', amount * shape)
      add('browLift', 0.7 * amount * shape)
      add('headNod', 0.25 * amount * shape)
      return shape
  }
}

function pulseShape(pulse: Pulse, local: number): number {
  if (local < pulse.attack) return smooth(local / pulse.attack)
  if (local < pulse.attack + pulse.hold) return 1
  if (pulse.release > 0 && local < pulse.attack + pulse.hold + pulse.release) {
    return 1 - smooth((local - pulse.attack - pulse.hold) / pulse.release)
  }
  return 0
}

function smooth(t: number): number {
  const x = Math.max(0, Math.min(1, t))
  return x * x * (3 - 2 * x)
}
