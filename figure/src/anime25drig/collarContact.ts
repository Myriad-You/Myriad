interface LayerBounds {
  x: number
  y: number
  w: number
  h: number
}

interface ContactRow {
  y: number
  outerLeft: number
  innerLeft: number
  innerRight: number
  outerRight: number
}

export interface FrontCollarContactModel {
  handles: Float32Array
  contactPairs: Uint16Array
  gridX: readonly number[]
  gridY: readonly number[]
  closurePoint: { x: number; y: number } | null
}

const ALPHA_THRESHOLD = 18
const MAX_CONTACT_SAMPLES = 6

export function buildFrontCollarContactModel(
  rgba: Uint8ClampedArray,
  pixelWidth: number,
  pixelHeight: number,
  source: LayerBounds,
  neckCenterX = source.x + source.w / 2,
): FrontCollarContactModel | null {
  if (
    pixelWidth < 8 ||
    pixelHeight < 8 ||
    rgba.length < pixelWidth * pixelHeight * 4
  ) {
    return null
  }

  const rows: ContactRow[] = []
  const maximumY = Math.max(1, Math.floor(pixelHeight * 0.62))
  const minimumGap = Math.max(3, Math.round(pixelWidth * 0.025))
  const centerX = Math.max(
    1,
    Math.min(
      pixelWidth - 2,
      ((neckCenterX - source.x) / Math.max(1, source.w)) * pixelWidth,
    ),
  )
  let missedRows = 0
  for (let y = 0; y <= maximumY; y += 1) {
    const row = alphaContactRow(
      rgba,
      pixelWidth,
      y,
      centerX,
      minimumGap,
      ALPHA_THRESHOLD,
    )
    if (row) {
      rows.push(row)
      missedRows = 0
    } else if (rows.length > 0) {
      missedRows += 1
      if (missedRows > Math.max(3, Math.round(pixelHeight * 0.035))) break
    }
  }
  if (rows.length < 5) return null

  const samples = sampleContactRows(rows, MAX_CONTACT_SAMPLES)
  if (samples.length < 4) return null

  const handles: number[] = []
  const contactPairs: number[] = []
  const gridX: number[] = []
  const gridY: number[] = []
  const scaleX = source.w / pixelWidth
  const scaleY = source.h / pixelHeight
  for (const sample of samples) {
    const y = source.y + (sample.y + 0.5) * scaleY
    const outerLeft = source.x + sample.outerLeft * scaleX
    const innerLeft = source.x + (sample.innerLeft + 1) * scaleX
    const innerRight = source.x + sample.innerRight * scaleX
    const outerRight = source.x + (sample.outerRight + 1) * scaleX
    const firstHandle = handles.length / 2
    handles.push(outerLeft, y, innerLeft, y, innerRight, y, outerRight, y)
    contactPairs.push(firstHandle + 1, firstHandle + 2)
    gridX.push(outerLeft, innerLeft, innerRight, outerRight)
    gridY.push(y)
  }

  const closure = findContactClosure(
    rgba,
    pixelWidth,
    pixelHeight,
    rows.at(-1)?.y ?? maximumY,
    centerX,
    ALPHA_THRESHOLD,
  )
  const closurePoint = closure
    ? {
        x: source.x + (closure.x + 0.5) * scaleX,
        y: source.y + (closure.y + 0.5) * scaleY,
      }
    : null
  if (closurePoint) {
    gridX.push(closurePoint.x)
    gridY.push(closurePoint.y)
  }

  const bottomY = source.y + source.h
  handles.push(
    source.x,
    bottomY,
    source.x + source.w / 2,
    bottomY,
    source.x + source.w,
    bottomY,
  )
  gridX.push(source.x, source.x + source.w / 2, source.x + source.w)
  gridY.push(bottomY)

  const restHandles = Float32Array.from(handles)
  return {
    handles: restHandles,
    contactPairs: Uint16Array.from(contactPairs),
    gridX,
    gridY,
    closurePoint,
  }
}

function findContactClosure(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  lastContactY: number,
  centerX: number,
  threshold: number,
): { x: number; y: number } | null {
  const centerLeft = Math.floor(centerX)
  const centerRight = Math.ceil(centerX)
  const maximumY = Math.min(height - 1, Math.ceil(height * 0.75))
  for (let y = lastContactY + 1; y <= maximumY; y += 1) {
    const leftOpaque = alphaAt(rgba, width, centerLeft, y) > threshold
    const rightOpaque = alphaAt(rgba, width, centerRight, y) > threshold
    if (leftOpaque || rightOpaque) return { x: centerX, y }
  }
  return null
}

function alphaContactRow(
  rgba: Uint8ClampedArray,
  width: number,
  y: number,
  centerX: number,
  minimumGap: number,
  threshold: number,
): ContactRow | null {
  const centerLeft = Math.floor(centerX)
  const centerRight = Math.ceil(centerX)
  let innerLeft = centerLeft
  while (innerLeft >= 0 && alphaAt(rgba, width, innerLeft, y) <= threshold) {
    innerLeft -= 1
  }
  let innerRight = centerRight
  while (
    innerRight < width &&
    alphaAt(rgba, width, innerRight, y) <= threshold
  ) {
    innerRight += 1
  }
  if (
    innerLeft < 0 ||
    innerRight >= width ||
    innerRight - innerLeft - 1 < minimumGap
  ) {
    return null
  }

  let outerLeft = 0
  while (
    outerLeft < innerLeft &&
    alphaAt(rgba, width, outerLeft, y) <= threshold
  ) {
    outerLeft += 1
  }
  let outerRight = width - 1
  while (
    outerRight > innerRight &&
    alphaAt(rgba, width, outerRight, y) <= threshold
  ) {
    outerRight -= 1
  }
  if (outerLeft >= innerLeft || outerRight <= innerRight) return null
  return { y, outerLeft, innerLeft, innerRight, outerRight }
}

function alphaAt(
  rgba: Uint8ClampedArray,
  width: number,
  x: number,
  y: number,
): number {
  return rgba[(y * width + x) * 4 + 3]
}

function sampleContactRows(
  rows: readonly ContactRow[],
  maximumSamples: number,
): ContactRow[] {
  const count = Math.min(maximumSamples, rows.length)
  const samples: ContactRow[] = []
  for (let sample = 0; sample < count; sample += 1) {
    const target =
      rows[0].y + ((rows.at(-1)!.y - rows[0].y) * sample) / (count - 1)
    let best = rows[0]
    for (const row of rows) {
      if (Math.abs(row.y - target) < Math.abs(best.y - target)) best = row
    }
    if (samples.at(-1)?.y !== best.y) samples.push(smoothContactRow(rows, best))
  }
  return samples
}

function smoothContactRow(
  rows: readonly ContactRow[],
  center: ContactRow,
): ContactRow {
  const nearby = rows.filter((row) => Math.abs(row.y - center.y) <= 2)
  return {
    y: center.y,
    outerLeft: median(nearby.map((row) => row.outerLeft)),
    innerLeft: median(nearby.map((row) => row.innerLeft)),
    innerRight: median(nearby.map((row) => row.innerRight)),
    outerRight: median(nearby.map((row) => row.outerRight)),
  }
}

function median(values: number[]): number {
  const sorted = values.toSorted((left, right) => left - right)
  return sorted[Math.floor(sorted.length / 2)]
}
