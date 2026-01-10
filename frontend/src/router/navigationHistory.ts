/**
 * 导航历史记录
 * 用于智能判断页面切换动画方向
 */

// 路由层级定义 - 数字越小优先级越高
const ROUTE_HIERARCHY: Record<string, number> = {
  '/': 0, // 首页
  '/library': 1, // 资料库
  '/config': 2, // 配置
  '/login': 3, // 登录
  '/setup': 3, // 设置（同级）
  '/details': 4, // 详情
}

// 导航栈
let navigationStack: string[] = ['/']

/**
 * 获取路由层级
 */
export function getRouteLevel(path: string): number {
  return ROUTE_HIERARCHY[path] ?? 99
}

/**
 * 判断导航方向
 * @returns 'forward' | 'back' | 'same'
 */
export function getNavigationDirection(fromPath: string, toPath: string): 'forward' | 'back' | 'same' {
  const fromLevel = getRouteLevel(fromPath)
  const toLevel = getRouteLevel(toPath)

  // 层级相同，检查是否在导航栈中
  if (fromLevel === toLevel) {
    // 如果目标路径是栈中的上一个路径，说明是后退
    const currentIndex = navigationStack.indexOf(fromPath)
    const targetIndex = navigationStack.indexOf(toPath)

    if (currentIndex > 0 && targetIndex === currentIndex - 1) {
      return 'back'
    }

    return 'same'
  }

  // 层级不同，根据层级判断
  return toLevel > fromLevel ? 'forward' : 'back'
}

/**
 * 记录导航
 */
export function recordNavigation(path: string) {
  const lastPath = navigationStack[navigationStack.length - 1]

  // 如果是后退导航，从栈中移除
  if (navigationStack.length > 1 && navigationStack[navigationStack.length - 2] === path) {
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

/**
 * 获取动画变体配置
 */
export function getAnimationVariants(direction: 'forward' | 'back' | 'same') {
  const distance = 50 // 移动距离
  const duration = 0.35 // 动画时长

  switch (direction) {
    case 'forward':
      return {
        initial: { opacity: 0, x: distance, scale: 0.98 },
        animate: { opacity: 1, x: 0, scale: 1 },
        exit: { opacity: 0, x: -distance * 0.3, scale: 0.98 },
        transition: {
          duration,
          ease: [0.4, 0.0, 0.2, 1], // cubic-bezier
        },
      }

    case 'back':
      return {
        initial: { opacity: 0, x: -distance, scale: 0.98 },
        animate: { opacity: 1, x: 0, scale: 1 },
        exit: { opacity: 0, x: distance * 0.3, scale: 0.98 },
        transition: {
          duration,
          ease: [0.4, 0.0, 0.2, 1],
        },
      }

    case 'same':
    default:
      return {
        initial: { opacity: 0, scale: 0.98 },
        animate: { opacity: 1, scale: 1 },
        exit: { opacity: 0, scale: 0.98 },
        transition: {
          duration: duration * 0.8, // 同级切换稍快
          ease: [0.4, 0.0, 0.2, 1],
        },
      }
  }
}
