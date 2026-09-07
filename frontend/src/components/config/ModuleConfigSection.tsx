import type {
  LibraryItemType,
  LibrarySourcePreferences,
} from '../../utils/librarySourcePreferences'
import type {
  ConfigurableModuleKey,
  ModuleVisibilityKey,
  ModuleVisibilityLevel,
  ModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import type { HitokotoConfig } from '../../utils/quote'
import type { ReportSettings } from '../../utils/reportSettings'
import {
  FaTrash,
  LuEye,
  MyriadStoreIcon,
  SiNeteasecloudmusic,
  SiQqmusic,
} from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import apiService from '../../services/api'
import {
  ISLAND_CONTENT_KEYS,
  islandBagKey,
  islandContentFromBagFields,
  type IslandContentKey,
} from '../../utils/islandContent'
import {
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,
  LIBRARY_ITEM_TYPES,
  normalizeLibraryPreferences,
} from '../../utils/librarySourcePreferences'
import {
  CONFIGURABLE_MODULE_KEYS,
  MODULE_VISIBILITY_LEVELS,
  normalizeModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import { normalizeMusicPlaylistId } from '../../utils/musicPlaylistId'
import PlatformIcon from '../PlatformIcon'
import {
  ButtonItem,
  InputItem,
  NumberItem,
  ProviderItem,
  SegmentedControl,
  SettingGroup,
  SettingGroupGrid,
  SettingSection,
  SettingTitleGuideEntry,
  SwitchItem,
  useSettingGuide,
} from '../settings'
import { Spinner } from '../Spinner'
import { MyriadConfigIcon } from './MyriadConfigIcon'

interface UiConfigField {
  key: string
  value: string
}

export interface LibrarySourceOption {
  source: string
  count: number
}

export type { LibraryItemType, LibrarySourcePreferences }
export {
  areLibrarySourcePreferencesEqual,
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,
  normalizeLibraryPreferences,
} from '../../utils/librarySourcePreferences'
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
  /** UI config fields (music player lives in ui_config). */
  uiConfigFields: UiConfigField[]
  updateUiFieldValue: (key: string, value: string) => void
  onMessage?: (message: string, type?: 'success' | 'error' | 'info') => void
}

const MODULE_SETTING_TITLE_ICON_CLASS =
  'h-3.5 w-3.5 shrink-0 text-[var(--cfg-accent)]'

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

function LibraryListLayoutIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <rect x="3" y="5" width="18" height="5" rx="1.5" strokeWidth="1.8" />
      <rect x="3" y="14" width="18" height="5" rx="1.5" strokeWidth="1.8" />
    </svg>
  )
}

function LibraryCanvasLayoutIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <rect x="3" y="3" width="18" height="18" rx="3" strokeWidth="1.8" />
      <rect x="8" y="8" width="8" height="8" rx="1.5" strokeWidth="1.8" />
      <path d="M12 3v5m0 8v5M3 12h5m8 0h5" strokeWidth="1.8" />
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

function IslandTitleIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <rect x="3" y="8" width="18" height="8" rx="4" strokeWidth="2" />
      <circle cx="8" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <path
        strokeLinecap="round"
        strokeWidth="1.8"
        d="M11.5 12h7"
      />
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
  uiConfigFields,
  updateUiFieldValue,
  onMessage,
}) => {
  const { t } = useI18n()
  const { catalog: g, renderGuide, bindGuide } = useSettingGuide()
  const hitokotoSourceGuide = renderGuide(g.modules.hitokotoSource)
  const islandContentGuide = renderGuide(g.modules.islandContent)
  const [loading, setLoading] = useState(true)

  const getUiFieldValue = useCallback(
    (key: string) => uiConfigFields.find((f) => f.key === key)?.value || '',
    [uiConfigFields],
  )
  const musicEnabled = getUiFieldValue('music_enabled') === 'true'
  const musicSource = getUiFieldValue('music_source')
  const playlistId = getUiFieldValue('music_playlist_id')
  const islandContent = islandContentFromBagFields(uiConfigFields)
  const islandSelected = ISLAND_CONTENT_KEYS.filter((key) => islandContent[key])

  const handleClearMusicCache = useCallback(() => {
    clearPlaylistCache()
    onMessage?.(t.config.musicCacheCleared, 'success')
  }, [onMessage, t.config.musicCacheCleared])
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

  const moduleLabels = useMemo<Record<ConfigurableModuleKey, string>>(
    () => ({
      library: t.nav.library,
      brew: t.nav.brewReading,
      reports: t.nav.reports,
      tapp: t.nav.tapp,
      agent: t.nav.agent,
    }),
    [t],
  )

  const moduleIcons = useMemo<Record<ConfigurableModuleKey, React.ReactNode>>(
    () => ({
      library: (
        <LibrarySubtitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />
      ),
      brew: <BrewTitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      reports: <ReportsTitleIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      tapp: <MyriadStoreIcon className={MODULE_SETTING_TITLE_ICON_CLASS} />,
      agent: (
        <MyriadConfigIcon
          kind="agent"
          className={MODULE_SETTING_TITLE_ICON_CLASS}
        />
      ),
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

  // 一言设置（存于后端数据库，随全局保存统一提交）
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

  const islandContentLabels = useMemo<Record<IslandContentKey, string>>(
    () => ({
      greeting: t.config.islandGreeting,
      weather: t.config.islandWeather,
      quote: t.config.islandQuote,
      music: t.config.islandMusic,
      tapp: t.config.islandTapp,
    }),
    [t],
  )

  const updateIslandContent = useCallback(
    (selected: IslandContentKey[]) => {
      const selectedSet = new Set(selected)
      for (const key of ISLAND_CONTENT_KEYS) {
        updateUiFieldValue(
          islandBagKey(key),
          String(selectedSet.has(key)),
        )
      }
    },
    [updateUiFieldValue],
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
      {/* 1. 可见性 → 2. 媒体库 → 3. 报告 → 4. 音乐 → 5. 智能岛 → 6. 一言 */}
      <SettingGroup
        title={t.config.moduleVisibilityTitle}
        description={t.config.moduleVisibilityDesc}
        {...bindGuide('modules.visibility', g.modules.visibility)}
        icon={<LuEye size={15} />}
      >
        <SettingGroupGrid
          columns={2}
          variant="card"
          align="stretch"
          minColumnWidth="16rem"
          ariaLabel={t.config.moduleVisibilityTitle}
        >
          {CONFIGURABLE_MODULE_KEYS.map((moduleKey) => {
            const selectedVisibility = visibilityDraft.modules[moduleKey]
            return (
              <SettingGroup
                key={moduleKey}
                title={moduleLabels[moduleKey]}
                icon={moduleIcons[moduleKey]}
                {...bindGuide(
                  'modules.visibilityItem',
                  g.modules.visibilityItem,
                )}
              >
                <SegmentedControl
                  size="sm"
                  columns={3}
                  value={selectedVisibility}
                  options={MODULE_VISIBILITY_LEVELS.map((visibility) => ({
                    value: visibility,
                    label: visibilityLabels[visibility],
                  }))}
                  onChange={(visibility) =>
                    updateVisibilityForModule(moduleKey, visibility)
                  }
                  ariaLabel={moduleLabels[moduleKey]}
                />
              </SettingGroup>
            )
          })}
        </SettingGroupGrid>
      </SettingGroup>

      <SettingGroup
        title={t.config.libraryModuleTitle}
        description={t.config.libraryModuleDesc}
        {...bindGuide('modules.library', g.modules.library)}
        icon={<LibrarySubtitleIcon />}
      >
        <SettingGroup
          title={t.config.libraryLayout}
          description={t.config.libraryLayoutDesc}
          {...bindGuide('modules.libraryLayout', g.modules.libraryLayout)}
        >
          <SegmentedControl
            value={sourceDraft.layout}
            options={[
              {
                value: 'list',
                label: t.config.libraryLayoutList,
                icon: <LibraryListLayoutIcon className="h-3.5 w-3.5" />,
              },
              {
                value: 'canvas',
                label: t.config.libraryLayoutCanvas,
                icon: <LibraryCanvasLayoutIcon className="h-3.5 w-3.5" />,
              },
            ]}
            onChange={(layout) =>
              setSourceDraft((prev) => ({ ...prev, layout }))
            }
            columns={2}
            ariaLabel={t.config.libraryLayout}
          />
        </SettingGroup>

        <div className="settings-text-3 text-xs">
          {rawTotal > 0 ? (
            t.config.librarySourceVisibleCount
              .replace('{shown}', String(shownTotal))
              .replace('{total}', String(rawTotal))
          ) : loading ? (
            <span className="inline-flex items-center" role="status">
              <Spinner size="xs" color="primary" />
            </span>
          ) : (
            t.config.librarySourceNoData
          )}
        </div>

        <SettingGroupGrid
          columns={2}
          variant="card"
          align="stretch"
          minColumnWidth="16rem"
          ariaLabel={t.config.libraryModuleTitle}
        >
          {LIBRARY_ITEM_TYPES.map((type) => {
            const options = getSourceOptionsForType(type)
            return (
              <SettingGroup
                key={type}
                title={typeLabels[type]}
                icon={typeIcons[type]}
                {...bindGuide('modules.libraryType', g.modules.libraryType)}
              >
                <SegmentedControl
                  mode="multi"
                  size="sm"
                  value={sourceDraft.categories[type] ?? []}
                  options={options.map((option) => ({
                    value: option.source,
                    label: option.source,
                    count: option.count,
                    icon: (
                      <PlatformIcon
                        platform={option.source}
                        className="h-3.5 w-3.5"
                      />
                    ),
                  }))}
                  onChange={(next) => {
                    setSourceDraft((prev) => ({
                      ...prev,
                      categories: {
                        ...prev.categories,
                        [type]: next,
                      },
                    }))
                  }}
                  ariaLabel={typeLabels[type]}
                />
              </SettingGroup>
            )
          })}
        </SettingGroupGrid>
      </SettingGroup>

      <SettingGroup
        title={t.config.reportSettingsTitle}
        description={t.config.reportSettingsDesc}
        {...bindGuide('modules.report', g.modules.report)}
        icon={<ReportsTitleIcon className="h-3.5 w-3.5" />}
      >
        <div className="space-y-3">
          <SwitchItem
            itemKey="report-expiry-enabled"
            label={t.config.reportExpiryEnabled}
            description={t.config.reportExpiryEnabledDesc}
            {...bindGuide('modules.reportExpiry', g.modules.reportExpiry)}
            value={reportSettingsDraft.expiryEnabled}
            onChange={(value) => updateReportSettings({ expiryEnabled: value })}
          />
          <SwitchItem
            itemKey="report-auto-regenerate"
            label={t.config.reportAutoRegenerate}
            description={t.config.reportAutoRegenerateDesc}
            {...bindGuide('modules.reportAutoRegen', g.modules.reportAutoRegen)}
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
            {...bindGuide(
              'modules.reportExpiryDays',
              g.modules.reportExpiryDays,
            )}
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

      {/* 音乐播放器（原独立「音乐」设置区，并入模块） */}
      <SettingGroup
        title={t.config.music}
        description={t.config.musicDesc}
        {...bindGuide('modules.music', g.modules.music)}
        icon={
          <LibraryTypeIcon
            type="music"
            className={MODULE_SETTING_TITLE_ICON_CLASS}
          />
        }
        switch={{
          checked: musicEnabled,
          onChange: (v) => updateUiFieldValue('music_enabled', v.toString()),
          ariaLabel: t.config.enableMusicPlayer,
        }}
      >
        <ProviderItem
          itemKey="music_source"
          label={t.config.musicPlatform}
          {...bindGuide('modules.musicPlatform', g.modules.musicPlatform)}
          value={musicSource}
          onChange={(v) => updateUiFieldValue('music_source', v)}
          options={[
            {
              value: 'netease',
              label: t.config.neteaseMusic,
              icon: <SiNeteasecloudmusic />,
            },
            {
              value: 'qq',
              label: t.config.qqMusic,
              icon: <SiQqmusic />,
            },
          ]}
          layout="horizontal"
          disabled={!musicEnabled}
        />
        <InputItem
          itemKey="music_playlist_id"
          label={t.config.playlistId}
          required
          value={playlistId}
          onChange={(v) => {
            // Paste full NetEase/QQ URL → extract numeric id (matches backend normalize)
            const next =
              v.includes('://') || v.includes('id=') || v.includes('/playlist/')
                ? normalizeMusicPlaylistId(v)
                : v
            updateUiFieldValue('music_playlist_id', next)
          }}
          placeholder={
            musicSource === 'netease'
              ? t.config.neteasePlaylistExample
              : t.config.qqPlaylistExample
          }
          hint={
            musicSource === 'netease'
              ? t.config.neteasePlaylistHint
              : t.config.qqPlaylistHint
          }
          {...bindGuide('modules.musicPlaylist', g.modules.musicPlaylist)}
          layout="vertical"
          disabled={!musicEnabled}
        />
        <ButtonItem
          itemKey="clear_music_cache"
          label={t.config.cacheManagement}
          description={t.config.clearMusicCacheDesc}
          {...bindGuide('modules.musicCache', g.modules.musicCache)}
          buttonText={t.config.clearMusicCacheBtn}
          buttonIcon={<FaTrash />}
          variant="secondary"
          layout="horizontal"
          size="md"
          onClick={handleClearMusicCache}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.islandContentTitle}
        description={t.config.islandContentDesc}
        {...bindGuide('modules.island', g.modules.island)}
        icon={<IslandTitleIcon className="h-3.5 w-3.5" />}
      >
        <div
          id="cfg-g-modules-islandContent"
          data-guide-path="modules.islandContent"
          className="setting-item setting-vertical has-guide-anchor"
        >
          <div className="setting-label">
            <span className="setting-label-text">
              {t.config.islandContentLocations}
              <SettingTitleGuideEntry
                title={t.config.islandContentLocations}
                guide={islandContentGuide}
              />
            </span>
          </div>
          <SegmentedControl
            mode="multi"
            size="sm"
            value={islandSelected}
            options={ISLAND_CONTENT_KEYS.map((key) => ({
              value: key,
              label: islandContentLabels[key],
            }))}
            onChange={updateIslandContent}
            ariaLabel={t.config.islandContentLocations}
          />
        </div>
      </SettingGroup>

      <SettingGroup
        title={t.config.hitokotoTitle}
        description={t.config.hitokotoDesc}
        {...bindGuide('modules.hitokoto', g.modules.hitokoto)}
        icon={<QuoteTitleIcon className="h-3.5 w-3.5" />}
      >
        <div className="space-y-3">
          <div
            id="cfg-g-modules-hitokotoSource"
            data-guide-path="modules.hitokotoSource"
            className="setting-item setting-vertical has-guide-anchor"
          >
            <div className="setting-label">
              <span className="setting-label-text">
                {t.config.hitokotoSourceLabel}
                <SettingTitleGuideEntry
                  title={t.config.hitokotoSourceLabel}
                  guide={hitokotoSourceGuide}
                />
              </span>
            </div>
            <SegmentedControl
              size="sm"
              value={hitokotoDraft.sourceId}
              options={HITOKOTO_SOURCE_IDS.map((sourceId) => ({
                value: sourceId,
                label: hitokotoSourceLabels[sourceId],
              }))}
              onChange={(sourceId) => updateHitokotoConfig({ sourceId })}
              ariaLabel={t.config.hitokotoSourceLabel}
            />
          </div>

          {hitokotoDraft.sourceId === 'custom' && (
            <div className="settings-inset-card space-y-1">
              <InputItem
                itemKey="hitokoto-custom-url"
                label={t.config.hitokotoCustomUrl}
                hint={t.config.hitokotoCustomUrlHint}
                {...bindGuide(
                  'modules.hitokotoCustomUrl',
                  g.modules.hitokotoCustomUrl,
                )}
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
                  {...bindGuide(
                    'modules.hitokotoTextField',
                    g.modules.hitokotoTextField,
                  )}
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
                  {...bindGuide(
                    'modules.hitokotoAuthorField',
                    g.modules.hitokotoAuthorField,
                  )}
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
