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
    throw new Error(text || response.statusText || `HTTP ${response.status}`)
  }

  try {
    return await response.json()
  }
  catch (error) {
    throw new Error(`服务器返回了无效的响应格式 (${response.status})`)
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
  defaultMessage: string = '操作失败',
): Promise<never> {
  const contentType = response.headers.get('content-type')
  const hasJson = contentType && contentType.includes('application/json')

  let errorMessage = defaultMessage

  if (hasJson) {
    try {
      const errorData = await response.json()
      errorMessage = errorData.message || errorData.error || defaultMessage
    }
    catch (jsonError) {
      errorMessage = `${defaultMessage} (${response.status})`
    }
  }
  else {
    const text = await response.text()
    errorMessage = text || response.statusText || `HTTP ${response.status}`
  }

  throw new Error(errorMessage)
}

/**
 * 执行 API 请求并安全地处理响应
 * @param url 请求 URL
 * @param options Fetch 选项
 * @param errorMessage 错误时的默认消息
 * @returns 解析后的 JSON 数据
 * @throws 抛出包含错误信息的 Error
 */
export async function fetchJson<T = any>(
  url: string,
  options?: RequestInit,
  errorMessage: string = '请求失败',
): Promise<T> {
  try {
    const response = await fetch(url, options)

    if (!response.ok) {
      await handleErrorResponse(response, errorMessage)
    }

    return await parseJsonResponse(response)
  }
  catch (error) {
    if (error instanceof Error) {
      throw error
    }
    throw new Error(errorMessage)
  }
}
