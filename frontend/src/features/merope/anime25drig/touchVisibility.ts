import type { TouchRegion } from '../interaction/touchGesture'
import type { Anime25DRenderableLayer, Anime25DRenderFrame } from './renderer'
import type { TouchMesh, TouchMeshHit } from './touchHitTest'
import { hitTestTouchMesh, sampleTouchAlpha } from './touchHitTest'

export interface TouchPaintLayer {
  paint: Pick<Anime25DRenderableLayer,
    'source' | 'renderKind' | 'frameOpacity' | 'retainWhenHidden' | 'neckSurfaceFade' | 'cryDirection'>
  /** Must be the replacement mesh for a clipped neck, never mesh. */
  mesh: TouchMesh | null
}

export interface TouchAtlas {
  alpha: Uint8Array
  width: number
  height: number
}

export interface VisibleTouchHit extends TouchMeshHit {
  layerIndex: number
  /** An opaque unknown layer blocks hits behind it, without guessing anatomy. */
  region: TouchRegion | null
}

export function readTouchAtlas(image: HTMLImageElement): TouchAtlas | null {
  const canvas = document.createElement('canvas')
  canvas.width = image.naturalWidth || image.width
  canvas.height = image.naturalHeight || image.height
  try {
    const context = canvas.getContext('2d')
    if (!context) return null
    context.drawImage(image, 0, 0)
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data
    const alpha = new Uint8Array(canvas.width * canvas.height)
    for (let i = 0; i < alpha.length; i++) alpha[i] = pixels[i * 4 + 3]
    return { alpha, width: canvas.width, height: canvas.height }
  } catch {
    return null
  } finally {
    canvas.width = 0
    canvas.height = 0
  }
}

/** Call only on contact samples, never as a render pass. */
export function hitTestVisibleTouch(
  x: number,
  y: number,
  layers: readonly TouchPaintLayer[],
  atlas: TouchAtlas,
  frame: Anime25DRenderFrame,
): VisibleTouchHit | null {
  const alphaAt = (hit: TouchMeshHit) => sampleTouchAlpha(atlas.alpha, atlas.width, atlas.height, hit.u, hit.v)
  const eyeVisible = (side: string | null) => {
    if (side !== 'L' && side !== 'R') return false
    return layers.some(({ paint, mesh }) => {
      if (!mesh || paint.renderKind !== 'eyewhite' || paint.source.side !== side
        || (paint.frameOpacity < 0.004 && !paint.retainWhenHidden)) { return false
}
      const hit = hitTestTouchMesh(x, y, mesh, frame)
      return hit !== null && alphaAt(hit) >= 0.25
    })
  }
  for (let index = layers.length - 1; index >= 0; index--) {
    const { paint, mesh } = layers[index]
    if (!mesh || !Number.isFinite(paint.frameOpacity) || paint.frameOpacity < 0.004) continue
    // They are a visual effect, not a touchable body surface
    if (paint.cryDirection !== 0) continue
    const hit = hitTestTouchMesh(x, y, mesh, frame)
    if (!hit) continue
    if (paint.renderKind === 'iris' && !eyeVisible(paint.source.side)) continue
    const opacity = alphaAt(hit) * paint.frameOpacity * touchNeckOpacity(paint, hit)
    if (opacity < 0.1) continue
    return { ...hit, layerIndex: index, region: touchRegionForRole(paint.source.role) }
  }
  return null
}

/** Never substring-match asset names. */
export function touchRegionForRole(role: string): TouchRegion | null {
  if (role === 'front-hair' || role === 'back-hair') return 'hair'
  if (['headwear', 'earwear', 'eyewear', 'neckwear', 'handwear'].includes(role)) return 'accessory'
  if (['face', 'eyewhite', 'iris', 'eyelash', 'eyelash-l', 'eyelash-r', 'eyebrow',
    'eyebrow-l', 'eyebrow-r', 'ears', 'ears-l', 'ears-r', 'mouth', 'eye-close', 'eye-close2'].includes(role)) { return 'face'
}
  if (['neck', 'topwear', 'bottomwear', 'collar-front', 'collar-back'].includes(role)) return 'body'
  return null
}

function touchNeckOpacity(paint: TouchPaintLayer['paint'], hit: TouchMeshHit): number {
  const fade = paint.neckSurfaceFade
  if (!fade || fade.end <= fade.start) return 1
  const rect = paint.source.atlas
  if (rect.w <= 0 || rect.h <= 0) return 0
  const localX = (hit.u - rect.x) / rect.w
  const localY = (hit.v - rect.y) / rect.h
  let start = fade.start
  let end = fade.end
  const contour = fade.contour
  if (contour && contour.right > contour.left && contour.bands.length >= 4) {
    const columns = contour.bands.length / 2
    const column = Math.max(0, Math.min(1, (localX - contour.left) / (contour.right - contour.left))) * (columns - 1)
    const left = Math.min(Math.floor(column), columns - 2)
    const weight = column - left
    start = contour.bands[left * 2] * (1 - weight) + contour.bands[left * 2 + 2] * weight
    end = contour.bands[left * 2 + 1] * (1 - weight) + contour.bands[left * 2 + 3] * weight
  }
  if (end <= start) return localY < start ? 1 : 0
  const t = Math.max(0, Math.min(1, (localY - start) / (end - start)))
  return 1 - t * t * (3 - 2 * t)
}
