/**
 * Tapp 详情 / 配置页
 * 对齐新版设置页：SettingSection + SettingGroup + 设置原语
 */

import type { MouseEvent as ReactMouseEvent, ReactNode } from 'react'
import type { ToastType } from '../../components/Toast'
import type {
  TappCredentialBindingSummary,
  TappCredentialStatus,
  TappInboundGuardStatus,
} from '../services/TappCredentialApi'
import type { TappVisibility } from '../services/TappLifecycleApi'
import type { TappInstance, TappPermission, TappSettingItem } from '../types'
import {
  FaCog,
  FaDownload,
  FaExclamationTriangle,
  FaKey,
  FaLock,
  FaPause,
  FaPlay,
  FaTrash,
  LuChevronLeft,
} from '@lib/icons'
import {

  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import AnimatedView from '../../components/AnimatedView'
import {
  getTappPermissionGuide,
  guideDomProps,
  InfoActionCard,
  InputItem,
  NumberItem,
  SegmentedControl,
  SelectItem,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  SwitchItem,
  tappPermissionGuidePath,
  useSettingGuide,
} from '../../components/settings'
import { SettingItemWrapper } from '../../components/settings/items/SettingItemWrapper'
import { Spinner } from '../../components/Spinner'
import Toast from '../../components/Toast'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { usePageSeo } from '../../hooks/usePageSeo'
import { sanitizeUrl } from '../../utils/inputSanitizer'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../../utils/moduleVisibility'
import { TappIconBadge } from '../components/TappIconBadge'
import { UninstallConfirmDialog } from '../components/UninstallConfirmDialog'
import { PERMISSION_CONFIG } from '../constants/permissions'
import { useTappShellPresence } from '../hooks/useTappShellPresence'
import { getTappRuntime } from '../runtime'
import { PERMISSION_LEVELS } from '../runtime/permissionConfig'
import * as TappApiService from '../services/TappApiService'
import {
  summarizeCredentialBindings,
  uniqueNonEmpty,
} from '../utils/credentialBindingDisplay'
import { resolveManifestText } from '../utils/manifestLocale'
import { getTappIconStyle } from '../utils/tappColors'
import { buildTappDetailPageSeo } from '../utils/tappPageSeo'
import { TAPP_LIST_PATH, tappRunPath } from '../utils/tappPaths'
import '../../components/ConfigForm.css'
import './TappDetailPage.css'

function formatCredentialBindingDetail(
  binding: TappCredentialBindingSummary,
  copy: { credentialBindingSign: string; credentialBindingPlacement: string },
  format: (template: string, params: Record<string, string | number>) => string,
): string {
  if (binding.signAlg && binding.signOver?.length) {
    return format(copy.credentialBindingSign, {
      alg: binding.signAlg,
      fields: binding.signOver.join(', '),
    })
  }
  return format(copy.credentialBindingPlacement, {
    placement: binding.placement,
    field: binding.field,
  })
}

export function TappDetailPage() {
  const { id } = useParams<{ id: string }>()
  // react-router already decodes path params; avoid double-decode (throws on lone `%`)
  const tappId = id ?? ''
  const navigate = useNavigate()
  const { t, format, locale } = useI18n()
  const { catalog: g, bindGuide, renderGuide } = useSettingGuide()
  const { isAuthenticated, hasChecked } = useAuth()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.tapp,
    { isAuthenticated: false, isAdmin: false },
  )

  const [tapp, setTapp] = useState<TappInstance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isRunning, setIsRunning] = useState(false)
  const [settingsValues, setSettingsValues] = useState<Record<string, unknown>>(
    {},
  )
  const [settingsSaving, setSettingsSaving] = useState<string | null>(null)
  const [credentialStatuses, setCredentialStatuses] = useState<
    Record<string, TappCredentialStatus>
  >({})
  const [credentialSaving, setCredentialSaving] = useState<string | null>(null)
  const [inboundGuard, setInboundGuard] = useState<TappInboundGuardStatus | null>(
    null,
  )
  const [inboundGuardBusy, setInboundGuardBusy] = useState(false)
  /** 本地输入缓存，避免中文输入被打断 */
  const [localInputValues, setLocalInputValues] = useState<
    Record<string, string>
  >({})
  const pendingChangesRef = useRef<Record<string, unknown>>({})
  const debounceTimersRef = useRef<
    Record<string, ReturnType<typeof setTimeout>>
  >({})
  const [toastMessage, setToastMessage] = useState<string>('')
  const [toastType, setToastType] = useState<ToastType>('info')
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const [uninstallAnchor, setUninstallAnchor] = useState<HTMLElement | null>(
    null,
  )
  const [appVisibility, setAppVisibility] = useState<TappVisibility>('all')
  const [visibilitySaving, setVisibilitySaving] = useState(false)
  const runtime = getTappRuntime()

  usePageSeo(
    useMemo(
      () =>
        buildTappDetailPageSeo({
          tapp,
          tappId,
          locale,
          moduleOpenToAll,
        }),
      [tapp, tappId, locale, moduleOpenToAll],
    ),
  )

  const showToastMessage = useCallback(
    (message: string, type: ToastType = 'info') => {
      setToastType(type)
      setToastMessage(message)
    },
    [],
  )

  const loadSettings = useCallback(
    async (manifest: TappInstance['manifest']) => {
      if (!manifest.settings?.length) return

      try {
        const storedSettings = await TappApiService.getTappSettings(tappId)
        const values: Record<string, unknown> = {}
        for (const setting of manifest.settings) {
          values[setting.key] =
            storedSettings[setting.key] ?? setting.defaultValue
        }
        setSettingsValues(values)
      } catch (err) {
        console.error('Failed to load settings:', err)
      }
    },
    [tappId],
  )

  const loadCredentialStatuses = useCallback(async () => {
    const statuses = await TappApiService.getTappCredentialStatuses(tappId)
    setCredentialStatuses(
      Object.fromEntries(statuses.map((status) => [status.key, status])),
    )
  }, [tappId])

  const loadInboundGuard = useCallback(async () => {
    setInboundGuard(await TappApiService.getTappInboundGuard(tappId))
  }, [tappId])

  const saveCredential = useCallback(
    async (key: string, value: string) => {
      if (!value.trim()) throw new Error('Credential is required')
      setCredentialSaving(key)
      try {
        await TappApiService.setTappCredential(tappId, key, value)
        await loadCredentialStatuses()
        showToastMessage(t.tapp.credentialSaved, 'success')
      } catch (err) {
        console.error('Failed to save Tapp credential:', err)
        showToastMessage(t.tapp.credentialSaveFailed, 'error')
        throw err
      } finally {
        setCredentialSaving(null)
      }
    },
    [loadCredentialStatuses, showToastMessage, t, tappId],
  )

  const removeCredential = useCallback(
    async (key: string) => {
      if (!window.confirm(t.tapp.credentialRemoveConfirm)) return
      setCredentialSaving(key)
      try {
        await TappApiService.removeTappCredential(tappId, key)
        await loadCredentialStatuses()
        showToastMessage(t.tapp.credentialRemoved, 'success')
      } catch (err) {
        console.error('Failed to remove Tapp credential:', err)
        showToastMessage(t.tapp.credentialRemoveFailed, 'error')
      } finally {
        setCredentialSaving(null)
      }
    },
    [loadCredentialStatuses, showToastMessage, t, tappId],
  )

  const saveSetting = useCallback(
    async (key: string, value: unknown, showHint = true) => {
      setSettingsSaving(key)
      try {
        await TappApiService.setTappSetting(tappId, key, value)
        setSettingsValues((prev) => ({ ...prev, [key]: value }))
        delete pendingChangesRef.current[key]
        if (showHint) {
          showToastMessage(t.tapp.settingSaved, 'success')
        }
      } catch (err) {
        console.error('Failed to save setting:', err)
        showToastMessage(t.tapp.settingSaveFailed, 'error')
      } finally {
        setSettingsSaving(null)
      }
    },
    [tappId, t, showToastMessage],
  )

  const handleInputChange = useCallback(
    (key: string, value: string) => {
      setLocalInputValues((prev) => ({ ...prev, [key]: value }))
      pendingChangesRef.current[key] = value

      if (debounceTimersRef.current[key]) {
        clearTimeout(debounceTimersRef.current[key])
      }

      debounceTimersRef.current[key] = setTimeout(() => {
        if (pendingChangesRef.current[key] !== undefined) {
          void saveSetting(key, pendingChangesRef.current[key])
        }
      }, 2000)
    },
    [saveSetting],
  )

  const handleNumberChange = useCallback(
    (key: string, value: number) => {
      setLocalInputValues((prev) => ({ ...prev, [key]: String(value) }))
      pendingChangesRef.current[key] = value

      if (debounceTimersRef.current[key]) {
        clearTimeout(debounceTimersRef.current[key])
      }

      debounceTimersRef.current[key] = setTimeout(() => {
        if (pendingChangesRef.current[key] !== undefined) {
          void saveSetting(key, pendingChangesRef.current[key])
        }
      }, 2000)
    },
    [saveSetting],
  )

  const handleInputBlur = useCallback(
    (key: string, type: 'input' | 'number') => {
      if (debounceTimersRef.current[key]) {
        clearTimeout(debounceTimersRef.current[key])
        delete debounceTimersRef.current[key]
      }

      if (pendingChangesRef.current[key] !== undefined) {
        const value =
          type === 'number'
            ? Number(pendingChangesRef.current[key])
            : pendingChangesRef.current[key]
        void saveSetting(key, value)
      }
    },
    [saveSetting],
  )

  const saveAllPendingChanges = useCallback(async () => {
    const keys = Object.keys(pendingChangesRef.current)
    for (const key of keys) {
      await saveSetting(key, pendingChangesRef.current[key], false)
    }
  }, [saveSetting])

  useEffect(() => {
    const handleBeforeUnload = () => {
      const keys = Object.keys(pendingChangesRef.current)
      for (const key of keys) {
        void TappApiService.setTappSetting(
          tappId,
          key,
          pendingChangesRef.current[key],
        )
      }
    }

    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload)
      Object.values(debounceTimersRef.current).forEach(clearTimeout)
      void saveAllPendingChanges()
    }
  }, [tappId, saveAllPendingChanges])

  useEffect(() => {
    const loadTapp = async () => {
      try {
        const instance = runtime.getTapp(tappId)
        if (!instance) {
          setError(t.tapp.appNotExist)
          setLoading(false)
          return
        }

        setTapp(instance)
        setIsRunning(runtime.isRunning(tappId))
        setAppVisibility(instance.visibility === 'admin' ? 'admin' : 'all')

        // 设置属于已登录查看者的控制面数据；访客只使用 manifest 默认值。
        if (hasChecked && isAuthenticated) {
          const mayManageInstallation =
            instance.userRole === 'admin' ||
            (instance.userRole === 'user' && instance.isTemporary === true)
          await Promise.all([
            loadSettings(instance.manifest),
            instance.manifest.credentials?.length && mayManageInstallation
              ? loadCredentialStatuses().catch((err) => {
                  console.error('Failed to load Tapp credential status:', err)
                })
              : Promise.resolve(),
            mayManageInstallation
              ? loadInboundGuard().catch((err) => {
                  console.error('Failed to load inbound guard:', err)
                })
              : Promise.resolve(),
          ])
        }

        setLoading(false)
      } catch (err) {
        setError(err instanceof Error ? err.message : t.tapp.loadAppFailed)
        setLoading(false)
      }
    }

    void loadTapp()

    const unsubStarted = runtime.on('tapp:started', (data) => {
      if ((data as { id: string }).id === tappId) setIsRunning(true)
    })
    const unsubStopped = runtime.on('tapp:stopped', (data) => {
      if ((data as { id: string }).id === tappId) setIsRunning(false)
    })

    return () => {
      unsubStarted()
      unsubStopped()
    }
  }, [
    tappId,
    runtime,
    hasChecked,
    isAuthenticated,
    loadSettings,
    loadCredentialStatuses,
    loadInboundGuard,
    t,
  ])

  // 与 run/store 同款壳层：进场 + 返回时 requestClose 再 navigate（列表下 fixed 叠化）
  const {
    shellClassName,
    scrimClassName,
    shellStyle,
    onShellAnimationEnd,
    requestClose,
    isExiting,
  } = useTappShellPresence({ enabled: true, fade: true })

  const goBack = useCallback(() => {
    requestClose(() => navigate(TAPP_LIST_PATH))
  }, [navigate, requestClose])

  const handleToggleRunning = useCallback(async () => {
    try {
      if (isRunning) {
        await runtime.stopTapp(tappId)
      } else {
        await runtime.startTapp(tappId)
        void import('../../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.TAPP_RUN, {
              target: tappId,
              throttleMs: 2000,
            })
          },
        )
        navigate(tappRunPath(tappId))
      }
    } catch (err) {
      console.error('Failed to toggle Tapp:', err)
    }
  }, [runtime, tappId, isRunning, navigate])

  const handleUninstall = useCallback(
    (event?: ReactMouseEvent<HTMLButtonElement>) => {
      const el = event?.currentTarget
      setUninstallAnchor(el instanceof HTMLElement ? el : null)
      setShowUninstallDialog(true)
    },
    [],
  )

  const handleConfirmUninstall = useCallback(
    async (keepData: boolean) => {
      try {
        await runtime.uninstallTapp(tappId, { keepData })
        setShowUninstallDialog(false)
        setUninstallAnchor(null)
        goBack()
      } catch (err) {
        console.error('Failed to uninstall Tapp:', err)
        showToastMessage(t.tapp.uninstallFailed || 'Uninstall failed', 'error')
        throw err
      }
    },
    [runtime, tappId, goBack, t, showToastMessage],
  )

  const handleExport = useCallback(async () => {
    try {
      await TappApiService.exportTapp(tappId)
    } catch (err) {
      console.error('Failed to export Tapp:', err)
      showToastMessage(t.tapp.exportFailed || 'Export failed', 'error')
    }
  }, [tappId, t, showToastMessage])

  const handleVisibilityChange = useCallback(
    async (visibility: TappVisibility) => {
      if (visibility === appVisibility || visibilitySaving) return
      const previous = appVisibility
      setAppVisibility(visibility)
      setVisibilitySaving(true)
      try {
        await TappApiService.setTappVisibility(tappId, visibility)
        setTapp((prev) => (prev ? { ...prev, visibility } : prev))
        showToastMessage(t.tapp.appVisibilitySaved, 'success')
        void runtime.syncFromBackend(true)
      } catch (err) {
        console.error('Failed to update visibility:', err)
        setAppVisibility(previous)
        showToastMessage(t.tapp.appVisibilitySaveFailed, 'error')
      } finally {
        setVisibilitySaving(false)
      }
    },
    [
      appVisibility,
      visibilitySaving,
      tappId,
      t,
      showToastMessage,
      runtime,
    ],
  )

  const pageShell = (body: ReactNode) => (
    <AnimatedView
      className="relative min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12"
      style={{ pointerEvents: isExiting ? 'none' : undefined }}
    >
      {scrimClassName ? (
        <div className={scrimClassName} style={shellStyle} aria-hidden />
      ) : null}
      <div
        className={`tapp-detail-page relative z-[1] ${shellClassName}`}
        style={shellStyle}
        onAnimationEnd={onShellAnimationEnd}
      >
        {body}
      </div>
    </AnimatedView>
  )

  if (!tappId) {
    return pageShell(
      <div className="config-section setting-section">
        <div className="tapp-detail-state">
          <FaExclamationTriangle className="tapp-detail-state-icon" />
          <h3 className="tapp-detail-state-title">{t.tapp.cannotLoadApp}</h3>
          <p className="tapp-detail-state-desc">{t.tapp.appNotExist}</p>
          <SettingsButton
            variant="primary"
            icon={<LuChevronLeft />}
            onClick={goBack}
          >
            {t.tapp.backToAppList}
          </SettingsButton>
        </div>
      </div>,
    )
  }

  if (loading) {
    return pageShell(
      <div className="config-section setting-section">
        <div className="tapp-detail-state">
          <Spinner size="xl" color="primary" />
        </div>
      </div>,
    )
  }

  if (error || !tapp) {
    return pageShell(
      <div className="config-section setting-section">
        <div className="tapp-detail-state">
          <FaExclamationTriangle className="tapp-detail-state-icon" />
          <h3 className="tapp-detail-state-title">{t.tapp.cannotLoadApp}</h3>
          <p className="tapp-detail-state-desc">
            {error || t.tapp.appNotExist}
          </p>
          <SettingsButton
            variant="primary"
            icon={<LuChevronLeft />}
            onClick={goBack}
          >
            {t.tapp.backToAppList}
          </SettingsButton>
        </div>
      </div>,
    )
  }

  const { manifest } = tapp
  const { name: displayName, description: displayDescription } =
    resolveManifestText(manifest, locale)
  const authorUrl = manifest.author?.url ? sanitizeUrl(manifest.author.url) : ''
  const homepageUrl = manifest.homepage ? sanitizeUrl(manifest.homepage) : ''
  const repositoryUrl = manifest.repository
    ? sanitizeUrl(manifest.repository)
    : ''
  const canManageSettings =
    tapp.userRole === 'admin' ||
    (tapp.userRole === 'user' && tapp.isTemporary === true)
  const canManageVisibility =
    tapp.userRole === 'admin' && tapp.isAdminTapp === true
  const canStartStop =
    tapp.userRole === 'admin' ||
    (tapp.userRole === 'user' && tapp.isTemporary === true)
  const canUninstall =
    tapp.userRole === 'admin' ||
    (tapp.userRole === 'user' && tapp.isTemporary === true)

  const iconStyle = getTappIconStyle(manifest)
  const hasManifestSettings = Boolean(manifest.settings?.length)
  const hasManifestCredentials = Boolean(manifest.credentials?.length)
  /** 已登录用户始终展示应用设置组（含空态 / 只读说明） */
  const showSettingsGroup = isAuthenticated

  const settingsGroupDesc = hasManifestSettings
    ? canManageSettings
      ? t.tapp.customizeBehavior
      : t.tapp.settingsReadOnly
    : canManageVisibility
      ? t.tapp.appVisibilityDesc
      : t.tapp.noSettingsDesc

  const levelLabels = {
    basic: t.tapp.basicPermission,
    elevated: t.tapp.elevatedPermission,
    privileged: t.tapp.privilegedPermission,
  } as const

  /** 高等级优先：特权 → 提升 → 基础 */
  const permissionLevels = ['privileged', 'elevated', 'basic'] as const
  type PermissionLevelKey = (typeof permissionLevels)[number]
  interface PermissionListItem {
    key: TappPermission
    label: string
    description: string
    Icon: (typeof PERMISSION_CONFIG)[keyof typeof PERMISSION_CONFIG]['icon']
  }

  const permissionsByLevel: Record<PermissionLevelKey, PermissionListItem[]> = {
    basic: [],
    elevated: [],
    privileged: [],
  }
  for (const permission of tapp.grantedPermissions) {
    const config = PERMISSION_CONFIG[permission]
    if (!config) continue
    const level = PERMISSION_LEVELS[permission]
    permissionsByLevel[level].push({
      key: permission,
      label: String(
        t.tapp[config.labelKey as keyof typeof t.tapp] ?? permission,
      ),
      description: String(
        t.tapp[config.descriptionKey as keyof typeof t.tapp] ?? '',
      ),
      Icon: config.icon,
    })
  }

  const renderSettingControl = (setting: TappSettingItem) => {
    const busy = settingsSaving === setting.key
    const disabled = !canManageSettings || busy

    if (setting.type === 'toggle') {
      return (
        <SwitchItem
          key={setting.key}
          itemKey={setting.key}
          label={setting.label}
          description={setting.description}
          value={settingsValues[setting.key] === true}
          onChange={(checked) => void saveSetting(setting.key, checked)}
          disabled={disabled}
          loading={busy}
          layout="horizontal"
        />
      )
    }

    if (setting.type === 'select') {
      return (
        <SelectItem
          key={setting.key}
          itemKey={setting.key}
          label={setting.label}
          description={setting.description}
          value={String(settingsValues[setting.key] ?? '')}
          onChange={(v) => void saveSetting(setting.key, v)}
          options={(setting.options ?? []).map((opt) => ({
            value: opt.value,
            label: opt.label,
          }))}
          disabled={disabled}
          loading={busy}
          layout="horizontal"
        />
      )
    }

    if (setting.type === 'input') {
      return (
        <InputItem
          key={setting.key}
          itemKey={setting.key}
          label={setting.label}
          description={setting.description}
          value={
            localInputValues[setting.key] ??
            String(settingsValues[setting.key] ?? '')
          }
          onChange={(v) => handleInputChange(setting.key, v)}
          onBlur={() => handleInputBlur(setting.key, 'input')}
          placeholder={setting.placeholder}
          disabled={disabled}
          loading={busy}
          layout="horizontal"
        />
      )
    }

    if (setting.type === 'number') {
      const numValue =
        localInputValues[setting.key] !== undefined
          ? Number(localInputValues[setting.key])
          : Number(settingsValues[setting.key] ?? setting.min ?? 0)
      return (
        <NumberItem
          key={setting.key}
          itemKey={setting.key}
          label={setting.label}
          description={setting.description}
          value={Number.isFinite(numValue) ? numValue : 0}
          onChange={(v) => handleNumberChange(setting.key, v)}
          onBlur={() => handleInputBlur(setting.key, 'number')}
          min={setting.min}
          max={setting.max}
          step={setting.step}
          disabled={disabled}
          loading={busy}
          layout="horizontal"
        />
      )
    }

    if (setting.type === 'color') {
      return (
        <SettingItemWrapper
          key={setting.key}
          itemKey={setting.key}
          label={setting.label}
          description={setting.description}
          disabled={disabled}
          layout="horizontal"
        >
          <input
            type="color"
            className="tapp-detail-color-input"
            value={String(settingsValues[setting.key] ?? '#6366f1')}
            onChange={(e) => void saveSetting(setting.key, e.target.value)}
            disabled={disabled}
            aria-label={setting.label}
            title={setting.label}
          />
        </SettingItemWrapper>
      )
    }

    return null
  }

  const overviewActions = [
    ...(canStartStop
      ? [
          {
            key: 'toggle-run',
            label: isRunning ? t.tapp.stop : t.tapp.start,
            onClick: () => void handleToggleRunning(),
            variant: (isRunning ? 'secondary' : 'primary') as
              | 'primary'
              | 'secondary',
            icon: isRunning ? <FaPause /> : <FaPlay />,
          },
        ]
      : []),
    {
      key: 'export',
      label: t.tapp.export || 'Export',
      onClick: () => void handleExport(),
      variant: 'secondary' as const,
      icon: <FaDownload />,
    },
    ...(canUninstall
      ? [
          {
            key: 'uninstall',
            label: t.tapp.uninstall,
            onClick: handleUninstall,
            variant: 'danger' as const,
            icon: <FaTrash />,
          },
        ]
      : []),
  ]

  // Copy disabled on the whole card (`copyable={false}`); no per-field copyText.
  const infoFields = [
    {
      key: 'id',
      label: t.tapp.appId,
      value: manifest.id,
      mono: true,
    },
    {
      key: 'version',
      label: t.tapp.version,
      value: `v${manifest.version}`,
    },
    ...(manifest.author
      ? [
          {
            key: 'author',
            label: t.tapp.author,
            value: (
              <span>
                {manifest.author.name}
                {manifest.author.email ? (
                  <>
                    <br />
                    <span className="settings-text-3">
                      {manifest.author.email}
                    </span>
                  </>
                ) : null}
                {authorUrl ? (
                  <>
                    <br />
                    <a
                      href={authorUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="settings-text-2"
                      style={{
                        color: 'var(--cfg-accent)',
                        textDecoration: 'underline',
                      }}
                    >
                      {t.tapp.homepage}
                    </a>
                  </>
                ) : null}
              </span>
            ),
          },
        ]
      : []),
    {
      key: 'installed',
      label: t.tapp.installedAt,
      value: new Date(tapp.installedAt).toLocaleDateString(),
    },
    ...(tapp.lastRunAt
      ? [
          {
            key: 'lastRun',
            label: t.tapp.lastRunAt,
            value: new Date(tapp.lastRunAt).toLocaleString(),
          },
        ]
      : []),
    ...(homepageUrl
      ? [
          {
            key: 'homepage',
            label: t.tapp.homepage,
            value: (
              <a
                href={homepageUrl}
                target="_blank"
                rel="noopener noreferrer"
                style={{
                  color: 'var(--cfg-accent)',
                  textDecoration: 'underline',
                }}
              >
                {t.tapp.visit}
              </a>
            ),
          },
        ]
      : []),
    ...(repositoryUrl
      ? [
          {
            key: 'repository',
            label: t.tapp.repository,
            value: (
              <a
                href={repositoryUrl}
                target="_blank"
                rel="noopener noreferrer"
                style={{
                  color: 'var(--cfg-accent)',
                  textDecoration: 'underline',
                }}
              >
                {t.tapp.visit}
              </a>
            ),
          },
        ]
      : []),
  ]

  return pageShell(
    <>
      <SettingSection
        sectionId="tapp-detail"
        title={displayName}
        description={displayDescription || undefined}
        detail={displayDescription || g.tapp.detail.what}
        showResetPage={false}
        helpToggle
        {...bindGuide('tapp.detail', g.tapp.detail)}
        icon={
          <TappIconBadge
            icon={manifest.icon}
            iconSvg={manifest.iconSvg}
            name={displayName}
            id={manifest.id}
            themeColor={manifest.themeColor}
            category={manifest.category}
            permissions={manifest.permissions}
            iconStyle={iconStyle}
            shellClassName="tapp-page-icon tapp-page-icon--header tapp-detail-icon"
            glyphSizeClass="w-7 h-7"
            glyphTextClass="text-xl"
          />
        }
        titleExtra={
          <span className="tapp-detail-status">
            <span
              className={`tapp-detail-status-dot${
                isRunning ? ' is-running' : ''
              }`}
              aria-hidden
            />
            {isRunning ? t.tapp.running : t.tapp.stopped}
          </span>
        }
        headerLeading={
          <button
            type="button"
            className="section-header-back"
            onClick={goBack}
            aria-label={t.tapp.backToAppList}
          >
            <LuChevronLeft size={18} aria-hidden />
            <span>{t.common.back}</span>
          </button>
        }
      >
        {/* 应用信息 + 主操作 */}
        <SettingGroup
          id="tapp-overview"
          title={t.tapp.appInfo}
          description={t.tapp.detailInfo}
          {...bindGuide('tapp.overview', g.tapp.overview)}
        >
          <InfoActionCard
            copyable={false}
            fields={infoFields}
            actions={overviewActions}
          />
        </SettingGroup>

        {/* 应用设置 */}
        {showSettingsGroup && (
          <SettingGroup
            id="tapp-app-settings"
            title={t.tapp.appSettings}
            description={settingsGroupDesc}
            icon={<FaCog />}
            {...bindGuide('tapp.appSettings', g.tapp.appSettings)}
          >
            {canManageVisibility && (
              <SettingItemWrapper
                itemKey="tapp-visibility"
                label={t.tapp.appVisibility}
                description={t.tapp.appVisibilityDesc}
                layout="horizontal"
                disabled={visibilitySaving}
                {...bindGuide('tapp.appVisibility', g.tapp.appVisibility)}
              >
                <SegmentedControl
                  size="sm"
                  columns={2}
                  value={appVisibility}
                  disabled={visibilitySaving}
                  options={[
                    {
                      value: 'all' as const,
                      label: t.tapp.appVisibilityAll,
                    },
                    {
                      value: 'admin' as const,
                      label: t.tapp.appVisibilityAdmin,
                    },
                  ]}
                  onChange={handleVisibilityChange}
                  ariaLabel={t.tapp.appVisibility}
                />
              </SettingItemWrapper>
            )}

            {hasManifestSettings
              ? manifest.settings!.map(renderSettingControl)
              : !canManageVisibility && (
                  <p className="settings-text-3" style={{ margin: 0 }}>
                    {t.tapp.noSettingsAvailable}
                  </p>
                )}
          </SettingGroup>
        )}

        {canManageSettings && hasManifestCredentials && (
          <SettingGroup
            id="tapp-api-credentials"
            title={t.tapp.apiCredentials}
            description={t.tapp.apiCredentialsDesc}
            icon={<FaKey />}
          >
            {manifest.credentials!.map((credential) => {
              const status = credentialStatuses[credential.key]
              const busy = credentialSaving === credential.key
              const destination = status?.origins.length
                ? format(t.tapp.credentialOrigins, {
                    origins: status.origins.join(', '),
                  })
                : (status?.bindings ?? []).some((binding) => binding.placement === 'verify')
                  ? t.tapp.credentialInboundVerify
                  : ''
              const bindingStatus = status?.bindings ?? []
              const bindingSummary = summarizeCredentialBindings(bindingStatus)
              const bindingRows = bindingSummary.rows.map((row, index) => ({
                ...row,
                detail: formatCredentialBindingDetail(
                  bindingStatus[index]!,
                  t.tapp,
                  format,
                ),
              }))
              const sharedAccess =
                bindingSummary.accesses.length === 1
                  ? bindingSummary.accesses[0]
                  : ''
              const sharedDetails = uniqueNonEmpty(
                bindingRows.map((row) => row.detail),
              )
              const canShareMeta =
                bindingSummary.accesses.length <= 1 && sharedDetails.length <= 1
              const sharedMeta = [
                sharedAccess,
                sharedDetails.length === 1 ? sharedDetails[0] : '',
              ]
                .filter(Boolean)
                .join(' · ')
              const splitMeta = !canShareMeta
              const description = [
                credential.description,
                destination,
                status?.needsReauthorization
                  ? t.tapp.credentialReauthorizationRequired
                  : undefined,
              ]
                .filter(Boolean)
                .join(' · ')

              return (
                <div className="tapp-detail-credential" key={credential.key}>
                  <InputItem
                    itemKey={`credential-${credential.key}`}
                    label={credential.label}
                    description={description}
                    value=""
                    onChange={() => undefined}
                    variant="clickToEdit"
                    inputType="password"
                    autoComplete="new-password"
                    placeholder={credential.placeholder}
                    emptyLabel={
                      status?.configured
                        ? t.tapp.credentialConfigured
                        : t.tapp.credentialNotConfigured
                    }
                    editLabel={
                      status?.configured
                        ? t.tapp.credentialReplace
                        : t.tapp.credentialConfigure
                    }
                    saveLabel={t.common.save}
                    disabled={busy}
                    loading={busy}
                    onCommit={(value) => saveCredential(credential.key, value)}
                  />
                  {bindingRows.length > 0 && (
                    <div className="tapp-detail-credential-bindings">
                      <ul
                        className="tapp-detail-credential-binding-list"
                        aria-label={t.tapp.credentialBindings}
                      >
                        {bindingRows.map((row) => (
                          <li key={row.api} title={row.endpoint}>
                            <span className="tapp-detail-credential-binding-method">
                              {row.method}
                            </span>
                            <span className="tapp-detail-credential-binding-api">
                              {row.api}
                            </span>
                            {splitMeta && (
                              <span className="tapp-detail-credential-binding-extra">
                                {[row.access, row.detail]
                                  .filter(Boolean)
                                  .join(' · ')}
                              </span>
                            )}
                          </li>
                        ))}
                      </ul>
                      {sharedMeta && canShareMeta && (
                        <p className="tapp-detail-credential-binding-meta">
                          {sharedMeta}
                        </p>
                      )}
                    </div>
                  )}
                  {status?.configured && (
                    <div className="tapp-detail-credential-actions">
                      <SettingsButton
                        variant="ghost"
                        size="sm"
                        disabled={busy}
                        onClick={() => void removeCredential(credential.key)}
                      >
                        {t.tapp.credentialRemove}
                      </SettingsButton>
                    </div>
                  )}
                </div>
              )
            })}
          </SettingGroup>
        )}

        {canManageSettings && inboundGuard && (
          <SettingGroup
            id="tapp-inbound-guard"
            title={t.tapp.inboundGuard}
            description={t.tapp.inboundGuardDesc}
            icon={<FaLock />}
          >
            <SwitchItem
              itemKey="inbound-paused"
              label={t.tapp.inboundPaused}
              description={t.tapp.inboundPausedHint}
              value={inboundGuard.paused}
              disabled={inboundGuardBusy}
              onChange={(checked) => {
                void (async () => {
                  setInboundGuardBusy(true)
                  try {
                    if (checked) await TappApiService.pauseTappInbound(tappId)
                    else await TappApiService.resumeTappInbound(tappId)
                    await loadInboundGuard()
                  } catch (err) {
                    console.error('Failed to update inbound guard:', err)
                    showToastMessage(t.tapp.inboundGuardSaveFailed, 'error')
                  } finally {
                    setInboundGuardBusy(false)
                  }
                })()
              }}
            />
            <p className="settings-text-3" style={{ margin: '12px 0 8px' }}>
              {t.tapp.inboundBlocks}
            </p>
            {inboundGuard.blocks.length === 0 ? (
              <p className="settings-text-3" style={{ margin: 0 }}>
                {t.tapp.inboundNoBlocks}
              </p>
            ) : (
              inboundGuard.blocks.map((block) => (
                <div
                  className="tapp-detail-credential-actions"
                  key={block.fingerprint}
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    gap: 12,
                    marginBottom: 8,
                  }}
                >
                  <span className="settings-text-3">
                    {block.fingerprint.slice(0, 16)} ·{' '}
                    {block.source === 'manual'
                      ? t.tapp.inboundBlockManual
                      : t.tapp.inboundBlockAuto}
                  </span>
                  <SettingsButton
                    variant="ghost"
                    size="sm"
                    disabled={inboundGuardBusy}
                    onClick={() => {
                      void (async () => {
                        setInboundGuardBusy(true)
                        try {
                          await TappApiService.unblockTappInbound(
                            tappId,
                            block.fingerprint,
                          )
                          await loadInboundGuard()
                        } catch (err) {
                          console.error('Failed to unblock inbound caller:', err)
                          showToastMessage(t.tapp.inboundGuardSaveFailed, 'error')
                        } finally {
                          setInboundGuardBusy(false)
                        }
                      })()
                    }}
                  >
                    {t.tapp.inboundUnblock}
                  </SettingsButton>
                </div>
              ))
            )}
          </SettingGroup>
        )}

        {/* 权限：按等级分组，等级内部三列 */}
        <SettingGroup
          id="tapp-permissions"
          title={t.tapp.permissions}
          description={format(t.tapp.grantedPermissions, {
            count: tapp.grantedPermissions.length,
          })}
          icon={<FaLock />}
          {...bindGuide('tapp.permissions', g.tapp.permissions)}
        >
          {tapp.grantedPermissions.length === 0 ? (
            <p className="settings-text-3" style={{ margin: 0 }}>
              {t.tapp.noPermissions}
            </p>
          ) : (
            <div className="tapp-perm-levels">
              {permissionLevels.map((level) => {
                const items = permissionsByLevel[level]
                if (items.length === 0) return null
                const levelGuide =
                  level === 'privileged'
                    ? g.tapp.permPrivileged
                    : level === 'elevated'
                      ? g.tapp.permElevated
                      : g.tapp.permBasic
                const levelGuidePath =
                  level === 'privileged'
                    ? 'tapp.permPrivileged'
                    : level === 'elevated'
                      ? 'tapp.permElevated'
                      : 'tapp.permBasic'
                return (
                  <SettingGroup
                    key={level}
                    toc={false}
                    className={`tapp-perm-group tapp-perm-group--${level}`}
                    title={levelLabels[level]}
                    titleExtra={
                      <span
                        className={`tapp-perm-level tapp-perm-level--${level}`}
                      >
                        {items.length}
                      </span>
                    }
                    {...bindGuide(levelGuidePath, levelGuide)}
                  >
                    {/* 等级内部三列排布权限卡（只读 + 单项指南） */}
                    <div className="tapp-perm-cards checkbox-group-options">
                      {items.map(({ key, label, description, Icon }) => {
                        const guidePath = tappPermissionGuidePath(key)
                        const guide = renderGuide(
                          getTappPermissionGuide(locale, key),
                        )
                        return (
                          <div
                            key={key}
                            {...guideDomProps(guidePath)}
                            className={`tapp-perm-card tapp-perm-card--${level} checkbox-group-card has-icon no-indicator has-guide-anchor`}
                            role="group"
                            aria-label={`${label} · ${levelLabels[level]}`}
                          >
                            <span className="checkbox-group-card-header">
                              <span
                                className="checkbox-group-card-icon"
                                aria-hidden
                              >
                                <Icon />
                              </span>
                              <span className="checkbox-group-card-text">
                                <span className="checkbox-group-card-label">
                                  {label}
                                  <SettingTitleGuideEntry
                                    title={label}
                                    guide={guide}
                                  />
                                </span>
                                {description ? (
                                  <span className="checkbox-group-card-desc">
                                    {description}
                                  </span>
                                ) : null}
                              </span>
                            </span>
                          </div>
                        )
                      })}
                    </div>
                  </SettingGroup>
                )
              })}
            </div>
          )}
        </SettingGroup>
      </SettingSection>

      {toastMessage && (
        <Toast
          message={toastMessage}
          type={toastType}
          onClose={() => setToastMessage('')}
        />
      )}

      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={displayName || tappId}
        anchorEl={uninstallAnchor}
        onCancel={() => {
          setShowUninstallDialog(false)
          setUninstallAnchor(null)
        }}
        onConfirm={handleConfirmUninstall}
      />
    </>,
  )
}

export default TappDetailPage
