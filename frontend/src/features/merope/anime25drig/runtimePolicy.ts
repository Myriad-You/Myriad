export interface Anime25DAnimationState {
  atlasReady: boolean
  pageVisible: boolean
  inViewport: boolean
  cancelled: boolean
}

export interface Anime25DRenderSurface {
  bufferWidth: number
  bufferHeight: number
  displayWidth: number
  displayHeight: number
}

export function resolveAnime25DRenderSurface(input: {
  sourceWidth: number
  sourceHeight: number
  cssWidth: number
  cssHeight: number
  devicePixelRatio: number
}): Anime25DRenderSurface {
  const sourceWidth = finiteDimension(input.sourceWidth)
  const sourceHeight = finiteDimension(input.sourceHeight)
  const cssWidth = finiteDimension(input.cssWidth)
  const cssHeight = finiteDimension(input.cssHeight)
  const dpr = Math.max(
    1,
    Math.min(
      2,
      Number.isFinite(input.devicePixelRatio) ? input.devicePixelRatio : 1,
    ),
  )
  const displayScale = Math.min(
    cssWidth / sourceWidth,
    cssHeight / sourceHeight,
  )
  const renderScale = Math.min(1, displayScale) * dpr
  return {
    bufferWidth: Math.max(1, Math.round(sourceWidth * renderScale)),
    bufferHeight: Math.max(1, Math.round(sourceHeight * renderScale)),
    displayWidth: sourceWidth * displayScale,
    displayHeight: sourceHeight * displayScale,
  }
}

export function anime25DRuntimeKey(
  sourceMasterAssetId: string | null | undefined,
  contractVersion: number | null | undefined,
  atlasUrl: string,
): string {
  return `${sourceMasterAssetId ?? ''}:${contractVersion ?? ''}:${atlasUrl}`
}

export function shouldUseAnime25DRuntime(input: {
  hasManifest: boolean
  hasPlayback: boolean
  atlasUrl: string
  runtimeKey: string
  failedRuntimeKey: string | null
}): boolean {
  return Boolean(
    input.hasManifest &&
    input.hasPlayback &&
    input.atlasUrl &&
    input.failedRuntimeKey !== input.runtimeKey,
  )
}

export function shouldApplyAnime25DResize(
  cssWidth: number,
  cssHeight: number,
): boolean {
  return (
    Number.isFinite(cssWidth) &&
    Number.isFinite(cssHeight) &&
    cssWidth >= 1 &&
    cssHeight >= 1
  )
}

export function shouldAnimateAnime25D(state: Anime25DAnimationState): boolean {
  return (
    state.atlasReady &&
    state.pageVisible &&
    state.inViewport &&
    !state.cancelled
  )
}

export function intersectionKeepsAnime25DVisible(entry: {
  isIntersecting: boolean
  boundingClientRect: { width: number; height: number }
}): boolean {
  if (
    entry.boundingClientRect.width < 1 ||
    entry.boundingClientRect.height < 1
  ) {
    return true
  }
  return entry.isIntersecting
}

function finiteDimension(value: number): number {
  return Number.isFinite(value) ? Math.max(1, value) : 1
}
