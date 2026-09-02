import type { MotionChannel } from './channels'
import type { MotionLeaseHandle } from './coordinator'
import type { MotionChannelPolicy } from './policy'
import { RigMotionCoordinator } from './coordinator'
import { policyFromOwners } from './policy'

const PREVIEW_CHANNELS: readonly MotionChannel[] = [
  'mouth',
  'expression',
  'gaze',
  'headBody',
]

/**
 * Isolated workbench scope. Preview outranks every live source on this
 * coordinator only; production faces never see these leases.
 */
export class PreviewMotionScope {
  readonly coordinator = new RigMotionCoordinator()
  private handle: MotionLeaseHandle | null = null

  take(channels: readonly MotionChannel[] = PREVIEW_CHANNELS): void {
    this.handle =
      this.coordinator.renew(this.handle, channels) ??
      this.coordinator.claim('preview', channels)
  }

  policy(): MotionChannelPolicy {
    return policyFromOwners(this.coordinator.snapshot().owners)
  }

  release(): void {
    this.coordinator.release(this.handle)
    this.handle = null
  }
}
