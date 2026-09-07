/**
 * 舞台还开着时，先让舞台退完再切页。
 * NavigationIsland 走这里；BrowserRouter 没有 data-router，用不上 useBlocker。
 */

type Proceed = () => void

let handler: ((proceed: Proceed) => void) | null = null
let pending = false

export function setStageLeaveHandler(
  next: ((proceed: Proceed) => void) | null,
) {
  handler = next
}

/** @returns true：这次导航已被接管，调用方不要自己 navigate */
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
