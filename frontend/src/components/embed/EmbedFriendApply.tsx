import type { ChangeEvent, CSSProperties, ReactNode, Ref } from 'react'
import type { FriendApplyField, FriendApplyFields, FriendApplyProblem } from './friendApplyForm'
import {
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuGlobe as Globe,
  LuLoader2 as Loader,
  LuRss as Rss,
  PiPaperPlaneTiltFill as Send,
} from '@lib/icons'
import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiApi from '../../services/phantasiApi'
import { formatUserFacingError } from '../../utils/formatUserFacingError'
import { commentHue } from './commentTime'
import { focusInFrame, useFitFrameHeight } from './embedFrame'
import {
  displaySiteUrl,
  EMPTY_FRIEND_APPLY,
  FRIEND_APPLY_MAX,
  friendApplyPayload,
  friendApplyProblems,
  normalizeSiteUrl,
} from './friendApplyForm'
import { useHostPalette } from './useHostPalette'
import './embed.css'

type Copy = ReturnType<typeof useI18n>['t']['embed']
type Stage = 'open' | 'done'

function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, key: string) => String(values[key] ?? match))
}

function problemText(problem: FriendApplyProblem, field: FriendApplyField, copy: Copy): string {
  switch (problem) {
    case 'required': return field === 'siteName' ? copy.siteNameRequired : copy.siteUrlRequired
    case 'url': return copy.urlInvalid
    case 'email': return copy.emailInvalid
    case 'tooLong': return fill(copy.tooLong, { max: FRIEND_APPLY_MAX[field] })
  }
}

/**
 * 站点的小图标：先放名字的首字（颜色按地址定），站点根上的 favicon 真加载出来了再盖上去。
 * 拿不到、还在加载、或者是空图时都只看到首字，不露出一个空方块。
 */
function SiteMark({ url, name }: { url: URL | null; name: string }) {
  const icon = url ? `${url.origin}/favicon.ico` : ''
  const [loaded, setLoaded] = useState<string | null>(null)
  const [broken, setBroken] = useState<string | null>(null)
  const initial = [...(name.trim() || url?.hostname || '')][0]?.toUpperCase() ?? ''
  const style = { '--fa-hue': commentHue(url?.hostname || name || 'site') } as CSSProperties
  return (
    <span className="fa-mark" style={style} aria-hidden="true">
      {initial || <Globe />}
      {icon && broken !== icon ? (
        <img
          className={loaded === icon ? 'is-loaded' : undefined}
          src={icon}
          alt=""
          onLoad={(event) => {
            if (event.currentTarget.naturalWidth > 1) setLoaded(icon)
            else setBroken(icon)
          }}
          onError={() => setBroken(icon)}
        />
      ) : null}
    </span>
  )
}

/** 通过后在友链页上大致的样子：图标、名字、地址、简介。 */
function Preview({ fields, copy, label }: { fields: FriendApplyFields; copy: Copy; label?: string }) {
  const url = useMemo(() => normalizeSiteUrl(fields.siteUrl), [fields.siteUrl])
  const name = fields.siteName.trim()
  const description = fields.description.trim()
  return (
    <figure className="fa-preview">
      {label ? <figcaption>{label}</figcaption> : null}
      <div className="fa-preview__card">
        <SiteMark url={url} name={name} />
        <div className="fa-preview__text">
          <span className={`fa-preview__name${name ? '' : ' is-empty'}`}>{name || copy.previewName}</span>
          <span className="fa-preview__url">{url ? displaySiteUrl(url) : copy.siteUrlPlaceholder}</span>
          {description ? <span className="fa-preview__desc">{description}</span> : null}
        </div>
      </div>
    </figure>
  )
}

interface FieldProps {
  name: FriendApplyField
  label: string
  optional?: string
  hint?: string
  icon?: ReactNode
  multiline?: boolean
  type?: 'text' | 'email'
  inputMode?: 'url' | 'email' | 'text'
  autoComplete?: string
  placeholder: string
  value: string
  problem?: string
  counter?: boolean
  onChange: (value: string) => void
  onBlur: () => void
  inputRef?: Ref<HTMLInputElement>
}

function Field({
  name,
  label,
  optional,
  hint,
  icon,
  multiline = false,
  type = 'text',
  inputMode,
  autoComplete,
  placeholder,
  value,
  problem,
  counter = false,
  onChange,
  onBlur,
  inputRef,
}: FieldProps) {
  const id = useId()
  const max = FRIEND_APPLY_MAX[name]
  const length = [...value].length
  const describedBy = problem ? `${id}-problem` : hint ? `${id}-hint` : undefined
  const common = {
    id,
    name,
    value,
    placeholder,
    'aria-invalid': problem ? true : undefined,
    'aria-describedby': describedBy,
    onBlur,
    onChange: (event: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => onChange(event.target.value),
  }
  return (
    <div className={`fa-field${problem ? ' is-invalid' : ''}`}>
      <div className="fa-field__top">
        <label htmlFor={id}>{label}</label>
        {optional ? <span className="fa-field__optional">{optional}</span> : null}
        {counter && length > max * 0.8 ? (
          <span className={`fa-field__count${length > max ? ' is-over' : ''}`}>{length}/{max}</span>
        ) : null}
      </div>
      <div className={`fa-control${icon ? ' has-icon' : ''}${multiline ? ' is-multiline' : ''}`}>
        {icon ? <span className="fa-control__icon" aria-hidden="true">{icon}</span> : null}
        {multiline
          ? <textarea {...common} rows={2} />
          : <input {...common} ref={inputRef} type={type} inputMode={inputMode} autoComplete={autoComplete} spellCheck={false} />}
      </div>
      {problem
        ? <p id={`${id}-problem`} className="fa-field__problem" role="alert">{problem}</p>
        : hint ? <p id={`${id}-hint`} className="fa-field__hint">{hint}</p> : null}
    </div>
  )
}

export function EmbedFriendApply() {
  const { t } = useI18n()
  const copy = t.embed
  const [stage, setStage] = useState<Stage>('open')
  const [fields, setFields] = useState<FriendApplyFields>(EMPTY_FRIEND_APPLY)
  const [touched, setTouched] = useState<Partial<Record<FriendApplyField, boolean>>>({})
  const [tried, setTried] = useState(false)
  const [contactOpen, setContactOpen] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const fitRef = useFitFrameHeight(true)
  const firstRef = useRef<HTMLInputElement>(null)
  const doneRef = useRef<HTMLHeadingElement>(null)
  const moved = useRef(false)
  const siteGroupId = useId()
  useHostPalette()

  const problems = useMemo(() => friendApplyProblems(fields), [fields])
  const ready = Object.keys(problems).length === 0

  // 提交成功、再填一份之后，焦点跟着去新出现的那一块；第一次渲染不抢焦点。
  useEffect(() => {
    if (!moved.current) return
    // 等框按新内容定好高度（useFitFrameHeight 的 ResizeObserver）再量位置。
    const frame = requestAnimationFrame(() => {
      focusInFrame(stage === 'open' ? firstRef.current : doneRef.current)
    })
    return () => cancelAnimationFrame(frame)
  }, [stage])

  const go = (next: Stage) => {
    moved.current = true
    setStage(next)
  }

  const set = (key: FriendApplyField) => (value: string) => {
    setFields((current) => ({ ...current, [key]: value }))
    if (error) setError('')
  }
  const touch = (key: FriendApplyField) => () => {
    if (fields[key].trim()) setTouched((current) => ({ ...current, [key]: true }))
  }
  const shown = (key: FriendApplyField) => {
    const problem = problems[key]
    if (!problem || (!tried && !touched[key])) return undefined
    return problemText(problem, key, copy)
  }

  const submit = async () => {
    setTried(true)
    if (!ready || busy) {
      // 有问题的栏在收起的「联系方式」里时，展开它，免得看不见错在哪。
      if (problems.applicantEmail || problems.applicantName || problems.message) setContactOpen(true)
      // 等错误提示画出来，再把焦点放到第一处要改的栏上。
      requestAnimationFrame(() => {
        focusInFrame(document.querySelector<HTMLElement>('.fa-form [aria-invalid="true"]'))
      })
      return
    }
    setBusy(true)
    setError('')
    try {
      await phantasiApi.applySourceApplication(friendApplyPayload(fields))
      go('done')
    } catch (err) {
      setError(await formatUserFacingError(err, copy.submitFailed))
    } finally {
      setBusy(false)
    }
  }

  const reset = () => {
    setFields(EMPTY_FRIEND_APPLY)
    setTouched({})
    setTried(false)
    setContactOpen(false)
    setError('')
    go('open')
  }

  if (stage === 'done') {
    return (
      <main ref={fitRef} className="embed-root embed-root--fit cm fa">
        <section className="fa-done" role="status">
          <span className="fa-done__badge" aria-hidden="true"><Check /></span>
          <h1 ref={doneRef} tabIndex={-1}>{copy.submittedTitle}</h1>
          <p>{copy.submitted}</p>
          <Preview fields={fields} copy={copy} />
          <button type="button" className="fa-text-btn" onClick={reset}>{copy.again}</button>
        </section>
      </main>
    )
  }

  return (
    <main ref={fitRef} className="embed-root embed-root--fit cm fa">
      <header className="cm-head">
        <h1>{copy.friendTitle}</h1>
      </header>

      <form
        className="fa-form"
        noValidate
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <p className="fa-lede">{copy.friendHint}</p>

        <div className="fa-layout">
          <div className="fa-group" role="group" aria-labelledby={siteGroupId}>
            <span id={siteGroupId} className="fa-group__title">{copy.sectionSite}</span>
            <div className="fa-row">
              <Field
                name="siteName"
                label={copy.siteName}
                placeholder={copy.siteNamePlaceholder}
                autoComplete="organization"
                value={fields.siteName}
                problem={shown('siteName')}
                onChange={set('siteName')}
                onBlur={touch('siteName')}
                inputRef={firstRef}
              />
              <Field
                name="siteUrl"
                label={copy.siteUrl}
                icon={<Globe />}
                inputMode="url"
                autoComplete="url"
                placeholder={copy.siteUrlPlaceholder}
                value={fields.siteUrl}
                problem={shown('siteUrl')}
                onChange={set('siteUrl')}
                onBlur={touch('siteUrl')}
              />
            </div>
            <Field
              name="description"
              label={copy.description}
              optional={copy.optional}
              multiline
              counter
              placeholder={copy.descriptionPlaceholder}
              value={fields.description}
              problem={shown('description')}
              onChange={set('description')}
              onBlur={touch('description')}
            />
            <Field
              name="feedUrl"
              label={copy.feedUrl}
              optional={copy.optional}
              icon={<Rss />}
              inputMode="url"
              placeholder={copy.feedPlaceholder}
              hint={copy.feedHint}
              value={fields.feedUrl}
              problem={shown('feedUrl')}
              onChange={set('feedUrl')}
              onBlur={touch('feedUrl')}
            />
          </div>

          <Preview fields={fields} copy={copy} label={copy.previewLabel} />
        </div>

        <div className={`fa-more${contactOpen ? ' is-open' : ''}`}>
          <button
            type="button"
            className="fa-more__toggle"
            aria-expanded={contactOpen}
            onClick={() => setContactOpen((open) => !open)}
          >
            <span>{copy.sectionContact}</span>
            <span className="fa-field__optional">{copy.optional}</span>
            <ChevronDown className="fa-more__chev" aria-hidden="true" />
          </button>
          {contactOpen ? (
            <div className="fa-more__body">
              <div className="fa-row">
                <Field
                  name="applicantName"
                  label={copy.applicantName}
                  autoComplete="name"
                  placeholder={copy.applicantNamePlaceholder}
                  value={fields.applicantName}
                  problem={shown('applicantName')}
                  onChange={set('applicantName')}
                  onBlur={touch('applicantName')}
                />
                <Field
                  name="applicantEmail"
                  label={copy.applicantEmail}
                  type="email"
                  inputMode="email"
                  autoComplete="email"
                  placeholder={copy.applicantEmailPlaceholder}
                  hint={copy.emailHint}
                  value={fields.applicantEmail}
                  problem={shown('applicantEmail')}
                  onChange={set('applicantEmail')}
                  onBlur={touch('applicantEmail')}
                />
              </div>
              <Field
                name="message"
                label={copy.message}
                multiline
                counter
                placeholder={copy.messagePlaceholder}
                value={fields.message}
                problem={shown('message')}
                onChange={set('message')}
                onBlur={touch('message')}
              />
            </div>
          ) : null}
        </div>

        {error ? <p className="cm-error" role="alert">{error}</p> : null}

        <footer className="fa-actions">
          <button type="submit" className={`fa-submit${ready ? ' is-ready' : ''}`} disabled={busy} aria-busy={busy}>
            {busy ? <Loader className="cm-spin" aria-hidden="true" /> : <Send aria-hidden="true" />}
            <span>{busy ? copy.submitting : copy.submit}</span>
          </button>
        </footer>
      </form>
    </main>
  )
}
