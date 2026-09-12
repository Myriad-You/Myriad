import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TappStorageChange } from '../../WidgetRuntimeSignals'
import { userFacingError } from '../../../../utils/userFacingError'
import { sanitizeStorageValue, validateStorageKey } from '../security'

interface KvResult {
  success: boolean
  data?: unknown
  error?: string
}

function argsOf(message: TappMessage): unknown[] {
  return (message.payload as { args?: unknown[] } | undefined)?.args || []
}

function fail(error: string): KvResult {
  return { success: false, error }
}

function ok(data: unknown = null): KvResult {
  return { success: true, data }
}

async function wrap(run: () => Promise<unknown>): Promise<KvResult> {
  try {
    return ok(await run())
  } catch (error) {
    return fail(userFacingError(error))
  }
}

function requireKey(raw: unknown): { key: string } | KvResult {
  if (!raw) return fail('Key is required')
  const keyValidation = validateStorageKey(raw as string)
  if (!keyValidation.valid) {
    return fail(`Invalid key: ${keyValidation.reason}`)
  }
  return { key: raw as string }
}

function isKeyed(
  parsed: { key: string } | KvResult,
): parsed is { key: string } {
  return Object.hasOwn(parsed, 'key')
}

export interface FullKvOps {
  get: (tappId: string, key: string, grant?: string) => Promise<unknown>
  set: (
    tappId: string,
    key: string,
    value: unknown,
    grant?: string,
  ) => Promise<void>
  remove: (tappId: string, key: string, grant?: string) => Promise<void>
  keys: (tappId: string, grant?: string) => Promise<string[]>
  getAll: (
    tappId: string,
    grant?: string,
  ) => Promise<Record<string, unknown>>
  clear: (tappId: string, grant?: string) => Promise<void>
  usage: (
    tappId: string,
    grant?: string,
  ) => Promise<{ used: number; quota: number }>
}

export function registerFullKvHandlers(
  bridge: TappBridge,
  tappId: string,
  api: 'storage' | 'shared' | 'private',
  ops: FullKvOps,
  emit: (change: TappStorageChange) => void,
  options: { maxValueSize: number; withGrant: boolean },
): void {
  const grant = async () =>
    options.withGrant ? await bridge.getRuntimeGrant() : undefined

  bridge.registerHandler(`${api}.get`, async (message) => {
    const parsed = requireKey(argsOf(message)[0])
    if (!isKeyed(parsed)) return parsed
    return wrap(async () => ops.get(tappId, parsed.key, await grant()))
  })

  bridge.registerHandler(`${api}.set`, async (message) => {
    const [rawKey, rawValue] = argsOf(message)
    const parsed = requireKey(rawKey)
    if (!isKeyed(parsed)) return parsed
    const sanitizedValue = sanitizeStorageValue(rawValue)
    let valueSize: number
    try {
      const serialized = JSON.stringify(sanitizedValue)
      if (serialized === undefined) {
        return fail('Value is not JSON-serializable')
      }
      valueSize = serialized.length
    } catch {
      return fail('Value is not JSON-serializable')
    }
    if (valueSize > options.maxValueSize) {
      return fail(
        `Value too large: ${valueSize} bytes (max ${options.maxValueSize})`,
      )
    }
    return wrap(async () => {
      await ops.set(tappId, parsed.key, sanitizedValue, await grant())
      emit({
        tappId,
        key: parsed.key,
        operation: 'set',
        source: bridge,
      })
      return null
    })
  })

  bridge.registerHandler(`${api}.remove`, async (message) => {
    const parsed = requireKey(argsOf(message)[0])
    if (!isKeyed(parsed)) return parsed
    return wrap(async () => {
      await ops.remove(tappId, parsed.key, await grant())
      emit({
        tappId,
        key: parsed.key,
        operation: 'remove',
        source: bridge,
      })
      return null
    })
  })

  bridge.registerHandler(`${api}.keys`, async () =>
    wrap(async () => ops.keys(tappId, await grant())),
  )

  bridge.registerHandler(`${api}.getAll`, async () =>
    wrap(async () => ops.getAll(tappId, await grant())),
  )

  bridge.registerHandler(`${api}.clear`, async () =>
    wrap(async () => {
      await ops.clear(tappId, await grant())
      emit({ tappId, operation: 'clear', source: bridge })
      return null
    }),
  )

  bridge.registerHandler(`${api}.usage`, async () =>
    wrap(async () => ops.usage(tappId, await grant())),
  )
}
