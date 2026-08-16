/** Featured + static store previews (sanitized iframes). */

import type { PreviewRenderState } from '../../utils/tappStorePreview'
import type { UnifiedAppItem } from './types'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { RemoteStoreService } from '../../services/RemoteStoreService'
import { buildSanitizedTappPreview } from '../../utils/sanitizeTappPreview'
import { normalizeTappCategory } from '../../utils/tappCategories'
import { CATEGORY_COLORS } from '../../utils/tappColors'
import {
  getPreviewCanvas,
  isRenderedPreviewAdapted,
  PREVIEW_LOAD_TIMEOUT_MS,

  previewTransform,
} from '../../utils/tappStorePreview'
import { TappIconBadge } from '../TappIconBadge'
import { getAppIconStyle } from './storeAppMeta'

/** Lazy, non-interactive 1280×720 preview used as an editorial-card backdrop. */
export function FeaturedTappPreview({ app }: { app: UnifiedAppItem }) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const [shouldLoad, setShouldLoad] = useState(false)
  const [srcDoc, setSrcDoc] = useState<string | null>(null)
  const [transform, setTransform] = useState({ scale: 1, x: 0, y: 0 })
  const [renderState, setRenderState] = useState<PreviewRenderState>('checking')
  const canvas = useMemo(
    () => getPreviewCanvas(app.preview ?? app.remoteApp?.preview),
    [app.preview, app.remoteApp?.preview],
  )

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    if (typeof IntersectionObserver === 'undefined') {
      setShouldLoad(true)
      return
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return
        setShouldLoad(true)
        observer.disconnect()
      },
      { rootMargin: '180px' },
    )
    observer.observe(host)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!shouldLoad) return
    let cancelled = false

    const loadPreview = async () => {
      // Catalog snapshot / page_template, or built-in example pageHtml.
      // No installed-package re-render.
      const remote = app.remoteApp
      const localPageHtml = app.localTapp?.code.pageHtml
      const snapshot = app.preview ?? remote?.preview
      if (
        !(snapshot?.html || remote?.download.page_template || localPageHtml)
      ) {
        if (!cancelled) setRenderState('fallback')
        return
      }

      try {
        let html: string | undefined
        let css: string | undefined
        let preserveControls = false

        if (remote && (snapshot?.html || remote.download.page_template)) {
          const preview = await RemoteStoreService.downloadAppPreview(
            remote,
            remote.sourceBaseUrl,
            snapshot,
          )
          html = preview.html
          css = preview.css
          preserveControls = Boolean(snapshot)
        } else if (localPageHtml) {
          html = localPageHtml
          css = [app.localTapp?.code.styles, app.localTapp?.code.pageCSS]
            .filter(Boolean)
            .join('\n')
        }

        const document = html
          ? buildSanitizedTappPreview(html, css, {
              theme: canvas.theme,
              preserveControls,
            })
          : null
        if (!cancelled) {
          setSrcDoc(document)
          if (!document) setRenderState('fallback')
        }
      } catch (error) {
        console.warn(
          `[TappStore] Featured preview unavailable for ${app.id}`,
          error,
        )
        if (!cancelled) setRenderState('fallback')
      }
    }

    void loadPreview()
    return () => {
      cancelled = true
    }
  }, [app.id, app.localTapp, app.preview, app.remoteApp, canvas.theme, shouldLoad])

  useEffect(() => {
    const host = hostRef.current
    if (!host || !srcDoc) return

    const measure = () => {
      setTransform(
        previewTransform(host.clientWidth, host.clientHeight, canvas),
      )
    }
    measure()

    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(host)
    return () => observer.disconnect()
  }, [canvas, srcDoc])

  // onLoad 可能不触发；超时后展示内容，避免永久 opacity:0
  useEffect(() => {
    if (!srcDoc || renderState !== 'checking') return
    const timer = window.setTimeout(() => {
      setRenderState((s) => (s === 'checking' ? 'ready' : s))
    }, PREVIEW_LOAD_TIMEOUT_MS)
    return () => window.clearTimeout(timer)
  }, [srcDoc, renderState])

  const handleLoad = useCallback(() => {
    // Featured backdrop: always show sanitized doc (timeout also promotes ready).
    // Strict adapted check is reserved for detail StaticTappPreview.
    setRenderState('ready')
  }, [])

  return (
    <span
      ref={hostRef}
      className="as-store__feature-preview"
      data-preview-state={renderState}
      aria-hidden
    >
      {srcDoc && (
        <iframe
          title=""
          srcDoc={srcDoc}
          sandbox="allow-same-origin"
          referrerPolicy="no-referrer"
          tabIndex={-1}
          width={canvas.width}
          height={canvas.height}
          onLoad={handleLoad}
          style={{
            opacity: renderState === 'ready' ? 1 : 0,
            width: canvas.width,
            height: canvas.height,
            transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
          }}
        />
      )}
    </span>
  )
}

export function TappPreviewPlaceholder({ app }: { app: UnifiedAppItem }) {
  const { t } = useI18n()
  const iconStyle = getAppIconStyle(app)
  const categoryColors = CATEGORY_COLORS[normalizeTappCategory(app.category)]
  const previewColor = app.themeColor || categoryColors.fromHex
  const previewColorAlt = app.themeColor
    ? `color-mix(in srgb, ${app.themeColor} 72%, black)`
    : categoryColors.toHex
  const previewStyle = {
    '--as-preview-color': previewColor,
    '--as-preview-color-alt': previewColorAlt,
  } as React.CSSProperties

  return (
    <div
      className="as-detail__preview-placeholder"
      role="img"
      aria-label={`${app.name}: ${t.tapp.storePreviewUnavailable}`}
      style={previewStyle}
    >
      <TappIconBadge
        icon={app.icon}
        iconSvg={app.iconSvg}
        name={app.name}
        themeColor={app.themeColor}
        category={app.category}
        id={app.id}
        permissions={app.permissions}
        iconStyle={iconStyle}
        shellClassName="as-detail__preview-placeholder-icon"
        glyphSizeClass="w-10 h-10"
        glyphTextClass="text-3xl"
      />
      <strong>{t.tapp.storePreviewUnavailable}</strong>
    </div>
  )
}

export function TappPreviewFallbackFrame({ app }: { app: UnifiedAppItem }) {
  return (
    <div className="as-detail__preview-frame" data-preview-state="fallback">
      <TappPreviewPlaceholder app={app} />
    </div>
  )
}

export function StaticTappPreview({
  app,
  srcDoc,
}: {
  app: UnifiedAppItem
  srcDoc: string
}) {
  const { t } = useI18n()
  const frameRef = useRef<HTMLDivElement | null>(null)
  const [transform, setTransform] = useState({ scale: 0, x: 0, y: 0 })
  const [renderState, setRenderState] = useState<PreviewRenderState>('checking')
  const canvas = useMemo(
    () => getPreviewCanvas(app.preview ?? app.remoteApp?.preview),
    [app.preview, app.remoteApp?.preview],
  )

  useEffect(() => {
    const frame = frameRef.current
    if (!frame) return

    const measure = () => {
      setTransform(
        previewTransform(frame.clientWidth, frame.clientHeight, canvas),
      )
    }
    measure()

    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(frame)
    return () => observer.disconnect()
  }, [canvas])

  useEffect(() => {
    setRenderState('checking')
  }, [srcDoc])

  useEffect(() => {
    if (renderState !== 'checking') return
    const timer = window.setTimeout(() => {
      setRenderState((s) => (s === 'checking' ? 'ready' : s))
    }, PREVIEW_LOAD_TIMEOUT_MS)
    return () => window.clearTimeout(timer)
  }, [srcDoc, renderState])

  const handleLoad = useCallback(
    (event: React.SyntheticEvent<HTMLIFrameElement>) => {
      // Extreme overflow → fallback; otherwise prefer showing sanitized content
      const ok = isRenderedPreviewAdapted(event.currentTarget, canvas)
      setRenderState(ok ? 'ready' : 'fallback')
    },
    [canvas],
  )

  return (
    <div
      ref={frameRef}
      className="as-detail__preview-frame"
      data-preview-state={renderState}
      style={{ aspectRatio: `${canvas.width} / ${canvas.height}` }}
    >
      {renderState === 'fallback' && <TappPreviewPlaceholder app={app} />}
      <iframe
        title={`${app.name} ${t.tapp.storePreview}`}
        srcDoc={srcDoc}
        sandbox="allow-same-origin"
        referrerPolicy="no-referrer"
        tabIndex={-1}
        aria-hidden={renderState !== 'ready'}
        width={canvas.width}
        height={canvas.height}
        onLoad={handleLoad}
        style={{
          width: canvas.width,
          height: canvas.height,
          opacity: transform.scale > 0 && renderState === 'ready' ? 1 : 0,
          transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale || 1})`,
        }}
      />
      <span className="as-detail__preview-scroll-shield" aria-hidden />
    </div>
  )
}
