import type {
  ModuleVisibilityKey,
  ModuleVisibilityLevel,
  ModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import type { HitokotoConfig } from '../../utils/quote'
import type { ReportSettings } from '../../utils/reportSettings'
import { LuEye, LuSparkles, MyriadStoreIcon } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import apiService from '../../services/api'
import {
  MODULE_VISIBILITY_KEYS,
  MODULE_VISIBILITY_LEVELS,
  normalizeModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import PlatformIcon from '../PlatformIcon'
import {
  InputItem,
  NumberItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
} from '../settings'

export type LibraryItemType =
  'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'

export interface LibrarySourceOption {
  source: string
  count: number
}

export interface LibrarySourcePreferences {
  categories: Record<LibraryItemType, string[]>
}

interface LibraryResponse {
  success: boolean
  total: number
  raw_total?: number
  preferences?: LibrarySourcePreferences
  available_sources?: Partial<Record<LibraryItemType, LibrarySourceOption[]>>
}

interface PreferencesResponse {
  success: boolean
  preferences?: LibrarySourcePreferences
}

interface ModuleConfigSectionProps {
  title: string
  icon?: React.ReactNode
  description?: string
  sectionId?: string
  sourceDraft: LibrarySourcePreferences
  setSourceDraft: React.Dispatch<React.SetStateAction<LibrarySourcePreferences>>
  visibilityDraft: ModuleVisibilityPreferences
  setVisibilityDraft: React.Dispatch<
    React.SetStateAction<ModuleVisibilityPreferences>
  >
  isSourceDirty: boolean
  saveRevision: number
  onSourcePreferencesLoaded: (
    preferences: LibrarySourcePreferences,
    options?: { resetDraft?: boolean },
  ) => void
  hitokotoDraft: HitokotoConfig
  setHitokotoDraft: React.Dispatch<React.SetStateAction<HitokotoConfig>>
  reportSettingsDraft: ReportSettings
  setReportSettingsDraft: React.Dispatch<React.SetStateAction<ReportSettings>>
  onMessage?: (message: string, type?: 'success' | 'error' | 'info') => void
}

const LIBRARY_ITEM_TYPES: LibraryItemType[] = [
  'game',
  'video',
  'music',
  'anime',
  'tv_series',
  'book',
]

const MODULE_SETTING_CARD_CLASS =
  'rounded-lg border border-gray-100 bg-gray-50/80 p-3 dark:border-white/10 dark:bg-white/[0.03]'

const MODULE_SETTING_SEGMENTED_CLASS =
  'grid grid-cols-3 gap-1 rounded-lg border border-gray-100 bg-gray-50/80 p-1 dark:border-white/10 dark:bg-white/[0.03]'

const MODULE_SETTING_TITLE_ICON_CLASS =
  'h-3.5 w-3.5 shrink-0 text-[var(--color-primary)]'

function LibrarySubtitleIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
        d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
      />
    </svg>
  )
}

function BrewTitleIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
        d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8zM6 1v3M10 1v3M14 1v3"
      />
    </svg>
  )
}

function ReportsTitleIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
        d="M7 12l3-3 3 3 4-4M8 21l4-4 4 4M3 4h18M4 4h16v12a1 1 0 01-1 1H5a1 1 0 01-1-1V4z"
      />
    </svg>
  )
}

function QuoteTitleIcon({ className }: { className?: string }) {
  return (
    <svg className={className} fill="currentColor" viewBox="0 0 24 24">
      <path d="M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z" />
    </svg>
  )
}

/** 一言源选项（顺序即展示顺序，custom 固定在末尾） */
const HITOKOTO_SOURCE_IDS = [
  'hitokoto-cn',
  'hitokoto-anime',
  'quotable-en',
  'meigen-ja',
  'custom',
] as const

function LibraryTypeIcon({
  type,
  className,
}: {
  type: LibraryItemType
  className?: string
}) {
  switch (type) {
    case 'game':
      return (
        <svg
          className={className}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <rect x="2" y="6" width="20" height="12" rx="3" strokeWidth={2} />
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M6 12h4m-2-2v4"
          />
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={3}
            d="M15 11h.01M17 13h.01"
          />
        </svg>
      )
    case 'video':
      return (
        <svg
          className={className}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
          />
        </svg>
      )
    case 'music':
      return (
        <svg
          className={className}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M9 19V6l12-3v13M9 19c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zm12-3c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zM9 10l12-3"
          />
        </svg>
      )
    case 'anime':
      return (
        <svg className={className} fill="currentColor" viewBox="0 0 24 24">
          <text
            x="50%"
            y="50%"
            textAnchor="middle"
            dominantBaseline="central"
            fontSize="22"
            fontWeight="bold"
            className="font-sans"
          >
            あ
          </text>
        </svg>
      )
    case 'tv_series':
      return (
        <svg
          className={className}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M6 20.25h12m-7.5-3v3m3-3v3m-10.125-3h17.25c.621 0 1.125-.504 1.125-1.125V4.875c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v11.25c0 .621.504 1.125 1.125 1.125z"
          />
        </svg>
      )
    case 'book':
      return (
        <svg
          className={className}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M4 19.5A2.5 2.5 0 016.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"
          />
        </svg>
      )
  }
}

export const DEFAULT_LIBRARY_SOURCE_PREFERENCES: LibrarySourcePreferences = {
  categories: {
    game: ['Steam', 'Bangumi'],
    video: ['Bilibili', 'Bangumi'],
    music: ['Netease', 'Bangumi'],
    anime: ['Bangumi', 'Bilibili', 'MyAnimeList'],
    tv_series: ['Bangumi', 'Bilibili'],
    book: ['Bangumi', 'MyAnimeList'],
  },
}

export function normalizeLibraryPreferences(
  preferences?: LibrarySourcePreferences,
): LibrarySourcePreferences {
  return {
    categories: LIBRARY_ITEM_TYPES.reduce(
      (acc, type) => {
        acc[type] = [
          ...(preferences?.categories?.[type] ??
            DEFAULT_LIBRARY_SOURCE_PREFERENCES.categories[type]),
        ]
        return acc
      },
      {} as Record<LibraryItemType, string[]>,
    ),
  }
}

export function areLibrarySourcePreferencesEqual(
  left: LibrarySourcePreferences,
  right: LibrarySourcePreferences,
) {
  return LIBRARY_ITEM_TYPES.every((type) => {
    const leftSources = left.categories[type] ?? []
    const rightSources = right.categories[type] ?? []
    return (
      leftSources.length === rightSources.length &&
      leftSources.every((source, index) => source === rightSources[index])
    )
  })
}

export const ModuleConfigSection: React.FC<ModuleConfigSectionProps> = ({
  title,
  icon,
  description,
  sectionId,
  sourceDraft,
  setSourceDraft,
  visibilityDraft,
  setVisibilityDraft,
  isSourceDirty,
  saveRevision,
  onSourcePreferencesLoaded,
  hitokotoDraft,
  setHitokotoDraft,
  reportSettingsDraft,
  setReportSettingsDraft,
  onMessage,
}) => {
  const { t } = useI18n()
  const [loading, setLoading] = useState(true)
  const [rawTotal, setRawTotal] = useState(0)
  const [shownTotal, setShownTotal] = useState(0)
  const isSourceDirtyRef = React.useRef(isSourceDirty)
  const [sourceOptions, setSourceOptions] = useState<
    Partial<Record<LibraryItemType, LibrarySourceOption[]>>
  >({})

  const typeLabels = useMemo<Record<LibraryItemType, string>>(
    () => ({
      game: t.library.game,
      video: t.nav.video,
      music: t.library.music,
      anime: t.library.anime,
      tv_series: t.library.tvSeries,
      book: t.library.book,
    }),
    [t],
  )

  const typeIcons = useMemo<Record<LibraryItemType, React.ReactNode>>(
    () => ({
      game: (
        <LibraryTypeIcon
          type="game"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
      video: (
        <LibraryTypeIcon
          type="video"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
      music: (
        <LibraryTypeIcon
          type="music"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
      anime: (
        <LibraryTypeIcon
          type="anime"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
      tv_series: (
        <LibraryTypeIcon
          type="tv_series"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
      book: (
        <LibraryTypeIcon
          type="book"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
    }),
    [],
  )

  const moduleLabels = useMemo<Record<ModuleVisibilityKey, string>>(
    () => ({
      library: t.nav.library,
      brew: t.nav.brewReading,
      reports: t.nav.reports,
      tapp: t.nav.tappStore,
      agent: t.nav.agent,
    }),
    [t],
  )

  const moduleIcons = useMemo<Record<ModuleVisibilityKey, React.ReactNode>>(
    () => ({
      library: (
        <LibrarySubtitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />
      ),
      brew: <BrewTitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      reports: <ReportsTitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      tapp: <MyriadStoreIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      agent: <LuSparkles className={MODULE_SETTING_TITLE_ICON_CLASS} />,
    }),
    [],
  )

  const visibilityLabels = useMemo<Record<ModuleVisibilityLevel, string>>(
    () => ({
      all: t.config.moduleVisibilityAll,
      authenticated: t.config.moduleVisibilityAuthenticated,
      admin: t.config.moduleVisibilityAdmin,
    }),
    [t],
  )

  // ===== 一言设置（存于后端数据库，随全局保存统一提交）=====
  const hitokotoSourceLabels = useMemo<Record<string, string>>(
    () => ({
      'hitokoto-cn': t.config.hitokotoSourceHitokotoCn,
      'hitokoto-anime': t.config.hitokotoSourceHitokotoAnime,
      'quotable-en': t.config.hitokotoSourceQuotableEn,
      'meigen-ja': t.config.hitokotoSourceMeigenJa,
      custom: t.config.hitokotoSourceCustom,
    }),
    [t],
  )

  const updateHitokotoConfig = useCallback(
    (patch: Partial<HitokotoConfig>) => {
      setHitokotoDraft((prev) => ({ ...prev, ...patch }))
    },
    [setHitokotoDraft],
  )

  const updateReportSettings = useCallback(
    (patch: Partial<ReportSettings>) => {
      setReportSettingsDraft((prev) => ({ ...prev, ...patch }))
    },
    [setReportSettingsDraft],
  )

  useEffect(() => {
    isSourceDirtyRef.current = isSourceDirty
  }, [isSourceDirty])

  const loadLibrarySourceSettings = useCallback(async () => {
    try {
      setLoading(true)
      const preferenceData = await apiService.get<PreferencesResponse>(
        '/library/preferences',
      )
      const preferences = normalizeLibraryPreferences(
        preferenceData.preferences,
      )
      onSourcePreferencesLoaded(preferences, {
        resetDraft: !isSourceDirtyRef.current,
      })

      try {
        const data = await apiService.get<LibraryResponse>('/library')
        setRawTotal(data.raw_total ?? data.total ?? 0)
        setShownTotal(data.total ?? 0)
        setSourceOptions(data.available_sources ?? {})
        onSourcePreferencesLoaded(
          normalizeLibraryPreferences(data.preferences),
          {
            resetDraft: !isSourceDirtyRef.current,
          },
        )
      } catch {
        setRawTotal(0)
        setShownTotal(0)
        setSourceOptions({})
      }
    } catch {
      onMessage?.(t.config.librarySourceLoadFailed, 'error')
    } finally {
      setLoading(false)
    }
  }, [onMessage, onSourcePreferencesLoaded, t])

  useEffect(() => {
    loadLibrarySourceSettings()
  }, [loadLibrarySourceSettings, saveRevision])

  const getSourceOptionsForType = useCallback(
    (type: LibraryItemType) => {
      const bySource = new Map<string, LibrarySourceOption>()
      // Always surface known default platforms (e.g. newly added MyAnimeList)
      // even when the user already has saved preferences without them.
      ;(DEFAULT_LIBRARY_SOURCE_PREFERENCES.categories[type] ?? []).forEach(
        (source) => {
          bySource.set(source, { source, count: 0 })
        },
      )
      ;(sourceOptions[type] ?? []).forEach((option) => {
        bySource.set(option.source, option)
      })
      ;(sourceDraft.categories[type] ?? []).forEach((source) => {
        if (!bySource.has(source)) {
          bySource.set(source, { source, count: 0 })
        }
      })
      return Array.from(bySource.values())
    },
    [sourceDraft.categories, sourceOptions],
  )

  const toggleSourceForType = useCallback(
    (type: LibraryItemType, source: string) => {
      setSourceDraft((prev) => {
        const current = prev.categories[type] ?? []
        const nextSources = current.includes(source)
          ? current.filter((candidate) => candidate !== source)
          : [...current, source]
        return {
          ...prev,
          categories: {
            ...prev.categories,
            [type]: nextSources,
          },
        }
      })
    },
    [setSourceDraft],
  )

  const updateVisibilityForModule = useCallback(
    (moduleKey: ModuleVisibilityKey, visibility: ModuleVisibilityLevel) => {
      setVisibilityDraft((prev) =>
        normalizeModuleVisibilityPreferences({
          modules: {
            ...prev.modules,
            [moduleKey]: visibility,
          },
          // 兼容旧字段；能力档位已迁至 Tapp 权限预设
          agentUsage: prev.agentUsage,
        }),
      )
    },
    [setVisibilityDraft],
  )

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <SettingGroup
        title={t.config.moduleVisibilityTitle}
        description={t.config.moduleVisibilityDesc}
        icon={<LuEye size={15} />}
      >
        <div className="grid gap-3 md:grid-cols-2">
          {MODULE_VISIBILITY_KEYS.map((moduleKey) => {
            const selectedVisibility = visibilityDraft.modules[moduleKey]
            return (
              <div key={moduleKey} className={MODULE_SETTING_CARD_CLASS}>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <span className="flex min-w-0 items-center gap-1.5 text-xs font-semibold text-gray-700 dark:text-gray-200">
                    {moduleIcons[moduleKey]}
                    <span className="truncate">{moduleLabels[moduleKey]}</span>
                  </span>
                  <span className="text-[11px] font-medium text-gray-500 dark:text-gray-400">
                    {visibilityLabels[selectedVisibility]}
                  </span>
                </div>
                <div className={MODULE_SETTING_SEGMENTED_CLASS}>
                  {MODULE_VISIBILITY_LEVELS.map((visibility) => {
                    const checked = selectedVisibility === visibility
                    return (
                      <button
                        key={visibility}
                        type="button"
                        aria-pressed={checked}
                        onClick={() =>
                          updateVisibilityForModule(moduleKey, visibility)
                        }
                        className={`min-h-8 rounded-md px-2 text-xs font-medium transition-colors ${
                          checked
                            ? 'text-[var(--color-primary)]'
                            : 'text-gray-500 hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-100'
                        }`}
                        style={
                          checked
                            ? {
                                backgroundColor:
                                  'color-mix(in srgb, var(--color-primary, #3b82f6) 12%, transparent)',
                                boxShadow:
                                  '0 0 0 1px color-mix(in srgb, var(--color-primary, #3b82f6) 18%, transparent)',
                              }
                            : undefined
                        }
                      >
                        {visibilityLabels[visibility]}
                      </button>
                    )
                  })}
                </div>
              </div>
            )
          })}
        </div>
      </SettingGroup>

      <SettingGroup
        title={t.config.reportSettingsTitle}
        description={t.config.reportSettingsDesc}
        icon={<ReportsTitleIcon className="h-3.5 w-3.5" />}
      >
        <div className="space-y-3">
          <SwitchItem
            itemKey="report-expiry-enabled"
            label={t.config.reportExpiryEnabled}
            description={t.config.reportExpiryEnabledDesc}
            value={reportSettingsDraft.expiryEnabled}
            onChange={(value) => updateReportSettings({ expiryEnabled: value })}
          />
          <SwitchItem
            itemKey="report-auto-regenerate"
            label={t.config.reportAutoRegenerate}
            description={t.config.reportAutoRegenerateDesc}
            value={reportSettingsDraft.autoRegenerate}
            onChange={(value) =>
              updateReportSettings({ autoRegenerate: value })
            }
            disabled={!reportSettingsDraft.expiryEnabled}
          />
          <NumberItem
            itemKey="report-expiry-days"
            label={t.config.reportExpiryDays}
            description={t.config.reportExpiryDaysHint}
            value={reportSettingsDraft.expiryDays}
            onChange={(value) => {
              if (Number.isFinite(value)) {
                updateReportSettings({ expiryDays: value })
              }
            }}
            onBlur={() =>
              updateReportSettings({
                expiryDays: Math.min(
                  365,
                  Math.max(1, Math.round(reportSettingsDraft.expiryDays)),
                ),
              })
            }
            min={1}
            max={365}
            step={1}
            unit={t.config.reportExpiryDaysUnit}
            disabled={!reportSettingsDraft.expiryEnabled}
            layout="horizontal"
            size="sm"
          />
        </div>
      </SettingGroup>

      <SettingGroup
        title={t.config.libraryModuleTitle}
        description={t.config.libraryModuleDesc}
        icon={<LibrarySubtitleIcon />}
      >
        <div className="space-y-4">
          <div className="text-xs text-gray-500 dark:text-gray-400">
            {rawTotal > 0
              ? t.config.librarySourceVisibleCount
                  .replace('{shown}', String(shownTotal))
                  .replace('{total}', String(rawTotal))
              : loading
                ? t.common.loading
                : t.config.librarySourceNoData}
          </div>

          <div className="grid gap-3 md:grid-cols-2">
            {LIBRARY_ITEM_TYPES.map((type) => {
              const options = getSourceOptionsForType(type)
              return (
                <div key={type} className={MODULE_SETTING_CARD_CLASS}>
                  <div className="mb-2 flex items-center gap-1.5 text-xs font-semibold text-gray-700 dark:text-gray-200">
                    {typeIcons[type]}
                    <span>{typeLabels[type]}</span>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {options.map((option) => {
                      const checked =
                        sourceDraft.categories[type]?.includes(option.source) ??
                        false
                      return (
                        <button
                          key={option.source}
                          type="button"
                          aria-pressed={checked}
                          onClick={() =>
                            toggleSourceForType(type, option.source)
                          }
                          className={`inline-flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs font-medium transition-colors ${
                            checked
                              ? 'text-[var(--color-primary)]'
                              : 'border-gray-200 bg-white text-gray-500 hover:border-[var(--color-primary)] hover:text-gray-800 dark:border-white/10 dark:bg-neutral-900 dark:text-gray-400 dark:hover:text-gray-100'
                          }`}
                          style={
                            checked
                              ? {
                                  backgroundColor:
                                    'color-mix(in srgb, var(--color-primary, #3b82f6) 12%, transparent)',
                                  borderColor:
                                    'color-mix(in srgb, var(--color-primary, #3b82f6) 50%, transparent)',
                                  boxShadow:
                                    '0 0 0 1px color-mix(in srgb, var(--color-primary, #3b82f6) 18%, transparent)',
                                }
                              : undefined
                          }
                        >
                          <PlatformIcon
                            platform={option.source}
                            className="h-3.5 w-3.5"
                          />
                          <span>{option.source}</span>
                          <span
                            className="rounded bg-black/5 px-1.5 py-0.5 text-[10px] text-gray-500 dark:bg-white/10 dark:text-gray-300"
                            style={
                              checked
                                ? {
                                    backgroundColor:
                                      'color-mix(in srgb, var(--color-primary, #3b82f6) 14%, transparent)',
                                    color: 'var(--color-primary, #3b82f6)',
                                  }
                                : undefined
                            }
                          >
                            {option.count}
                          </span>
                        </button>
                      )
                    })}
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </SettingGroup>

      <SettingGroup
        title={t.config.hitokotoTitle}
        description={t.config.hitokotoDesc}
        icon={<QuoteTitleIcon className="h-3.5 w-3.5" />}
      >
        <div className="space-y-3">
          <div className="text-xs font-medium text-gray-600 dark:text-gray-300">
            {t.config.hitokotoSourceLabel}
          </div>
          <div className="flex flex-wrap gap-2">
            {HITOKOTO_SOURCE_IDS.map((sourceId) => {
              const checked = hitokotoDraft.sourceId === sourceId
              return (
                <button
                  key={sourceId}
                  type="button"
                  aria-pressed={checked}
                  onClick={() => updateHitokotoConfig({ sourceId })}
                  className={`inline-flex items-center rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
                    checked
                      ? 'text-[var(--color-primary)]'
                      : 'border-gray-200 bg-white text-gray-500 hover:border-[var(--color-primary)] hover:text-gray-800 dark:border-white/10 dark:bg-neutral-900 dark:text-gray-400 dark:hover:text-gray-100'
                  }`}
                  style={
                    checked
                      ? {
                          backgroundColor:
                            'color-mix(in srgb, var(--color-primary, #3b82f6) 12%, transparent)',
                          borderColor:
                            'color-mix(in srgb, var(--color-primary, #3b82f6) 50%, transparent)',
                          boxShadow:
                            '0 0 0 1px color-mix(in srgb, var(--color-primary, #3b82f6) 18%, transparent)',
                        }
                      : undefined
                  }
                >
                  {hitokotoSourceLabels[sourceId]}
                </button>
              )
            })}
          </div>

          {hitokotoDraft.sourceId === 'custom' && (
            <div className={`${MODULE_SETTING_CARD_CLASS} space-y-1`}>
              <InputItem
                itemKey="hitokoto-custom-url"
                label={t.config.hitokotoCustomUrl}
                hint={t.config.hitokotoCustomUrlHint}
                value={hitokotoDraft.customUrl ?? ''}
                onChange={(customUrl) => updateHitokotoConfig({ customUrl })}
                placeholder={t.config.hitokotoCustomUrlPlaceholder}
                inputType="url"
                layout="vertical"
                size="sm"
              />
              <div className="grid gap-2 sm:grid-cols-2">
                <InputItem
                  itemKey="hitokoto-text-field"
                  label={t.config.hitokotoTextField}
                  hint={t.config.hitokotoTextFieldHint}
                  value={hitokotoDraft.customTextField ?? ''}
                  onChange={(customTextField) =>
                    updateHitokotoConfig({ customTextField })
                  }
                  placeholder="hitokoto"
                  inputType="text"
                  layout="vertical"
                  size="sm"
                />
                <InputItem
                  itemKey="hitokoto-author-field"
                  label={t.config.hitokotoAuthorField}
                  hint={t.config.hitokotoAuthorFieldHint}
                  value={hitokotoDraft.customAuthorField ?? ''}
                  onChange={(customAuthorField) =>
                    updateHitokotoConfig({ customAuthorField })
                  }
                  placeholder="from"
                  inputType="text"
                  layout="vertical"
                  size="sm"
                />
              </div>
            </div>
          )}
        </div>
      </SettingGroup>
    </SettingSection>
  )
}

export default ModuleConfigSection
