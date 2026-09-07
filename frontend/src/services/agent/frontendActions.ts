/**
 * 前端动作处理器
 *
 * 处理 Agent 返回的前端操作指令
 */

import type { FrontendAction, FrontendActionType } from './types'

/** 前端操作处理器类型 */
export type FrontendActionHandler = (action: FrontendAction) => Promise<unknown>

/** 分类型处理器注册表 */
const typedActionHandlers = new Map<
  FrontendActionType | string,
  FrontendActionHandler
>()

/** 全局处理器集合（处理所有类型） */
const globalActionHandlers = new Set<FrontendActionHandler>()

/**
 * 注册前端操作处理器
 *
 * 支持两种模式：
 * 1. 分类型处理器：registerActionHandler('open_window', handler)
 * 2. 全局处理器：registerActionHandler(handler)
 *
 * @example
 * // 分类型处理器
 * registerActionHandler('navigate', async (action) => {
 *   router.push(action.path);
 * });
 *
 * // 全局处理器
 * registerActionHandler(async (action) => {
 *   console.log('Action:', action.type);
 * });
 */
export function registerActionHandler(
  typeOrHandler: FrontendActionType | string | FrontendActionHandler,
  handler?: FrontendActionHandler,
): void {
  if (typeof typeOrHandler === 'function') {
    globalActionHandlers.add(typeOrHandler)
  } else if (handler) {
    typedActionHandlers.set(typeOrHandler, handler)
  }
}

/**
 * 注销前端操作处理器
 */
export function unregisterActionHandler(
  typeOrHandler: FrontendActionType | string | FrontendActionHandler,
): void {
  if (typeof typeOrHandler === 'function') {
    globalActionHandlers.delete(typeOrHandler)
  } else {
    typedActionHandlers.delete(typeOrHandler)
  }
}

/**
 * 执行前端操作
 *
 * 当 Agent 返回 frontendAction 时调用
 * 优先使用分类型处理器，否则使用全局处理器
 */
export async function executeFrontendAction(
  action: FrontendAction,
): Promise<unknown> {
  // 优先使用分类型处理器
  const typedHandler = typedActionHandlers.get(action.type)
  if (typedHandler) {
    return typedHandler(action)
  }

  // 尝试全局处理器
  for (const handler of globalActionHandlers) {
    try {
      const result = await handler(action)
      if (result !== false && result !== undefined) {
        return result
      }
    } catch (e) {
      console.warn('[FrontendActions] Global handler error:', e)
    }
  }

  console.warn(
    `[FrontendActions] No handler registered for action type: ${action.type}`,
  )
  return null
}

/**
 * 检查是否有特定类型的处理器
 */
export function hasActionHandler(type: FrontendActionType | string): boolean {
  return typedActionHandlers.has(type) || globalActionHandlers.size > 0
}

/** Stable key so step_completed and the final response do not run the same action twice. */
export function frontendActionDedupeKey(
  action: Pick<FrontendAction, 'type' | 'timestamp'>,
): string {
  if (typeof action.timestamp === 'number') {
    return `${action.type}:${action.timestamp}`
  }
  return JSON.stringify(action)
}

/**
 * 获取所有已注册的处理器类型
 */
export function getRegisteredActionTypes(): string[] {
  return [...typedActionHandlers.keys()]
}

/**
 * 清空所有处理器（用于测试）
 */
export function clearAllHandlers(): void {
  typedActionHandlers.clear()
  globalActionHandlers.clear()
}
