import { isInternalDump, isUselessErrorText } from './uselessErrorText'

/**
 * 错误码表按需加载；它自己没加载下来时（断网、刚发版旧分块已删），出错的地方照样要有一句话。
 * 退回时只带上能看懂的原始说明，不把内部堆栈和「HTTP 500」这类空话交给用户。
 */
export function formatWithoutErrorTable(reason: unknown, fallback?: string): string {
  const raw = reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : ''
  const detail = raw.trim()
  const readable = detail && !isUselessErrorText(detail) && !isInternalDump(detail) ? detail : ''
  if (fallback && readable) return `${fallback}: ${readable}`
  return fallback || readable || detail || String(reason)
}

export async function formatUserFacingError(
  reason: unknown,
  fallback?: string,
): Promise<string> {
  try {
    const { userFacingError } = await import('./userFacingError')
    return userFacingError(reason, fallback)
  } catch {
    return formatWithoutErrorTable(reason, fallback)
  }
}
