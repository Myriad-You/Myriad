import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

type ReadPixels = (source: Anime25DPlaybackLayer) => CroppedLayerPixels | null

/** Only lift visible art actually buried by the recovered skin. */
export function canLiftNeckwearOverSkin(
  accessory: Anime25DPlaybackLayer,
  neck: Anime25DPlaybackLayer,
  body: Anime25DPlaybackLayer,
  crossedLayers: readonly Anime25DPlaybackLayer[],
  readPixels: ReadPixels,
): boolean {
  if (accessory.role !== 'neckwear') return false
  const art = readPixels(accessory)
  const neckPixels = readPixels(neck)
  const bodyPixels = readPixels(body)
  if (!art || !neckPixels || !bodyPixels) return false
  const obstacles = crossedLayers
    .filter(
      (layer) =>
        layer !== accessory &&
        layer !== neck &&
        layer !== body &&
        overlaps(accessory, layer),
    )
    .map((layer) => ({ layer, image: readPixels(layer) }))
  if (obstacles.some(({ image }) => !image)) return false
  let contact = 0
  for (let y = 0; y < art.height; y++) {
    for (let x = 0; x < art.width; x++) {
      const alpha = art.pixels[(y * art.width + x) * 4 + 3]
      if (alpha < 16) continue
      const wx = accessory.x + ((x + 0.5) / art.width) * accessory.w
      const wy = accessory.y + ((y + 0.5) / art.height) * accessory.h
      if (
        obstacles.some(
          ({ layer, image }) => alphaAt(image!, layer, wx, wy) >= 16,
        )
      ) {
        return false
      }
      if (
        alphaAt(neckPixels, neck, wx, wy) >= 16 &&
        alphaAt(bodyPixels, body, wx, wy) >= 250
      ) {
        contact += alpha / 255
      }
    }
  }
  return contact >= 1
}

function overlaps(a: Anime25DPlaybackLayer, b: Anime25DPlaybackLayer): boolean {
  return (
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
  )
}

function alphaAt(
  image: CroppedLayerPixels,
  source: Anime25DPlaybackLayer,
  x: number,
  y: number,
): number {
  const px = Math.floor(((x - source.x) / source.w) * image.width)
  const py = Math.floor(((y - source.y) / source.h) * image.height)
  return px < 0 || py < 0 || px >= image.width || py >= image.height
    ? 0
    : image.pixels[(py * image.width + px) * 4 + 3]
}
