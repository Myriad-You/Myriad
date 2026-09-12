export const LARGE_TAPP_INSTALL_BYTES = 1024 * 1024

export type TappInstallProgressPhase =
  | 'prepare'
  | 'download'
  | 'install'
  | 'done'

export interface TappInstallProgress {
  phase: TappInstallProgressPhase
  message: string
  percent?: number
  loadedBytes?: number
  totalBytes?: number
  detail?: string
}

export type TappInstallProgressCallback = (progress: TappInstallProgress) => void

export function isLargeTappInstall(estimatedBytes?: number | null): boolean {
  return (estimatedBytes ?? 0) >= LARGE_TAPP_INSTALL_BYTES
}

export function clampInstallPercent(n: number): number {
  if (!Number.isFinite(n)) return 0
  return Math.max(0, Math.min(100, Math.round(n)))
}
