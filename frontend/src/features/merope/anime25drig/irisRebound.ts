/** Uses the existing blink/player clock, not a new timer or director channel. */
export class Anime25DIrisRebound {
  x = 1
  y = 1
  private previousBlink = -1
  private startedAt = -Infinity
  private pendingUntil = -Infinity

  step(
    time: number,
    blinkSeconds: number,
    suppressed: boolean,
    visibleEyeOpen: number,
  ): void {
    if (suppressed) {
      this.startedAt = -Infinity
      this.pendingUntil = -Infinity
    } else if (blinkSeconds >= 0.42 && this.previousBlink < 0.42) {
      this.pendingUntil = time + 0.35
    }
    if (!suppressed && time <= this.pendingUntil && visibleEyeOpen >= 0.6) {
      this.startedAt = time
      this.pendingUntil = -Infinity
    }
    this.previousBlink = blinkSeconds
    const phase = (time - this.startedAt) / 0.52
    this.x = 1
    this.y = 1
    if (phase <= 0 || phase >= 1) return
    const attack = Math.min(1, phase / 0.12)
    const envelope =
      attack *
      attack *
      (3 - 2 * attack) *
      Math.exp(-2.2 * phase) *
      (1 - phase) ** 2
    const scale = 0.045 * Math.sin(phase * Math.PI * 4) * envelope
    const squash = 0.025 * Math.sin(phase * Math.PI * 6) * envelope
    this.x += scale + squash
    this.y += scale - squash
  }
}
