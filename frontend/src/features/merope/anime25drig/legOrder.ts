/**
 * Which of a standing figure's legs is drawn over the other. A drawing puts
 * one leg's layers over the other's once and for all; a rigger keys the draw
 * order to the stance (Cubism's parameter-keyed draw order), so the free leg,
 * its knee bent towards the viewer, comes over the leg bearing the weight
 * where the two meet. The legs' layers trade places only among the places
 * they hold in the stack; everything drawn between them keeps its place.
 */

interface LegLayer {
  source: { name: string; role: string; side?: 'L' | 'R' | null }
  attachment?: { hostName?: string } | null
}

/**
 * The freer leg comes over the other once it bends this much more: about as
 * soon as the two pass, while the knees are still apart, and not back and forth.
 */
const LEAD = 0.02

const LEG_ROLES = new Set(['legwear', 'footwear'])

export class LegOrder<T> {
  /** The stack's places the legs' layers hold, lowest first. */
  private readonly slots: readonly number[]
  private readonly sides: readonly ('L' | 'R')[]
  private front: 'L' | 'R' | null = null
  private readonly ordered: T[] = []

  private constructor(slots: number[], sides: ('L' | 'R')[]) {
    this.slots = slots
    this.sides = sides
  }

  /** The legs of a stack: each leg's legwear and footwear, and what hangs from them. Null without two legs. */
  static of<T extends LegLayer>(layers: readonly T[]): LegOrder<T> | null {
    const legSide = new Map<string, 'L' | 'R'>()
    for (const layer of layers) {
      const side = layer.source.side
      if (LEG_ROLES.has(layer.source.role) && (side === 'L' || side === 'R')) legSide.set(layer.source.name, side)
    }
    const slots: number[] = []
    const sides: ('L' | 'R')[] = []
    layers.forEach((layer, index) => {
      const side = legSide.get(layer.source.name) ?? legSide.get(layer.attachment?.hostName ?? '')
      if (!side) return
      slots.push(index)
      sides.push(side)
    })
    return sides.includes('L') && sides.includes('R') ? new LegOrder(slots, sides) : null
  }

  /** Takes this frame's bends (0 bearing weight … 1 free); the freer leg leads once it is clearly freer. */
  step(bendL: number, bendR: number): void {
    if (bendL - bendR > LEAD) this.front = 'L'
    else if (bendR - bendL > LEAD) this.front = 'R'
  }

  /** The stack to draw: as given until a leg leads, then that leg's layers over the other's. */
  order(layers: readonly T[]): readonly T[] {
    const front = this.front
    if (!front) return layers
    const ordered = this.ordered
    ordered.length = 0
    for (const layer of layers) ordered.push(layer)
    let slot = 0
    for (const side of [front === 'L' ? 'R' : 'L', front]) {
      this.sides.forEach((own, index) => {
        if (own === side) ordered[this.slots[slot++]] = layers[this.slots[index]]
      })
    }
    return ordered
  }
}

const bound = new WeakMap<readonly object[], LegOrder<never> | null>()

/** The stack to draw this frame: a standing figure's freer leg over the other (`stance` absent: as given). */
export function drawOrder<T extends LegLayer>(
  layers: readonly T[],
  stance: { bendL: number; bendR: number } | null | undefined,
): readonly T[] {
  if (!stance) return layers
  let legs = bound.get(layers) as LegOrder<T> | null | undefined
  if (legs === undefined) bound.set(layers, (legs = LegOrder.of(layers)) as LegOrder<never> | null)
  if (!legs) return layers
  legs.step(stance.bendL, stance.bendR)
  return legs.order(layers)
}
