/** 多实例合到一帧里量；满 8ms 把剩下的丢下一帧，避开 Long Task。 */
export const FIT_TEXT_SLICE_MS = 8

const pending = new Map<HTMLElement, () => void>()
let frame = 0

function flush(): void {
  frame = 0
  const jobs = [...pending]
  pending.clear()
  const started = performance.now()
  let index = 0
  for (; index < jobs.length; index++) {
    const [el, run] = jobs[index]!
    if (el.isConnected) run()
    if (performance.now() - started >= FIT_TEXT_SLICE_MS) {
      index += 1
      break
    }
  }
  for (; index < jobs.length; index++) {
    const [el, run] = jobs[index]!
    pending.set(el, run)
  }
  if (pending.size > 0) frame = requestAnimationFrame(flush)
}

export function scheduleFitText(el: HTMLElement, run: () => void): () => void {
  pending.set(el, run)
  if (frame === 0) frame = requestAnimationFrame(flush)
  return () => {
    pending.delete(el)
    if (pending.size === 0 && frame !== 0) {
      cancelAnimationFrame(frame)
      frame = 0
    }
  }
}
