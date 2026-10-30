import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'

export class SpeechMotionLease {
  private handle: MotionLeaseHandle | null = null

  constructor(private readonly coordinator: RigMotionCoordinator) {}

  setBusy(busy: boolean): void {
    if (busy) {
      this.handle ??= this.coordinator.claim('speech', ['mouth'])
      return
    }
    this.release()
  }

  release(): void {
    this.coordinator.release(this.handle)
    this.handle = null
  }
}
