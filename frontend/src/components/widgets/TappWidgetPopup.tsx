import type { RegisteredWidget, TappCodeStructure, TappInstance } from '../../tapp/types'
import type { WidgetPopupProps } from '../widgetGridTypes'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n, withI18nNamespace } from '../../contexts/I18nContext'
import { dropWidgetPopups } from '../../lib/widgetPopup'
import { loadPopupResources } from '../../tapp/runtime/sandbox/resourceLoader'
import { getTappRuntime } from '../../tapp/runtime/TappRuntime'
import { TappWidgetSandbox } from '../../tapp/runtime/TappWidgetSandbox'
import { userFacingError } from '../../utils/userFacingError'
import { TAPP_WIDGET_SKELETON, WidgetSkeletonCover } from './shared/WidgetSkeleton'
import { acceptTappInstanceSettingsPatch } from './tappInstanceSettings'

interface TappWidgetPopupProps extends WidgetPopupProps {
  tappWidgetId: string
}

/**
 * TAPP 小组件的弹窗层：单独一个沙箱，不复用卡片的 iframe（卡片被裁在格子里，
 * invalidate 会重挂它）。权限与卡片同源同授予，不另加一层。
 */
function TappWidgetPopupComponent({
  tappWidgetId,
  config,
  params,
  active,
  close,
  onConfigChange,
}: TappWidgetPopupProps) {
  const { t, locale } = useI18n()
  const runtime = getTappRuntime()
  const [widget, setWidget] = useState<RegisteredWidget | null>(null)
  const [tappInstance, setTappInstance] = useState<TappInstance | null>(null)
  const [code, setCode] = useState<TappCodeStructure | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [generation, setGeneration] = useState(0)

  useEffect(() => {
    let cancelled = false
    setCode(null)
    setError(null)
    const load = async () => {
      try {
        await runtime.waitForSync()
        const found = runtime.getRegisteredWidgets().find((w) => w.id === tappWidgetId)
        const tapp = found ? runtime.getTapp(found.tappId) : undefined
        if (!found || !tapp || !runtime.isRunning(found.tappId)) {
          if (!cancelled) setError(t.tapp.widgetNotFound)
          return
        }
        const resources = await loadPopupResources(tapp, found.config.id)
        if (cancelled) return
        setWidget(found)
        setTappInstance(tapp)
        setCode({
          modules: resources.modules,
          moduleResolutions: resources.moduleResolutions,
          coreEntry: resources.coreEntry,
          popupEntries: resources.popupEntries,
          styles: resources.styles,
          widgetCSS: resources.css,
          i18n: resources.i18n,
        })
      } catch (err) {
        if (!cancelled) setError(userFacingError(err, t.tapp.appCodeLoadFailed))
      }
    }
    void load()
    return () => {
      cancelled = true
    }
  }, [runtime, tappWidgetId, generation, t.tapp.widgetNotFound, t.tapp.appCodeLoadFailed])

  // 停用、卸载：授权前提没了，连暂存一起销毁。更新：代码变了，重载这一层。
  useEffect(() => {
    const tappId = widget?.tappId
    if (!tappId) return
    const drop = (data: unknown) => {
      if ((data as { id?: string })?.id === tappId) {
        dropWidgetPopups((entry) => entry.key === config.id)
      }
    }
    const reload = (data: unknown) => {
      if ((data as { id?: string })?.id === tappId) setGeneration((n) => n + 1)
    }
    const offStop = runtime.on('tapp:stopped', drop)
    const offUninstall = runtime.on('tapp:uninstalled', drop)
    const offUpdate = runtime.on('tapp:updated', reload)
    return () => {
      offStop()
      offUninstall()
      offUpdate()
    }
  }, [runtime, widget?.tappId, config.id])

  const configString = JSON.stringify(config.config || {})
  const widgetProps = useMemo(() => {
    const declaredDefaults = Object.fromEntries(
      (widget?.config.settings || [])
        .filter((setting) => setting.defaultValue !== undefined)
        .map((setting) => [setting.key, setting.defaultValue]),
    )
    const isDark = document.documentElement.classList.contains('dark')
    const primaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#8b5cf6'
    return {
      size: config.size,
      config: { ...declaredDefaults, ...(config.config || {}) },
      isEditMode: false,
      isPreview: false,
      theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
      primaryColor,
      locale,
    }
    // configString 代替 config.config 的引用比较。
  }, [config.size, configString, widget, locale])

  const handleInstanceSettingsChange = useCallback(
    (patch: Record<string, unknown>): boolean => {
      if (!widget || !onConfigChange) return false
      const accepted = acceptTappInstanceSettingsPatch(widget.config.settings || [], patch)
      if (!accepted) return false
      onConfigChange({ ...(config.config || {}), ...accepted })
      return true
    },
    [config.config, onConfigChange, widget],
  )

  const accent = tappInstance?.manifest.themeColor?.trim() || 'var(--color-primary, #6366f1)'

  if (error) {
    return <div className="widget-popup-frame__empty">{error}</div>
  }
  if (!widget || !tappInstance || !code) {
    return (
      <div className="relative flex-1">
        <WidgetSkeletonCover
          active
          preset={TAPP_WIDGET_SKELETON.preset}
          deferMs={TAPP_WIDGET_SKELETON.deferMs}
          accent={accent}
          label={t.common.loading}
        />
      </div>
    )
  }

  return (
    <div className="relative flex-1 min-h-0">
      <TappWidgetSandbox
        key={generation}
        surface="popup"
        paused={!active}
        tappInstance={tappInstance}
        code={code}
        widgetId={widget.config.id}
        widgetProps={widgetProps}
        popupParams={params}
        onClosePopup={close}
        onInstanceSettingsChange={handleInstanceSettingsChange}
        className="w-full h-full"
      />
    </div>
  )
}

export default withI18nNamespace(['tapp'], TappWidgetPopupComponent)
