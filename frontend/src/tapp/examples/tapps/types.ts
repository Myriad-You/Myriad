import type { TappPlaygroundCode } from '../../services/TappPlaygroundService'
import type { TappManifest } from '../../types'

export type { TappPlaygroundCode } from '../../services/TappPlaygroundService'

export interface ExampleTapp {
  manifest: TappManifest
  code: TappPlaygroundCode
  tags: string[]
}
