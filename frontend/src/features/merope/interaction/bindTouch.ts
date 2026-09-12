import type { TouchObservation, TouchRegion } from './touchGesture'
import { TouchGestureTracker } from './touchGesture'

export function bindCharacterTouch(
  canvas: HTMLCanvasElement,
  hit: (x: number, y: number) => { region: TouchRegion | null } | null,
  emit: (touch: TouchObservation, now: number) => void,
  cancelReaction: () => void,
): () => void {
  const tracker = new TouchGestureTracker()
  let pointer: { id: number; x: number; y: number; width: number } | null = null
  let frame = 0
  const send = (observation: TouchObservation | null, now: number) => {
    if (observation && pointer) {
      const rect = canvas.getBoundingClientRect()
      emit({ ...observation, position: {
        x: Math.max(-1, Math.min(1, (pointer.x - rect.left) / Math.max(1, rect.width) * 2 - 1)),
        y: Math.max(-1, Math.min(1, (pointer.y - rect.top) / Math.max(1, rect.height) * 2 - 1)),
      } }, now)
    }
  }
  const sample = (now: number) => {
    const p = pointer!
    return { pointerId: p.id, atMs: now, x: p.x / p.width, y: p.y / p.width,
      region: document.elementFromPoint(p.x, p.y) === canvas ? hit(p.x, p.y)?.region ?? null : null }
  }
  const releaseCapture = () => {
    const id = pointer?.id
    pointer = null
    delete canvas.dataset.meropeTouchActive
    cancelAnimationFrame(frame)
    frame = 0
    if (id !== undefined && canvas.hasPointerCapture(id)) canvas.releasePointerCapture(id)
  }
  const cancel = () => {
    const now = performance.now()
    send(tracker.reset(now), now)
    releaseCapture()
    cancelReaction()
  }
  const tick = () => {
    if (!pointer) return
    const now = performance.now()
    const update = tracker.update(sample(now))
    send(update, now)
    if (update?.phase === 'cancel') releaseCapture()
    else frame = requestAnimationFrame(tick)
  }
  const down = (event: PointerEvent) => {
    if (pointer || !event.isPrimary || event.button !== 0) return
    const width = canvas.getBoundingClientRect().width
    if (width <= 0) return
    pointer = { id: event.pointerId, x: event.clientX, y: event.clientY, width }
    const now = performance.now()
    const start = tracker.begin(sample(now))
    if (!start) { pointer = null; return }
    canvas.dataset.meropeTouchActive = 'true'
    event.stopPropagation()
    try { canvas.setPointerCapture(event.pointerId) } catch { cancel(); return }
    send(start, now)
    frame = requestAnimationFrame(tick)
  }
  const move = (event: PointerEvent) => {
    if (pointer?.id !== event.pointerId) return
    pointer.x = event.clientX
    pointer.y = event.clientY
    const now = performance.now()
    const update = tracker.update(sample(now))
    send(update, now)
    if (update?.phase === 'cancel') releaseCapture()
  }
  const up = (event: PointerEvent) => {
    if (pointer?.id !== event.pointerId) return
    pointer.x = event.clientX
    pointer.y = event.clientY
    const now = performance.now()
    send(tracker.end(sample(now)), now)
    releaseCapture()
  }
  const pointerCancel = (event: PointerEvent) => { if (pointer?.id === event.pointerId) cancel() }
  const visibility = () => { if (document.hidden) cancel() }
  canvas.addEventListener('pointerdown', down)
  canvas.addEventListener('pointermove', move)
  canvas.addEventListener('pointerup', up)
  canvas.addEventListener('pointercancel', pointerCancel)
  canvas.addEventListener('lostpointercapture', pointerCancel)
  window.addEventListener('blur', cancel)
  window.addEventListener('auth-state-changed', cancel)
  document.addEventListener('visibilitychange', visibility)
  return () => {
    cancel()
    canvas.removeEventListener('pointerdown', down)
    canvas.removeEventListener('pointermove', move)
    canvas.removeEventListener('pointerup', up)
    canvas.removeEventListener('pointercancel', pointerCancel)
    canvas.removeEventListener('lostpointercapture', pointerCancel)
    window.removeEventListener('blur', cancel)
    window.removeEventListener('auth-state-changed', cancel)
    document.removeEventListener('visibilitychange', visibility)
  }
}
