import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'

/** Idle floor for gaze and head/body at ambient priority. */
export class AmbientMotionSource {
  private handle: MotionLeaseHandle | null = null

  constructor(private readonly coordinator: RigMotionCoordinator) {}

  claim(): void {
    this.handle =
      this.coordinator.renew(this.handle, ['gaze', 'headBody']) ??
      this.coordinator.claim('ambient', ['gaze', 'headBody'])
  }

  release(): void {
    this.coordinator.release(this.handle)
    this.handle = null
  }
}
