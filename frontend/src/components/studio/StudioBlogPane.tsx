import type { ReactNode } from 'react'
import type { BlogSchemeSwitch, BlogSettings, BlogSnapshot, BlogTheme } from '../../types/blog'
import type { BlogSettingsTab } from './studioRoutes'
import {
  LuActivity,
  LuCheck,
  LuCopy,
  LuLink,
  LuPalette,
  LuRefreshCw,
  LuTrash2,
  LuUpload,
  LuX,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import * as blogApi from '../../services/blogApi'
import {
  blogBuildPolls,
  blogHugoVersionLabel,
  blogThemeCanUninstall,
  blogThemeInstallUrl,
  blogThemeRemovalReturnsToDefault,
  parseBlogParams,
} from '../../services/blogState'
import { getNotesRssSettings, setNotesRssEnabled } from '../../services/phantasiApi'
import { useModuleVisibilityPreferences } from '../../utils/moduleVisibility'
import { showError } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { noteScheduleLabel } from '../phantasi/notes/noteBoard'
import { PageAction, WorkbenchPage } from '../phantasi/skin/PhantasiWorkbenchChrome'
import { SettingsButton } from '../settings'
import { BlogThemeScheme } from './BlogThemeScheme'
import { studioOverlayApi } from './studioIsland'
import { BLOG_SETTINGS_TABS, blogSettingsTabForArg } from './studioRoutes'
import './StudioBlogPane.css'

const POLL_MS = 2000

type BuildStatus = BlogSnapshot['build']['status']
type Busy = 'site' | 'params' | 'install' | 'rebuild' | `theme:${string}` | null

function paramsTextOf(settings: BlogSettings): string {
  return JSON.stringify(settings.params, null, 2)
}

/**
 * 博客设置窗口。像桌面应用的偏好设置：工具条上分四页（站点、主题、主题参数、构建），
 * 构建状态一直挂在工具条右边；有没保存的修改时，窗口底部出现保存条。
 */
export function StudioBlogPane({ back, arg = null }: { back: ReactNode; arg?: string | null }) {
  const { t, locale } = useI18n()
  const copy = t.studio
  const [tab, setTab] = useState<BlogSettingsTab>(() => blogSettingsTabForArg(arg))
  const [snapshot, setSnapshot] = useState<BlogSnapshot | null>(null)
  const [siteTitle, setSiteTitle] = useState('')
  const [description, setDescription] = useState('')
  const [paramsText, setParamsText] = useState('{}')
  const [busy, setBusy] = useState<Busy>(null)
  const [loadError, setLoadError] = useState('')
  const [loading, setLoading] = useState(true)

  const resetSite = (settings: BlogSettings) => {
    setSiteTitle(settings.site_title)
    setDescription(settings.description)
  }

  const pickTab = (next: BlogSettingsTab) => {
    setTab(next)
    // 下次打开这个窗口还停在这一页。
    studioOverlayApi()?.remember({ tool: 'settings', arg: next === 'site' ? null : next })
  }

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    void blogApi.getBlogSettings(controller.signal).then((next) => {
      if (controller.signal.aborted) return
      setSnapshot(next)
      resetSite(next.settings)
      setParamsText(paramsTextOf(next.settings))
    }).catch((err: unknown) => {
      if (!controller.signal.aborted) setLoadError(userFacingError(err, copy.loadFailed))
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [copy.loadFailed])

  const status: BuildStatus = snapshot?.build.status ?? 'idle'
  useEffect(() => {
    if (!blogBuildPolls(status)) return
    const timer = window.setInterval(() => {
      void blogApi.getBlogSettings().then((next) => {
        setSnapshot(next)
      }).catch(() => {
        // 轮询失败就留着上一份状态。
      })
    }, POLL_MS)
    return () => window.clearInterval(timer)
  }, [status])

  const run = async (key: Busy, fallback: string, task: () => Promise<void>) => {
    setBusy(key)
    try {
      await task()
    } catch (err) {
      showError(userFacingError(err, fallback))
    } finally {
      setBusy(null)
    }
  }

  /** 每块各存各的：以已保存的设置为底，只换这一块的字段，别的块里没保存的输入不跟着提交。 */
  const saveSettings = async (patch: Partial<BlogSettings>): Promise<BlogSnapshot | null> => {
    if (!snapshot) return null
    const saved = await blogApi.putBlogSettings({ ...snapshot.settings, ...patch })
    setSnapshot(saved)
    return saved
  }

  const refresh = async () => {
    setSnapshot(await blogApi.getBlogSettings())
  }

  const params = parseBlogParams(paramsText)
  const settings = snapshot?.settings
  const siteDirty = settings != null && (
    siteTitle !== settings.site_title || description !== settings.description
  )
  const paramsDirty = settings != null && paramsText !== paramsTextOf(settings)
  const dirty: Record<BlogSettingsTab, boolean> = {
    site: siteDirty,
    themes: false,
    params: paramsDirty,
    build: false,
  }
  const tabLabel: Record<BlogSettingsTab, string> = {
    site: copy.site,
    themes: copy.themes,
    params: copy.params,
    build: copy.build,
  }

  const tabs = (
    <div className="blog-tabs" role="tablist" aria-label={copy.blogSettings}>
      {BLOG_SETTINGS_TABS.map((key) => (
        <button
          key={key}
          type="button"
          role="tab"
          id={`blog-tab-${key}`}
          aria-selected={tab === key}
          aria-controls={`blog-panel-${key}`}
          className="blog-tabs__tab"
          onClick={() => pickTab(key)}
        >
          {tabLabel[key]}
          {dirty[key] ? <span className="blog-tabs__dot" aria-label={copy.settingsUnsaved} /> : null}
        </button>
      ))}
    </div>
  )

  if (!snapshot || !settings) {
    return (
      <WorkbenchPage title={copy.blogSettings} back={back} lead={tabs}>
        <div className="blog-pane">
          {loading ? <p className="blog-note">{copy.loading}</p> : null}
          {loadError ? <p className="blog-alert is-bad" role="alert">{loadError}</p> : null}
        </div>
      </WorkbenchPage>
    )
  }

  const { build, hugo } = snapshot
  const building = blogBuildPolls(build.status)
  const builtAt = build.last_built_at
    ? noteScheduleLabel(Date.parse(build.last_built_at), locale)
    : ''
  const currentTheme = snapshot.themes.find((theme) => theme.id === settings.theme_id)
  const rebuild = () => {
    void run('rebuild', copy.rebuildFailed, async () => {
      const next = await blogApi.rebuildBlog()
      setSnapshot((current) => current ? { ...current, build: next } : current)
    })
  }

  return (
    <WorkbenchPage
      title={copy.blogSettings}
      back={back}
      lead={tabs}
      action={
        <>
          {tab === 'build' ? (
            // 已经在构建页：状态只是状态，不再是跳到这一页的按钮。
            <span
              className={`blog-status is-${statusTone(build.status)} is-here`}
              title={builtAt ? `${copy.lastBuilt} ${builtAt}` : copy.neverBuilt}
            >
              <span className="blog-status__dot" aria-hidden />
              {copy[statusKey(build.status)]}
            </span>
          ) : (
            <button
              type="button"
              className={`blog-status is-${statusTone(build.status)}`}
              title={builtAt ? `${copy.lastBuilt} ${builtAt}` : copy.neverBuilt}
              onClick={() => pickTab('build')}
            >
              <span className="blog-status__dot" aria-hidden />
              {copy[statusKey(build.status)]}
            </button>
          )}
          <PageAction
            label={copy.rebuild}
            icon={<LuRefreshCw />}
            loading={busy === 'rebuild' || building}
            disabled={busy != null || !hugo.available}
            onPick={rebuild}
          />
        </>
      }
    >
      <div
        className="blog-pane"
        role="tabpanel"
        id={`blog-panel-${tab}`}
        aria-labelledby={`blog-tab-${tab}`}
      >
        {tab === 'site' ? (
          <>
            <div className="blog-form">
              <label className="blog-form__label" htmlFor="blog-site-title">{copy.siteTitle}</label>
              <input
                id="blog-site-title"
                className="blog-input"
                value={siteTitle}
                autoComplete="off"
                onChange={(event) => setSiteTitle(event.target.value)}
              />
              <label className="blog-form__label" htmlFor="blog-site-description">{copy.siteDescription}</label>
              <div className="blog-form__control">
                <textarea
                  id="blog-site-description"
                  className="blog-input"
                  rows={3}
                  value={description}
                  autoComplete="off"
                  onChange={(event) => setDescription(event.target.value)}
                />
                <p className="blog-note">{copy.languageNote}</p>
              </div>
              <div className="blog-form__rule" />
              <NotesRssRow />
            </div>
            {siteDirty ? (
              <SaveBar
                busy={busy === 'site'}
                disabled={busy != null}
                onDiscard={() => resetSite(settings)}
                onSave={() => {
                  void run('site', copy.saveFailed, async () => {
                    const saved = await saveSettings({ site_title: siteTitle, description })
                    if (saved) resetSite(saved.settings)
                  })
                }}
              />
            ) : null}
          </>
        ) : null}

        {tab === 'themes' ? (
          <ThemesPanel
            snapshot={snapshot}
            busy={busy}
            onUse={(theme) => {
              void run(`theme:${theme.id}`, copy.saveFailed, async () => {
                await saveSettings({ theme_id: theme.id })
              })
            }}
            onRemove={(theme) => {
              void run(`theme:${theme.id}`, copy.themeRemoveFailed, async () => {
                await blogApi.deleteBlogTheme(theme.id)
                await refresh()
              })
            }}
            onScheme={async (theme, value) => {
              let saved = false
              await run(`theme:${theme.id}`, copy.themeSchemeSaveFailed, async () => {
                await blogApi.setBlogThemeScheme(theme.id, value)
                await refresh()
                saved = true
              })
              return saved
            }}
            onInstallFile={(file) => run('install', copy.themeInstallFailed, async () => {
              await blogApi.uploadBlogTheme(file)
              await refresh()
            })}
            onInstallUrl={(url) => run('install', copy.themeInstallFailed, async () => {
              await blogApi.installBlogThemeFromUrl(url)
              await refresh()
            })}
          />
        ) : null}

        {tab === 'params' ? (
          <div className="blog-params">
            <p className="blog-note">{copy.paramsHint}</p>
            <textarea
              className={`blog-input blog-code${params == null ? ' is-invalid' : ''}`}
              aria-label={copy.paramsJson}
              aria-invalid={params == null || undefined}
              spellCheck={false}
              value={paramsText}
              onChange={(event) => setParamsText(event.target.value)}
            />
            {params == null ? <p className="blog-alert is-bad" role="alert">{copy.paramsInvalid}</p> : null}
            {paramsDirty ? (
              <SaveBar
                busy={busy === 'params'}
                disabled={busy != null || params == null}
                onDiscard={() => setParamsText(paramsTextOf(settings))}
                onSave={() => {
                  if (!params) return
                  void run('params', copy.saveFailed, async () => {
                    const saved = await saveSettings({ params })
                    if (saved) setParamsText(paramsTextOf(saved.settings))
                  })
                }}
              />
            ) : null}
          </div>
        ) : null}

        {tab === 'build' ? (
          <div className="blog-build">
            <div className={`blog-build__state is-${statusTone(build.status)}`} role="status">
              <span className="blog-build__mark" aria-hidden>{statusIcon(build.status)}</span>
              <span className="blog-build__text">
                <strong>{copy[statusKey(build.status)]}</strong>
                <span>{builtAt ? `${copy.lastBuilt} ${builtAt}` : copy.neverBuilt}</span>
              </span>
            </div>
            <dl className="blog-facts">
              <dt>{copy.hugo}</dt>
              <dd title={hugo.version}>
                {hugo.available
                  ? (hugo.version ? blogHugoVersionLabel(hugo.version) : copy.hugoAvailable)
                  : copy.hugoMissing}
              </dd>
              <dt>{copy.themes}</dt>
              <dd>{currentTheme?.name ?? settings.theme_id}</dd>
            </dl>
            {!hugo.available ? <p className="blog-alert is-warn" role="alert">{copy.hugoUnavailable}</p> : null}
            {build.last_error ? (
              <div className="blog-alert is-bad" role="alert">
                <strong>{copy.lastError}</strong>
                <span className="blog-alert__detail">{build.last_error}</span>
              </div>
            ) : null}
            {build.log_tail ? (
              <section className="blog-log" aria-label={copy.buildLog}>
                <div className="blog-section-title">{copy.buildLog}</div>
                <pre>{build.log_tail}</pre>
              </section>
            ) : null}
          </div>
        ) : null}
      </div>
    </WorkbenchPage>
  )
}

/** 窗口底部的保存条：只在这一页有改动时出现。 */
function SaveBar({
  busy,
  disabled,
  onDiscard,
  onSave,
}: {
  busy: boolean
  disabled: boolean
  onDiscard: () => void
  onSave: () => void
}) {
  const { t } = useI18n()
  const copy = t.studio
  return (
    <div className="blog-savebar" role="region" aria-label={copy.settingsUnsaved}>
      <span className="blog-savebar__text">{copy.settingsUnsaved}</span>
      <SettingsButton size="sm" variant="ghost" disabled={busy} onClick={onDiscard}>
        {copy.discard}
      </SettingsButton>
      <SettingsButton
        size="sm"
        variant="primary"
        icon={<LuCheck />}
        loading={busy}
        disabled={disabled}
        onClick={onSave}
      >
        {copy.save}
      </SettingsButton>
    </div>
  )
}

/** 主题：装好的一行一个，下面一行安装。压缩包可以拖到整页任何地方。 */
function ThemesPanel({
  snapshot,
  busy,
  onUse,
  onRemove,
  onScheme,
  onInstallFile,
  onInstallUrl,
}: {
  snapshot: BlogSnapshot
  busy: Busy
  onUse: (theme: BlogTheme) => void
  onRemove: (theme: BlogTheme) => void
  onScheme: (theme: BlogTheme, value: BlogSchemeSwitch) => Promise<boolean>
  onInstallFile: (file: File) => Promise<void>
  onInstallUrl: (url: string) => Promise<void>
}) {
  const { t } = useI18n()
  const copy = t.studio
  const currentId = snapshot.settings.theme_id
  const [url, setUrl] = useState('')
  const [urlError, setUrlError] = useState('')
  const [dragOver, setDragOver] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)
  const installing = busy === 'install'

  const takeFile = (file: File | undefined) => {
    if (file && busy == null) void onInstallFile(file)
  }

  const installFromUrl = async () => {
    const parsed = blogThemeInstallUrl(url)
    if (!parsed) {
      setUrlError(copy.themeUrlInvalid)
      return
    }
    setUrlError('')
    await onInstallUrl(parsed)
    setUrl('')
  }

  return (
    <div
      className={`blog-themes${dragOver ? ' is-drop' : ''}`}
      onDragOver={(event) => {
        event.preventDefault()
        if (busy == null) setDragOver(true)
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragOver(false)
      }}
      onDrop={(event) => {
        event.preventDefault()
        setDragOver(false)
        takeFile(event.dataTransfer.files[0])
      }}
    >
      <p className="blog-note">{copy.themesHint}</p>
      <ul className="blog-themes__list">
        {snapshot.themes.map((theme) => {
          const current = theme.id === currentId
          const rowBusy = busy === `theme:${theme.id}`
          return (
            <li
              key={theme.id}
              className={`blog-theme${current ? ' is-current' : ''}`}
              aria-current={current || undefined}
            >
              <span className="blog-theme__swatch" aria-hidden><LuPalette /></span>
              <span className="blog-theme__text">
                <span className="blog-theme__name">
                  {theme.name}
                  {theme.builtin ? <span className="blog-tag">{copy.themeBuiltin}</span> : null}
                </span>
                <span className="blog-theme__meta">
                  {[theme.id, theme.version].filter(Boolean).join(' · ')}
                </span>
              </span>
              <span className="blog-theme__actions">
                {blogThemeCanUninstall(theme) ? (
                  <SettingsButton
                    size="sm"
                    variant="ghost"
                    icon={<LuTrash2 />}
                    disabled={busy != null}
                    confirm={blogThemeRemovalReturnsToDefault(theme.id, currentId)
                      ? copy.themeRemoveCurrent
                      : copy.themeRemoveDetail}
                    onClick={() => onRemove(theme)}
                  >
                    {copy.themeRemove}
                  </SettingsButton>
                ) : null}
                {current ? (
                  <span className="blog-theme__inuse">
                    <LuCheck aria-hidden />
                    {copy.themeCurrent}
                  </span>
                ) : (
                  <SettingsButton
                    size="sm"
                    variant="secondary"
                    loading={rowBusy}
                    disabled={busy != null}
                    onClick={() => onUse(theme)}
                  >
                    {copy.themeUse}
                  </SettingsButton>
                )}
              </span>
              <BlogThemeScheme
                theme={theme}
                disabled={busy != null}
                onSave={(value) => onScheme(theme, value)}
              />
            </li>
          )
        })}
      </ul>

      <section className="blog-install" aria-label={copy.themeInstallTitle}>
        <div className="blog-section-title">{copy.themeInstallTitle}</div>
        <div className="blog-install__row">
          <input
            ref={fileRef}
            type="file"
            accept=".zip,application/zip"
            hidden
            disabled={installing}
            onChange={(event) => {
              const file = event.target.files?.[0]
              event.target.value = ''
              takeFile(file)
            }}
          />
          <SettingsButton
            size="sm"
            variant="secondary"
            icon={<LuUpload />}
            loading={installing}
            disabled={busy != null}
            onClick={() => fileRef.current?.click()}
          >
            {copy.themeUploadZip}
          </SettingsButton>
          <span className="blog-install__or" aria-hidden />
          <input
            className={`blog-input blog-install__url${urlError ? ' is-invalid' : ''}`}
            type="url"
            value={url}
            placeholder="https://example.com/theme.zip"
            aria-label={copy.themeUrl}
            aria-invalid={Boolean(urlError) || undefined}
            autoComplete="off"
            onChange={(event) => {
              setUrl(event.target.value)
              setUrlError('')
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && url.trim() && busy == null) void installFromUrl()
            }}
          />
          <SettingsButton
            size="sm"
            variant="secondary"
            icon={<LuLink />}
            disabled={busy != null || !url.trim()}
            onClick={() => { void installFromUrl() }}
          >
            {copy.themeInstall}
          </SettingsButton>
        </div>
        {urlError ? <p className="blog-alert is-bad" role="alert">{urlError}</p> : null}
        <p className="blog-note">{copy.themeUploadFormats}</p>
      </section>

      {dragOver ? (
        <div className="blog-themes__drop" aria-hidden>
          <LuUpload />
          <span>{copy.themeDrop}</span>
        </div>
      ) : null}
    </div>
  )
}

/** 后端要开关打开且博客对所有访客开放，`/api/phantasi/notes.xml` 才存在。 */
function NotesRssRow() {
  const { t } = useI18n()
  const copy = t.studio
  const { preferences } = useModuleVisibilityPreferences()
  const open = preferences.modules.phantasi === 'all'
  const [enabled, setEnabled] = useState<boolean | null>(null)
  const [saving, setSaving] = useState(false)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    let cancelled = false
    void getNotesRssSettings().then((settings) => {
      if (!cancelled) setEnabled(settings.enabled)
    }).catch((err: unknown) => {
      if (!cancelled) showError(userFacingError(err, copy.notesRssLoadFailed))
    })
    return () => {
      cancelled = true
    }
  }, [copy.notesRssLoadFailed])

  useEffect(() => {
    if (!copied) return
    const timer = window.setTimeout(setCopied, 1500, false)
    return () => window.clearTimeout(timer)
  }, [copied])

  const toggle = async (next: boolean) => {
    setSaving(true)
    try {
      setEnabled(await setNotesRssEnabled(next))
    } catch (err) {
      showError(userFacingError(err, copy.saveFailed))
    } finally {
      setSaving(false)
    }
  }

  const feedUrl = `${window.location.origin}/api/phantasi/notes.xml`
  // 两列表单里的一行：左边标签，右边开关和说明。
  return (
    <>
      <span className="blog-form__label" id="blog-notes-rss">{copy.notesRss}</span>
      <div className="blog-switch-row">
        <button
          type="button"
          role="switch"
          className="blog-switch"
          aria-checked={enabled ?? false}
          aria-labelledby="blog-notes-rss"
          aria-describedby="blog-notes-rss-hint"
          aria-busy={saving || undefined}
          disabled={enabled == null || saving}
          onClick={() => { void toggle(!enabled) }}
        >
          <span className="blog-switch__knob" aria-hidden />
        </button>
        <span className="blog-switch-row__text">
          <span className="blog-note" id="blog-notes-rss-hint">
            {enabled && !open ? copy.notesRssPrivate : copy.notesRssHint}
          </span>
        {enabled && open ? (
          <span className="blog-feed">
            <code title={feedUrl}>{feedUrl}</code>
            <button
              type="button"
              className="blog-feed__copy"
              aria-label={copied ? t.common.copied : t.common.copy}
              title={copied ? t.common.copied : t.common.copy}
              onClick={() => {
                void navigator.clipboard.writeText(feedUrl).then(() => setCopied(true))
              }}
            >
              {copied ? <LuCheck aria-hidden /> : <LuCopy aria-hidden />}
            </button>
          </span>
          ) : null}
        </span>
      </div>
    </>
  )
}

function statusTone(status: BuildStatus): 'ok' | 'busy' | 'bad' | 'idle' {
  switch (status) {
    case 'ok':
      return 'ok'
    case 'queued':
    case 'running':
      return 'busy'
    case 'failed':
    case 'unavailable':
      return 'bad'
    default:
      return 'idle'
  }
}

function statusIcon(status: BuildStatus): ReactNode {
  switch (statusTone(status)) {
    case 'ok':
      return <LuCheck />
    case 'busy':
      return <LuRefreshCw className="studio-blog__spin" />
    case 'bad':
      return <LuX />
    default:
      return <LuActivity />
  }
}

function statusKey(status: BuildStatus):
  | 'buildIdle'
  | 'buildQueued'
  | 'buildRunning'
  | 'buildOk'
  | 'buildFailed'
  | 'buildUnavailable' {
  switch (status) {
    case 'queued':
      return 'buildQueued'
    case 'running':
      return 'buildRunning'
    case 'ok':
      return 'buildOk'
    case 'failed':
      return 'buildFailed'
    case 'unavailable':
      return 'buildUnavailable'
    default:
      return 'buildIdle'
  }
}
