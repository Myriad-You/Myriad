/** Width and height in pixels. */
export interface FrameSize {
  width: number
  height: number
}

/** The rectangle the portrait fills on a PSD, in PSD pixels. */
export interface PsdPlacement {
  x: number
  y: number
  width: number
  height: number
  /** A portrait pixel position on the PSD. */
  map: (x: number, y: number) => [number, number]
}

/**
 * Where a portrait of `frame` size sits on a See-through PSD of `width` ×
 * `height`, as See-through placed the portrait it split. A square PSD is the
 * portrait padded to a square, then scaled. Any other PSD is a canvas fitted
 * to the portrait: scaled by one factor and centred, sizes rounded half up
 * (See-through's `fit_placement`, which this must match pixel for pixel).
 */
export function psdPlacement(
  frame: Readonly<FrameSize>,
  width: number,
  height: number,
): PsdPlacement {
  if (width === height) {
    const edge = Math.max(frame.width, frame.height)
    const paddingX = Math.floor((edge - frame.width) / 2)
    const paddingY = Math.floor((edge - frame.height) / 2)
    const scaleX = width / edge
    const scaleY = height / edge
    return {
      x: paddingX * scaleX,
      y: paddingY * scaleY,
      width: frame.width * scaleX,
      height: frame.height * scaleY,
      map: (x, y) => [(x + paddingX) * scaleX, (y + paddingY) * scaleY],
    }
  }
  const scale = Math.min(width / frame.width, height / frame.height)
  const placedWidth = Math.min(width, Math.floor(frame.width * scale + 0.5))
  const placedHeight = Math.min(height, Math.floor(frame.height * scale + 0.5))
  const x = Math.floor((width - placedWidth) / 2)
  const y = Math.floor((height - placedHeight) / 2)
  const scaleX = placedWidth / frame.width
  const scaleY = placedHeight / frame.height
  return {
    x,
    y,
    width: placedWidth,
    height: placedHeight,
    map: (px, py) => [x + px * scaleX, y + py * scaleY],
  }
}
