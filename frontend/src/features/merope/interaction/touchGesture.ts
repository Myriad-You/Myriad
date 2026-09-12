/** Pointer evidence only */
export type TouchRegion = 'hair' | 'face' | 'body' | 'accessory'
export type TouchGesture = 'contact' | 'hold' | 'stroke' | 'tap'

export interface TouchSample {
  pointerId: number
  atMs: number
  x: number
  y: number
  region: TouchRegion | null
}

export interface TouchObservation {
  /** Canvas-relative contact, local only */
  position?: { x: number; y: number }
  id: number
  phase: 'start' | 'update' | 'end' | 'cancel'
  gesture: TouchGesture
  region: TouchRegion
  durationMs: number
  distance: number
  speed: number
  repeatCount: number
  x: number
  y: number
}

interface Contact {
  id: number
  start: TouchSample
  last: TouchSample
  region: TouchRegion
  distance: number
  speed: number
  gesture: TouchGesture
  minX: number
  maxX: number
  minY: number
  maxY: number
}

// Product thresholds, not biological constants.
const HOLD_MS = 400
const STROKE_DISTANCE = 0.12
const TAP_DISTANCE = 0.035
const REPEAT_GAP_MS = 850

export class TouchGestureTracker {
  private sequence = 0
  private contact: Contact | null = null
  private previousTap: {
    atMs: number
    region: TouchRegion
    count: number
  } | null = null

  begin(sample: TouchSample): TouchObservation | null {
    if (this.contact || !validSample(sample) || !sample.region) return null
    if (this.previousTap && (this.previousTap.region !== sample.region
      || sample.atMs - this.previousTap.atMs > REPEAT_GAP_MS
      || sample.atMs < this.previousTap.atMs)) { this.previousTap = null
}
    this.contact = {
      id: ++this.sequence,
      start: { ...sample },
      last: { ...sample },
      region: sample.region,
      distance: 0,
      speed: 0,
      gesture: 'contact',
      minX: sample.x,
      maxX: sample.x,
      minY: sample.y,
      maxY: sample.y,
    }
    return this.observe('start')
  }

  update(sample: TouchSample): TouchObservation | null {
    const contact = this.contact
    if (
      !contact ||
      !validSample(sample) ||
      sample.pointerId !== contact.last.pointerId ||
      sample.atMs < contact.last.atMs
    ) {
      return null
}
    if (!sample.region) return this.cancel(sample.atMs)
    const dt = sample.atMs - contact.last.atMs
    // A duplicate timestamp cannot manufacture velocity or path length.
    if (dt === 0) return null
    const distance = Math.hypot(
      sample.x - contact.last.x,
      sample.y - contact.last.y,
    )
    contact.distance += distance
    contact.speed +=
      (distance / (dt / 1000) - contact.speed) * -Math.expm1(-dt / 120)
    if (contact.region !== sample.region) this.previousTap = null
    contact.region = sample.region
    contact.last = { ...sample }
    contact.minX = Math.min(contact.minX, sample.x)
    contact.maxX = Math.max(contact.maxX, sample.x)
    contact.minY = Math.min(contact.minY, sample.y)
    contact.maxY = Math.max(contact.maxY, sample.y)
    const elapsed = sample.atMs - contact.start.atMs
    const span = Math.hypot(
      contact.maxX - contact.minX,
      contact.maxY - contact.minY,
    )
    // Tremor can accumulate a long path without being an intentional stroke.
    if (elapsed >= 120 && contact.distance >= STROKE_DISTANCE && span >= 0.06)
      contact.gesture = 'stroke'
    else if (elapsed >= HOLD_MS) contact.gesture = 'hold'
    return this.observe('update')
  }

  end(sample: TouchSample): TouchObservation | null {
    const contact = this.contact
    if (
      !contact ||
      !validSample(sample) ||
      sample.pointerId !== contact.last.pointerId ||
      sample.atMs < contact.last.atMs
    ) {
      return null
}
    const update = this.update(sample)
    if (update?.phase === 'cancel') return update
    if (
      sample.atMs - contact.start.atMs < HOLD_MS &&
      contact.distance <= TAP_DISTANCE
    ) {
      contact.gesture = 'tap'
      const previous = this.previousTap
      this.previousTap = {
        atMs: sample.atMs,
        region: contact.region,
        count:
          previous?.region === contact.region &&
          sample.atMs - previous.atMs >= 0 &&
          sample.atMs - previous.atMs <= REPEAT_GAP_MS
            ? Math.min(8, previous.count + 1)
            : 1,
      }
    } else { this.previousTap = null
}
    const result = this.observe('end')
    this.contact = null
    return result
  }

  cancel(atMs: number): TouchObservation | null {
    const contact = this.contact
    if (!contact) return null
    if (Number.isFinite(atMs))
      contact.last.atMs = Math.max(contact.last.atMs, atMs)
    const result = this.observe('cancel')
    this.contact = null
    this.previousTap = null
    return result
  }

  reset(atMs: number): TouchObservation | null {
    const result = this.cancel(atMs)
    this.previousTap = null
    return result
  }

  private observe(phase: TouchObservation['phase']): TouchObservation {
    const contact = this.contact!
    return {
      id: contact.id,
      phase,
      gesture: contact.gesture,
      region: contact.region,
      durationMs: contact.last.atMs - contact.start.atMs,
      distance: contact.distance,
      speed: contact.speed,
      repeatCount:
        contact.gesture === 'tap' ? (this.previousTap?.count ?? 1) : (this.previousTap?.count ?? 0),
      x: contact.last.x,
      y: contact.last.y,
    }
  }
}

function validSample(sample: TouchSample): boolean {
  return (
    Number.isInteger(sample.pointerId) &&
    Number.isFinite(sample.atMs) &&
    sample.atMs >= 0 &&
    Number.isFinite(sample.x) &&
    Number.isFinite(sample.y)
  )
}
