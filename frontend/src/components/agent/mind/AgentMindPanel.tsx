import type { ReactNode } from 'react'
import type {
  MindAlert,
  MindDoing,
  MindEntry,
  MindGroup,
  MindPace,
  MindPerson,
  MindSnapshot,
  MindSore,
  MindTaste,
  MindVitalsDay,
  MindVoiceWeek,
  MindWant,
} from '../../../services/agent'
import { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { SegmentedControl, SettingsButton } from '../../settings'
import { fill, Fold, History, newestFirst, Section, shortDate, Tag } from './mindParts'
import './AgentMindPanel.css'

type Tab = 'her' | 'people' | 'groups' | 'signs'

/** How many of a long list show before the rest is asked for. */
const SHOWN = { story: 1, doing: 4, days: 3, list: 6, chat: 4, alerts: 0 }

/**
 * Looking into her, for the site admin: who she has been lately, what she
 * wants and thinks, what she holds about each person and each group, and
 * how each of those changed. Read only; meant to be looked at a little each
 * week, since what she is for is presence over weeks. A tab of her persona
 * workbench, which only the admin reaches.
 */
export default function AgentMindPanel() {
  const { t, locale } = useI18n()
  const copy = t.merope.mind
  const [tab, setTab] = useState<Tab>('her')
  const [mind, setMind] = useState<MindSnapshot | null>(null)
  const [loading, setLoading] = useState(false)
  const [failed, setFailed] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setFailed(false)
    try {
      setMind(await agentService.getMind())
    } catch {
      setFailed(true)
    } finally {
      setLoading(false)
    }
  }, [])

  // Loaded when the tab opens; again only on refresh.
  useEffect(() => {
    void load()
  }, [load])

  const date = (at?: string | null) => shortDate(at, locale)
  const endedWhy = (why?: string | null) =>
    (why && (copy.ended as Record<string, string>)[why]) || copy.ended.other
  const more = (hidden: number) => fill(copy.more, { count: hidden })
  const fold = { moreText: more, lessText: copy.less }

  const section = (
    title: string,
    size: number,
    body: () => ReactNode,
    extra: { note?: string; count?: boolean } = {},
  ) => (
    <Section
      title={title}
      note={extra.note}
      count={extra.count ? size : undefined}
      empty={size === 0}
      emptyText={copy.empty}
    >
      {size > 0 ? body() : null}
    </Section>
  )

  const row = (key: string, at: ReactNode, body: ReactNode, ended = false) => (
    <li key={key} className={`merope-mind__row${ended ? ' is-ended' : ''}`}>
      <time className="merope-mind__date">{at}</time>
      <div className="merope-mind__text">{body}</div>
    </li>
  )

  const entryRow = (entry: MindEntry, index: number) =>
    row(
      `${entry.at}-${index}`,
      date(entry.at),
      <>
        {entry.text}
        {!entry.current && (
          <Tag tone="muted">
            {endedWhy(entry.endedWhy)}
            {entry.endedAt ? ` · ${date(entry.endedAt)}` : ''}
          </Tag>
        )}
      </>,
      !entry.current,
    )

  const historyOf = <T,>(current: T[], ended: T[], limit: number, render: (item: T, index: number) => ReactNode) => (
    <History
      current={current}
      ended={ended}
      limit={limit}
      render={render}
      moreText={more}
      endedText={(count) => fill(copy.endedFold, { count })}
      lessText={copy.less}
    />
  )

  /** What still holds, newest first; what stopped holding behind a click. */
  const history = (entries: MindEntry[], limit = SHOWN.list) => {
    const sorted = newestFirst(entries, (entry) => entry.at)
    return historyOf(
      sorted.filter((entry) => entry.current),
      sorted.filter((entry) => !entry.current),
      limit,
      entryRow,
    )
  }

  const days = (list: { day?: string | null; text: string; current: boolean }[], limit: number) =>
    history(
      list.map((day) => ({ text: day.text, at: day.day ?? '', current: day.current })),
      limit,
    )

  const story = (entries: MindEntry[]) => {
    const sorted = newestFirst(entries, (entry) => entry.at)
    const [now, ...before] = sorted
    return (
      <>
        <blockquote className="merope-mind__story">
          <p>{now.text}</p>
          <footer>{date(now.at)}</footer>
        </blockquote>
        {before.length > 0 && (
          <Fold
            items={before}
            limit={0}
            render={entryRow}
            moreText={(hidden) => fill(copy.earlierFold, { count: hidden })}
            lessText={copy.less}
          />
        )}
      </>
    )
  }

  const want = (it: MindWant, index: number) => (
    <li key={`${it.since}-${index}`} className="merope-mind__card">
      <div className="merope-mind__card-title">{it.want}</div>
      <p className="merope-mind__why">{it.why}</p>
      <div className="merope-mind__tags">
        {it.longing && <Tag tone="accent">{copy.longing}</Tag>}
        <Tag>{copy.reach[it.reach]}</Tag>
        <Tag tone="muted">{fill(copy.since, { date: date(it.since) })}</Tag>
      </div>
      {it.notes.length > 0 && (
        <ul className="merope-mind__notes">
          {newestFirst(it.notes, (note) => note.at).map((note, noteIndex) =>
            row(`${note.at}-${noteIndex}`, date(note.at), note.note),
          )}
        </ul>
      )}
    </li>
  )

  const doingCopy = copy.doing
  const thingTitle = (thing: MindDoing['thing']) => {
    switch (thing.kind) {
      case 'song':
        return { title: thing.name, by: thing.artist, part: '' }
      case 'note':
        return { title: thing.title, by: '', part: '' }
      case 'chapter':
        return {
          title: thing.title,
          by: thing.author,
          part: thing.total
            ? fill(doingCopy.part, { index: thing.index + 1, total: thing.total })
            : doingCopy.unopened,
        }
      case 'inquiry':
        return { title: thing.question, by: '', part: '' }
    }
  }

  /** What its kind kept, a line each; what she heard in a song is measurements, not hers to read. */
  const keptLines = (kept: MindDoing['kept']) => {
    const lines: ReactNode[] = []
    const held = (tone: string, label: string) => (
      <b className={`merope-mind__held is-${tone}`}>{label}</b>
    )
    if (kept.guessed) {
      const guessed = kept.guessed
      lines.push(<><b>{guessed.remembered ? doingCopy.remembered : doingCopy.guessed}</b>{guessed.said}</>)
      lines.push(<>{held(guessed.held, doingCopy.held[guessed.held])}{guessed.happened}</>)
    }
    if (kept.explored) {
      const explored = kept.explored
      const compared = explored.compared
      lines.push(
        <>
          <b>{explored.sources.length === 0 && !compared ? doingCopy.fromMemory : doingCopy.thought}</b>
          {explored.thought}
        </>,
      )
      if (compared) {
        lines.push(
          <>
            {held(
              compared.answered,
              (doingCopy.answered as Record<string, string>)[compared.answered] ?? compared.answered,
            )}
            {compared.new ? `${doingCopy.newToHer}：${compared.new}` : null}
            {compared.alreadyKnown ? ` · ${doingCopy.knewIt}` : ''}
          </>,
        )
      }
    }
    if (kept.ended) lines.push(<b>{doingCopy.ended[kept.ended]}</b>)
    return lines
  }

  const doing = (item: MindDoing, index: number) => {
    // A backend from before things were sent as themselves sends only the line.
    if (!item.thing) return row(`${item.at}-${index}`, date(item.at), item.text)
    const { title, by, part } = thingTitle(item.thing)
    const kept = keptLines(item.kept ?? {})
    return (
      <li key={`${item.at}-${index}`} className="merope-mind__card">
        <div className="merope-mind__doing-head">
          <Tag>{doingCopy.kind[item.thing.kind]}</Tag>
          <span className="merope-mind__doing-title">
            「{title}」
            {by && <span className="merope-mind__muted"> {by}</span>}
            {part && <span className="merope-mind__muted"> · {part}</span>}
          </span>
          <span className="merope-mind__doing-side">
            {item.reaction && (
              <span className={`merope-mind__reaction is-${item.reaction}`}>
                {doingCopy.reaction[item.reaction]}
              </span>
            )}
            <time className="merope-mind__date">{date(item.at)}</time>
          </span>
        </div>
        <p className="merope-mind__said">{item.text}</p>
        {kept.length > 0 && (
          <ul className="merope-mind__kept">
            {kept.map((line, lineIndex) => (
              <li key={lineIndex}>{line}</li>
            ))}
          </ul>
        )}
      </li>
    )
  }

  const sore = (it: MindSore, index: number) =>
    row(
      `${it.since}-${index}`,
      date(it.since),
      <>
        {it.who ? <strong>{it.who}：</strong> : null}
        {it.what}
        <span className="merope-mind__tags is-inline">
          <Tag tone={it.weight === 'petty' ? undefined : 'warn'}>{copy.weight[it.weight]}</Tag>
          <Tag tone="muted">{copy.where[it.where]}</Tag>
          {it.mended && <Tag tone="muted">{copy.mended}</Tag>}
          <Tag tone={it.status === 'open' ? 'warn' : 'muted'}>
            {(copy.status as Record<string, string>)[it.status] ?? endedWhy(it.status)}
            {it.endedAt ? ` · ${date(it.endedAt)}` : ''}
          </Tag>
        </span>
      </>,
      it.status !== 'open',
    )

  const sores = (list: MindSore[]) => {
    const sorted = newestFirst(list, (it) => it.since)
    return historyOf(
      sorted.filter((it) => it.status === 'open'),
      sorted.filter((it) => it.status !== 'open'),
      SHOWN.list,
      sore,
    )
  }

  const bits = (list: { handle?: string | null; how: string; current: boolean }[]) => (
    <Fold
      items={[...list].sort((left, right) => Number(right.current) - Number(left.current))}
      limit={SHOWN.list}
      {...fold}
      render={(bit, index) => (
        <li key={index} className={`merope-mind__bit${bit.current ? '' : ' is-ended'}`}>
          {/* A handle the line already says is not said twice. */}
          {bit.handle && !bit.how.includes(bit.handle) ? (
            <>
              <strong>{bit.handle}</strong>
              <span className="merope-mind__muted">{bit.how}</span>
            </>
          ) : (
            bit.how
          )}
        </li>
      )}
    />
  )

  const number = (value?: number | null) => (value == null ? '—' : value.toFixed(2))
  const hours = (minutes: number) => (minutes / 60).toFixed(1)

  const table = (head: string[], rows: ReactNode[][]) => (
    <div className="merope-mind__table-wrap">
      <table className="merope-mind__table">
        <thead>
          <tr>
            {head.map((cell) => (
              <th key={cell}>{cell}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((cells, rowIndex) => (
            <tr key={rowIndex}>
              {cells.map((cell, cellIndex) => (
                <td key={cellIndex}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )

  const voice = (weeks: MindVoiceWeek[]) =>
    table(
      [copy.voice.week, copy.voice.lines, copy.voice.drift, copy.voice.peopleDrift, copy.voice.fromPeople],
      newestFirst(weeks, (week) => week.week).map((week) => [
        date(week.week),
        week.lines,
        number(week.drift),
        number(week.peopleDrift),
        number(week.fromPeople),
      ]),
    )

  const paceView = (pace: MindPace) => (
    <>
      <div className="merope-mind__tags">
        <Tag>{fill(copy.pace.usual, { hours: hours(pace.usualMinutes) })}</Tag>
        <Tag tone={pace.tone === 'flat' ? 'warn' : undefined}>{copy.pace.tone[pace.tone]}</Tag>
        {pace.daysPastUsual > 0 && (
          <Tag tone="accent">{fill(copy.pace.pastDays, { count: pace.daysPastUsual })}</Tag>
        )}
      </div>
      {table(
        [copy.pace.day, copy.pace.minutes, copy.pace.lazed],
        newestFirst(pace.days, (day) => day.day).map((day) => [date(day.day), hours(day.minutes), hours(day.lazed)]),
      )}
    </>
  )

  const tasteView = (taste: MindTaste) => {
    const names = (list: MindTaste['likedBy']) =>
      list.length === 0 ? (
        <span className="merope-mind__muted">—</span>
      ) : (
        <span className="merope-mind__tags is-inline">
          {list.map((by) => (
            <Tag key={`${by.kind}:${by.name}`}>{fill(copy.taste.by[by.kind], { name: by.name })}</Tag>
          ))}
        </span>
      )
    return (
      <dl className="merope-mind__pairs">
        <dt>{copy.taste.likedBy}</dt>
        <dd>{names(taste.likedBy)}</dd>
        <dt>{copy.taste.notForHer}</dt>
        <dd>{names(taste.notForHer)}</dd>
      </dl>
    )
  }

  const alertText = (alert: MindAlert) => {
    const alerts = copy.vitals.alerts
    switch (alert.kind) {
      case 'unreadable':
        return fill(alerts.unreadable, { count: alert.count })
      case 'failedCalls':
        return fill(alerts.failedCalls, { failed: alert.failed, calls: alert.calls })
      case 'stopped':
        return fill(alerts.stopped, { source: alert.source })
      case 'notesLeanOn':
        return fill(alerts.notesLeanOn, { phrase: alert.phrase, percent: alert.percent })
      case 'repliesLeanOn':
        return fill(alerts.repliesLeanOn, { phrase: alert.phrase, percent: alert.percent })
      case 'repliesAsking':
        return fill(alerts.repliesAsking, { percent: alert.percent })
      case 'openersLeanOn':
        return fill(alerts.openersLeanOn, { phrase: alert.phrase, percent: alert.percent })
      case 'proactiveUnanswered':
        return fill(alerts.proactiveUnanswered, { sent: alert.sent, answered: alert.answered })
      case 'slowReplies':
        return fill(alerts.slowReplies, { seconds: alert.seconds })
      case 'manyCalls':
        return fill(alerts.manyCalls, { calls: alert.calls, usual: alert.usual })
      case 'nothingLearned':
        return fill(alerts.nothingLearned, { days: alert.days })
    }
  }

  const leaning = (list: [string, number][]) =>
    list.map(([phrase, share]) => `「${phrase}」${Math.round(share * 100)}%`).join('、')

  const vitalsView = (list: MindVitalsDay[]) => {
    const sorted = newestFirst(list, (day) => day.day)
    const latest = sorted[0]
    const alertRow = (day: MindVitalsDay) => (alert: MindAlert, index: number) =>
      row(`${day.day}-${alert.kind}-${index}`, date(day.day), alertText(alert))
    const earlier = sorted.slice(1).flatMap((day) => day.alerts.map((alert, index) => alertRow(day)(alert, index)))
    const pairs: [string, ReactNode][] = [
      [copy.vitals.landed, Object.entries(latest.landed).map(([reaction, count]) => `${reaction} ${count}`).join(' · ')],
      [copy.vitals.busiest, latest.busiest.map(([operation, calls]) => `${operation} ${calls}`).join(' · ')],
      [copy.vitals.notesLeanOn, leaning(latest.notesLeanOn)],
      [copy.vitals.repliesLeanOn, leaning(latest.repliesLeanOn)],
      [copy.vitals.repliesAsking, latest.repliesAsking != null ? `${Math.round(latest.repliesAsking * 100)}%` : ''],
      [copy.vitals.openersLeanOn, leaning(latest.openersLeanOn ?? [])],
      [
        copy.vitals.proactive,
        fill(copy.vitals.proactiveCount, { sent: latest.proactive ?? 0, answered: latest.proactiveAnswered ?? 0 }),
      ],
      [copy.vitals.learned, latest.learned ?? ''],
      [
        copy.vitals.groups,
        (latest.groups ?? []).map((group) => fill(copy.vitals.groupsCount, group)).join(' · '),
      ],
    ]
    return (
      <>
        {latest.alerts.length > 0 && (
          <ul className="merope-mind__alerts">
            {latest.alerts.map((alert, index) => (
              <li key={`${alert.kind}-${index}`}>{alertText(alert)}</li>
            ))}
          </ul>
        )}
        {earlier.length > 0 && (
          <Fold
            items={earlier}
            limit={SHOWN.alerts}
            render={(item) => item}
            moreText={(hidden) => fill(copy.earlierAlerts, { count: hidden })}
            lessText={copy.less}
            className="merope-mind__list is-alerts"
          />
        )}
        {table(
          [copy.pace.day, copy.vitals.calls, copy.vitals.tokens, copy.vitals.things, copy.vitals.replies],
          sorted.map((day) => [
            date(day.day),
            <>
              {day.calls}
              {day.failedCalls > 0 && <span className="merope-mind__failed"> ({day.failedCalls})</span>}
            </>,
            (day.inputTokens / 10000).toFixed(1),
            day.things,
            `${day.replies}${day.replyP50 != null ? ` · ${day.replyP50}s` : ''}`,
          ]),
        )}
        <dl className="merope-mind__pairs">
          {pairs.map(([label, value]) => (
            <div key={label} className={value === '' ? 'is-blank' : undefined}>
              <dt>{label}</dt>
              <dd>{value === '' ? '—' : value}</dd>
            </div>
          ))}
        </dl>
      </>
    )
  }

  const entity = (
    key: string | number,
    title: ReactNode,
    meta: string,
    parts: [string, number, () => ReactNode][],
  ) => {
    const filled = parts.filter(([, size]) => size > 0)
    const none = parts.filter(([, size]) => size === 0).map(([name]) => name)
    return (
      <article key={key} className="merope-mind__entity">
        <header className="merope-mind__entity-head">
          <h3>{title}</h3>
          {meta && <span className="merope-mind__muted">{meta}</span>}
        </header>
        {filled.map(([name, size, body]) => (
          <Section key={name} title={name} count={size > 1 ? size : undefined} emptyText={copy.empty}>
            {body()}
          </Section>
        ))}
        {none.length > 0 && (
          <p className="merope-mind__none-of">{fill(copy.noneOf, { names: none.join('、') })}</p>
        )}
      </article>
    )
  }

  const person = (who: MindPerson) =>
    entity(
      who.id,
      who.name || `#${who.id}`,
      [
        who.firstTalked ? fill(copy.person.firstTalked, { date: date(who.firstTalked) }) : '',
        fill(copy.person.daysTalked, { count: who.daysTalked }),
      ]
        .filter(Boolean)
        .join(' · '),
      [
        [copy.person.us, who.us.length, () => history(who.us, 1)],
        [copy.person.lands, (who.lands ?? []).length, () => history(who.lands ?? [], 1)],
        [copy.person.threads, who.threads.length, () =>
          history(
            who.threads.map((thread) => ({
              text: thread.about ? `${thread.about}：${thread.then}` : thread.then,
              at: thread.at,
              current: thread.current,
              endedWhy: thread.endedWhy,
            })),
          )],
        [copy.person.sore, who.sore.length, () => sores(who.sore)],
        [copy.person.chatDays, (who.chatDays ?? []).length, () => days(who.chatDays ?? [], SHOWN.chat)],
        [copy.person.bits, who.bits.length, () => bits(who.bits)],
      ],
    )

  const venueTitle = (venue: string) => {
    const [platform, ...rest] = venue.split(':')
    return rest.length === 0 ? venue : (
      <>
        {rest.join(':')}
        <Tag tone="muted">{platform}</Tag>
      </>
    )
  }

  const group = (it: MindGroup) =>
    entity(it.venue, venueTitle(it.venue), '', [
      [copy.group.guesses, (it.guesses ?? []).length, () => (
        <ul className="merope-mind__list">
          {newestFirst(it.guesses ?? [], (guess) => guess.at).map((guess, index) =>
            row(
              `${guess.at}-${index}`,
              date(guess.at),
              <>
                <strong>
                  {fill(copy.guess.mightBe, { stranger: guess.stranger ?? '?', candidate: guess.candidate })}
                </strong>
                {guess.sure && <Tag tone="accent">{copy.guess.sure[guess.sure]}</Tag>}
                <div className="merope-mind__why">{guess.why}</div>
              </>,
            ),
          )}
        </ul>
      )],
      [copy.group.lands, (it.lands ?? []).length, () => history(it.lands ?? [], 1)],
      [copy.group.days, it.days.length, () => days(it.days, SHOWN.days)],
      [copy.group.sore, it.sore.length, () => sores(it.sore)],
      [copy.group.bits, it.bits.length, () => bits(it.bits)],
    ])

  const her = mind?.her

  return (
    <div className="merope-mind">
      <header className="merope-mind__header">
        <SegmentedControl<Tab>
          size="md"
          columns={4}
          value={tab}
          onChange={setTab}
          options={[
            { value: 'her', label: copy.tabs.her },
            { value: 'people', label: copy.tabs.people, count: mind?.people.length },
            { value: 'groups', label: copy.tabs.groups, count: mind?.groups.length },
            { value: 'signs', label: copy.tabs.signs },
          ]}
        />
        <div className="merope-mind__toolbar">
          {mind && (
            <span className="merope-mind__stamp">
              <span>{copy.updatedAt}</span>
              <time dateTime={mind.generatedAt}>
                {new Date(mind.generatedAt).toLocaleString(locale, {
                  month: 'numeric',
                  day: 'numeric',
                  hour: '2-digit',
                  minute: '2-digit',
                })}
              </time>
            </span>
          )}
          <SettingsButton size="sm" loading={loading} onClick={() => void load()}>
            {copy.refresh}
          </SettingsButton>
        </div>
      </header>

      {failed && <p className="merope-mind__status" role="alert">{copy.error}</p>}
      {!mind && !failed && <p className="merope-mind__status">{copy.loading}</p>}

      {her && tab === 'her' && (
        <div className="merope-mind__body">
          {section(copy.her.selfStory, her.selfStory.length, () => story(her.selfStory))}
          {section(copy.her.wants, her.wants.length, () => (
            <ul className="merope-mind__cards">{her.wants.map(want)}</ul>
          ), { count: true })}
          {section(copy.her.doing, her.doingThisWeek.length, () => (
            <Fold
              items={newestFirst(her.doingThisWeek, (item) => item.at)}
              limit={SHOWN.doing}
              render={doing}
              className="merope-mind__cards"
              {...fold}
            />
          ), { count: true })}
          {section(copy.her.days, her.days.length, () => history(her.days, SHOWN.days))}
          {section(copy.her.views, her.views.length, () => history(her.views))}
          {section(copy.her.questions, her.questions.length, () => history(her.questions))}
          {her.taste && (her.taste.likedBy.length > 0 || her.taste.notForHer.length > 0)
            ? section(copy.her.taste, 1, () => tasteView(her.taste!), { note: copy.taste.desc })
            : section(copy.her.taste, 0, () => null)}
          {section(copy.her.wantsEnded, her.wantsEnded.length, () => history(her.wantsEnded))}
          {section(copy.her.corrected, her.corrected.length, () => history(her.corrected))}
        </div>
      )}

      {her && tab === 'signs' && (
        <div className="merope-mind__body">
          {section(copy.her.vitals, her.vitals?.length ?? 0, () => vitalsView(her.vitals!), { note: copy.vitals.desc })}
          {section(copy.her.pace, her.pace ? 1 : 0, () => paceView(her.pace!), { note: copy.pace.desc })}
          {section(copy.her.voice, her.voice.length, () => voice(her.voice), { note: copy.voice.desc })}
        </div>
      )}

      {mind && tab === 'people' && (
        <div className="merope-mind__body">
          {mind.people.length === 0 ? (
            <p className="merope-mind__status">{copy.noPeople}</p>
          ) : (
            mind.people.map(person)
          )}
        </div>
      )}

      {mind && tab === 'groups' && (
        <div className="merope-mind__body">
          {mind.groups.length === 0 ? (
            <p className="merope-mind__status">{copy.noGroups}</p>
          ) : (
            mind.groups.map(group)
          )}
        </div>
      )}
    </div>
  )
}
