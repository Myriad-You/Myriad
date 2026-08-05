/** Store app detail view + permission groups. */

import type { TappPermission } from '../../types'
import type { StorePermissionLevel, UnifiedAppItem } from './types'
import { FaGithub, FaHome, FaLock, FaTrash } from '@lib/icons'
import { useEffect, useMemo, useState } from 'react'
import {
  InfoActionCard,
  SettingGroup,
} from '../../../components/settings'
import { Spinner } from '../../../components/Spinner'
import { useI18n } from '../../../contexts/I18nContext'
import { sanitizeUrl } from '../../../utils/inputSanitizer'
import { PERMISSION_CONFIG } from '../../constants/permissions'
import { RemoteStoreService } from '../../services/RemoteStoreService'
import { buildSanitizedTappPreview } from '../../utils/sanitizeTappPreview'
import { TAPP_CATEGORY_I18N_KEYS } from '../../utils/tappCategories'
import {
  compareVersions,
  formatSize,
  packageProgressLabel,
} from '../../utils/tappStoreHelpers'
import { formatDownloadCount } from '../../utils/formatDownloadCount'
import { getPreviewCanvas } from '../../utils/tappStorePreview'
import { TappIconBadge } from '../TappIconBadge'
import {
  collectAppLanguageTags,
  formatAppLanguages,
  getAppIconStyle,
  getPermissionLevel,
  isOfficialStoreApp,
  OfficialVerifiedDot,
} from './storeAppMeta'
import {
  ProgressPercent,
  RotatingDetailSubtitle,
  StoreGetButton,
} from './StoreChrome'
import {
  StaticTappPreview,
  TappPreviewFallbackFrame,
} from './StorePreviews'
import {
  LEVEL_LABEL_KEYS,
  PERMISSION_LEVEL_ORDER,

} from './types'
import '../../../components/ConfigForm.css'

export function DetailHeaderActions({ app }: { app: UnifiedAppItem }) {
  const { t } = useI18n()
  const homepageUrl = app.homepage ? sanitizeUrl(app.homepage) : ''
  const repositoryUrl = app.repository ? sanitizeUrl(app.repository) : ''

  if (!homepageUrl && !repositoryUrl) return null

  return (
    <div className="as-detail__header-actions">
      {homepageUrl && (
        <a
          href={homepageUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="as-detail__link as-detail__fluid-control glass glass-liquid"
          aria-label={t.tapp.homepage}
          title={t.tapp.homepage}
        >
          <FaHome />
          <span>{t.tapp.homepage}</span>
        </a>
      )}
      {repositoryUrl && (
        <a
          href={repositoryUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="as-detail__link as-detail__fluid-control glass glass-liquid"
          aria-label={t.tapp.repository}
          title={t.tapp.repository}
        >
          <FaGithub />
          <span>{t.tapp.repository}</span>
        </a>
      )}
    </div>
  )
}

/** 商店应用详情视图（模态框内的二级页面） */
export function AppDetailView({
  app,
  isInstalled,
  installedVersion,
  canUninstall,
  installing,
  installPercent,
  installPhase,
  installDetail,
  updating,
  onInstall,
  onUpdate,
  onLaunch,
  onUninstall,
}: {
  app: UnifiedAppItem
  isInstalled: boolean
  installedVersion?: string
  canUninstall: boolean
  installing: boolean
  installPercent?: number | null
  installPhase?: string | null
  installDetail?: string | null
  updating: boolean
  onInstall: () => void
  onUpdate: () => void
  onLaunch: () => void
  onUninstall: (anchor: HTMLElement) => void
}) {
  const { t, locale } = useI18n()
  const [previewDocument, setPreviewDocument] = useState<string | null>(null)
  const [previewFallback, setPreviewFallback] = useState(false)
  const [previewLoading, setPreviewLoading] = useState(false)
  const tappStrings = t.tapp as unknown as Record<string, string>
  const iconStyle = getAppIconStyle(app)
  const hasUpdate =
    isInstalled &&
    !!installedVersion &&
    compareVersions(app.version, installedVersion) > 0
  const busyProgress = installPercent != null && (installing || updating)
  const progressMode: 'install' | 'update' = updating ? 'update' : 'install'

  const description = app.longDescription || app.description
  const categoryName =
    tappStrings[TAPP_CATEGORY_I18N_KEYS[app.category]] ?? app.category
  const authorName = app.author?.name?.trim() || ''
  const subtitleLines = useMemo(() => {
    const lines: string[] = []
    if (categoryName) lines.push(categoryName)
    if (authorName) lines.push(authorName)
    return lines
  }, [categoryName, authorName])
  const languagesValue = useMemo(
    () => formatAppLanguages(collectAppLanguageTags(app), locale),
    [app, locale],
  )

  const permissionsByLevel = PERMISSION_LEVEL_ORDER.map((level) => ({
    level,
    permissions: app.permissions.filter(
      (permission) => getPermissionLevel(permission) === level,
    ),
  })).filter(({ permissions }) => permissions.length > 0)

  useEffect(() => {
    let cancelled = false

    const loadPreview = async () => {
      setPreviewDocument(null)
      setPreviewFallback(false)
      setPreviewLoading(true)

      const remote = app.remoteApp
      const localPageHtml = app.localTapp?.code.pageHtml
      // Catalog snapshot / page_template, or built-in example pageHtml.
      // Do not re-render installed package resources (empty white shells).
      const previewDeclared = Boolean(
        remote?.preview?.html ||
          remote?.download.page_template ||
          localPageHtml,
      )
      if (!previewDeclared) {
        if (!cancelled) setPreviewLoading(false)
        return
      }

      try {
        let html: string | undefined
        let css: string | undefined
        let preserveControls = false
        let theme = getPreviewCanvas(remote?.preview).theme

        if (remote?.preview?.html || remote?.download.page_template) {
          const preview = await RemoteStoreService.downloadAppPreview(
            remote,
            remote.sourceBaseUrl,
          )
          html = preview.html
          css = preview.css
          preserveControls = Boolean(remote.preview)
          theme = getPreviewCanvas(remote.preview).theme
        } else if (localPageHtml) {
          html = localPageHtml
          css = [app.localTapp?.code.styles, app.localTapp?.code.pageCSS]
            .filter(Boolean)
            .join('\n')
        }

        const document = html
          ? buildSanitizedTappPreview(html, css, {
              theme,
              preserveControls,
            })
          : null
        if (!cancelled) {
          setPreviewDocument(document)
          setPreviewFallback(!document)
        }
      } catch (error) {
        console.warn(
          `[TappStore] Static preview unavailable for ${app.id}`,
          error,
        )
        if (!cancelled) setPreviewFallback(true)
      } finally {
        if (!cancelled) setPreviewLoading(false)
      }
    }

    void loadPreview()
    return () => {
      cancelled = true
    }
  }, [app.id, app.localTapp, app.remoteApp])

  const factItems = [
    {
      label: t.tapp.languagesLabel,
      value: languagesValue,
    },
    {
      label: t.tapp.author,
      value: app.author.name,
    },
    {
      label: t.tapp.version,
      value: app.version,
    },
    ...(app.size
      ? [{ label: t.tapp.sizeLabel, value: formatSize(app.size) }]
      : []),
    ...(typeof app.downloads === 'number' && app.downloads > 0
      ? [
          {
            label: t.tapp.downloadsLabel,
            value: formatDownloadCount(app.downloads, locale),
          },
        ]
      : []),
    ...(app.updatedAt
      ? [
          {
            label: t.tapp.updatedAtLabel,
            value: new Date(app.updatedAt).toLocaleDateString(),
          },
        ]
      : []),
    {
      label: t.tapp.sourceLabel,
      value:
        app.source === 'remote'
          ? (app.remoteApp?.sourceName ?? t.tapp.remoteStore)
          : t.tapp.builtinExample,
    },
  ]

  const metaItems = [
    { label: t.tapp.version, value: app.version },
    { label: t.tapp.author, value: app.author.name },
    ...(app.size
      ? [{ label: t.tapp.sizeLabel, value: formatSize(app.size) }]
      : []),
    ...(typeof app.downloads === 'number' && app.downloads > 0
      ? [
          {
            label: t.tapp.downloadsLabel,
            value: formatDownloadCount(app.downloads, locale),
          },
        ]
      : []),
    ...(app.license
      ? [{ label: t.tapp.licenseLabel, value: app.license }]
      : []),
    ...(app.updatedAt
      ? [
          {
            label: t.tapp.updatedAtLabel,
            value: new Date(app.updatedAt).toLocaleDateString(),
          },
        ]
      : []),
    {
      label: t.tapp.sourceLabel,
      value:
        app.source === 'remote'
          ? (app.remoteApp?.sourceName ?? t.tapp.remoteStore)
          : t.tapp.builtinExample,
    },
  ]

  return (
    <div className="as-detail">
      <div className="as-detail__hero">
        <TappIconBadge
          icon={app.icon}
          iconSvg={app.iconSvg}
          name={app.name}
          themeColor={app.themeColor}
          category={app.category}
          id={app.id}
          permissions={app.permissions}
          iconStyle={iconStyle}
          shellClassName="as-detail__icon"
          glyphSizeClass="w-12 h-12 sm:w-14 sm:h-14"
          glyphTextClass="text-4xl sm:text-5xl"
        />

        <div className="as-detail__info">
          <h3 className="as-detail__name">
            {app.name}
            {isOfficialStoreApp(app) ? (
              <OfficialVerifiedDot label={t.tapp.official} />
            ) : null}
          </h3>
          <RotatingDetailSubtitle lines={subtitleLines} />

          <div className="as-detail__cta">
            {busyProgress && (installing || updating) ? (
              <StoreGetButton
                kind="busy"
                disabled
                label={
                  <ProgressPercent
                    value={installPercent!}
                    className="text-sm"
                  />
                }
              />
            ) : hasUpdate ? (
              <StoreGetButton
                kind="update"
                label={updating ? t.tapp.updating : t.tapp.update}
                disabled={updating}
                onClick={() => onUpdate()}
              />
            ) : isInstalled ? (
              <StoreGetButton
                kind="open"
                label={t.tapp.start}
                title={t.tapp.start}
                onClick={() => onLaunch()}
              />
            ) : (
              <StoreGetButton
                kind="get"
                label={installing ? t.tapp.installing : t.tapp.install}
                disabled={installing}
                onClick={() => onInstall()}
              />
            )}
            {isInstalled && canUninstall && (
              <button
                type="button"
                className="as-detail__link as-detail__link--danger"
                onClick={(e) => onUninstall(e.currentTarget)}
                aria-label={t.tapp.uninstall}
                title={t.tapp.uninstall}
              >
                <FaTrash className="h-3 w-3" />
                <span>{t.tapp.uninstall}</span>
              </button>
            )}
          </div>
        </div>
      </div>

      {busyProgress && (
        <div className="as-detail__progress" role="status" aria-live="polite">
          <div className="as-detail__progress-top">
            <span>
              {packageProgressLabel(
                t.tapp,
                progressMode,
                installPhase,
                installPercent!,
                installDetail,
              )}
            </span>
            <span>
              <ProgressPercent value={installPercent!} />
            </span>
          </div>
          <div className="as-detail__progress-bar">
            <i style={{ width: `${Math.max(2, installPercent!)}%` }} />
          </div>
        </div>
      )}

      <div className="as-detail__facts">
        {factItems.map((item, index) => (
          <div
            key={item.label}
            className="as-detail__fact"
            style={{ ['--as-enter-i' as string]: index }}
          >
            <span className="as-detail__fact-label">{item.label}</span>
            <strong className="as-detail__fact-value" title={item.value}>
              {item.value}
            </strong>
          </div>
        ))}
      </div>

      <div className="as-detail__content-grid">
        {(previewLoading || previewDocument || previewFallback) && (
          <section
            className="as-detail__block as-detail__block--preview"
            style={{ ['--as-block-i' as string]: 0 }}
          >
            <h4 className="sr-only">{t.tapp.storePreview}</h4>
            {previewDocument ? (
              <StaticTappPreview
                key={app.id}
                app={app}
                srcDoc={previewDocument}
              />
            ) : previewFallback ? (
              <TappPreviewFallbackFrame app={app} />
            ) : (
              <div className="as-detail__preview-loading" role="status">
                <Spinner size="sm" color="primary" />
              </div>
            )}
          </section>
        )}

        {description && (
          <section
            className="as-detail__block as-detail__block--about"
            style={{ ['--as-block-i' as string]: 1 }}
          >
            <h4 className="as-detail__h">{t.tapp.appDescription}</h4>
            <p className="as-detail__desc">{description}</p>
          </section>
        )}

        <section
          className="as-detail__block as-detail__block--permissions"
          style={{ ['--as-block-i' as string]: 2 }}
        >
          <h4 className="as-detail__h">{t.tapp.permissions}</h4>
          <SettingGroup toc={false}>
            {permissionsByLevel.length > 0 ? (
              <div className="as-detail__perm-levels">
                {permissionsByLevel.map(({ level, permissions }) => (
                  <PermissionLevelGroup
                    key={level}
                    level={level}
                    permissions={permissions}
                    tappStrings={tappStrings}
                  />
                ))}
              </div>
            ) : (
              <p className="settings-text-3" style={{ margin: 0 }}>
                {t.tapp.noPermissions}
              </p>
            )}
          </SettingGroup>
        </section>

        <section
          className="as-detail__block as-detail__block--info"
          style={{ ['--as-block-i' as string]: 3 }}
        >
          <h4 className="as-detail__h">{t.tapp.detailInfo}</h4>
          <SettingGroup toc={false}>
            <InfoActionCard
              copyable={false}
              fields={metaItems.map((item) => ({
                key: item.label,
                label: item.label,
                value: item.value,
              }))}
            />
          </SettingGroup>
        </section>
      </div>
    </div>
  )
}

export function PermissionLevelGroup({
  level,
  permissions,
  tappStrings,
}: {
  level: StorePermissionLevel
  permissions: string[]
  tappStrings: Record<string, string>
}) {
  return (
    <SettingGroup
      toc={false}
      title={tappStrings[LEVEL_LABEL_KEYS[level]]}
      className={`as-detail__perm-group as-detail__perm-group--${level}`}
    >
      <div className="as-detail__perm-cards checkbox-group-options">
        {permissions.map((permission, index) => {
          const config = PERMISSION_CONFIG[permission as TappPermission]
          const Icon = config?.icon ?? FaLock
          const label = config
            ? (tappStrings[config.labelKey] ?? permission)
            : permission
          const description = config
            ? tappStrings[config.descriptionKey]
            : undefined

          return (
            <div
              key={permission}
              className={`as-detail__perm-card as-detail__perm-card--${level} checkbox-group-card has-icon no-indicator`}
              style={{ ['--as-enter-i' as string]: index }}
              role="group"
              aria-label={`${label} · ${tappStrings[LEVEL_LABEL_KEYS[level]]}`}
            >
              <span className="checkbox-group-card-header">
                <span className="checkbox-group-card-icon" aria-hidden>
                  <Icon />
                </span>
                <span className="checkbox-group-card-text">
                  <span className="checkbox-group-card-label">{label}</span>
                  {description && (
                    <span className="checkbox-group-card-desc">
                      {description}
                    </span>
                  )}
                </span>
              </span>
            </div>
          )
        })}
      </div>
    </SettingGroup>
  )
}
