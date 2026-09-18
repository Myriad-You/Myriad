import type { SourceSortMode } from '../components/phantasi/logic/board'
import { useCallback, useEffect, useState } from 'react'
import { normalizeSourceSortMode } from '../components/phantasi/logic/sourceSort'
import { currentCopy } from '../i18n/localeCopy'
import apiService from '../services/api'
import { formatUserFacingError } from './formatUserFacingError'
import { dedupedFetch } from './requestDedup'

export type ModuleVisibilityLevel = 'all' | 'authenticated' | 'admin'
export type ModuleVisibilityKey =
  | 'library'
  | 'phantasi'
  | 'reports'
  | 'tapp'
  | 'agent'

export type AgentGuestUsageLevel = 'none' | 'visible'
export type AgentUserUsageLevel = 'none' | 'chat' | 'standard' | 'elevated'

export interface AgentUsagePreferences {
  guest: AgentGuestUsageLevel
  user: AgentUserUsageLevel
}

export type JournalBoardVisibility = Record<'feeds' | 'notes' | 'sites', ModuleVisibilityLevel>

export interface ModuleVisibilityPreferences {
  modules: Record<ModuleVisibilityKey, ModuleVisibilityLevel>
  agentUsage: AgentUsagePreferences
  journalSourceSort: SourceSortMode
  journalBoards: JournalBoardVisibility
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
  'reports',
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
      reports: 'all',
      tapp: 'all',
      agent: 'all',
    },
    agentUsage: { ...DEFAULT_AGENT_USAGE_PREFERENCES },
    journalSourceSort: 'smart',
    journalBoards: { feeds: 'all', notes: 'all', sites: 'all' },
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
    journalSourceSort: normalizeSourceSortMode(preferences?.journalSourceSort),
    journalBoards: {
      feeds: isVisibilityLevel(preferences?.journalBoards?.feeds) ? preferences.journalBoards.feeds : 'all',
      notes: isVisibilityLevel(preferences?.journalBoards?.notes) ? preferences.journalBoards.notes : 'all',
      sites: isVisibilityLevel(preferences?.journalBoards?.sites) ? preferences.journalBoards.sites : 'all',
    },
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
    left.journalSourceSort === right.journalSourceSort &&
    left.journalBoards.feeds === right.journalBoards.feeds &&
    left.journalBoards.notes === right.journalBoards.notes &&
    left.journalBoards.sites === right.journalBoards.sites &&
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

/** Soft TTL: remounts reuse; refresh when stale. */
const SESSION_PREFERENCES_TTL_MS = 60_000

function rememberSessionPreferences(
  preferences: ModuleVisibilityPreferences,
): ModuleVisibilityPreferences {
  sessionPreferences = preferences
  sessionLoadedAt = Date.now()
  return preferences
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
  rememberSessionPreferences(next)
  return next
}

export function dispatchModuleVisibilityPreferencesUpdated(
  preferences: ModuleVisibilityPreferences,
) {
  const normalized = normalizeModuleVisibilityPreferences(preferences)
  rememberSessionPreferences(normalized)
  if (typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent(MODULE_VISIBILITY_UPDATED_EVENT, {
      detail: normalized,
    }),
  )
}

export function useModuleVisibilityPreferences() {
  const [preferences, setPreferences] = useState<ModuleVisibilityPreferences>(
    () => sessionPreferences ?? DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  )
  // Block first paint until session resolves once.
  const [isLoading, setIsLoading] = useState(() => sessionPreferences === null)

  const reload = useCallback(async (force = true) => {
    // Do not set isLoading true again once session data exists.
    if (sessionPreferences === null) {
      setIsLoading(true)
    }
    try {
      const nextPreferences = await ensureModuleVisibilityPreferences(force)
      setPreferences(nextPreferences)
    } catch {
      const fallback =
        sessionPreferences ?? DEFAULT_MODULE_VISIBILITY_PREFERENCES
      setPreferences(fallback)
    } finally {
      setIsLoading(false)
    }
  }, [])

  useEffect(() => {
    // Warm session: paint immediately; refresh when stale.
    const age = Date.now() - sessionLoadedAt
    if (sessionPreferences && age < SESSION_PREFERENCES_TTL_MS) {
      setPreferences(sessionPreferences)
      setIsLoading(false)
    } else {
      void reload(sessionPreferences !== null)
    }

    const handleUpdated = (event: Event) => {
      const detail = (event as CustomEvent<ModuleVisibilityPreferences>).detail
      const next = normalizeModuleVisibilityPreferences(detail)
      rememberSessionPreferences(next)
      setPreferences(next)
      setIsLoading(false)
    }

    window.addEventListener(MODULE_VISIBILITY_UPDATED_EVENT, handleUpdated)
    return () => {
      window.removeEventListener(MODULE_VISIBILITY_UPDATED_EVENT, handleUpdated)
    }
  }, [reload])

  return { preferences, isLoading, reload }
}
