const BLOBS = [
  { fx: 0.41, fy: 0.29, fs: 0.23, ax: 34, ay: 30, as: 0.16 },
  { fx: 0.53, fy: 0.37, fs: 0.19, ax: 38, ay: 28, as: 0.14 },
  { fx: 0.31, fy: 0.47, fs: 0.27, ax: 30, ay: 36, as: 0.18 },
] as const

export function attachOrbDriftSpeed(status: string): number {
  switch (status) {
    case 'working':
      return 0.85
    case 'error':
      return 0.62
    case 'listening':
      return 0.52
    case 'needsInput':
      return 0.4
    case 'done':
      return 0.28
    default:
      return 0.42
  }
}

export function startAttachOrbDrift(
  el: HTMLElement,
  speed: () => number,
): () => void {
  const blobs = Iterator.from(
    el.querySelectorAll<HTMLElement>('[data-orb-blob]'),
  ).toArray()
  if (blobs.length === 0) return () => {}

  const seed = Math.random() * Math.PI * 2
  let started = performance.now()
  let hiddenAt = 0
  let frame = 0

  const tick = (now: number) => {
    if (document.hidden) {
      frame = 0
      return
    }
    const t = ((now - started) / 1000) * speed()
    for (let i = 0; i < blobs.length; i += 1) {
      const spec = BLOBS[i] ?? BLOBS[0]
      const phase = seed + i * 2.15
      const x =
        Math.sin(t * spec.fx + phase) * spec.ax +
        Math.sin(t * spec.fx * 0.37 + phase * 1.7) * spec.ax * 0.35
      const y =
        Math.cos(t * spec.fy + phase * 1.3) * spec.ay +
        Math.sin(t * spec.fy * 0.43 + phase) * spec.ay * 0.32
      const scale = 1.08 + Math.sin(t * spec.fs + phase * 0.8) * spec.as
      blobs[i].style.transform = `translate(${x}%, ${y}%) scale(${scale})`
    }
    frame = requestAnimationFrame(tick)
  }

  const onVis = () => {
    if (document.hidden) {
      hiddenAt = performance.now()
      if (frame) cancelAnimationFrame(frame)
      frame = 0
      return
    }
    if (hiddenAt) {
      started += performance.now() - hiddenAt
      hiddenAt = 0
    }
    if (!frame) frame = requestAnimationFrame(tick)
  }

  document.addEventListener('visibilitychange', onVis)
  frame = requestAnimationFrame(tick)
  return () => {
    document.removeEventListener('visibilitychange', onVis)
    if (frame) cancelAnimationFrame(frame)
    for (const blob of blobs) blob.style.removeProperty('transform')
  }
}
