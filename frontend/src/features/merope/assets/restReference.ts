import type { Anime25DPlayback } from '../anime25drig/types'

/** Layers the import leaves out of its analysis reference: expression effects. */
const EFFECT_ROLES = new Set([
  'maniac-eye-shadow', 'maniac-mouth-shadow', 'iris-silly', 'lovestruck-heart',
  'lovestruck-face-effect', 'lovestruck-drool', 'anger-mark', 'speechless-sweat',
])

/**
 * A package's layers composed at rest, the way its import composed them for
 * the vision model (anime25dAtlasCompiler): each layer's drawing where it
 * sits, in draw order; faded variants and expression effects left out.
 */
export async function composeRestReference(playback: Readonly<Anime25DPlayback>, atlasUrl: string): Promise<Blob> {
  // Loaded, not decode()d: a page in the background is never granted the decode.
  const atlas = new Image()
  atlas.crossOrigin = 'anonymous'
  await new Promise<void>((resolve, reject) => {
    atlas.onload = () => resolve()
    atlas.onerror = () => reject(new Error('Rig atlas could not be loaded'))
    atlas.src = atlasUrl
  })
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(playback.pixelCanvas.width))
  canvas.height = Math.max(1, Math.round(playback.pixelCanvas.height))
  const context = canvas.getContext('2d')
  if (!context) throw new Error('2D canvas is unavailable')
  const layers = playback.layers
    .map((layer, index) => ({ layer, z: layer.z ?? index }))
    .filter(({ layer }) => !layer.fade && !EFFECT_ROLES.has(layer.role))
    .sort((a, b) => a.z - b.z)
  for (const { layer } of layers) {
    const a = layer.atlas
    context.drawImage(
      atlas,
      a.x * atlas.naturalWidth, a.y * atlas.naturalHeight, a.w * atlas.naturalWidth, a.h * atlas.naturalHeight,
      layer.x, layer.y, layer.w, layer.h,
    )
  }
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('Rest reference could not be encoded'))), 'image/png'))
}
