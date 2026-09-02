import type { Anime25DDriver } from './driver'
import type { Anime25DDebugSnapshot } from './player'

/** Anime2.5D-only authoring and diagnostics; never used by production motion. */
export interface Anime25DWorkbenchPort {
  setDriver: (partial: Partial<Anime25DDriver>) => void
  replaceDriver: (driver: Anime25DDriver) => void
  blinkNow: () => void
  debugSnapshot: () => Anime25DDebugSnapshot | null
}
