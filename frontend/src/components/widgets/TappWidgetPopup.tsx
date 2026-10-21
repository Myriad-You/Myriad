import type { RegisteredWidget, TappCodeStructure, TappInstance } from '../../tapp/types'
import type { WidgetPopupProps } from '../widgetGridTypes'
import { useEffect, useState } from 'react'
import { useI18n, withI18nNamespace } from '../../contexts/I18nContext'
import { dropWidgetPopups } from '../../lib/widgetPopup'
import { hiddenWidgetPool } from '../../tapp/runtime/resourceBounds'
import { loadPopupResources } from '../../tapp/runtime/sandbox/resourceLoader'
import { getTappRuntime } from '../../tapp/runtime/TappRuntime'
import { TappWidgetSandbox } from '../../tapp/runtime/TappWidgetSandbox'
import { userFacingError } from '../../utils/userFacingError'
import { TAPP_WIDGET_SKELETON, WidgetSkeletonCover } from './shared/WidgetSkeleton'
import { useTappInstanceSettingsPatch, useTappWidgetProps } from './useTappWidgetProps'

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

  // 卡片所属应用。加载前就从注册表认出来，加载期间的停用、卸载、更新也接得住。
  const tappId = runtime.getRegisteredWidgets().find((w) => w.id === tappWidgetId)?.tappId ?? widget?.tappId

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
        // 拉代码期间被停用：前提已经没了，不挂。
        if (!runtime.isRunning(found.tappId)) {
          dropWidgetPopups((entry) => entry.widget.id === config.id)
          return
        }
        setWidget(found)
        setTappInstance(runtime.getTapp(found.tappId) ?? tapp)
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
  }, [runtime, tappWidgetId, generation, config.id, t.tapp.widgetNotFound, t.tapp.appCodeLoadFailed])

  // 停用、卸载：授权前提没了，连收回留着的一起销毁。更新（代码或权限变了）：重载这一层。
  useEffect(() => {
    if (!tappId) return
    const drop = (data: unknown) => {
      if ((data as { id?: string })?.id === tappId) {
        dropWidgetPopups((entry) => entry.widget.id === config.id)
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
  }, [runtime, tappId, config.id])

  // 收回的 TAPP 弹窗和隐藏的卡片沙箱共用一个上限：iframe 总数不因弹窗多出一截。
  const sandboxMounted = Boolean(code)
  useEffect(() => {
    if (!sandboxMounted) return undefined
    const key = `popup:${config.id}`
    if (active) hiddenWidgetPool.remove(key)
    else hiddenWidgetPool.add(key, () => dropWidgetPopups((entry) => entry.widget.id === config.id))
    return () => hiddenWidgetPool.remove(key)
  }, [active, sandboxMounted, config.id])

  const widgetProps = useTappWidgetProps(widget, config, locale)
  // 收回后不在前台，不替卡片改设置；卡片本身被删时弹窗已随之销毁。
  const handleInstanceSettingsChange = useTappInstanceSettingsPatch(
    widget,
    config.config,
    onConfigChange,
    active,
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
