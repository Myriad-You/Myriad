/**
 * Tapp 璇︽儏/閰嶇疆椤甸潰
 * 鏄剧ず Tapp 璇︾粏淇℃伅鍜岄厤缃€夐」
 */

import type { TappInstance, TappPermission, TappSettingItem } from '../types'
import {
  FaArrowLeft,
  FaBell,
  FaChartBar,
  FaCheck,
  FaChevronUp,
  FaCog,
  FaDatabase,
  FaDownload,
  FaExclamationTriangle,
  FaGamepad,
  FaHdd,
  FaInfoCircle,
  FaLock,
  FaPause,
  FaPlay,
  FaRobot,
  FaSpinner,
  FaTrash,
} from '@lib/icons'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../../components/AnimatedView'
import Toast from '../../components/Toast'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePerformanceProfile } from '../../hooks/usePerformanceProfile'
import { TappIcon } from '../components/TappIcon'
import { UninstallConfirmDialog } from '../components/UninstallConfirmDialog'
import { getTappRuntime } from '../runtime'
import * as TappApiService from '../services/TappApiService'
import { getTappIconStyle } from '../utils/tappColors'

// 使用 TappIcon 组件统一处理图标渲染

interface TappDetailPageProps {
  tappId: string
}

// 权限配置 - 使用 i18n 键名（对应 t.tapp 中的扁平键）
const PERMISSION_CONFIG: Record<TappPermission, {
  icon: typeof FaGamepad
  labelKey: string
  descriptionKey: string
  level: 'basic' | 'elevated' | 'privileged'
}> = {
  'widget:register': {
    icon: FaGamepad,
    labelKey: 'permRegisterWidget',
    descriptionKey: 'permRegisterWidgetDesc',
    level: 'basic',
  },
  'platform:read': {
    icon: FaDatabase,
    labelKey: 'permReadPlatform',
    descriptionKey: 'permReadPlatformDesc',
    level: 'basic',
  },
  'platform:write': {
    icon: FaDatabase,
    labelKey: 'permWritePlatform',
    descriptionKey: 'permWritePlatformDesc',
    level: 'elevated',
  },
  'platform:register': {
    icon: FaDatabase,
    labelKey: 'permRegisterPlatform',
    descriptionKey: 'permRegisterPlatformDesc',
    level: 'privileged',
  },
  'ai:generate': {
    icon: FaRobot,
    labelKey: 'permAiGenerate',
    descriptionKey: 'permAiGenerateDesc',
    level: 'elevated',
  },
  'ai:analyze': {
    icon: FaRobot,
    labelKey: 'permAiAnalyze',
    descriptionKey: 'permAiAnalyzeDesc',
    level: 'elevated',
  },
  'ai:chat': {
    icon: FaRobot,
    labelKey: 'permAiChat',
    descriptionKey: 'permAiChatDesc',
    level: 'elevated',
  },
  'ai:image': {
    icon: FaRobot,
    labelKey: 'permAiImage',
    descriptionKey: 'permAiImageDesc',
    level: 'elevated',
  },
  'report:read': {
    icon: FaChartBar,
    labelKey: 'permReadReport',
    descriptionKey: 'permReadReportDesc',
    level: 'basic',
  },
  'report:write': {
    icon: FaChartBar,
    labelKey: 'permWriteReport',
    descriptionKey: 'permWriteReportDesc',
    level: 'elevated',
  },
  'storage': {
    icon: FaHdd,
    labelKey: 'permStorage',
    descriptionKey: 'permStorageDesc',
    level: 'basic',
  },
  'ui:notification': {
    icon: FaBell,
    labelKey: 'permNotification',
    descriptionKey: 'permNotificationDesc',
    level: 'basic',
  },
  'ui:fullscreen': {
    icon: FaChevronUp,
    labelKey: 'permFullscreen',
    descriptionKey: 'permFullscreenDesc',
    level: 'basic',
  },
  'ui:theme': {
    icon: FaChevronUp,
    labelKey: 'permReadTheme',
    descriptionKey: 'permReadThemeDesc',
    level: 'basic',
  },
  'ui:confirm': {
    icon: FaChevronUp,
    labelKey: 'permConfirm',
    descriptionKey: 'permConfirmDesc',
    level: 'basic',
  },
  'network:fetch': {
    icon: FaDatabase,
    labelKey: 'permNetworkFetch',
    descriptionKey: 'permNetworkFetchDesc',
    level: 'elevated',
  },
  'media:control': {
    icon: FaGamepad,
    labelKey: 'permMediaControl',
    descriptionKey: 'permMediaControlDesc',
    level: 'elevated',
  },
  'media:read': {
    icon: FaGamepad,
    labelKey: 'permMediaRead',
    descriptionKey: 'permMediaReadDesc',
    level: 'basic',
  },
  'component:theme': {
    icon: FaChevronUp,
    labelKey: 'permRegisterTheme',
    descriptionKey: 'permRegisterThemeDesc',
    level: 'elevated',
  },
  'component:agent': {
    icon: FaRobot,
    labelKey: 'permRegisterAgent',
    descriptionKey: 'permRegisterAgentDesc',
    level: 'privileged',
  },
  'shortcut:register': {
    icon: FaGamepad,
    labelKey: 'permRegisterShortcut',
    descriptionKey: 'permRegisterShortcutDesc',
    level: 'elevated',
  },
  'event:publish': {
    icon: FaBell,
    labelKey: 'permPublishEvent',
    descriptionKey: 'permPublishEventDesc',
    level: 'elevated',
  },
  'event:subscribe': {
    icon: FaBell,
    labelKey: 'permSubscribeEvent',
    descriptionKey: 'permSubscribeEventDesc',
    level: 'basic',
  },
  'scheduler:register': {
    icon: FaCog,
    labelKey: 'permSchedulerRegister',
    descriptionKey: 'permSchedulerRegisterDesc',
    level: 'elevated',
  },
}

/**
 * Tapp 璇︽儏椤甸潰缁勪欢
 */
export function TappDetailPage({ tappId }: TappDetailPageProps) {
  const navigate = useNavigate()
  const { t, format } = useI18n()

  // 🎬 动画和性能配置
  const animConfig = useAnimationLevel()
  const perf = usePerformanceProfile()

  const [tapp, setTapp] = useState<TappInstance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isRunning, setIsRunning] = useState(false)
  const [settingsValues, setSettingsValues] = useState<Record<string, unknown>>({})
  const [settingsSaving, setSettingsSaving] = useState<string | null>(null)
  // 本地输入值缓存（用于防止中文输入被打断）
  const [localInputValues, setLocalInputValues] = useState<Record<string, string>>({})
  // 待保存的设置（用于延迟保存和页面退出时保存）
  const pendingChangesRef = useRef<Record<string, unknown>>({})
  // 防抖定时器引用
  const debounceTimersRef = useRef<Record<string, ReturnType<typeof setTimeout>>>({})
  // Toast 消息
  const [toastMessage, setToastMessage] = useState<string>('')
  // 卸载确认对话框状态
  const [showUninstallDialog, setShowUninstallDialog] = useState(false)
  const runtime = getTappRuntime()

  // 鍔犺浇璁剧疆鍊?- 骞惰鍔犺浇浼樺寲
  const loadSettings = useCallback(async (manifest: TappInstance['manifest']) => {
    if (!manifest.settings?.length)
      return

    try {
      // 骞惰鍔犺浇鎵€鏈夎缃€硷紝鎻愬崌鍔犺浇鎬ц兘
      const settingsPromises = manifest.settings.map(async (setting) => {
        const stored = await TappApiService.getStorage(tappId, `_settings.${setting.key}`)
        return {
          key: setting.key,
          value: stored !== null ? stored : setting.defaultValue,
        }
      })

      const results = await Promise.all(settingsPromises)
      const values: Record<string, unknown> = {}
      for (const { key, value } of results) {
        values[key] = value
      }
      setSettingsValues(values)
    }
    catch (err) {
      console.error('Failed to load settings:', err)
    }
  }, [tappId])

  // 淇濆瓨璁剧疆鍊?
  const saveSetting = useCallback(async (key: string, value: unknown, showHint = true) => {
    setSettingsSaving(key)
    try {
      await TappApiService.setStorage(tappId, `_settings.${key}`, value)
      setSettingsValues(prev => ({ ...prev, [key]: value }))
      // 从待保存列表中移除
      delete pendingChangesRef.current[key]
      // 显示保存成功 Toast
      if (showHint) {
        setToastMessage(`✓ ${t.tapp.settingSaved}`)
      }
    }
    catch (err) {
      console.error('Failed to save setting:', err)
      setToastMessage(`✗ ${t.tapp.settingSaveFailed}`)
    }
    finally {
      setSettingsSaving(null)
    }
  }, [tappId, t])

  // 处理输入框变化（仅更新本地状态，不立即保存）
  const handleInputChange = useCallback((key: string, value: string) => {
    setLocalInputValues(prev => ({ ...prev, [key]: value }))
    // 标记为待保存
    pendingChangesRef.current[key] = value

    // 清除之前的防抖定时器
    if (debounceTimersRef.current[key]) {
      clearTimeout(debounceTimersRef.current[key])
    }

    // 设置新的防抖定时器（2秒无操作后保存）
    debounceTimersRef.current[key] = setTimeout(() => {
      if (pendingChangesRef.current[key] !== undefined) {
        saveSetting(key, pendingChangesRef.current[key])
      }
    }, 2000)
  }, [saveSetting])

  // 处理数字输入框变化
  const handleNumberChange = useCallback((key: string, value: number) => {
    setLocalInputValues(prev => ({ ...prev, [key]: String(value) }))
    pendingChangesRef.current[key] = value

    if (debounceTimersRef.current[key]) {
      clearTimeout(debounceTimersRef.current[key])
    }

    debounceTimersRef.current[key] = setTimeout(() => {
      if (pendingChangesRef.current[key] !== undefined) {
        saveSetting(key, pendingChangesRef.current[key])
      }
    }, 2000)
  }, [saveSetting])

  // 输入框失焦时保存
  const handleInputBlur = useCallback((key: string, type: 'input' | 'number') => {
    // 清除防抖定时器
    if (debounceTimersRef.current[key]) {
      clearTimeout(debounceTimersRef.current[key])
      delete debounceTimersRef.current[key]
    }

    // 如果有待保存的更改，立即保存
    if (pendingChangesRef.current[key] !== undefined) {
      const value = type === 'number'
        ? Number(pendingChangesRef.current[key])
        : pendingChangesRef.current[key]
      saveSetting(key, value)
    }
  }, [saveSetting])

  // 保存所有待保存的更改
  const saveAllPendingChanges = useCallback(async () => {
    const keys = Object.keys(pendingChangesRef.current)
    for (const key of keys) {
      await saveSetting(key, pendingChangesRef.current[key], false)
    }
  }, [saveSetting])

  // 页面退出时保存所有待保存的更改
  useEffect(() => {
    const handleBeforeUnload = () => {
      // 同步保存（尽力而为）
      const keys = Object.keys(pendingChangesRef.current)
      for (const key of keys) {
        TappApiService.setStorage(tappId, `_settings.${key}`, pendingChangesRef.current[key])
      }
    }

    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload)
      // 组件卸载时清除所有定时器并保存
      Object.values(debounceTimersRef.current).forEach(clearTimeout)
      saveAllPendingChanges()
    }
  }, [tappId, saveAllPendingChanges])

  // 鍔犺浇 Tapp
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

        // 鍔犺浇璁剧疆鍊?
        await loadSettings(instance.manifest)

        setLoading(false)
      }
      catch (err) {
        setError(err instanceof Error ? err.message : t.tapp.loadAppFailed)
        setLoading(false)
      }
    }

    loadTapp()

    // 鐩戝惉鐘舵€佸彉鍖?
    const unsubStarted = runtime.on('tapp:started', (id) => {
      if (id === tappId)
        setIsRunning(true)
    })
    const unsubStopped = runtime.on('tapp:stopped', (id) => {
      if (id === tappId)
        setIsRunning(false)
    })

    return () => {
      unsubStarted()
      unsubStopped()
    }
  }, [tappId, runtime])

  // 杩斿洖
  const goBack = useCallback(() => {
    navigate('/tapp')
  }, [navigate])

  // 鍚姩/鍋滄
  const handleToggleRunning = useCallback(async () => {
    try {
      if (isRunning) {
        await runtime.stopTapp(tappId)
      }
      else {
        await runtime.startTapp(tappId)
        navigate(`/tapp/run/${tappId}`)
      }
    }
    catch (err) {
      console.error('Failed to toggle Tapp:', err)
    }
  }, [runtime, tappId, isRunning, navigate])

  // 鍗歌浇 - 显示确认对话框
  const handleUninstall = useCallback(() => {
    setShowUninstallDialog(true)
  }, [])

  // 确认卸载
  const handleConfirmUninstall = useCallback(async (keepData: boolean) => {
    try {
      await runtime.uninstallTapp(tappId, { keepData })
      setShowUninstallDialog(false)
      goBack()
    }
    catch (err) {
      console.error('Failed to uninstall Tapp:', err)
      setToastMessage(t.tapp.uninstallFailed || 'Uninstall failed')
      throw err // 让组件处理 loading 状态
    }
  }, [runtime, tappId, goBack, t])
  // 导出
  const handleExport = useCallback(async () => {
    try {
      await TappApiService.exportTapp(tappId)
    }
    catch (err) {
      console.error('Failed to export Tapp:', err)
      alert(t.tapp.exportFailed || 'Export failed')
    }
  }, [tappId, t])
  // 鍔犺浇鐘舵€?
  if (loading) {
    return (
      <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
        <div className="max-w-4xl mx-auto">
          <div className="glass rounded-xl p-8 md:p-12 text-center">
            <FaSpinner className="w-12 h-12 mx-auto text-indigo-500 animate-spin mb-4" />
            <h3 className="text-lg font-medium text-gray-800 dark:text-gray-100 mb-2">
              {t.tapp.loading}
            </h3>
          </div>
        </div>
      </AnimatedView>
    )
  }

  // 閿欒鐘舵€?
  if (error || !tapp) {
    return (
      <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
        <div className="max-w-4xl mx-auto">
          <div className="glass rounded-xl p-8 md:p-12 text-center">
            <FaExclamationTriangle className="w-12 h-12 mx-auto text-red-500 mb-4" />
            <h3 className="text-lg font-medium text-gray-800 dark:text-gray-100 mb-2">
              {t.tapp.cannotLoadApp}
            </h3>
            <p className="text-gray-500 dark:text-gray-400 mb-6 text-sm">
              {error || t.tapp.appNotExist}
            </p>
            <button
              onClick={goBack}
              className="inline-flex items-center gap-2 px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white font-medium rounded-lg transition-colors"
            >
              <FaArrowLeft className="w-4 h-4" />
              {t.tapp.backToAppList}
            </button>
          </div>
        </div>
      </AnimatedView>
    )
  }

  const { manifest } = tapp

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <div className="max-w-4xl mx-auto">
        {/* 澶撮儴鍗＄墖 */}
        <div className="mb-4 md:mb-6">
          <div className="glass rounded-xl p-4 md:p-5">
            <div className="flex items-start gap-4">
              <button
                onClick={goBack}
                className="p-2 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700 rounded-lg transition-colors flex-shrink-0"
                title={t.tapp.back}
                aria-label={t.tapp.backToAppList}
              >
                <FaArrowLeft className="w-5 h-5" />
              </button>

              <div
                className={`w-14 h-14 sm:w-16 sm:h-16 rounded-xl ${getTappIconStyle(manifest).className} flex items-center justify-center text-white text-2xl sm:text-3xl font-bold shadow-lg flex-shrink-0`}
                style={getTappIconStyle(manifest).style}
              >
                <TappIcon
                  icon={manifest.icon}
                  iconSvg={manifest.iconSvg}
                  name={manifest.name}
                  sizeClass="w-10 h-10 sm:w-12 sm:h-12"
                  textSizeClass="text-2xl sm:text-3xl"
                />
              </div>

              <div className="flex-1 min-w-0">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <h1 className="text-xl sm:text-2xl font-bold text-gray-800 dark:text-gray-100">
                      {manifest.name}
                    </h1>
                    <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                      v
                      {manifest.version}
                      {manifest.author && ` · ${manifest.author.name}`}
                    </p>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    <div className={`w-2 h-2 rounded-full ${isRunning ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}`} />
                    <span className="text-sm text-gray-500 dark:text-gray-400">
                      {isRunning ? t.tapp.running : t.tapp.stopped}
                    </span>
                  </div>
                </div>
                <p className="text-gray-600 dark:text-gray-300 mt-2 text-sm">
                  {manifest.description}
                </p>
              </div>
            </div>

            {/* 鎿嶄綔鎸夐挳 */}
            {(() => {
              // 权限检查：判断当前用户是否可以执行操作
              // - admin: 可以操作所有 Tapp
              // - user: 只能操作自己临时安装的 Tapp（isTemporary=true），不能操作管理员的 Tapp
              // - guest: 只能查看，不能操作
              const canStartStop = tapp.userRole === 'admin' || (tapp.userRole === 'user' && tapp.isTemporary === true)
              const canUninstall = tapp.userRole === 'admin' || (tapp.userRole === 'user' && tapp.isTemporary === true)

              return (
                <div className="flex items-center gap-3 mt-4 pt-4 border-t border-gray-200/50 dark:border-neutral-700/50">
                  {canStartStop && (
                    <button
                      onClick={handleToggleRunning}
                      className={`flex-1 sm:flex-none px-4 py-2 font-medium rounded-lg transition-colors flex items-center justify-center gap-2 ${
                        isRunning
                          ? 'bg-orange-600 hover:bg-orange-700 text-white'
                          : 'bg-green-600 hover:bg-green-700 text-white'
                      }`}
                      title={isRunning ? t.tapp.stop : t.tapp.start}
                    >
                      {isRunning
                        ? (
                            <>
                              <FaPause className="w-4 h-4" />
                              {t.tapp.stop}
                            </>
                          )
                        : (
                            <>
                              <FaPlay className="w-4 h-4" />
                              {t.tapp.start}
                            </>
                          )}
                    </button>
                  )}
                  {canUninstall && (
                    <button
                      onClick={handleUninstall}
                      className="px-4 py-2 font-medium rounded-lg transition-colors flex items-center gap-2 text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20 cursor-pointer"
                      title={t.tapp.uninstall}
                    >
                      <FaTrash className="w-4 h-4" />
                      <span className="hidden sm:inline">{t.tapp.uninstall}</span>
                    </button>
                  )}
                  <button
                    onClick={handleExport}
                    className="px-4 py-2 font-medium rounded-lg transition-colors flex items-center gap-2 text-blue-600 hover:bg-blue-50 dark:hover:bg-blue-900/20 cursor-pointer"
                    title={t.tapp.export || 'Export'}
                  >
                    <FaDownload className="w-4 h-4" />
                    <span className="hidden sm:inline">{t.tapp.export || 'Export'}</span>
                  </button>
                </div>
              )
            })()}
          </div>
        </div>

        {/* 搴旂敤璁剧疆 - 鏀惧湪鏉冮檺涔嬪墠 */}
        <div className="mb-4 md:mb-6">
          <div className="glass rounded-xl p-4 md:p-5">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-emerald-100 to-teal-100 dark:from-emerald-900/50 dark:to-teal-900/50 flex items-center justify-center">
                <FaCog className="text-emerald-600 dark:text-emerald-400" />
              </div>
              <div>
                <h2 className="text-base font-bold text-gray-800 dark:text-gray-100">{t.tapp.appSettings}</h2>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  {manifest.settings && manifest.settings.length > 0
                    ? t.tapp.customizeBehavior
                    : t.tapp.noSettingsDesc}
                </p>
              </div>
            </div>

            {manifest.settings && manifest.settings.length > 0 ? (
              <div className="space-y-4">
                {manifest.settings.map((setting: TappSettingItem) => (
                  <div
                    key={setting.key}
                    className="p-3 bg-white/50 dark:bg-neutral-900/50 rounded-lg border border-gray-200/50 dark:border-neutral-700/50"
                  >
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex-1 min-w-0">
                        <label className="font-medium text-gray-800 dark:text-gray-100 text-sm">
                          {setting.label}
                        </label>
                        {setting.description && (
                          <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {setting.description}
                          </p>
                        )}
                      </div>
                      <div className="flex-shrink-0">
                        {/* Toggle 寮€鍏? */}
                        {setting.type === 'toggle' && (
                          <label className="relative inline-flex cursor-pointer">
                            <input
                              type="checkbox"
                              checked={settingsValues[setting.key] === true}
                              onChange={e => saveSetting(setting.key, e.target.checked)}
                              disabled={settingsSaving === setting.key}
                              className="sr-only peer"
                              aria-label={setting.label}
                            />
                            <div className={`w-11 h-6 rounded-full transition-colors peer-focus:ring-2 peer-focus:ring-indigo-300 ${
                              settingsValues[setting.key] === true
                                ? 'bg-indigo-600'
                                : 'bg-gray-300 dark:bg-neutral-600'
                            } ${settingsSaving === setting.key ? 'opacity-50' : ''}`}
                            >
                              <span
                                className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform ${
                                  settingsValues[setting.key] === true ? 'translate-x-5' : ''
                                }`}
                              />
                            </div>
                          </label>
                        )}

                        {/* Select 涓嬫媺 */}
                        {setting.type === 'select' && (
                          <select
                            value={String(settingsValues[setting.key] ?? '')}
                            onChange={e => saveSetting(setting.key, e.target.value)}
                            disabled={settingsSaving === setting.key}
                            aria-label={setting.label}
                            className="px-3 py-1.5 text-sm bg-white dark:bg-neutral-800 border border-gray-300 dark:border-neutral-600 rounded-lg text-gray-800 dark:text-gray-100"
                          >
                            {setting.options?.map(opt => (
                              <option key={opt.value} value={opt.value}>{opt.label}</option>
                            ))}
                          </select>
                        )}

                        {/* Input 杈撳叆妗? */}
                        {setting.type === 'input' && (
                          <input
                            type="text"
                            value={localInputValues[setting.key] ?? String(settingsValues[setting.key] ?? '')}
                            onChange={e => handleInputChange(setting.key, e.target.value)}
                            onBlur={() => handleInputBlur(setting.key, 'input')}
                            placeholder={setting.placeholder}
                            disabled={settingsSaving === setting.key}
                            className="w-40 px-3 py-1.5 text-sm bg-white dark:bg-neutral-800 border border-gray-300 dark:border-neutral-600 rounded-lg text-gray-800 dark:text-gray-100"
                          />
                        )}

                        {/* Number 鏁板瓧杈撳叆 */}
                        {setting.type === 'number' && (
                          <input
                            type="number"
                            value={localInputValues[setting.key] ?? Number(settingsValues[setting.key] ?? setting.min ?? 0)}
                            onChange={e => handleNumberChange(setting.key, Number(e.target.value))}
                            onBlur={() => handleInputBlur(setting.key, 'number')}
                            min={setting.min}
                            max={setting.max}
                            step={setting.step}
                            disabled={settingsSaving === setting.key}
                            aria-label={setting.label}
                            className="w-24 px-3 py-1.5 text-sm bg-white dark:bg-neutral-800 border border-gray-300 dark:border-neutral-600 rounded-lg text-gray-800 dark:text-gray-100"
                          />
                        )}

                        {/* Color 棰滆壊閫夋嫨 */}
                        {setting.type === 'color' && (
                          <input
                            type="color"
                            value={String(settingsValues[setting.key] ?? '#6366f1')}
                            onChange={e => saveSetting(setting.key, e.target.value)}
                            disabled={settingsSaving === setting.key}
                            aria-label={setting.label}
                            title={setting.label}
                            className="w-10 h-8 rounded cursor-pointer border-0"
                          />
                        )}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
                {t.tapp.noSettingsAvailable}
              </p>
            )}
          </div>
        </div>

        {/* 鏉冮檺鍒楄〃 */}
        <div className="mb-4 md:mb-6">
          <div className="glass rounded-xl p-4 md:p-5">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-amber-100 to-orange-100 dark:from-amber-900/50 dark:to-orange-900/50 flex items-center justify-center">
                <FaLock className="text-amber-600 dark:text-amber-400" />
              </div>
              <div>
                <h2 className="text-base font-bold text-gray-800 dark:text-gray-100">{t.tapp.permissions}</h2>
                <p className="text-xs text-gray-500 dark:text-gray-400">{format(t.tapp.grantedPermissions, { count: tapp.grantedPermissions.length })}</p>
              </div>
            </div>

            <div className="space-y-2">
              {tapp.grantedPermissions.map((permission) => {
                const config = PERMISSION_CONFIG[permission]
                if (!config)
                  return null

                const Icon = config.icon
                const levelColors = {
                  basic: 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400',
                  elevated: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400',
                  privileged: 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400',
                }

                const levelLabels = {
                  basic: t.tapp.basicPermission,
                  elevated: t.tapp.elevatedPermission,
                  privileged: t.tapp.privilegedPermission,
                }

                return (
                  <div
                    key={permission}
                    className="flex items-center gap-3 p-3 bg-white/50 dark:bg-neutral-900/50 rounded-lg border border-gray-200/50 dark:border-neutral-700/50"
                  >
                    <div className="w-8 h-8 rounded-lg bg-gray-100 dark:bg-neutral-800 flex items-center justify-center flex-shrink-0">
                      <Icon className="w-4 h-4 text-gray-600 dark:text-gray-400" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-gray-800 dark:text-gray-100 text-sm">
                          {t.tapp[config.labelKey as keyof typeof t.tapp]}
                        </span>
                        <span className={`px-1.5 py-0.5 text-xs rounded ${levelColors[config.level]}`}>
                          {levelLabels[config.level]}
                        </span>
                      </div>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        {t.tapp[config.descriptionKey as keyof typeof t.tapp]}
                      </p>
                    </div>
                    <FaCheck className="w-4 h-4 text-green-500 flex-shrink-0" />
                  </div>
                )
              })}
            </div>
          </div>
        </div>

        {/* 搴旂敤淇℃伅 */}
        <div className="mb-4 md:mb-6">
          <div className="glass rounded-xl p-4 md:p-5">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-gray-100 to-slate-100 dark:from-gray-800 dark:to-slate-800 flex items-center justify-center">
                <FaInfoCircle className="text-gray-600 dark:text-gray-400" />
              </div>
              <div>
                <h2 className="text-base font-bold text-gray-800 dark:text-gray-100">{t.tapp.appInfo}</h2>
                <p className="text-xs text-gray-500 dark:text-gray-400">{t.tapp.detailInfo}</p>
              </div>
            </div>

            <div className="space-y-3">
              <div className="flex items-center justify-between py-2 border-b border-gray-200/50 dark:border-neutral-700/50">
                <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.appId}</span>
                <span className="text-sm font-mono text-gray-800 dark:text-gray-100">{manifest.id}</span>
              </div>
              <div className="flex items-center justify-between py-2 border-b border-gray-200/50 dark:border-neutral-700/50">
                <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.version}</span>
                <span className="text-sm text-gray-800 dark:text-gray-100">
                  v
                  {manifest.version}
                </span>
              </div>
              {manifest.author && (
                <div className="flex items-center justify-between py-2 border-b border-gray-200/50 dark:border-neutral-700/50">
                  <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.author}</span>
                  <span className="text-sm text-gray-800 dark:text-gray-100">{manifest.author.name}</span>
                </div>
              )}
              <div className="flex items-center justify-between py-2 border-b border-gray-200/50 dark:border-neutral-700/50">
                <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.installedAt}</span>
                <span className="text-sm text-gray-800 dark:text-gray-100">
                  {new Date(tapp.installedAt).toLocaleDateString()}
                </span>
              </div>
              {tapp.lastRunAt && (
                <div className="flex items-center justify-between py-2">
                  <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.lastRunAt}</span>
                  <span className="text-sm text-gray-800 dark:text-gray-100">
                    {new Date(tapp.lastRunAt).toLocaleString()}
                  </span>
                </div>
              )}
              {manifest.homepage && (
                <div className="flex items-center justify-between py-2">
                  <span className="text-sm text-gray-500 dark:text-gray-400">{t.tapp.homepage}</span>
                  <a
                    href={manifest.homepage}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-sm text-indigo-600 dark:text-indigo-400 hover:underline"
                  >
                    {t.tapp.visit}
                  </a>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Toast 提示 */}
      {toastMessage && <Toast message={toastMessage} onClose={() => setToastMessage('')} />}

      {/* 卸载确认对话框 */}
      <UninstallConfirmDialog
        isOpen={showUninstallDialog}
        appName={tapp?.manifest.name || tappId}
        onCancel={() => setShowUninstallDialog(false)}
        onConfirm={handleConfirmUninstall}
      />
    </AnimatedView>
  )
}

export default TappDetailPage
