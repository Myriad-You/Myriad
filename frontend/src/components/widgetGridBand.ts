import { HOME_STANDARD_COLS } from '../utils/homeLayout'
import {
  HOME_GRID_COLS_PHONE,
  resolveHomeGridColumns,
} from '../utils/viewportBands'

export const GRID_BAND_OUT_MS = 160
export const GRID_BAND_IN_MS = 220

export type BandSwitchPhase = 'out' | 'in' | null

export type BandMorphDecision =
  | { type: 'already-current' }
  | { type: 'busy' }
  | { type: 'hard-apply'; columns: number }
  | { type: 'fade-out' }

export type LockedHomeGridColumns =
  | { locked: true; columns?: number }
  | { locked: false }

export function shouldHardCutHomeBand(input: {
  hardCut: boolean
  settledOnce: boolean
  from: number
  desired: number
}): boolean {
  return (
    input.hardCut ||
    !input.settledOnce ||
    input.from === HOME_GRID_COLS_PHONE ||
    input.desired === HOME_GRID_COLS_PHONE
  )
}

export function decideHomeBandMorph(input: {
  desired: number
  previous: number
  switching: boolean
  settledOnce: boolean
  hardCut: boolean
}): BandMorphDecision {
  if (input.desired === input.previous) return { type: 'already-current' }
  if (input.switching) return { type: 'busy' }
  if (
    shouldHardCutHomeBand({
      hardCut: input.hardCut,
      settledOnce: input.settledOnce,
      from: input.previous,
      desired: input.desired,
    })
  ) {
    return { type: 'hard-apply', columns: input.desired }
  }
  return { type: 'fade-out' }
}

export function lockedHomeGridColumns(input: {
  isFreeLayout: boolean
  customGridColumns?: number
}): LockedHomeGridColumns {
  if (input.isFreeLayout || input.customGridColumns) {
    return input.customGridColumns
      ? { locked: true, columns: input.customGridColumns }
      : { locked: true }
  }
  return { locked: false }
}

export function readInitialHomeGridColumns(custom?: number): number {
  if (custom) return custom
  if (typeof window === 'undefined') return HOME_STANDARD_COLS
  return resolveHomeGridColumns(window.innerWidth, 0)
}

export function homeGridGeometryMotion(input: {
  exlight: boolean
  bandSwitch: BandSwitchPhase
  motionMode: string
  layoutMode: string
}): boolean {
  return (
    !input.exlight &&
    input.bandSwitch === null &&
    input.motionMode === input.layoutMode
  )
}
