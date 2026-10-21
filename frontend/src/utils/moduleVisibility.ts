import { useCallback, useEffect, useSyncExternalStore } from 'react'
import { currentCopy } from '../i18n/localeCopy'
import apiService from '../services/api'
import { formatUserFacingError } from './formatUserFacingError'
import { clearDedupCache, dedupedFetch } from './requestDedup'

export type ModuleVisibilityLevel = 'all' | 'authenticated' | 'admin'
export type ModuleVisibilityKey =
  | 'library'
  | 'phantasi'
  | 'tapp'
  | 'agent'

export type AgentGuestUsageLevel = 'none' | 'visible'
export type AgentUserUsageLevel = 'none' | 'chat' | 'standard' | 'elevated'

export interface AgentUsagePreferences {
  guest: AgentGuestUsageLevel
  user: AgentUserUsageLevel
}

export interface ModuleVisibilityPreferences {
  modules: Record<ModuleVisibilityKey, ModuleVisibilityLevel>
  agentUsage: AgentUsagePreferences
}

interface ModuleVisibilityResponse {
  success: boolean
  preferences?: ModuleVisibilityPreferences
  message?: string
}

/** Keep in sync with MODULE_VISIBILITY_KEYS. */
export const MODULE_VISIBILITY_KEYS: ModuleVisibilityKey[] = [
  'library',
  'phantasi',
  'tapp',
  'agent',
]

export type ConfigurableModuleKey = ModuleVisibilityKey
export const CONFIGURABLE_MODULE_KEYS: ConfigurableModuleKey[] =
  MODULE_VISIBILITY_KEYS

export const MODULE_VISIBILITY_LEVELS: ModuleVisibilityLevel[] = [
  'all',
  'authenticated',
  'admin',
]

export const MODULE_VISIBILITY_UPDATED_EVENT =
  'module-visibility-preferences-updated'

export const DEFAULT_AGENT_USAGE_PREFERENCES: AgentUsagePreferences = {
  guest: 'none',
  user: 'standard',
}

export const DEFAULT_MODULE_VISIBILITY_PREFERENCES: ModuleVisibilityPreferences =
  {
    modules: {
      library: 'all',
      phantasi: 'all',
      tapp: 'all',
      agent: 'all',
    },
    agentUsage: { ...DEFAULT_AGENT_USAGE_PREFERENCES },
  }

function isVisibilityLevel(value: unknown): value is ModuleVisibilityLevel {
  return (
    value === 'all' || value === 'authenticated' || value === 'admin'
  )
}

function isGuestUsageLevel(value: unknown): value is AgentGuestUsageLevel {
  return value === 'none' || value === 'visible'
}

function isUserUsageLevel(value: unknown): value is AgentUserUsageLevel {
  return (
    value === 'none' ||
    value === 'chat' ||
    value === 'standard' ||
    value === 'elevated'
  )
}

export function normalizeModuleVisibilityPreferences(
  preferences?: Partial<ModuleVisibilityPreferences> & {
    agentUsage?: Partial<AgentUsagePreferences>
  },
): ModuleVisibilityPreferences {
  const usage = preferences?.agentUsage
  return {
    modules: MODULE_VISIBILITY_KEYS.reduce(
      (acc, key) => {
        const value = preferences?.modules?.[key]
        acc[key] = isVisibilityLevel(value)
          ? value
          : DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules[key]
        return acc
      },
      {} as Record<ModuleVisibilityKey, ModuleVisibilityLevel>,
    ),
    agentUsage: {
      guest: isGuestUsageLevel(usage?.guest)
        ? usage.guest
        : DEFAULT_AGENT_USAGE_PREFERENCES.guest,
      user: isUserUsageLevel(usage?.user)
        ? usage.user
        : DEFAULT_AGENT_USAGE_PREFERENCES.user,
    },
  }
}

export function areModuleVisibilityPreferencesEqual(
  left: ModuleVisibilityPreferences,
  right: ModuleVisibilityPreferences,
) {
  const modulesEqual = MODULE_VISIBILITY_KEYS.every(
    (key) => left.modules[key] === right.modules[key],
  )
  return (
    modulesEqual &&
    left.agentUsage.guest === right.agentUsage.guest &&
    left.agentUsage.user === right.agentUsage.user
  )
}

export function canAccessModuleVisibility(
  visibility: ModuleVisibilityLevel,
  viewer: { isAuthenticated: boolean; isAdmin: boolean },
) {
  if (visibility === 'all') return true
  if (visibility === 'authenticated') return viewer.isAuthenticated
  return viewer.isAdmin
}

export function canUseAgent(
  preferences: ModuleVisibilityPreferences,
  viewer: { isAuthenticated: boolean; isAdmin: boolean },
  elevatedAiChat?: { user?: boolean; guest?: boolean },
): boolean {
  if (
    !canAccessModuleVisibility(preferences.modules.agent, viewer)
  ) {
    return false
  }
  if (viewer.isAdmin) return true
  if (!viewer.isAuthenticated) {
    if (elevatedAiChat !== undefined) {
      return elevatedAiChat.guest === true
    }
    return preferences.agentUsage.guest === 'visible'
  }
  if (elevatedAiChat !== undefined) {
    return elevatedAiChat.user === true
  }
  return preferences.agentUsage.user !== 'none'
}

export async function fetchModuleVisibilityPreferences() {
  // cacheTTL 0: coalesce in-flight only; do not cache.
  const response = await dedupedFetch(
    '/config/module-visibility',
    () =>
      apiService.get<ModuleVisibilityResponse>('/config/module-visibility'),
    { cacheTTL: 0 },
  )
  return normalizeModuleVisibilityPreferences(response.preferences)
}

let sessionPreferences: ModuleVisibilityPreferences | null = null
let sessionLoadedAt = 0
let sessionInflight: Promise<ModuleVisibilityPreferences> | null = null
let sessionLoadGeneration = 0
const sessionSubscribers = new Set<() => void>()

/** Soft TTL: remounts reuse; refresh when stale. */
const SESSION_PREFERENCES_TTL_MS = 60_000

function rememberSessionPreferences(
  preferences: ModuleVisibilityPreferences,
): ModuleVisibilityPreferences {
  sessionPreferences = preferences
  sessionLoadedAt = Date.now()
  for (const subscriber of [...sessionSubscribers]) subscriber()
  return preferences
}

/** Confirmed writes and update events supersede every older read. */
function acceptSessionPreferences(preferences: ModuleVisibilityPreferences) {
  ++sessionLoadGeneration
  sessionInflight = null
  clearDedupCache('/config/module-visibility')
  return rememberSessionPreferences(preferences)
}

export function getCachedModuleVisibilityPreferences(): ModuleVisibilityPreferences | null {
  return sessionPreferences
}

/** Force refresh ignores older in-flight responses (generation). */
export async function ensureModuleVisibilityPreferences(
  force = false,
): Promise<ModuleVisibilityPreferences> {
  const age = Date.now() - sessionLoadedAt
  if (
    !force &&
    sessionPreferences &&
    age < SESSION_PREFERENCES_TTL_MS
  ) {
    return sessionPreferences
  }
  if (!force && sessionInflight) {
    return sessionInflight
  }

  if (force) clearDedupCache('/config/module-visibility')
  const generation = ++sessionLoadGeneration
  const flight = fetchModuleVisibilityPreferences()
    .then((prefs) => {
      if (generation !== sessionLoadGeneration) {
        return sessionPreferences ?? prefs
      }
      return rememberSessionPreferences(prefs)
    })
    .catch(() => {
      if (generation !== sessionLoadGeneration) {
        return (
          sessionPreferences ?? DEFAULT_MODULE_VISIBILITY_PREFERENCES
        )
      }
      if (sessionPreferences) return sessionPreferences
      return rememberSessionPreferences(DEFAULT_MODULE_VISIBILITY_PREFERENCES)
    })
    .finally(() => {
      // Only the active in-flight request may clear the slot.
      if (sessionInflight === flight) {
        sessionInflight = null
      }
    })

  sessionInflight = flight
  return flight
}

export async function updateModuleVisibilityPreferences(
  preferences: ModuleVisibilityPreferences,
) {
  const normalized = normalizeModuleVisibilityPreferences(preferences)
  const response = await apiService.put<ModuleVisibilityResponse>(
    '/config/module-visibility',
    normalized,
  )
  if (!response.success) {
    throw new Error(
      await formatUserFacingError(
        response.message,
        currentCopy().config.moduleVisibilitySaveFailed,
      ),
    )
  }
  const next = normalizeModuleVisibilityPreferences(response.preferences)
  acceptSessionPreferences(next)
  return next
}

export function dispatchModuleVisibilityPreferencesUpdated(
  preferences: ModuleVisibilityPreferences,
) {
  const normalized = normalizeModuleVisibilityPreferences(preferences)
  acceptSessionPreferences(normalized)
  if (typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent(MODULE_VISIBILITY_UPDATED_EVENT, {
      detail: normalized,
    }),
  )
}

function handleSessionUpdate(event: Event) {
  const detail = (event as CustomEvent<ModuleVisibilityPreferences>).detail
  // The dispatcher already published this exact snapshot to the store.
  if (detail === sessionPreferences) return
  acceptSessionPreferences(normalizeModuleVisibilityPreferences(detail))
}

function subscribeSessionPreferences(subscriber: () => void) {
  if (sessionSubscribers.size === 0) {
    window.addEventListener(MODULE_VISIBILITY_UPDATED_EVENT, handleSessionUpdate)
  }
  sessionSubscribers.add(subscriber)
  return () => {
    sessionSubscribers.delete(subscriber)
    if (sessionSubscribers.size === 0) {
      window.removeEventListener(MODULE_VISIBILITY_UPDATED_EVENT, handleSessionUpdate)
    }
  }
}

const getServerSessionPreferences = () => null

/** Every consumer observes the same confirmed snapshot; reads belong to the session. */
export function useModuleVisibilityPreferences() {
  const snapshot = useSyncExternalStore(
    subscribeSessionPreferences,
    getCachedModuleVisibilityPreferences,
    getServerSessionPreferences,
  )
  const reload = useCallback(async (force = true) => {
    await ensureModuleVisibilityPreferences(force)
  }, [])

  useEffect(() => {
    // Stale mounts share one refresh and keep displaying the last snapshot.
    void ensureModuleVisibilityPreferences()
  }, [])

  return {
    preferences: snapshot ?? DEFAULT_MODULE_VISIBILITY_PREFERENCES,
    isLoading: snapshot === null,
    reload,
  }
}
