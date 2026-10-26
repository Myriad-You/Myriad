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

// At usual tempo; half again as long as the first cut, whose bounces and nods
// swung the head at well over 100°/s and read as twitchy.
const PULSES: Record<ScoreMove['kind'], Pulse> = {
  nod: { attack: 0.3, hold: 0, release: 0.36, gap: 0.03 },
  shake: { attack: 0.75, hold: 0, release: 0, gap: 0 },
  glance: { attack: 0.18, hold: 0.75, release: 0.45, gap: 0.22 },
  blink: { attack: 0.1, hold: 0.08, release: 0.21, gap: 0.15 },
  wink: { attack: 0.15, hold: 0.42, release: 0.27, gap: 0.22 },
  beat: { attack: 0.15, hold: 0.06, release: 0.42, gap: 0.12 },
  shrug: { attack: 0.3, hold: 0.52, release: 0.6, gap: 0.15 },
  sigh: { attack: 0.75, hold: 1.5, release: 1.2, gap: 0.3 },
  bounce: { attack: 0.24, hold: 0, release: 0.24, gap: 0.03 },
  startle: { attack: 0.1, hold: 0.38, release: 1.05, gap: 0.22 },
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
      // The chest lifting alone reads as a shrug: the head carries the hop.
      add('torsoRise', 0.2 * amount * shape)
      add('headNod', 0.3 * amount * shape)
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
