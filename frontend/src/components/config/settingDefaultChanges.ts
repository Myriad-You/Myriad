import type { SettingDefaultChangeNotice } from '../settings/SettingsDefaultsContext'

const STORAGE_KEY = 'myriad_setting_default_notices_v1'

export const SETTING_PRODUCT_DEFAULTS: Readonly<Record<string, string>> = {
  ai_model: 'minimax/minimax-m3',
  pro_ai_model: 'anthropic/claude-opus-5.5',
  ai_image_model: 'openai/gpt-image-2.5-sunburst',
}

interface StoredState {
  known: Record<string, string>
  dismissed: string[]
}

const listeners = new Set<() => void>()

function notify(): void {
  for (const fn of listeners) {
    try {
      fn()
    } catch {
      /* ignore */
    }
  }
}

export function subscribeSettingDefaultChanges(
  listener: () => void,
): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function readState(): StoredState {
  if (typeof localStorage === 'undefined') {
    return { known: {}, dismissed: [] }
  }
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) {
      return seedInitialState()
    }
    const parsed = JSON.parse(raw) as Partial<StoredState>
    return {
      known:
        parsed.known && typeof parsed.known === 'object' ? parsed.known : {},
      dismissed: Array.isArray(parsed.dismissed)
        ? parsed.dismissed.filter((x): x is string => typeof x === 'string')
        : [],
    }
  } catch {
    return seedInitialState()
  }
}

function writeState(state: StoredState, options?: { silent?: boolean }): void {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    /* quota / private mode */
  }
  if (!options?.silent) notify()
}

/** first run: today's defaults are known, so nothing is a change yet */
function seedInitialState(): StoredState {
  const state: StoredState = {
    known: { ...SETTING_PRODUCT_DEFAULTS },
    dismissed: [],
  }
  // write without notifying; first-paint storage read must not rerender
  writeState(state, { silent: true })
  return state
}

function transitionId(key: string, from: string, to: string): string {
  return `${key}:${from}→${to}`
}

/** notice when product default moved vs known and not dismissed */
export function getSettingDefaultChangeNotice(
  fieldKey: string | undefined | null,
): SettingDefaultChangeNotice | null {
  if (!fieldKey) return null
  const product = SETTING_PRODUCT_DEFAULTS[fieldKey]
  if (product == null) return null

  const state = readState()
  const known = state.known[fieldKey]
  // missing known: record current default, no notice
  if (known == null) {
    const next = {
      ...state,
      known: { ...state.known, [fieldKey]: product },
    }
    writeState(next, { silent: true })
    return null
  }
  if (known === product) return null

  const id = transitionId(fieldKey, known, product)
  if (state.dismissed.includes(id)) return null

  return {
    fieldKey,
    from: known,
    to: product,
    transitionId: id,
  }
}

/** dismiss and set known to current product default */
export function dismissSettingDefaultChange(
  fieldKey: string | undefined | null,
): void {
  if (!fieldKey) return
  const product = SETTING_PRODUCT_DEFAULTS[fieldKey]
  if (product == null) return

  const state = readState()
  const known = state.known[fieldKey] ?? product
  const id = transitionId(fieldKey, known, product)
  const dismissed = state.dismissed.includes(id)
    ? state.dismissed
    : [...state.dismissed, id]

  writeState({
    known: { ...state.known, [fieldKey]: product },
    dismissed,
  })
}
