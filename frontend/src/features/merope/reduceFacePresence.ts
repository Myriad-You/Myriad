/** This is not a clothing performance */

export const FACE_PRESENCE_EXIT_MS = 360
export const FACE_PRESENCE_REST_MS = 100
export const FACE_PRESENCE_ENTER_MS = 540

export type FacePresencePhase =
  | 'hidden'
  | 'pending'
  | 'enter'
  | 'shown'
  | 'exit'
  | 'rest'

export interface FacePresenceState {
  phase: FacePresencePhase
  packageKey: string
  nextPackageKey: string | null
  liveMounted: boolean
  vacant: boolean
}

export const INITIAL_FACE_PRESENCE: FacePresenceState = {
  phase: 'hidden',
  packageKey: '',
  nextPackageKey: null,
  liveMounted: false,
  vacant: false,
}

export type FacePresenceEvent =
  | { type: 'show'; packageKey: string }
  | { type: 'swap'; packageKey: string }
  | { type: 'ready' }
  | { type: 'hide' }
  | { type: 'fail' }
  | { type: 'elapsed' }

function pending(packageKey: string): FacePresenceState {
  return {
    phase: 'pending',
    packageKey,
    nextPackageKey: null,
    liveMounted: true,
    vacant: false,
  }
}

function hiddenVacant(): FacePresenceState {
  return {
    phase: 'hidden',
    packageKey: '',
    nextPackageKey: null,
    liveMounted: false,
    vacant: true,
  }
}

export function reduceFacePresence(
  state: FacePresenceState,
  event: FacePresenceEvent,
): FacePresenceState {
  switch (event.type) {
    case 'show':
      if (state.phase === 'pending' && state.packageKey === event.packageKey) {
        return state
      }
      if (state.phase === 'shown' && state.packageKey === event.packageKey) {
        return state
      }
      if (state.phase === 'enter' && state.packageKey === event.packageKey) {
        return state
      }
      return pending(event.packageKey)
    case 'swap':
      if (state.packageKey === event.packageKey && !state.nextPackageKey) {
        return state
      }
      if (state.phase === 'pending') {
        return { ...state, packageKey: event.packageKey, nextPackageKey: null }
      }
      if (state.phase === 'exit' || state.phase === 'rest') {
        return { ...state, nextPackageKey: event.packageKey, vacant: false }
      }
      if (!state.liveMounted) {
        return pending(event.packageKey)
      }
      return {
        phase: 'exit',
        packageKey: state.packageKey,
        nextPackageKey: event.packageKey,
        liveMounted: false,
        vacant: false,
      }
    case 'ready':
      if (state.phase === 'pending') {
        return { ...state, phase: 'enter', liveMounted: true, vacant: false }
      }
      return state
    case 'hide':
      if (state.phase === 'hidden' && !state.liveMounted) return state
      if (state.phase === 'pending' || state.phase === 'rest') {
        return hiddenVacant()
      }
      return {
        phase: 'exit',
        packageKey: state.packageKey,
        nextPackageKey: null,
        liveMounted: false,
        vacant: false,
      }
    case 'fail':
      return hiddenVacant()
    case 'elapsed':
      if (state.phase === 'enter') {
        return { ...state, phase: 'shown' }
      }
      if (state.phase === 'exit') {
        if (state.nextPackageKey) {
          return {
            phase: 'rest',
            packageKey: state.packageKey,
            nextPackageKey: state.nextPackageKey,
            liveMounted: false,
            vacant: false,
          }
        }
        return hiddenVacant()
      }
      if (state.phase === 'rest') {
        if (state.nextPackageKey) return pending(state.nextPackageKey)
        return hiddenVacant()
      }
      return state
    default:
      return state
  }
}

export function copySurfaceFrame(
  root: ParentNode | null,
): HTMLCanvasElement | null {
  if (!root) return null
  const live = root.querySelector('.face-presence__live')
  const scope: ParentNode = live ?? root
  const canvases = scope.querySelectorAll('canvas')
  for (const canvas of canvases) {
    if (!(canvas instanceof HTMLCanvasElement)) continue
    if (canvas.classList.contains('face-presence__hold')) continue
    if (canvas.width <= 0 || canvas.height <= 0) continue
    const copy = copyCanvas(canvas)
    if (copy) return copy
  }
  const image = scope.querySelector('img')
  if (
    image instanceof HTMLImageElement &&
    image.naturalWidth > 0 &&
    image.naturalHeight > 0
  ) {
    return copyImage(image)
  }
  return null
}

function copyCanvas(source: HTMLCanvasElement): HTMLCanvasElement | null {
  const copy = document.createElement('canvas')
  copy.width = source.width
  copy.height = source.height
  const context = copy.getContext('2d')
  if (!context) return null
  try {
    context.drawImage(source, 0, 0)
  } catch {
    return null
  }
  return copy
}

function copyImage(source: HTMLImageElement): HTMLCanvasElement | null {
  const copy = document.createElement('canvas')
  copy.width = source.naturalWidth
  copy.height = source.naturalHeight
  const context = copy.getContext('2d')
  if (!context) return null
  try {
    context.drawImage(source, 0, 0)
  } catch {
    return null
  }
  return copy
}

export function facePresenceDurationMs(
  phase: FacePresencePhase,
  reduceMotion: boolean,
): number {
  if (reduceMotion) return 0
  if (phase === 'exit') return FACE_PRESENCE_EXIT_MS
  if (phase === 'rest') return FACE_PRESENCE_REST_MS
  if (phase === 'enter') return FACE_PRESENCE_ENTER_MS
  return 0
}
