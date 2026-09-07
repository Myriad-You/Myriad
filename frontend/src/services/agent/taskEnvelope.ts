/**
 * Tapp AI Task envelope: `{ format, value, contextProvenance }`.
 *
 * Agent AI handlers that share that contract wrap their payload this way.
 * Consumers that want `summary` / `analysis` / `url` look inside `value`.
 */

export function taskInnerValue(data: unknown): unknown {
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const obj = data as Record<string, unknown>
    if (typeof obj.format === 'string' && 'value' in obj) {
      return obj.value
    }
  }
  return data
}

export function messageFromStepOutput(data: unknown): string | undefined {
  const inner = taskInnerValue(data)
  if (typeof inner === 'string' && inner) {
    return inner
  }
  if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
    const obj = inner as Record<string, unknown>
    return ['reply', 'aiSummary', 'analysis', 'summary', 'message']
      .map((key) => obj[key])
      .find((value): value is string => typeof value === 'string' && value.length > 0)
  }
  return undefined
}

export function imageUrlFromStepOutput(data: unknown): string | undefined {
  const candidates: unknown[] = []
  const inner = taskInnerValue(data)
  if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
    const obj = inner as Record<string, unknown>
    candidates.push(obj.url, obj.imageUrl)
  }
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    candidates.push((data as Record<string, unknown>).imageUrl)
  }
  return candidates.find(
    (value): value is string => typeof value === 'string' && value.length > 0,
  )
}

export function imageUrlsFromAgentPayload(
  data: unknown,
  stepHistory?: ReadonlyArray<{ imageUrl?: unknown }> | null,
): string[] {
  const urls: string[] = []
  const push = (url: string | undefined) => {
    if (url && !urls.includes(url)) urls.push(url)
  }
  push(imageUrlFromStepOutput(data))
  if (stepHistory) {
    for (const step of stepHistory) {
      if (typeof step.imageUrl === 'string') push(step.imageUrl)
    }
  }
  return urls
}
