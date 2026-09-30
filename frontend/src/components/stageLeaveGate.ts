// 舞台开着时先退完再切页。BrowserRouter 无 data-router，不能用 useBlocker。

type Proceed = () => void

let handler: ((proceed: Proceed) => void) | null = null
// 退场中再点导航只换目标，退完走最后一次（#599）。
let queued: Proceed | null = null

export function setStageLeaveHandler(
  next: ((proceed: Proceed) => void) | null,
) {
  handler = next
}

// true：导航已接管，调用方不要自己 navigate。
export function navigateAfterStageLeave(proceed: Proceed): boolean {
  if (queued) {
    queued = proceed
    return true
  }
  if (!handler) return false
  queued = proceed
  handler(() => {
    const latest = queued
    queued = null
    latest?.()
  })
  return true
}
