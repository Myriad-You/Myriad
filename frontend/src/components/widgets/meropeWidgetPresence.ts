import type { MoodBand } from '../agent/meropeVitals'

export const STYLE_REFERENCE_PREVIEW = '/merope/style-reference.png'
export const PREVIEW_MOOD_BAND: MoodBand = 'calm'

export function previewPortraitSrc(): string {
  return STYLE_REFERENCE_PREVIEW
}

export function previewMoodBand(compact?: boolean): MoodBand | null {
  return compact ? null : PREVIEW_MOOD_BAND
}

export function shouldShowNameplate(agentName: string | null): boolean {
  return Boolean(agentName)
}

export function nameplateHidden(wantLive: boolean): boolean {
  return !wantLive
}

export function readyKeyAfterMotionChange(
  motionReady: boolean,
  currentKey: string,
): string {
  return motionReady ? currentKey : ''
}

export function liveCanvasHidden(ready: boolean): boolean {
  return !ready
}
