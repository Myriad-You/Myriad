import type { FrontendAction, FrontendActionType } from './types'
import { authSubject } from '../../utils/authSubject'

export type FrontendActionHandler = (action: FrontendAction, signal?: AbortSignal) => Promise<unknown>

const typedActionHandlers = new Map<
  FrontendActionType | string,
  FrontendActionHandler
>()

const globalActionHandlers = new Set<FrontendActionHandler>()

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

export function unregisterActionHandler(
  typeOrHandler: FrontendActionType | string | FrontendActionHandler,
): void {
  if (typeof typeOrHandler === 'function') {
    globalActionHandlers.delete(typeOrHandler)
  } else {
    typedActionHandlers.delete(typeOrHandler)
  }
}

export async function executeFrontendAction(
  action: FrontendAction,
  signal = authSubject.signal,
): Promise<unknown> {
  if (signal.aborted) return
  const typedHandler = typedActionHandlers.get(action.type)
  if (typedHandler) {
    const result = await typedHandler(action, signal)
    return signal.aborted ? undefined : result
  }

  for (const handler of globalActionHandlers) {
    if (signal.aborted) return
    try {
      const result = await handler(action, signal)
      if (signal.aborted) return
      if (result !== false && result !== undefined) {
        return result
      }
    } catch (e) {
      if (signal.aborted) return
      console.warn('[FrontendActions] Global handler error:', e)
    }
  }

  console.warn(
    `[FrontendActions] No handler registered for action type: ${action.type}`,
  )
  return null
}

export function hasActionHandler(type: FrontendActionType | string): boolean {
  return typedActionHandlers.has(type) || globalActionHandlers.size > 0
}

/** Dedupe step_completed vs final response. */
export function frontendActionDedupeKey(
  action: Pick<FrontendAction, 'type' | 'timestamp'>,
): string {
  if (typeof action.timestamp === 'number') {
    return `${action.type}:${action.timestamp}`
  }
  return JSON.stringify(action)
}

export function getRegisteredActionTypes(): string[] {
  return Iterator.from(typedActionHandlers.keys()).toArray()
}

export function clearAllHandlers(): void {
  typedActionHandlers.clear()
  globalActionHandlers.clear()
}
