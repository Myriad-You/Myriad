import type { ReactNode } from 'react'
import type merope from '../../../i18n/merope.zh-CN.json'
import type {
  MemoryNodeAudience,
  MemoryNodeEntry,
  MemoryNodeIntent,
  MemoryNodeLine,
  MemoryNodeList,
  MemoryNodePage,
} from '../../../services/agent'
import { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { memoryNodesApi } from '../../../services/agent'
import { SettingsButton } from '../../settings'
import { fill, Section, shortDate, Tag } from './mindParts'
import './AgentNodesPanel.css'

type Copy = typeof merope.mind.nodes
interface Open {
  audience: string
  node: string
}

/** Codes the server refuses an edit with, as the page words them. */
const AWAY = new Set([
  'host_frozen',
  'host_moved',
  'mind_elsewhere',
  'mind_away',
])

function failure(copy: Copy, error: unknown): string {
  const code = (error as { code?: string } | null)?.code ?? ''
  if (AWAY.has(code)) return copy.failed.away
  return (
    (
      {
        bad_name: copy.failed.badName,
        heard_unkept: copy.failed.heardUnkept,
        node_not_found: copy.failed.notFound,
      } as Record<string, string>
    )[code] ?? copy.failed.other
  )
}

function day(micros: number, locale: string) {
  return micros > 0
    ? shortDate(new Date(micros / 1000).toISOString(), locale)
    : ''
}

/**
 * The character's nodes, for the site owner: the owner's private talk, the
 * character's own, each group, each node with what is kept on it. The
 * owner may delete a node or an entry, rename a node, cancel an intent and
 * answer what was heard; nothing else changes here.
 */
export default function AgentNodesPanel() {
  const { t, locale } = useI18n()
  const copy = t.merope.mind.nodes as Copy
  const mind = t.merope.mind
  const [list, setList] = useState<MemoryNodeList | null>(null)
  const [open, setOpen] = useState<Open | null>(null)
  const [page, setPage] = useState<MemoryNodePage | null>(null)
  const [failed, setFailed] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [draft, setDraft] = useState<string | null>(null)

  const name = list?.herName || copy.unnamed

  const loadList = useCallback(async () => {
    setFailed(false)
    try {
      setList(await memoryNodesApi.list())
    } catch {
      setFailed(true)
    }
  }, [])

  const loadPage = useCallback(async (which: Open) => {
    setFailed(false)
    try {
      setPage(await memoryNodesApi.node(which.audience, which.node))
    } catch {
      setFailed(true)
    }
  }, [])

  useEffect(() => {
    void loadList()
  }, [loadList])

  useEffect(() => {
    setPage(null)
    setConfirming(null)
    setDraft(null)
    setProblem(null)
    if (open) void loadPage(open)
  }, [open, loadPage])

  /**
   * One edit, then the list read again, and the page unless `after` left
   * it (it returns true then).
   */
  const edit = async (
    act: () => Promise<void>,
    after?: () => boolean | void,
  ) => {
    setBusy(true)
    setProblem(null)
    try {
      await act()
      setConfirming(null)
      const left = after?.() === true
      await loadList()
      if (open && !left) await loadPage(open)
    } catch (error) {
      setProblem(failure(copy, error))
    } finally {
      setBusy(false)
    }
  }

  const audienceTitle = (line: MemoryNodeAudience) =>
    line.place === 'private'
      ? copy.private
      : line.place === 'own'
        ? fill(copy.own, { name })
        : fill(copy.group, { group: line.groupName || line.group || '' })

  const nodeName = (
    node: { own: boolean; name?: string | null },
    audience: string,
  ) =>
    node.own && !audience.startsWith('own:')
      ? copy.aboutYou
      : node.name || copy.unnamed

  const kindTag = (kind?: string | null) =>
    kind && kind in copy.kind ? (
      <Tag tone="muted">{copy.kind[kind as keyof Copy['kind']]}</Tag>
    ) : null

  const statusTag = (status: string) =>
    status !== 'active' && status in copy.status ? (
      <Tag tone="muted">{copy.status[status as keyof Copy['status']]}</Tag>
    ) : null

  const saidTag = (saidBy: string[]) => {
    const who = saidBy.find((one) => one !== 'her') ?? saidBy[0]
    if (!who) return null
    const label =
      who === 'her'
        ? fill(copy.said.her, { name })
        : (copy.said as Record<string, string>)[who]
    return label ? <Tag tone="muted">{label}</Tag> : null
  }

  /** A delete that asks first, on its own line. */
  const deleteButton = (
    key: string,
    label: string,
    title: string,
    message: string,
    act: () => Promise<void>,
    after?: () => boolean | void,
  ) =>
    confirming === key ? (
      <div
        className="merope-nodes__confirm"
        role="alertdialog"
        aria-label={title}
      >
        <strong>{title}</strong>
        <span>{message}</span>
        <div className="merope-nodes__buttons">
          <SettingsButton
            size="sm"
            variant="danger"
            loading={busy}
            onClick={() => void edit(act, after)}
          >
            {copy.confirmDelete}
          </SettingsButton>
          <SettingsButton size="sm" onClick={() => setConfirming(null)}>
            {copy.cancel}
          </SettingsButton>
        </div>
      </div>
    ) : (
      <button
        type="button"
        className="merope-nodes__action"
        disabled={busy}
        onClick={() => setConfirming(key)}
      >
        {label}
      </button>
    )

  const line = (node: MemoryNodeLine, audience: string) => (
    <li key={node.id}>
      <button
        type="button"
        className="merope-nodes__node"
        data-node={node.id}
        onClick={() => setOpen({ audience, node: node.id })}
      >
        <span className="merope-nodes__name">{nodeName(node, audience)}</span>
        <span className="merope-mind__tags is-inline">
          {kindTag(node.kind)}
          {statusTag(node.status)}
        </span>
        <span className="merope-nodes__counts">
          {[
            node.entries > 0
              ? fill(copy.count.entries, { count: node.entries })
              : null,
            node.spans > 0
              ? fill(copy.count.spans, { count: node.spans })
              : null,
            node.waiting > 0
              ? fill(copy.count.waiting, { count: node.waiting })
              : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        </span>
      </button>
    </li>
  )

  const entryRow = (
    entry: MemoryNodeEntry,
    audience: string,
    node: string,
    heard = false,
  ) => (
    <li key={entry.row} className="merope-mind__row">
      <time className="merope-mind__date">{day(entry.at, locale)}</time>
      <div className="merope-mind__text">
        {entry.text}
        <span className="merope-mind__tags is-inline">
          {saidTag(entry.saidBy)}
          {entry.seen && <Tag tone="muted">{copy.seenElsewhere}</Tag>}
          {entry.quoteOnly && <Tag tone="muted">{copy.quoteOnly}</Tag>}
        </span>
        <div className="merope-nodes__buttons">
          {heard ? (
            <>
              <button
                type="button"
                className="merope-nodes__action"
                disabled={busy}
                onClick={() =>
                  void edit(() =>
                    memoryNodesApi.answerHeard(
                      audience,
                      node,
                      entry.row,
                      'confirmed',
                    ),
                  )
                }
              >
                {copy.heardYes}
              </button>
              <button
                type="button"
                className="merope-nodes__action"
                disabled={busy}
                onClick={() =>
                  void edit(() =>
                    memoryNodesApi.answerHeard(
                      audience,
                      node,
                      entry.row,
                      'denied',
                    ),
                  )
                }
              >
                {copy.heardNo}
              </button>
            </>
          ) : (
            deleteButton(
              `entry:${entry.row}`,
              copy.deleteEntry,
              copy.deleteEntryTitle,
              copy.deleteEntryMessage,
              () => memoryNodesApi.forget(audience, node, entry.row),
            )
          )}
        </div>
      </div>
    </li>
  )

  const intentState = (intent: MemoryNodeIntent) => {
    const way = intent.instruction?.way
    if (way && way in copy.intent.way)
      return copy.intent.way[way as keyof Copy['intent']['way']]
    return (
      (copy.intent.state as Record<string, string>)[intent.status] ??
      intent.status
    )
  }

  const intentWhen = (intent: MemoryNodeIntent) => {
    const when = intent.when
    if (!when) return copy.intent.whenNone
    if (when.at) {
      return new Date(when.at / 1000).toLocaleString(locale, {
        month: 'numeric',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        timeZone: when.zone && !/^[+-]/.test(when.zone) ? when.zone : undefined,
      })
    }
    if (when.node) {
      return fill(
        when.change === 'done' ? copy.intent.whenDone : copy.intent.whenChanged,
        { name: when.node },
      )
    }
    return copy.intent.whenNone
  }

  const intentRow = (
    intent: MemoryNodeIntent,
    audience: string,
    node: string,
  ) => (
    <li
      key={intent.id}
      className={`merope-mind__row${intent.status === 'waiting' ? '' : ' is-ended'}`}
    >
      <time className="merope-mind__date">{day(intent.at, locale)}</time>
      <div className="merope-mind__text">
        {intent.text}
        <span className="merope-mind__tags is-inline">
          <Tag tone="muted">
            {intent.origin === 'partner'
              ? copy.intent.byYou
              : fill(copy.intent.byHer, { name })}
          </Tag>
          <Tag tone="muted">
            {intent.action === 'work' ? copy.intent.work : copy.intent.say}
          </Tag>
          <Tag tone={intent.status === 'waiting' ? 'accent' : 'muted'}>
            {intentState(intent)}
          </Tag>
        </span>
        <div className="merope-mind__why">{intentWhen(intent)}</div>
        {intent.instruction && (
          <dl className="merope-nodes__instruction">
            <dt>{copy.intent.asked}</dt>
            <dd>{intent.instruction.message ?? copy.deleted}</dd>
            <dt>{copy.intent.reply}</dt>
            <dd>{intent.instruction.echo}</dd>
            {intent.instruction.line ? (
              <>
                <dt>{copy.intent.result}</dt>
                <dd>{intent.instruction.line}</dd>
              </>
            ) : null}
          </dl>
        )}
        {intent.status === 'waiting' && (
          <div className="merope-nodes__buttons">
            <button
              type="button"
              className="merope-nodes__action"
              disabled={busy}
              onClick={() =>
                void edit(() =>
                  memoryNodesApi.cancelIntent(audience, node, intent.id),
                )
              }
            >
              {copy.intent.cancel}
            </button>
          </div>
        )}
      </div>
    </li>
  )

  const listOf = <T,>(items: T[], render: (item: T) => ReactNode) => (
    <ul className="merope-mind__list">{items.map(render)}</ul>
  )

  const texts = (items: { id: string; text: string; at: number }[]) =>
    listOf(items, (item) => (
      <li key={item.id} className="merope-mind__row">
        <time className="merope-mind__date">{day(item.at, locale)}</time>
        <div className="merope-mind__text">{item.text}</div>
      </li>
    ))

  const nodeView = (shown: MemoryNodePage) => {
    const { audience, id } = shown
    if (shown.gone) {
      return (
        <div className="merope-mind__body">
          <p className="merope-mind__status">
            {shown.movedTo ? copy.movedTo : copy.deleted}
          </p>
          {shown.movedTo && (
            <SettingsButton
              size="sm"
              onClick={() => setOpen({ audience, node: shown.movedTo! })}
            >
              {copy.openMoved}
            </SettingsButton>
          )}
        </div>
      )
    }
    const title = nodeName(shown, audience)
    return (
      <div className="merope-mind__body">
        <div className="merope-mind__entity-head">
          <h3>{title}</h3>
          <span className="merope-mind__tags is-inline">
            {kindTag(shown.kind)}
            {statusTag(shown.status)}
            {shown.namedByOwner && <Tag tone="accent">{copy.namedByYou}</Tag>}
          </span>
        </div>
        <Section
          title={copy.summary}
          empty={shown.summary.length === 0}
          emptyText={mind.empty}
        >
          {shown.summary.map((sentence, index) => (
            <p key={index} className="merope-nodes__summary">
              {sentence}
            </p>
          ))}
        </Section>
        <Section
          title={copy.views}
          empty={shown.views.length === 0}
          emptyText={mind.empty}
        >
          {texts(shown.views)}
        </Section>
        <Section
          title={copy.open}
          empty={shown.open.length === 0}
          emptyText={mind.empty}
        >
          {texts(shown.open)}
        </Section>
        <Section
          title={copy.entries}
          count={shown.entries.length}
          empty={shown.entries.length === 0}
          emptyText={mind.empty}
        >
          {listOf(shown.entries, (entry) => entryRow(entry, audience, id))}
        </Section>
        <Section
          title={copy.heard}
          count={shown.heard.length}
          empty={shown.heard.length === 0}
          emptyText={mind.empty}
        >
          {listOf(shown.heard, (entry) => entryRow(entry, audience, id, true))}
        </Section>
        <Section
          title={copy.spans}
          count={shown.spans.length}
          empty={shown.spans.length === 0}
          emptyText={mind.empty}
        >
          {listOf(shown.spans, (span) => (
            <li key={span.row} className="merope-mind__row">
              <time className="merope-mind__date">{day(span.at, locale)}</time>
              <div className="merope-mind__text">
                {span.text}
                {span.wordsGone && (
                  <div className="merope-mind__why">
                    {fill(copy.wordsGone, { name })}
                  </div>
                )}
              </div>
            </li>
          ))}
        </Section>
        <Section
          title={copy.intents}
          count={shown.intents.length}
          empty={shown.intents.length === 0}
          emptyText={mind.empty}
        >
          {listOf(shown.intents, (intent) => intentRow(intent, audience, id))}
        </Section>
        <Section title={copy.actions} emptyText={mind.empty}>
          <div className="merope-nodes__actions">
            {draft === null ? (
              <button
                type="button"
                className="merope-nodes__action"
                disabled={busy}
                onClick={() => setDraft(shown.name ?? '')}
              >
                {copy.rename}
              </button>
            ) : (
              <form
                className="merope-nodes__rename"
                onSubmit={(event) => {
                  event.preventDefault()
                  const next = draft
                  void edit(
                    () => memoryNodesApi.rename(audience, id, next),
                    () => setDraft(null),
                  )
                }}
              >
                <label>
                  <span>{copy.nameField}</span>
                  <input
                    value={draft}
                    maxLength={80}
                    onChange={(event) => setDraft(event.target.value)}
                  />
                </label>
                <span className="merope-mind__note">{copy.renameHint}</span>
                <div className="merope-nodes__buttons">
                  <SettingsButton
                    size="sm"
                    variant="primary"
                    type="submit"
                    loading={busy}
                    disabled={!draft.trim()}
                  >
                    {copy.save}
                  </SettingsButton>
                  <SettingsButton size="sm" onClick={() => setDraft(null)}>
                    {copy.cancel}
                  </SettingsButton>
                </div>
              </form>
            )}
            {shown.kind !== 'person' &&
              deleteButton(
                'node',
                copy.deleteNode,
                fill(copy.deleteNodeTitle, { name: title }),
                copy.deleteNodeMessage,
                () => memoryNodesApi.forget(audience, id),
                () => {
                  setOpen(null)
                  return true
                },
              )}
          </div>
        </Section>
      </div>
    )
  }

  return (
    <div className="merope-nodes">
      <div className="merope-nodes__toolbar">
        {open && (
          <button
            type="button"
            className="merope-mind__more"
            onClick={() => setOpen(null)}
          >
            {copy.back}
          </button>
        )}
        <SettingsButton
          size="sm"
          onClick={() => void (open ? loadPage(open) : loadList())}
        >
          {mind.refresh}
        </SettingsButton>
      </div>
      {failed && (
        <p className="merope-mind__status" role="alert">
          {mind.error}
        </p>
      )}
      {problem && (
        <p className="merope-mind__status" role="alert">
          {problem}
        </p>
      )}
      {open ? (
        page ? (
          nodeView(page)
        ) : (
          !failed && <p className="merope-mind__status">{mind.loading}</p>
        )
      ) : !list ? (
        !failed && <p className="merope-mind__status">{mind.loading}</p>
      ) : list.audiences.length === 0 ? (
        <p className="merope-mind__status">{copy.none}</p>
      ) : (
        <div className="merope-mind__body">
          {list.audiences.map((audience) => (
            <section
              key={audience.audience}
              className="merope-mind__section"
              data-audience={audience.audience}
            >
              <h4 className="merope-mind__heading">
                {audienceTitle(audience)}
                <span className="merope-mind__count">
                  {audience.nodes.length}
                </span>
              </h4>
              <ul className="merope-nodes__list">
                {audience.nodes.map((node) => line(node, audience.audience))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}
