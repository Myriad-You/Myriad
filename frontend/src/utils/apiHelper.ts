import { currentCopy } from '../i18n/localeCopy'
import { ApiError, parseApiErrorBody } from '../services/api'
import { httpStatusMessage, isUselessErrorText } from './userFacingError'

/**
 * 安全地解析 JSON 响应，处理各种错误情况
 * @param response Fetch API 响应对象
 * @returns 解析后的 JSON 数据
 * @throws 抛出包含错误信息的 Error
 */
export async function parseJsonResponse(response: Response): Promise<any> {
  const contentType = response.headers.get('content-type')
  const hasJson = contentType && contentType.includes('application/json')

  if (!hasJson) {
    // 响应不是 JSON 格式
    const text = await response.text()
    throw new Error(
      text || response.statusText || httpStatusMessage(response.status),
    )
  }

  try {
    return await response.json()
  } catch (_error) {
    throw new Error(
      currentCopy().errors.invalidResponse.replace(
        '{status}',
        String(response.status),
      ),
    )
  }
}

/**
 * 处理 API 错误响应
 * @param response Fetch API 响应对象
 * @param defaultMessage 默认错误消息
 * @returns 永不返回，总是抛出错误
 * @throws 抛出包含错误信息的 Error
 */
export async function handleErrorResponse(
  response: Response,
  defaultMessage: string = currentCopy().errors.operationFailed,
): Promise<never> {
  const contentType = response.headers.get('content-type')
  const hasJson = contentType && contentType.includes('application/json')

  if (hasJson) {
    try {
      const parsed = parseApiErrorBody(await response.json(), response.status)
      const message =
        !isUselessErrorText(parsed.message)
          ? parsed.message
          : defaultMessage
      throw new ApiError(
        message,
        response.status,
        parsed.code,
        parsed.details,
        parsed.hint,
      )
    } catch (error) {
      if (error instanceof ApiError) throw error
      throw new ApiError(
        `${defaultMessage} (${response.status})`,
        response.status,
      )
    }
  }

  const text = await response.text()
  throw new ApiError(
    text || response.statusText || httpStatusMessage(response.status),
    response.status,
  )
}

/** 开发代理或网络层可能短暂产生的瞬时状态码。 */
const TRANSIENT_STATUSES = new Set([502, 503, 504])
const MAX_RETRIES = 3

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** 仅幂等方法可安全重试，避免 POST 等重复提交 */
function isIdempotent(method?: string): boolean {
  const m = (method ?? 'GET').toUpperCase()
  return m === 'GET' || m === 'HEAD'
}

/**
 * 执行 API 请求并安全地处理响应
 *
 * 幂等请求（GET/HEAD）在遇到网络层错误或瞬时 5xx 时自动重试。
 *
 * @param url 请求 URL
 * @param options Fetch 选项
 * @param errorMessage 错误时的默认消息
 * @returns 解析后的 JSON 数据
 * @throws 抛出包含错误信息的 Error
 */
export async function fetchJson<T = any>(
  url: string,
  options?: RequestInit,
  errorMessage: string = currentCopy().errors.requestFailed,
): Promise<T> {
  const retryable = isIdempotent(options?.method)

  for (let attempt = 0; ; attempt++) {
    try {
      const response = await fetch(url, {
        credentials: 'include',
        ...options,
      })

      if (
        retryable &&
        TRANSIENT_STATUSES.has(response.status) &&
        attempt < MAX_RETRIES
      ) {
        await sleep(150 * (attempt + 1))
        continue
      }

      if (!response.ok) {
        await handleErrorResponse(response, errorMessage)
      }

      return await parseJsonResponse(response)
    } catch (error) {
      // fetch 自身抛出 = 网络层错误（连接被拒 / socket hang up）。
      const isNetworkError = error instanceof TypeError
      if (retryable && isNetworkError && attempt < MAX_RETRIES) {
        await sleep(150 * (attempt + 1))
        continue
      }
      if (error instanceof Error) {
        throw error
      }
      throw new Error(errorMessage)
    }
  }
}
