import type { CommentItem } from '../../types/phantasi'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiApi from '../../services/phantasiApi'
import { articleReturnPath, loginHref } from '../../utils/safeReturnPath'
import { userFacingError } from '../../utils/userFacingError'
import { useFitFrameHeight } from './embedFrame'
import { publicThread } from './publicComments'
import './embed.css'

function authorName(comment: CommentItem, fallback: string): string {
  return comment.user_display_name?.trim() || comment.user_name?.trim() || fallback
}

/** 同源父页才能读到文章地址。跨源或读不到时交给 referrer / `/blog/`。 */
function parentArticleHref(): string | null {
  try {
    if (window.parent === window) return null
    return window.parent.location.href
  } catch {
    return null
  }
}

export function EmbedComments() {
  const { itemId: rawId } = useParams()
  const itemId = Number(rawId)
  const valid = Number.isSafeInteger(itemId) && itemId > 0
  const { t, locale } = useI18n()
  const copy = t.embed
  const { isAuthenticated, hasChecked, user } = useAuth()
  const [comments, setComments] = useState<CommentItem[]>([])
  const [canWrite, setCanWrite] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [replyTo, setReplyTo] = useState<number | null>(null)
  const [replyDraft, setReplyDraft] = useState('')
  const [replies, setReplies] = useState<Record<number, CommentItem[]>>({})
  const [openReplies, setOpenReplies] = useState<number | null>(null)
  const rootRef = useRef<HTMLElement>(null)
  // 博客文章底下的评论框（crates/myriad-blog/src/rewrite.rs）按内容撑高，不留一大块空白。
  useFitFrameHeight(rootRef, 'myriad-comments', !loading && hasChecked)

  const load = useCallback(async (signal?: AbortSignal) => {
    if (!valid) return
    const response = await phantasiApi.getComments(itemId, undefined, { signal })
    if (signal?.aborted) return
    setComments(publicThread(response.comments ?? []))
    setCanWrite(Boolean(response.can_write))
  }, [itemId, valid])

  useEffect(() => {
    if (!valid) {
      setLoading(false)
      return
    }
    const controller = new AbortController()
    setLoading(true)
    setError('')
    void load(controller.signal).catch((err: unknown) => {
      if (!controller.signal.aborted) setError(userFacingError(err, copy.loadFailed))
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [copy.loadFailed, load, valid])

  const articlePath = typeof document === 'undefined'
    ? '/blog/'
    : articleReturnPath({
        origin: window.location.origin,
        search: window.location.search,
        referrer: document.referrer,
        parentHref: parentArticleHref(),
      })

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
      if (created?.is_public) setComments((rows) => [...rows, created])
      setDraft('')
    } catch (err) {
      setError(userFacingError(err, copy.sendFailed))
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
        setReplies((current) => ({
          ...current,
          [parentId]: [...(current[parentId] ?? []), created],
        }))
      }
      setReplyDraft('')
      setReplyTo(null)
      setOpenReplies(parentId)
    } catch (err) {
      setError(userFacingError(err, copy.sendFailed))
    } finally {
      setBusy(false)
    }
  }

  const toggleReplies = async (commentId: number) => {
    if (openReplies === commentId) {
      setOpenReplies(null)
      return
    }
    setOpenReplies(commentId)
    if (replies[commentId]) return
    try {
      const response = await phantasiApi.getCommentReplies(commentId)
      setReplies((current) => ({ ...current, [commentId]: publicThread(response.replies ?? []) }))
    } catch (err) {
      setError(userFacingError(err, copy.loadFailed))
    }
  }

  const remove = async (commentId: number) => {
    setBusy(true)
    setError('')
    try {
      await phantasiApi.deleteComment(commentId)
      setComments((rows) => rows.filter((row) => row.id !== commentId))
    } catch (err) {
      setError(userFacingError(err, copy.sendFailed))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main ref={rootRef} className="embed-root embed-root--fit">
      <h1>{copy.commentsTitle}</h1>
      {!valid ? <p className="embed-error">{copy.loadFailed}</p> : null}
      {error ? <p className="embed-error" role="alert">{error}</p> : null}
      {loading ? <p>{copy.loading}</p> : null}
      {!loading && valid && comments.length === 0 ? <p>{copy.empty}</p> : null}
      <ul className="embed-list">
        {comments.map((comment) => (
          <li key={comment.id} className="embed-comment">
            <div className="embed-meta">
              <span>{authorName(comment, copy.anonymous)}</span>
              <time dateTime={new Date(comment.created_at).toISOString()}>
                {new Date(comment.created_at).toLocaleString(locale)}
              </time>
            </div>
            <p>{comment.comment}</p>
            <div className="embed-actions">
              <button type="button" onClick={() => { void toggleReplies(comment.id) }}>
                {copy.replies}{comment.reply_count ? ` ${comment.reply_count}` : ''}
              </button>
              {canWrite ? (
                <button type="button" onClick={() => { setReplyTo(comment.id); setReplyDraft('') }}>
                  {copy.reply}
                </button>
              ) : null}
              {user?.id === comment.user_id ? (
                <button type="button" disabled={busy} onClick={() => { void remove(comment.id) }}>
                  {copy.delete}
                </button>
              ) : null}
            </div>
            {openReplies === comment.id ? (
              <ul className="embed-list">
                {(replies[comment.id] ?? []).map((reply) => (
                  <li key={reply.id} className="embed-reply">
                    <div className="embed-meta">
                      <span>{authorName(reply, copy.anonymous)}</span>
                    </div>
                    <p>{reply.comment}</p>
                  </li>
                ))}
              </ul>
            ) : null}
            {replyTo === comment.id ? (
              <form
                className="embed-form"
                onSubmit={(event) => {
                  event.preventDefault()
                  void sendReply(comment.id)
                }}
              >
                <textarea
                  rows={2}
                  value={replyDraft}
                  aria-label={copy.replyPlaceholder}
                  placeholder={copy.replyPlaceholder}
                  onChange={(event) => setReplyDraft(event.target.value)}
                />
                <button type="submit" disabled={busy || !replyDraft.trim()}>{copy.send}</button>
              </form>
            ) : null}
          </li>
        ))}
      </ul>
      {hasChecked && !isAuthenticated ? (
        <p>
          <a className="embed-login" href={loginHref(articlePath)} target="_top">
            {copy.login}
          </a>
          {' '}
          {copy.loginHint}
        </p>
      ) : null}
      {canWrite ? (
        <form
          className="embed-form"
          onSubmit={(event) => {
            event.preventDefault()
            void send()
          }}
        >
          <textarea
            rows={3}
            value={draft}
            aria-label={copy.commentPlaceholder}
            placeholder={copy.commentPlaceholder}
            onChange={(event) => setDraft(event.target.value)}
          />
          <button type="submit" disabled={busy || !draft.trim()}>{busy ? copy.sending : copy.send}</button>
        </form>
      ) : null}
      {hasChecked && isAuthenticated && !canWrite && !loading ? <p>{copy.cannotWrite}</p> : null}
    </main>
  )
}
