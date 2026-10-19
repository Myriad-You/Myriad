import type { ReactNode } from 'react'
import type { BlogSettings, BlogSnapshot, BlogTheme } from '../../types/blog'
import {
  LuActivity,
  LuCheck,
  LuCode,
  LuExternalLink,
  LuGlobe,
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
import { PhantasiWorkbenchIcon } from '../phantasi/ui/PhantasiWorkbenchIcon'
import {
  InfoActionCard,
  InfoCard,
  InputItem,
  SettingGroup,
  SettingsButton,
  SettingTitleTag,
  SwitchItem,
} from '../settings'
import { openBlogPage } from './studioIsland'
import './StudioBlogPane.css'

const POLL_MS = 2000

type BuildStatus = BlogSnapshot['build']['status']
type Busy = 'site' | 'params' | 'install' | 'rebuild' | `theme:${string}` | null

function paramsTextOf(settings: BlogSettings): string {
  return JSON.stringify(settings.params, null, 2)
}

export function StudioBlogPane({ back }: { back: ReactNode }) {
  const { t, locale } = useI18n()
  const copy = t.studio
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

  if (!snapshot || !settings) {
    return (
      <WorkbenchPage
        title={copy.blogSettings}
        icon={<PhantasiWorkbenchIcon kind="studio" />}
        back={back}
      >
        {loading ? <p className="setting-description">{copy.loading}</p> : null}
        {loadError ? <InfoCard variant="error" content={loadError} /> : null}
      </WorkbenchPage>
    )
  }

  const { build, hugo } = snapshot
  const building = blogBuildPolls(build.status)
  const builtAt = build.last_built_at
    ? noteScheduleLabel(Date.parse(build.last_built_at), locale)
    : ''
  const currentTheme = snapshot.themes.find((theme) => theme.id === settings.theme_id)

  return (
    <WorkbenchPage
      title={copy.blogSettings}
      icon={<PhantasiWorkbenchIcon kind="studio" />}
      back={back}
      action={
        <PageAction
          label={copy.openBlog}
          icon={<LuExternalLink />}
          onPick={() => openBlogPage('/blog/')}
        />
      }
    >
      <div className="studio-blog">
        <SettingGroup title={copy.site} icon={<LuGlobe />} description={copy.languageNote}>
          <InputItem
            itemKey="blog-site-title"
            label={copy.siteTitle}
            value={siteTitle}
            onChange={setSiteTitle}
            autoComplete="off"
          />
          <InputItem
            itemKey="blog-site-description"
            label={copy.siteDescription}
            value={description}
            onChange={setDescription}
            multiline
            rows={3}
            autoComplete="off"
          />
          {siteDirty ? (
            <div className="studio-blog__actions">
              <SettingsButton
                size="sm"
                variant="ghost"
                disabled={busy === 'site'}
                onClick={() => resetSite(settings)}
              >
                {copy.discard}
              </SettingsButton>
              <SettingsButton
                size="sm"
                variant="primary"
                icon={<LuCheck />}
                loading={busy === 'site'}
                disabled={busy != null}
                onClick={() => {
                  void run('site', copy.saveFailed, async () => {
                    const saved = await saveSettings({ site_title: siteTitle, description })
                    if (saved) resetSite(saved.settings)
                  })
                }}
              >
                {copy.save}
              </SettingsButton>
            </div>
          ) : null}
          <NotesRssSwitch />
        </SettingGroup>

        <SettingGroup title={copy.build} icon={<LuActivity />}>
          <div className={`studio-blog__status is-${statusTone(build.status)}`} role="status">
            <span className="studio-blog__status-mark" aria-hidden>
              {statusIcon(build.status)}
            </span>
            <span className="studio-blog__status-text">
              <strong>{copy[statusKey(build.status)]}</strong>
              <span>{builtAt ? `${copy.lastBuilt} ${builtAt}` : copy.neverBuilt}</span>
            </span>
            <SettingsButton
              variant="primary"
              icon={<LuRefreshCw />}
              loading={busy === 'rebuild' || building}
              disabled={busy != null || !hugo.available}
              className="studio-blog__rebuild"
              onClick={() => {
                void run('rebuild', copy.rebuildFailed, async () => {
                  const next = await blogApi.rebuildBlog()
                  setSnapshot((current) => current ? { ...current, build: next } : current)
                })
              }}
            >
              {copy.rebuild}
            </SettingsButton>
          </div>
          <dl className="studio-blog__stats">
            <div>
              <dt>{copy.hugo}</dt>
              <dd title={hugo.version}>
                {hugo.available
                  ? (hugo.version ? blogHugoVersionLabel(hugo.version) : copy.hugoAvailable)
                  : copy.hugoMissing}
              </dd>
            </div>
            <div>
              <dt>{copy.themes}</dt>
              <dd>{currentTheme?.name ?? settings.theme_id}</dd>
            </div>
          </dl>
          {!hugo.available ? <InfoCard variant="warning" content={copy.hugoUnavailable} /> : null}
          {build.last_error ? (
            <InfoCard
              variant="error"
              title={copy.lastError}
              content={<span className="studio-blog__error-text">{build.last_error}</span>}
            />
          ) : null}
        </SettingGroup>

        <SettingGroup
          title={copy.themes}
          icon={<LuPalette />}
          description={copy.themesHint}
        >
          <ThemeList
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
            onInstallFile={(file) => run('install', copy.themeInstallFailed, async () => {
              await blogApi.uploadBlogTheme(file)
              await refresh()
            })}
            onInstallUrl={(url) => run('install', copy.themeInstallFailed, async () => {
              await blogApi.installBlogThemeFromUrl(url)
              await refresh()
            })}
          />
        </SettingGroup>

        <SettingGroup
          title={copy.params}
          icon={<LuCode />}
          description={copy.paramsHint}
          collapsible
          defaultExpanded={Object.keys(settings.params).length > 0}
        >
          <InputItem
            itemKey="blog-theme-params"
            label={copy.paramsJson}
            value={paramsText}
            onChange={setParamsText}
            multiline
            rows={8}
            autoComplete="off"
            error={params == null ? copy.paramsInvalid : undefined}
            className="studio-blog__code"
          />
          {paramsDirty ? (
            <div className="studio-blog__actions">
              <SettingsButton
                size="sm"
                variant="ghost"
                disabled={busy === 'params'}
                onClick={() => setParamsText(paramsTextOf(settings))}
              >
                {copy.discard}
              </SettingsButton>
              <SettingsButton
                size="sm"
                variant="primary"
                icon={<LuCheck />}
                loading={busy === 'params'}
                disabled={busy != null || params == null}
                onClick={() => {
                  if (!params) return
                  void run('params', copy.saveFailed, async () => {
                    const saved = await saveSettings({ params })
                    if (saved) setParamsText(paramsTextOf(saved.settings))
                  })
                }}
              >
                {copy.save}
              </SettingsButton>
            </div>
          ) : null}
        </SettingGroup>

        {build.log_tail ? (
          <SettingGroup
            title={copy.buildLog}
            collapsible
            defaultExpanded={build.status === 'failed'}
          >
            <pre className="studio-blog__log">{build.log_tail}</pre>
          </SettingGroup>
        ) : null}
      </div>
    </WorkbenchPage>
  )
}

function ThemeList({
  snapshot,
  busy,
  onUse,
  onRemove,
  onInstallFile,
  onInstallUrl,
}: {
  snapshot: BlogSnapshot
  busy: Busy
  onUse: (theme: BlogTheme) => void
  onRemove: (theme: BlogTheme) => void
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
    <div className="studio-blog__themes">
      {snapshot.themes.map((theme) => {
        const current = theme.id === currentId
        const rowBusy = busy === `theme:${theme.id}`
        return (
          <article
            key={theme.id}
            className={`studio-blog__theme${current ? ' is-current' : ''}`}
            aria-current={current || undefined}
          >
            <span className="studio-blog__theme-swatch" aria-hidden>
              <LuPalette />
            </span>
            <div className="studio-blog__theme-head">
              <strong>{theme.name}</strong>
              {theme.builtin ? (
                <SettingTitleTag variant="muted">{copy.themeBuiltin}</SettingTitleTag>
              ) : null}
            </div>
            <span className="studio-blog__theme-meta">
              {[theme.id, theme.version].filter(Boolean).join(' · ')}
            </span>
            <div className="studio-blog__theme-actions">
              {current ? (
                <span className="studio-blog__theme-inuse">
                  <LuCheck aria-hidden />
                  {copy.themeCurrent}
                </span>
              ) : (
                <SettingsButton
                  size="sm"
                  variant="primary"
                  loading={rowBusy}
                  disabled={busy != null}
                  onClick={() => onUse(theme)}
                >
                  {copy.themeUse}
                </SettingsButton>
              )}
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
            </div>
          </article>
        )
      })}

      <section className="studio-blog__theme studio-blog__theme--install" aria-label={copy.themeInstallTitle}>
        <div className="studio-blog__theme-head">
          <strong>{copy.themeInstallTitle}</strong>
        </div>
        <button
          type="button"
          className={`phantasi-workbench__drop studio-blog__drop${dragOver ? ' is-on' : ''}`}
          disabled={installing}
          onDragOver={(event) => {
            event.preventDefault()
            setDragOver(true)
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(event) => {
            event.preventDefault()
            setDragOver(false)
            takeFile(event.dataTransfer.files[0])
          }}
          onClick={() => fileRef.current?.click()}
        >
          <input
            ref={fileRef}
            type="file"
            accept=".zip,application/zip"
            className="phantasi-workbench__drop-file"
            disabled={installing}
            onChange={(event) => {
              const file = event.target.files?.[0]
              event.target.value = ''
              takeFile(file)
            }}
          />
          <LuUpload />
          <p>{copy.themeDrop}</p>
          <small>{copy.themeUploadFormats}</small>
        </button>
        <div className="studio-blog__install-url">
          <InputItem
            itemKey="blog-theme-url"
            label={copy.themeUrl}
            value={url}
            onChange={(next) => {
              setUrl(next)
              setUrlError('')
            }}
            placeholder="https://example.com/theme.zip"
            inputType="url"
            size="sm"
            autoComplete="off"
            error={urlError || undefined}
          />
          <SettingsButton
            size="sm"
            variant="secondary"
            icon={<LuLink />}
            loading={installing}
            disabled={busy != null || !url.trim()}
            onClick={() => { void installFromUrl() }}
          >
            {copy.themeInstall}
          </SettingsButton>
        </div>
      </section>
    </div>
  )
}

/** 后端要开关打开且博客对所有访客开放，`/api/phantasi/notes.xml` 才存在。 */
function NotesRssSwitch() {
  const { t } = useI18n()
  const copy = t.studio
  const { preferences } = useModuleVisibilityPreferences()
  const open = preferences.modules.phantasi === 'all'
  const [enabled, setEnabled] = useState<boolean | null>(null)
  const [saving, setSaving] = useState(false)

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
  return (
    <>
      <SwitchItem
        itemKey="blog-notes-rss"
        label={copy.notesRss}
        description={enabled && !open ? copy.notesRssPrivate : copy.notesRssHint}
        value={enabled ?? false}
        disabled={enabled == null}
        loading={saving}
        onChange={(next) => { void toggle(next) }}
      />
      {enabled && open ? (
        <InfoActionCard
          embedded
          fields={[{ key: 'feed', label: copy.notesRssFeed, value: feedUrl, mono: true }]}
        />
      ) : null}
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
