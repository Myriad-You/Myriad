import type { Anime25DLayerRole } from './anime25d'
import type { RigPoint, RigRect } from './types'

export type EyeSide = 'left' | 'right'

export interface Anime25DSourceReference {
  width: number
  height: number
  data: Uint8ClampedArray
}

export interface HairStrand {
  x: number
  rootY: number
  tipY: number
}

export interface RasterLayer {
  id: string
  role: Anime25DLayerRole | 'unknown'
  sourceName: string
  order: number
  side: EyeSide | null
  group: 'head' | 'body'
  left: number
  top: number
  width: number
  height: number
  data: Uint8ClampedArray
  synthetic?: boolean
  slot?: 'eye-left' | 'eye-right' | 'mouth'
  variant?:
    | 'open'
    | 'closed'
    | 'wide'
    | 'round'
    | 'narrow'
    | 'dizzy'
    | 'squeeze'
    | 'cry'
    | 'maniac'
    | 'silly'
  documentStrands?: HairStrand[]
}

export interface PreparedLayer extends RasterLayer {
  bounds: RigRect
  textureBounds: RigRect
  strands: HairStrand[]
}

export type RigCanvasFrame = RigRect

export interface AnimeAnchors {
  face: RigRect
  faceCenter: RigPoint
  neck: RigPoint
  bodyBottom: RigPoint
  eyes: Partial<Record<EyeSide, RigPoint>>
  irises: Partial<Record<EyeSide, RigPoint>>
  mouth: RigPoint | null
}
