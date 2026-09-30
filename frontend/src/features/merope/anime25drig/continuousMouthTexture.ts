import type { PaintedLips } from '../expressionShapes/lipMouth'
import type { MouthExpressionPalette } from '../expressionShapes/mouthExpression'
import type { ContinuousMouthShape } from './continuousMouth'
import type { Anime25DOwnTexture } from './renderer'
import type { Anime25DPlaybackAnchors, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { detectPaintedLips } from '../expressionShapes/lipMouth'
import { sampleMouthExpressionPalette } from '../expressionShapes/mouthExpression'
import { paintContinuousLipMouth, paintContinuousMouth } from './continuousMouth'

/**
 * The speaking mouth drawn live: the open mouth's layer is repainted, in its
 * own small texture, as whatever blend of open, wide, round and narrow the
 * voice asks for, so the other speaking drawings are never shown and nothing
 * is ever handed from one drawing to another.
 *
 * Only a mouth the importer drew is redrawn. An artist's own open mouth is
 * theirs; a portrait with one keeps its drawings.
 */

const SPEAKING_FADES = ['mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow'] as const
/**
 * A line mouth is painted finer than the atlas: it is small and its edges
 * are crisp. Painted lips are soft already and cost several times as much
 * per pixel; they stay at the atlas's own resolution.
 */
const LINE_RESOLUTION = 2
const LIP_RESOLUTION = 1
/** Transparent texels round the drawing, so its edge filters as it does in the atlas. */
const PAD = 2
/** Below this change in any weight the mouth looks the same; it is not repainted. */
const REPAINT_STEP = 0.004

export interface ContinuousMouthSetup {
  /** The open mouth's layer, which the live mouth takes over. */
  layer: Anime25DPlaybackLayer
  regionWidth: number
  regionHeight: number
  roll: number
  lips: PaintedLips | null
  palette: MouthExpressionPalette
}

/**
 * Whether this portrait's speaking mouth can be drawn live, and how: from its
 * closed mouth (the artist's) the lips or line colours, as the importer read
 * them. Null keeps the drawings as they are.
 */
export function prepareContinuousMouth(
  layers: readonly Anime25DPlaybackLayer[],
  anchors: Readonly<Anime25DPlaybackAnchors>,
  atlasWidth: number,
  atlasHeight: number,
  readPixels: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): ContinuousMouthSetup | null {
  const speaking = SPEAKING_FADES.map((fade) => layers.find((layer) => layer.fade === fade))
  const open = speaking[0]
  if (!open || speaking.some((layer) => layer && !layer.synthetic)) return null
  const closed = layers.find((layer) => layer.fade === 'mouthClose')
  const pixels = closed ? readPixels(closed) : null
  if (!pixels) return null
  const regionWidth = Math.round(open.atlas.w * atlasWidth)
  const regionHeight = Math.round(open.atlas.h * atlasHeight)
  if (regionWidth < 4 || regionHeight < 4) return null
  return {
    layer: open,
    regionWidth,
    regionHeight,
    roll: resolveAnime25DFaceFrame(anchors).roll,
    lips: detectPaintedLips({ width: pixels.width, height: pixels.height, data: pixels.pixels }),
    palette: sampleMouthExpressionPalette(pixels.pixels),
  }
}

export class ContinuousMouthTexture {
  readonly own: Anime25DOwnTexture
  private readonly width: number
  private readonly height: number
  private readonly innerWidth: number
  private readonly innerHeight: number
  private readonly painted: Uint8ClampedArray
  private readonly upload: Uint8Array
  private readonly last: ContinuousMouthShape = { wide: Number.NaN, round: Number.NaN, narrow: Number.NaN }

  constructor(
    private readonly gl: WebGL2RenderingContext,
    private readonly setup: Readonly<ContinuousMouthSetup>,
  ) {
    const resolution = setup.lips ? LIP_RESOLUTION : LINE_RESOLUTION
    this.innerWidth = setup.regionWidth * resolution
    this.innerHeight = setup.regionHeight * resolution
    this.width = this.innerWidth + 2 * PAD
    this.height = this.innerHeight + 2 * PAD
    this.painted = new Uint8ClampedArray(this.innerWidth * this.innerHeight * 4)
    this.upload = new Uint8Array(this.width * this.height * 4)
    const texture = gl.createTexture()
    if (!texture) throw new Error('continuous mouth texture')
    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, this.width, this.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null)
    this.own = {
      texture,
      uv: [PAD / this.width, PAD / this.height, this.innerWidth / this.width, this.innerHeight / this.height],
    }
    this.paint({ wide: 0, round: 0, narrow: 0 })
  }

  /** Repaints for the voice's current blend, when it has changed enough to show. */
  paint(shape: Readonly<ContinuousMouthShape>): boolean {
    const last = this.last
    if (
      Math.abs(shape.wide - last.wide) < REPAINT_STEP &&
      Math.abs(shape.round - last.round) < REPAINT_STEP &&
      Math.abs(shape.narrow - last.narrow) < REPAINT_STEP
    ) {
      return false
    }
    last.wide = shape.wide
    last.round = shape.round
    last.narrow = shape.narrow
    const { innerWidth, innerHeight, painted, upload, width } = this
    if (this.setup.lips) {
      paintContinuousLipMouth(shape, this.setup.lips, innerWidth, innerHeight, painted, this.setup.roll)
    } else {
      paintContinuousMouth(shape, this.setup.palette, innerWidth, innerHeight, painted, this.setup.roll)
    }
    // Premultiplied, as the atlas is.
    for (let y = 0; y < innerHeight; y++) {
      for (let x = 0; x < innerWidth; x++) {
        const from = (y * innerWidth + x) * 4
        const to = ((y + PAD) * width + x + PAD) * 4
        const alpha = painted[from + 3]
        upload[to] = Math.round((painted[from] * alpha) / 255)
        upload[to + 1] = Math.round((painted[from + 1] * alpha) / 255)
        upload[to + 2] = Math.round((painted[from + 2] * alpha) / 255)
        upload[to + 3] = alpha
      }
    }
    const gl = this.gl
    gl.bindTexture(gl.TEXTURE_2D, this.own.texture)
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, this.width, this.height, gl.RGBA, gl.UNSIGNED_BYTE, upload)
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
    gl.generateMipmap(gl.TEXTURE_2D)
    return true
  }

  dispose(): void {
    this.gl.deleteTexture(this.own.texture)
  }
}
