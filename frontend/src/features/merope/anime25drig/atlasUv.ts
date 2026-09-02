export interface Anime25DAtlasRect {
  x: number
  y: number
  w: number
  h: number
}

export function localToAtlasUv(
  rect: Anime25DAtlasRect,
  localU: number,
  localV: number,
): readonly [number, number] {
  return [rect.x + localU * rect.w, rect.y + localV * rect.h]
}
