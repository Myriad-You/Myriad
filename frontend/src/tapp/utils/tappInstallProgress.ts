/** Packages at or above this size show install progress UI. */
export const LARGE_TAPP_INSTALL_BYTES = 1024 * 1024 // 1 MiB

export type TappInstallProgressPhase =
  | 'prepare'
  | 'download'
  | 'install'
  | 'done'

export interface TappInstallProgress {
  phase: TappInstallProgressPhase
  /** Human-readable stage (i18n applied by caller when needed) */
  message: string
  /** 0–100 when known; omit for indeterminate */
  percent?: number
  loadedBytes?: number
  totalBytes?: number
  /** Current file or step detail */
  detail?: string
}

export type TappInstallProgressCallback = (progress: TappInstallProgress) => void

export function isLargeTappInstall(estimatedBytes?: number | null): boolean {
  return (estimatedBytes ?? 0) >= LARGE_TAPP_INSTALL_BYTES
}

export function formatInstallBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Clamp percent into 0–100 integer. */
export function clampInstallPercent(n: number): number {
  if (!Number.isFinite(n)) return 0
  return Math.max(0, Math.min(100, Math.round(n)))
}
