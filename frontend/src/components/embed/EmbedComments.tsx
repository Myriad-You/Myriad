import type { CSSProperties, KeyboardEvent, ReactNode } from 'react'
import type { CommentItem } from '../../types/phantasi'
import {
  LuChevronDown as ChevronDown,
  LuMessageCircle as MessageCircle,
  LuSend as Send,
  LuTrash2 as Trash2,
} from '@lib/icons'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiApi from '../../services/phantasiApi'
import { formatUserFacingError } from '../../utils/formatUserFacingError'
import { articleReturnPath, loginHref } from '../../utils/safeReturnPath'
import { commentHue, commentWhen } from './commentTime'
import { useFitFrameHeight } from './embedFrame'
import { embedHost } from './embedHost'
import { watchPalette } from './pagePaletteDom'
import { publicThread } from './publicComments'
import './embed.css'

type Copy = ReturnType<typeof useI18n>['t']['embed']

function authorName(comment: CommentItem, fallback: string): string {
  return comment.user_display_name?.trim() || comment.user_name?.trim() || fallback
}

/** 同源父页才能读到文章地址。跨源或读不到时交给 referrer / `/blog/`。 */
function parentArticleHref(): string | null {
  return embedHost()?.win.location.href ?? null
}

/**
 * 颜色从框背后那块文章页上取（和 AI 伴读同一套，pagePalette.ts）：底色、字色、线条、
 * 主站的强调色都写到自己的根上，评论区看起来是文章的一部分，而不是嵌进去的另一张页。
 */
function useHostPalette(): void {
  useEffect(() => {
    const host = embedHost()
    const frame = host?.frame
    if (!host || !frame) return
    return watchPalette(host.doc, [{
      element: document.documentElement,
      prefix: 'p-',
      // 框不在窗口里时取不到它背后那块（会取到页头之类），滚进来再取。
      followScroll: true,
      textFrom: () => frame.parentElement,
      at: () => {
        const rect = frame.getBoundingClientRect()
        const y = Math.max(rect.top + 12, 0)
        if (rect.bottom <= 0 || y >= host.win.innerHeight) return null
        return { x: rect.left + rect.width / 2, y }
      },
    }])
  }, [])
}

function useNow(): number {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000)
    return () => window.clearInterval(timer)
  }, [])
  return now
}

function Avatar({ name, src, seed, small = false }: { name: string; src?: string; seed: string | number; small?: boolean }) {
  const [broken, setBroken] = useState(false)
  const className = `cm-avatar${small ? ' is-small' : ''}`
  if (src && !broken) {
    return <img className={className} src={src} alt="" loading="lazy" onError={() => setBroken(true)} />
  }
  return (
    <span className={className} style={{ '--cm-hue': commentHue(seed) } as CSSProperties} aria-hidden="true">
      {Array.from(name.trim())[0]?.toUpperCase() ?? '·'}
    </span>
  )
}

/** 输入框跟着字数长高；⌘/Ctrl+Enter 发，Esc 收起（有收起的话）。 */
function Composer({
  value,
  onChange,
  onSubmit,
  onCancel,
  busy,
  placeholder,
  label,
  copy,
  autoFocus = false,
  avatar,
}: {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  onCancel?: () => void
  busy: boolean
  placeholder: string
  label?: string
  copy: Copy
  autoFocus?: boolean
  avatar?: ReactNode
}) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const [focused, setFocused] = useState(autoFocus)
  useEffect(() => {
    const node = ref.current
    if (!node) return
    node.style.height = 'auto'
    node.style.height = `${node.scrollHeight}px`
  }, [value])
  const open = focused || value.trim() !== '' || onCancel != null
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault()
      onSubmit()
    } else if (event.key === 'Escape' && onCancel) {
      event.preventDefault()
      onCancel()
    }
  }
  return (
    <form
      className={`cm-composer${open ? ' is-open' : ''}`}
      onSubmit={(event) => {
        event.preventDefault()
        onSubmit()
      }}
    >
      {avatar}
      <div className="cm-composer__box">
        {label ? <span className="cm-composer__label">{label}</span> : null}
        <textarea
          ref={ref}
          rows={1}
          value={value}
          autoFocus={autoFocus}
          aria-label={placeholder}
          placeholder={placeholder}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onKeyDown={onKeyDown}
          onChange={(event) => onChange(event.target.value)}
        />
        <div className="cm-composer__foot">
          <span className="cm-composer__hint">{copy.sendShortcut}</span>
          {onCancel ? (
            <button type="button" className="cm-btn cm-btn--quiet" onClick={onCancel}>{copy.cancel}</button>
          ) : null}
          <button type="submit" className="cm-btn cm-btn--accent" disabled={busy || !value.trim()}>
            <Send aria-hidden="true" />
            {busy ? copy.sending : copy.send}
          </button>
        </div>
      </div>
    </form>
  )
}

export function EmbedComments() {
  const { itemId: rawId } = useParams()
  const itemId = Number(rawId)
  const valid = Number.isSafeInteger(itemId) && itemId > 0
  const { t, locale, format } = useI18n()
  const copy = t.embed
  const { isAuthenticated, hasChecked, user } = useAuth()
  const now = useNow()
  const [comments, setComments] = useState<CommentItem[]>([])
  const [canWrite, setCanWrite] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [replyTo, setReplyTo] = useState<number | null>(null)
  const [replyDraft, setReplyDraft] = useState('')
  const [replies, setReplies] = useState<Record<number, CommentItem[]>>({})
  const [openReplies, setOpenReplies] = useState<Set<number>>(() => new Set())
  const [confirming, setConfirming] = useState<number | null>(null)
  /** 刚发出的那条：进场动画只给它。 */
  const [fresh, setFresh] = useState<number | null>(null)
  const fitRef = useFitFrameHeight(!loading && hasChecked)
  useHostPalette()

  const load = useCallback(async (signal?: AbortSignal) => {
    if (!valid) return
    const response = await phantasiApi.getComments(itemId, undefined, { signal })
    if (signal?.aborted) return
    setComments(publicThread(response.comments ?? []))
    setCanWrite(Boolean(response.can_write))
  }, [itemId, valid])

  // 等登录检查有了结果再拉：先按访客拉、身份随后落定的话，评论接口会作废那次请求。
  // 身份变了（登录、换号）就按新身份重拉，看得到的评论不一样。
  const viewerKey = hasChecked ? (user ? `${user.id}:${user.is_admin ? 'admin' : ''}` : 'guest') : null
  useEffect(() => {
    if (!valid) {
      setLoading(false)
      return
    }
    if (viewerKey == null) return
    const controller = new AbortController()
    setLoading(true)
    setError('')
    void load(controller.signal).catch(async (err: unknown) => {
      if (controller.signal.aborted || (err instanceof DOMException && err.name === 'AbortError')) return
      const message = await formatUserFacingError(err, copy.loadFailed)
      if (!controller.signal.aborted) setError(message)
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [copy.loadFailed, load, valid, viewerKey])

  const articlePath = typeof document === 'undefined'
    ? '/blog/'
    : articleReturnPath({
        origin: window.location.origin,
        search: window.location.search,
        referrer: document.referrer,
        parentHref: parentArticleHref(),
      })

  const fetchReplies = async (commentId: number) => {
    const response = await phantasiApi.getCommentReplies(commentId)
    setReplies((current) => ({ ...current, [commentId]: publicThread(response.replies ?? []) }))
  }

  const send = async () => {
    const text = draft.trim()
    if (!text || busy) return
    setBusy(true)
    setError('')
    try {
      const response = await phantasiApi.createComment(itemId, {
        selected_text: '',
        comment: text,
        is_public: true,
      })
      const created = response.comment
      if (created?.is_public) {
        setComments((rows) => [...rows, created])
        setFresh(created.id)
      }
      setDraft('')
    } catch (err) {
      setError(await formatUserFacingError(err, copy.sendFailed))
    } finally {
      setBusy(false)
    }
  }

  const sendReply = async (parentId: number) => {
    const text = replyDraft.trim()
    if (!text || busy) return
    setBusy(true)
    setError('')
    try {
      const response = await phantasiApi.createReply(itemId, parentId, text, undefined, {
        is_public: true,
      })
      const created = response.comment
      if (created?.is_public) {
        setComments((rows) => rows.map((row) => row.id === parentId ? { ...row, reply_count: (row.reply_count ?? 0) + 1 } : row))
        // 还没展开过的，取一遍完整的回复，免得只剩刚发的这一条。
        if (replies[parentId]) {
          setReplies((current) => ({ ...current, [parentId]: [...(current[parentId] ?? []), created] }))
        } else {
          await fetchReplies(parentId)
        }
        setFresh(created.id)
      }
      setReplyDraft('')
      setReplyTo(null)
      setOpenReplies((current) => new Set(current).add(parentId))
    } catch (err) {
      setError(await formatUserFacingError(err, copy.sendFailed))
    } finally {
      setBusy(false)
    }
  }

  const toggleReplies = async (commentId: number) => {
    if (openReplies.has(commentId)) {
      setOpenReplies((current) => {
        const next = new Set(current)
        next.delete(commentId)
        return next
      })
      return
    }
    setOpenReplies((current) => new Set(current).add(commentId))
    if (replies[commentId]) return
    try {
      await fetchReplies(commentId)
    } catch (err) {
      setError(await formatUserFacingError(err, copy.loadFailed))
    }
  }

  const remove = async (target: CommentItem) => {
    setBusy(true)
    setError('')
    try {
      await phantasiApi.deleteComment(target.id)
      if (target.parent_id) {
        const parentId = target.parent_id
        setReplies((current) => ({ ...current, [parentId]: (current[parentId] ?? []).filter((row) => row.id !== target.id) }))
        setComments((rows) => rows.map((row) => row.id === parentId ? { ...row, reply_count: Math.max(0, (row.reply_count ?? 1) - 1) } : row))
      } else {
        setComments((rows) => rows.filter((row) => row.id !== target.id))
      }
      setConfirming(null)
    } catch (err) {
      setError(await formatUserFacingError(err, copy.sendFailed))
    } finally {
      setBusy(false)
    }
  }

  const me = user ? (user.display_name?.trim() || user.username) : ''
  const myAvatar = user ? <Avatar name={me} src={user.avatar_url} seed={user.id} /> : null

  const renderEntry = (comment: CommentItem, reply: boolean, actions: ReactNode) => {
    const name = authorName(comment, copy.anonymous)
    const mine = user?.id === comment.user_id
    const at = new Date(comment.created_at)
    return (
      <article className={`cm-entry${comment.id === fresh ? ' is-fresh' : ''}`}>
        <Avatar name={name} src={comment.user_avatar} seed={comment.user_id || name} small={reply} />
        <div className="cm-entry__main">
          <header className="cm-entry__head">
            <span className="cm-entry__name">{name}</span>
            {mine ? <span className="cm-entry__you">{copy.you}</span> : null}
            <time className="cm-entry__time" dateTime={at.toISOString()} title={at.toLocaleString(locale)}>
              {commentWhen(comment.created_at, now, locale, copy.justNow)}
            </time>
            {/* 删除在名字这一行的最右边，悬停这条才露出。 */}
            {mine && confirming !== comment.id ? (
              <button
                type="button"
                className="cm-link cm-link--danger"
                aria-label={copy.delete}
                title={copy.delete}
                onClick={() => setConfirming(comment.id)}
              >
                <Trash2 aria-hidden="true" />
              </button>
            ) : null}
          </header>
          <p className="cm-entry__body">{comment.comment}</p>
          {confirming === comment.id ? (
            <div className="cm-confirm" role="alertdialog" aria-label={copy.deleteConfirm}>
              <span>{copy.deleteConfirm}</span>
              <button type="button" className="cm-btn cm-btn--quiet" onClick={() => setConfirming(null)}>{copy.cancel}</button>
              <button type="button" className="cm-btn cm-btn--danger" disabled={busy} onClick={() => { void remove(comment) }}>
                {copy.delete}
              </button>
            </div>
          ) : actions ? (
            <div className="cm-entry__actions">{actions}</div>
          ) : null}
        </div>
      </article>
    )
  }

  return (
    <main ref={fitRef} className="embed-root embed-root--fit cm">
      <header className="cm-head">
        <h1>{copy.commentsTitle}</h1>
        {!loading && comments.length ? <span className="cm-head__count">{comments.length}</span> : null}
      </header>

      {!valid ? <p className="cm-error">{copy.loadFailed}</p> : null}
      {error ? <p className="cm-error" role="alert">{error}</p> : null}

      {canWrite ? (
        <Composer
          value={draft}
          onChange={setDraft}
          onSubmit={() => { void send() }}
          busy={busy}
          placeholder={copy.commentPlaceholder}
          copy={copy}
          avatar={myAvatar}
        />
      ) : null}
      {hasChecked && !isAuthenticated ? (
        <a className="cm-login" href={loginHref(articlePath)} target="_top">
          <MessageCircle aria-hidden="true" />
          <span>{copy.loginPrompt}</span>
          <span className="cm-btn cm-btn--accent">{copy.login}</span>
        </a>
      ) : null}
      {hasChecked && isAuthenticated && !canWrite && !loading ? <p className="cm-note">{copy.cannotWrite}</p> : null}

      {loading ? (
        <div className="cm-skeleton" aria-label={copy.loading} role="status">
          {[0, 1, 2].map((key) => (
            <div key={key} className="cm-skeleton__row">
              <span className="cm-skeleton__dot" />
              <span className="cm-skeleton__lines"><span /><span /></span>
            </div>
          ))}
        </div>
      ) : null}
      {!loading && valid && comments.length === 0 ? (
        <p className="cm-empty">
          <MessageCircle aria-hidden="true" />
          {canWrite ? copy.emptyInvite : copy.empty}
        </p>
      ) : null}

      <ol className="cm-list">
        {comments.map((comment) => {
          const count = comment.reply_count ?? 0
          const shown = openReplies.has(comment.id)
          const name = authorName(comment, copy.anonymous)
          return (
            <li key={comment.id} className="cm-item">
              {renderEntry(comment, false, (
                <>
                  {canWrite ? (
                    <button
                      type="button"
                      className={`cm-link${replyTo === comment.id ? ' is-on' : ''}`}
                      onClick={() => {
                        setReplyTo(replyTo === comment.id ? null : comment.id)
                        setReplyDraft('')
                      }}
                    >
                      {copy.reply}
                    </button>
                  ) : null}
                  {count > 0 ? (
                    <button
                      type="button"
                      className={`cm-link${shown ? ' is-on' : ''}`}
                      aria-expanded={shown}
                      onClick={() => { void toggleReplies(comment.id) }}
                    >
                      <ChevronDown className="cm-link__chev" aria-hidden="true" />
                      {shown ? copy.hideReplies : format(copy.repliesCount, { count })}
                    </button>
                  ) : null}
                </>
              ))}
              {shown && replies[comment.id]?.length ? (
                <ol className="cm-replies">
                  {replies[comment.id]!.map((reply) => (
                    <li key={reply.id}>{renderEntry(reply, true, null)}</li>
                  ))}
                </ol>
              ) : null}
              {replyTo === comment.id ? (
                <div className="cm-reply-box">
                  <Composer
                    value={replyDraft}
                    onChange={setReplyDraft}
                    onSubmit={() => { void sendReply(comment.id) }}
                    onCancel={() => setReplyTo(null)}
                    busy={busy}
                    placeholder={copy.replyPlaceholder}
                    label={format(copy.replyingTo, { name })}
                    copy={copy}
                    autoFocus
                  />
                </div>
              ) : null}
            </li>
          )
        })}
      </ol>
    </main>
  )
}
