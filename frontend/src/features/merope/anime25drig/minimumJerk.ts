/** A finite quintic trajectory with position/velocity/acceleration boundaries. */
export class MinimumJerkMotion {
  value = 0
  velocity = 0
  acceleration = 0
  private start = 0
  private duration = 1
  private target = 0
  private c0 = 0
  private c1 = 0
  private c2 = 0
  private c3 = 0
  private c4 = 0
  private c5 = 0

  retarget(now: number, target: number, duration: number, delay = 0): void {
    this.sample(now)
    // A moving segment cannot wait motionless for a new head-latency period.
    this.start =
      now +
      (Math.abs(this.velocity) + Math.abs(this.acceleration) < 1e-6 ? delay : 0)
    this.duration = Math.max(0.001, duration)
    this.target = target
    this.c0 = this.value
    this.c1 = this.velocity * this.duration
    this.c2 = (this.acceleration * this.duration ** 2) / 2
    const distance = target - this.c0
    this.c3 = 10 * distance - 6 * this.c1 - 3 * this.c2
    this.c4 = -15 * distance + 8 * this.c1 + 3 * this.c2
    this.c5 = 6 * distance - 3 * this.c1 - this.c2
  }

  sample(now: number): number {
    const u = Math.max(0, Math.min(1, (now - this.start) / this.duration))
    if (u >= 1 - 1e-12) {
      this.value = this.target
      this.velocity = 0
      this.acceleration = 0
    } else {
      this.value =
        this.c0 +
        u *
          (this.c1 +
            u * (this.c2 + u * (this.c3 + u * (this.c4 + u * this.c5))))
      this.velocity =
        (this.c1 +
          u *
            (2 * this.c2 +
              u * (3 * this.c3 + u * (4 * this.c4 + u * 5 * this.c5)))) /
        this.duration
      this.acceleration =
        (2 * this.c2 +
          u * (6 * this.c3 + u * (12 * this.c4 + u * 20 * this.c5))) /
        this.duration ** 2
    }
    return this.value
  }
}
