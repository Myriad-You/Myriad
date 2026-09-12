// 舞台开着时先退完再切页。BrowserRouter 无 data-router，不能用 useBlocker。

type Proceed = () => void

let handler: ((proceed: Proceed) => void) | null = null
let pending = false

export function setStageLeaveHandler(
  next: ((proceed: Proceed) => void) | null,
) {
  handler = next
}

// true：导航已接管，调用方不要自己 navigate。
export function navigateAfterStageLeave(proceed: Proceed): boolean {
  if (pending) return true
  if (!handler) return false
  pending = true
  handler(() => {
    pending = false
    proceed()
  })
  return true
}
