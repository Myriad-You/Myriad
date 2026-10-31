import type { TappManifest } from '@myriad/tapp-runtime'
import type { TappPlaygroundCode } from '../../services/TappPlaygroundService'

export type { TappPlaygroundCode } from '../../services/TappPlaygroundService'

export interface ExampleTapp {
  manifest: TappManifest
  code: TappPlaygroundCode
  tags: string[]
}
