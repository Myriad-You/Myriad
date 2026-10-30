import type { RasterLayer } from './anime25dImportTypes'
import type { RigRect } from './types'

export function rasterBounds(layer: RasterLayer): RigRect | null {
  let minX = layer.width
  let minY = layer.height
  let maxX = -1
  let maxY = -1
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      if (layer.data[(y * layer.width + x) * 4 + 3] <= 8) continue
      minX = Math.min(minX, x)
      minY = Math.min(minY, y)
      maxX = Math.max(maxX, x)
      maxY = Math.max(maxY, y)
    }
  }
  return maxX < minX
    ? null
    : {
        x: minX,
        y: minY,
        width: maxX - minX + 1,
        height: maxY - minY + 1,
      }
}

export function trimRaster(layer: RasterLayer): RasterLayer {
  const bounds = rasterBounds(layer)
  if (!bounds) return layer
  const padding = 2
  const left = Math.max(0, Math.floor(bounds.x) - padding)
  const top = Math.max(0, Math.floor(bounds.y) - padding)
  const right = Math.min(
    layer.width,
    Math.ceil(bounds.x + bounds.width) + padding,
  )
  const bottom = Math.min(
    layer.height,
    Math.ceil(bounds.y + bounds.height) + padding,
  )
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const start = ((y + top) * layer.width + left) * 4
    data.set(layer.data.subarray(start, start + width * 4), y * width * 4)
  }
  return {
    ...layer,
    left: layer.left + left,
    top: layer.top + top,
    width,
    height,
    data,
  }
}

export function uniquePartId(preferred: string, used: Set<string>): string {
  let id = preferred
  let suffix = 2
  while (used.has(id)) id = `${preferred}-${suffix++}`
  used.add(id)
  return id
}
