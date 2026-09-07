import type { Anime25DDriver } from './driver'
import type { Anime25DPlaybackLayer } from './types'

/** Two authored drawings share the existing closed-eye channel, never two blinks. */
export class ClosedEyePresentation {
  private available = [false, false]
  private selected = [false, false]
  private mix = [0, 0]

  bind(layers: readonly Anime25DPlaybackLayer[]): void {
    for (const [index, side] of (['L', 'R'] as const).entries()) {
      this.available[index] = layers.some(
        (layer) =>
          layer.role === 'eye-close2' &&
          layer.fade === 'eyeClose' &&
          layer.side === side,
      )
      this.selected[index] = false
      this.mix[index] = 0
    }
  }

  /** Read the composed expression BEFORE the automatic blink closes its lids. */
  step(
    expression: Pick<Anime25DDriver, 'eyeOpenL' | 'eyeOpenR'>,
    dt: number,
  ): void {
    const response = 1 - Math.exp(-Math.max(0, dt) / 0.055)
    for (let index = 0; index < 2; index += 1) {
      const open = index === 0 ? expression.eyeOpenL : expression.eyeOpenR
      if (open < 0.28) this.selected[index] = true
      else if (open > 0.38) this.selected[index] = false
      const target = this.available[index] && this.selected[index] ? 1 : 0
      this.mix[index] += (target - this.mix[index]) * response
      if (Math.abs(target - this.mix[index]) < 0.0001) this.mix[index] = target
    }
  }

  opacity(
    layer: Pick<Anime25DPlaybackLayer, 'fade' | 'role' | 'side'>,
  ): number {
    if (layer.fade !== 'eyeClose') return 1
    const blend =
      layer.side === 'L' ? this.mix[0] : layer.side === 'R' ? this.mix[1] : 0
    return layer.role === 'eye-close2' ? blend : 1 - blend
  }
}
