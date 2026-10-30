import type { MouthExpressionKind, MouthExpressionPalette, MouthExpressionSize, Point, Rgb } from './mouthShared'
import { maniacCavityTone, maniacOutlineTone, maniacTongueShadeDepth, maniacTongueTone, tongueBoundary } from './maniacMouth'
import { insetPath, mouthOuterPath, paint, pointInPolygon } from './mouthPaths'
import { clampInt, mixColor } from './mouthShared'

export { createManiacMouthShadowBitmap } from './maniacMouth'
export { mouthExpressionOuterPath } from './mouthPaths'
export { type MouthExpressionKind, type MouthExpressionPalette, type MouthExpressionSize } from './mouthShared'

const FALLBACK_LINE = { red: 104, green: 57, blue: 75 }
const FALLBACK_CAVITY = { red: 91, green: 45, blue: 65 }
const FALLBACK_FILL = { red: 232, green: 139, blue: 151 }
/** Speaking mouths are sized as if the painted one were at least this wide. */
const SPEAKING_MOUTH_FACE_SHARE = 0.12

export function mouthExpressionGeneratedSizes(
  source: {
    width: number
    height: number
  },
  face?: { width: number; height: number; mouthToChin?: number },
): Record<MouthExpressionKind, MouthExpressionSize> {
  const sourceWidth = Math.max(1, source.width - 4)
  const sourceHeight = Math.max(1, source.height - 4)
  const faceWidth = Math.max(1, face?.width ?? 1)
  // A tiny painted mouth still has to read as speaking on its face.
  const speakingWidth = face
    ? Math.max(sourceWidth, faceWidth * SPEAKING_MOUTH_FACE_SHARE)
    : sourceWidth
  const openWidth = clampInt(Math.round(speakingWidth * 0.96), 24, 128)
  const wideWidth = clampInt(Math.round(speakingWidth * 1.24), 28, 152)
  const roundWidth = clampInt(Math.round(speakingWidth * 0.76), 20, 112)
  const narrowWidth = clampInt(Math.round(speakingWidth * 1.08), 24, 136)
  const cryWidth = clampInt(Math.round(speakingWidth * 1.3), 30, 160)
  const sillyWidth = clampInt(
    Math.round(Math.max(sourceWidth * 1.1, faceWidth * 0.115)),
    26,
    128,
  )
  const maniacPreferredWidth = Math.max(sourceWidth * 1.62, faceWidth * 0.29)
  const maniacWidth = clampInt(
    Math.round(
      face
        ? Math.min(maniacPreferredWidth, faceWidth * 0.33)
        : maniacPreferredWidth,
    ),
    64,
    280,
  )
  const maniacUnboundedHeight = Math.max(
    sourceHeight * 2.7,
    maniacWidth * 0.64,
    Math.max(1, face?.height ?? 1) * 0.162,
  )
  const mouthToChin = face?.mouthToChin
  const maniacHeightLimit =
    mouthToChin && mouthToChin > 0
      ? Math.max(
          42,
          (mouthToChin - Math.max(2, (face?.height ?? 1) * 0.01)) / 0.44,
        )
      : 220
  return {
    open: {
      width: openWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 1.5, openWidth * 0.62)),
        16,
        96,
      ),
    },
    wide: {
      width: wideWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 1.08, wideWidth * 0.4)),
        14,
        72,
      ),
    },
    round: {
      width: roundWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 1.62, roundWidth * 0.94)),
        18,
        104,
      ),
    },
    narrow: {
      width: narrowWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 0.88, narrowWidth * 0.27)),
        12,
        52,
      ),
    },
    cry: {
      width: cryWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 2, cryWidth * 0.64)),
        22,
        120,
      ),
    },
    maniac: {
      width: maniacWidth,
      height: clampInt(
        Math.round(Math.min(maniacUnboundedHeight, maniacHeightLimit)),
        42,
        220,
      ),
    },
    silly: {
      width: sillyWidth,
      height: clampInt(
        Math.round(Math.max(sourceHeight * 1.6, sillyWidth * 0.72)),
        20,
        96,
      ),
    },
  }
}

/** Samples only color identity */
export function sampleMouthExpressionPalette(
  source: Uint8ClampedArray | undefined,
): MouthExpressionPalette {
  if (!source || source.length < 4) {
    return {
      line: FALLBACK_LINE,
      cavity: FALLBACK_CAVITY,
      fill: FALLBACK_FILL,
    }
  }
  const visible: Array<Rgb & { luminance: number }> = []
  for (let index = 0; index < source.length; index += 4) {
    if (source[index + 3] < 40) continue
    const red = source[index]
    const green = source[index + 1]
    const blue = source[index + 2]
    visible.push({
      red,
      green,
      blue,
      luminance: red * 0.299 + green * 0.587 + blue * 0.114,
    })
  }
  if (visible.length === 0) {
    return {
      line: FALLBACK_LINE,
      cavity: FALLBACK_CAVITY,
      fill: FALLBACK_FILL,
    }
  }
  const ranked = visible.toSorted(
    (left, right) => left.luminance - right.luminance,
  )
  const darkCount = Math.max(1, Math.ceil(ranked.length * 0.08))
  const sampledLine = average(ranked.slice(0, darkCount))
  const lineLuminance = luminance(sampledLine)
  const line =
    lineLuminance <= 145
      ? mixColor(sampledLine, FALLBACK_LINE, 0.18)
      : FALLBACK_LINE
  const warmPixels = ranked.filter(
    (color) => color.red > color.green * 1.04 && color.red > color.blue * 0.96,
  )
  const sampledFill = average(warmPixels.length > 0 ? warmPixels : ranked)
  const fill = mixColor(sampledFill, FALLBACK_FILL, 0.72)
  return {
    line,
    cavity: mixColor(line, FALLBACK_CAVITY, 0.58),
    fill,
  }
}

export function createMouthExpressionBitmap(
  kind: MouthExpressionKind,
  requestedSize: Readonly<MouthExpressionSize>,
  palette: Readonly<MouthExpressionPalette>,
): { width: number; height: number; data: Uint8ClampedArray } {
  const width = clampInt(
    Math.round(requestedSize.width),
    16,
    kind === 'maniac' ? 360 : 160,
  )
  const height = clampInt(
    Math.round(requestedSize.height),
    12,
    kind === 'maniac' ? 300 : 120,
  )
  const data = new Uint8ClampedArray(width * height * 4)
  const outer = mouthOuterPath(kind)
  const maniacTongueShade = mixColor(palette.cavity, palette.fill, 0.48)
  const inner = insetPath(
    outer,
    kind === 'maniac' ? 0.985 : kind === 'cry' ? 0.83 : 0.78,
    kind === 'maniac' ? 0.975 : kind === 'cry' ? 0.76 : 0.75,
    kind === 'maniac'
      ? 0.006
      : kind === 'cry'
        ? 0.035
        : kind === 'wide'
          ? 0.045
          : 0.025,
  )
  const samples: readonly Point[] = [
    [0.25, 0.25],
    [0.75, 0.25],
    [0.25, 0.75],
    [0.75, 0.75],
  ]
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let outerCoverage = 0
      let innerCoverage = 0
      let tongueShadeCoverage = 0
      let tongueCoverage = 0
      for (const [offsetX, offsetY] of samples) {
        const px = ((x + offsetX) / width - 0.5) * 2.2
        const py = ((y + offsetY) / height - 0.5) * 2.2
        if (pointInPolygon(px, py, outer)) outerCoverage += 0.25
        if (pointInPolygon(px, py, inner)) {
          innerCoverage += 0.25
          if (kind !== 'cry' && py > tongueBoundary(kind, px)) {
            if (
              kind === 'maniac' &&
              py <= tongueBoundary(kind, px) + maniacTongueShadeDepth(px)
            ) {
              tongueShadeCoverage += 0.25
            } else {
              tongueCoverage += 0.25
            }
          }
        }
      }
      if (outerCoverage <= 0) continue
      const offset = (y * width + x) * 4
      const pixelX = ((x + 0.5) / width - 0.5) * 2.2
      const pixelY = ((y + 0.5) / height - 0.5) * 2.2
      if (outerCoverage > 0) {
        paint(
          data,
          offset,
          kind === 'maniac'
            ? maniacOutlineTone(pixelX, pixelY, palette)
            : palette.line,
          outerCoverage,
        )
      }
      if (innerCoverage > 0) {
        paint(
          data,
          offset,
          kind === 'cry'
            ? palette.fill
            : kind === 'maniac'
              ? maniacCavityTone(pixelX, pixelY, palette)
              : palette.cavity,
          innerCoverage,
        )
      }
      if (tongueShadeCoverage > 0) {
        paint(data, offset, maniacTongueShade, tongueShadeCoverage)
      }
      if (tongueCoverage > 0) {
        paint(
          data,
          offset,
          kind === 'maniac'
            ? maniacTongueTone(pixelX, pixelY, palette)
            : palette.fill,
          tongueCoverage,
        )
      }
    }
  }
  return { width, height, data }
}

function average(colors: readonly Rgb[]): Rgb {
  if (colors.length === 0) return FALLBACK_FILL
  let red = 0
  let green = 0
  let blue = 0
  for (const color of colors) {
    red += color.red
    green += color.green
    blue += color.blue
  }
  return {
    red: Math.round(red / colors.length),
    green: Math.round(green / colors.length),
    blue: Math.round(blue / colors.length),
  }
}

function luminance(color: Readonly<Rgb>): number {
  return color.red * 0.299 + color.green * 0.587 + color.blue * 0.114
}
