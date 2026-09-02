/**
 * 导航历史记录
 * 用于智能判断页面切换动画方向
 */

// 导航栈
let navigationStack: string[] = ['/']

/**
 * 记录导航
 */
export function recordNavigation(path: string) {
  const lastPath = navigationStack[navigationStack.length - 1]

  // 如果是后退导航，从栈中移除
  if (
    navigationStack.length > 1 &&
    navigationStack[navigationStack.length - 2] === path
  ) {
    navigationStack.pop()
    return
  }

  // 前进导航，添加到栈
  if (lastPath !== path) {
    navigationStack.push(path)

    // 限制栈深度
    if (navigationStack.length > 10) {
      navigationStack = navigationStack.slice(-10)
    }
  }
}
