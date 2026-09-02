import type { PerceptionAdapter } from './types'
import { capturePerceptionSnapshots } from '../perception/capture'

export class LocalPerceptionAdapter implements PerceptionAdapter {
  capture(
    input: Parameters<PerceptionAdapter['capture']>[0],
  ): ReturnType<PerceptionAdapter['capture']> {
    return capturePerceptionSnapshots(input)
  }
}
