import type {
  MindAlert,
  MindEntry,
  MindGroup,
  MindPace,
  MindPerson,
  MindSnapshot,
  MindSore,
  MindVitalsDay,
  MindVoiceWeek,
  MindWant,
} from '../services/agent'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { AGENT_MIND_PATH } from '../components/agent/settings/agentMindPath'
import AnimatedView from '../components/AnimatedView'
import { SegmentedControl, SettingGroup, SettingsButton } from '../components/settings'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { usePageSeo } from '../hooks/usePageSeo'
import { agentService } from '../services/agent'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'

type Tab = 'her' | 'people' | 'groups'

function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (whole, key: string) =>
    key in values ? String(values[key]) : whole,
  )
}

/**
 * Looking into her, for the site admin: who she has been lately, what she
 * wants and thinks, what she holds about each person and each group, and
 * how each of those changed. Read only; meant to be looked at a little each
 * week, since what she is for is presence over weeks.
 */
export default function AgentMind() {
  const navigate = useNavigate()
  const { t, locale } = useI18n()
  const copy = t.merope.mind
  const { isAdmin, isAuthenticated, hasChecked } = useAuth()
  const [tab, setTab] = useState<Tab>('her')
  const [mind, setMind] = useState<MindSnapshot | null>(null)
  const [loading, setLoading] = useState(false)
  const [failed, setFailed] = useState(false)

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: copy.title,
          path: AGENT_MIND_PATH,
          description: copy.desc,
        }),
      [copy],
    ),
  )

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

  useEffect(() => {
    if (!hasChecked) return
    if (!isAuthenticated) navigate('/login', { replace: true })
    else if (!isAdmin) navigate('/', { replace: true })
  }, [hasChecked, isAdmin, isAuthenticated, navigate])

  // Loaded once the viewer is known to be the admin; again only on refresh.
  useEffect(() => {
    if (hasChecked && isAuthenticated && isAdmin) void load()
  }, [hasChecked, isAuthenticated, isAdmin, load])

  const date = useCallback(
    (at?: string | null) => (at ? new Date(at).toLocaleDateString(locale) : ''),
    [locale],
  )
  const endedWhy = (why?: string | null) =>
    (why && (copy.ended as Record<string, string>)[why]) || copy.ended.other

  if (!hasChecked || !isAdmin) return null

  const empty = <p className="text-sm text-secondary">{copy.empty}</p>

  const history = (entries: MindEntry[]) =>
    entries.length === 0 ? (
      empty
    ) : (
      <ul className="space-y-2">
        {[...entries].reverse().map((entry, index) => (
          <li
            key={`${entry.at}-${index}`}
            className={`text-sm leading-relaxed ${entry.current ? '' : 'opacity-60'}`}
          >
            <span className="text-secondary mr-2">{date(entry.at)}</span>
            {entry.text}
            {!entry.current && (
              <span className="text-secondary ml-2">
                （{endedWhy(entry.endedWhy)}
                {entry.endedAt ? ` · ${date(entry.endedAt)}` : ''}）
              </span>
            )}
          </li>
        ))}
      </ul>
    )

  const wants = (list: MindWant[]) =>
    list.length === 0 ? (
      empty
    ) : (
      <ul className="space-y-3">
        {list.map((want, index) => (
          <li key={`${want.since}-${index}`} className="text-sm leading-relaxed">
            <div>
              {want.want}
              <span className="text-secondary ml-2">
                （{want.longing ? `${copy.longing} · ` : ''}
                {copy.reach[want.reach]} · {date(want.since)}）
              </span>
            </div>
            <div className="text-secondary">{want.why}</div>
            {want.notes.map((note, noteIndex) => (
              <div key={noteIndex} className="text-secondary pl-3">
                {date(note.at)} {note.note}
              </div>
            ))}
          </li>
        ))}
      </ul>
    )

  const sores = (list: MindSore[]) =>
    list.length === 0 ? (
      empty
    ) : (
      <ul className="space-y-2">
        {[...list].reverse().map((sore, index) => (
          <li
            key={`${sore.since}-${index}`}
            className={`text-sm leading-relaxed ${sore.status === 'open' ? '' : 'opacity-60'}`}
          >
            <span className="text-secondary mr-2">{date(sore.since)}</span>
            {sore.who ? <strong className="mr-1">{sore.who}：</strong> : null}
            {sore.what}
            <span className="text-secondary ml-2">
              （{copy.weight[sore.weight]} · {copy.where[sore.where]}
              {sore.mended ? ` · ${copy.mended}` : ''} ·{' '}
              {(copy.status as Record<string, string>)[sore.status] ?? endedWhy(sore.status)}
              {sore.endedAt ? ` ${date(sore.endedAt)}` : ''}）
            </span>
          </li>
        ))}
      </ul>
    )

  const bits = (list: { handle?: string | null; how: string; current: boolean }[]) =>
    list.length === 0 ? (
      empty
    ) : (
      <ul className="space-y-1">
        {list.map((bit, index) => (
          <li
            key={index}
            className={`text-sm leading-relaxed ${bit.current ? '' : 'opacity-60'}`}
          >
            {bit.handle ? <strong className="mr-1">{bit.handle}</strong> : null}
            {bit.how}
          </li>
        ))}
      </ul>
    )

  const number = (value?: number | null) => (value == null ? '—' : value.toFixed(2))

  const voice = (weeks: MindVoiceWeek[]) =>
    weeks.length === 0 ? (
      empty
    ) : (
      <table className="w-full text-sm">
        <thead className="text-secondary text-left">
          <tr>
            <th className="font-normal py-1">{copy.voice.week}</th>
            <th className="font-normal py-1">{copy.voice.lines}</th>
            <th className="font-normal py-1">{copy.voice.drift}</th>
            <th className="font-normal py-1">{copy.voice.peopleDrift}</th>
            <th className="font-normal py-1">{copy.voice.fromPeople}</th>
          </tr>
        </thead>
        <tbody>
          {[...weeks].reverse().map((week) => (
            <tr key={week.week}>
              <td className="py-1">{date(week.week)}</td>
              <td className="py-1">{week.lines}</td>
              <td className="py-1">{number(week.drift)}</td>
              <td className="py-1">{number(week.peopleDrift)}</td>
              <td className="py-1">{number(week.fromPeople)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    )

  const hours = (minutes: number) => (minutes / 60).toFixed(1)

  const paceView = (pace?: MindPace) =>
    !pace ? (
      empty
    ) : (
      <>
        <p className="text-sm text-secondary mb-2">
          {fill(copy.pace.usual, { hours: hours(pace.usualMinutes) })} · {copy.pace.tone[pace.tone]}
          {pace.daysPastUsual > 0
            ? ` · ${fill(copy.pace.pastDays, { count: pace.daysPastUsual })}`
            : ''}
        </p>
        <table className="w-full text-sm">
          <thead className="text-secondary text-left">
            <tr>
              <th className="font-normal py-1">{copy.pace.day}</th>
              <th className="font-normal py-1">{copy.pace.minutes}</th>
              <th className="font-normal py-1">{copy.pace.lazed}</th>
            </tr>
          </thead>
          <tbody>
            {[...pace.days].reverse().map((day) => (
              <tr key={day.day}>
                <td className="py-1">{date(day.day)}</td>
                <td className="py-1">{hours(day.minutes)}</td>
                <td className="py-1">{hours(day.lazed)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </>
    )

  const alertText = (alert: MindAlert) => {
    switch (alert.kind) {
      case 'unreadable':
        return fill(copy.vitals.alerts.unreadable, { count: alert.count })
      case 'failedCalls':
        return fill(copy.vitals.alerts.failedCalls, { failed: alert.failed, calls: alert.calls })
      case 'stopped':
        return fill(copy.vitals.alerts.stopped, { source: alert.source })
      case 'notesLeanOn':
        return fill(copy.vitals.alerts.notesLeanOn, { phrase: alert.phrase, percent: alert.percent })
      case 'repliesLeanOn':
        return fill(copy.vitals.alerts.repliesLeanOn, { phrase: alert.phrase, percent: alert.percent })
      case 'repliesAsking':
        return fill(copy.vitals.alerts.repliesAsking, { percent: alert.percent })
      case 'slowReplies':
        return fill(copy.vitals.alerts.slowReplies, { seconds: alert.seconds })
      case 'manyCalls':
        return fill(copy.vitals.alerts.manyCalls, { calls: alert.calls, usual: alert.usual })
    }
  }

  const leaning = (list: [string, number][]) =>
    list.map(([phrase, share]) => `「${phrase}」${Math.round(share * 100)}%`).join('、')

  const vitalsView = (days?: MindVitalsDay[]) => {
    if (!days || days.length === 0) return empty
    const latest = days[days.length - 1]
    const raised = [...days].reverse().flatMap((day) =>
      day.alerts.map((alert) => `${date(day.day)} ${alertText(alert)}`),
    )
    return (
      <>
        {raised.length > 0 && (
          <ul className="text-sm space-y-1 mb-3" style={{ color: 'var(--color-danger, #d9534f)' }}>
            {raised.map((line, index) => (
              <li key={index}>{line}</li>
            ))}
          </ul>
        )}
        <table className="w-full text-sm">
          <thead className="text-secondary text-left">
            <tr>
              <th className="font-normal py-1">{copy.pace.day}</th>
              <th className="font-normal py-1">{copy.vitals.calls}</th>
              <th className="font-normal py-1">{copy.vitals.tokens}</th>
              <th className="font-normal py-1">{copy.vitals.things}</th>
              <th className="font-normal py-1">{copy.vitals.replies}</th>
            </tr>
          </thead>
          <tbody>
            {[...days].reverse().map((day) => (
              <tr key={day.day}>
                <td className="py-1">{date(day.day)}</td>
                <td className="py-1">
                  {day.calls}
                  {day.failedCalls > 0 ? ` (${day.failedCalls})` : ''}
                </td>
                <td className="py-1">{(day.inputTokens / 10000).toFixed(1)}</td>
                <td className="py-1">{day.things}</td>
                <td className="py-1">
                  {day.replies}
                  {day.replyP50 != null ? ` · ${day.replyP50}s` : ''}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="text-sm text-secondary mt-3 space-y-1">
          <div>
            {copy.vitals.landed}：
            {Object.entries(latest.landed)
              .map(([reaction, count]) => `${reaction} ${count}`)
              .join(' · ') || '—'}
          </div>
          <div>
            {copy.vitals.busiest}：{latest.busiest.map(([operation, calls]) => `${operation} ${calls}`).join(' · ') || '—'}
          </div>
          <div>
            {copy.vitals.notesLeanOn}：{leaning(latest.notesLeanOn) || '—'}
          </div>
          <div>
            {copy.vitals.repliesLeanOn}：{leaning(latest.repliesLeanOn) || '—'}
          </div>
          <div>
            {copy.vitals.repliesAsking}：{latest.repliesAsking != null ? `${Math.round(latest.repliesAsking * 100)}%` : '—'}
          </div>
        </div>
      </>
    )
  }

  const heading = (text: string) => <h4 className="text-sm font-semibold mt-4 mb-2">{text}</h4>

  const person = (who: MindPerson) => (
    <SettingGroup
      key={who.id}
      title={who.name || `#${who.id}`}
      description={[
        who.firstTalked ? fill(copy.person.firstTalked, { date: date(who.firstTalked) }) : '',
        fill(copy.person.daysTalked, { count: who.daysTalked }),
      ]
        .filter(Boolean)
        .join(' · ')}
      descriptionVisible
      toc={false}
    >
      {heading(copy.person.us)}
      {history(who.us)}
      {heading(copy.person.lands)}
      {history(who.lands ?? [])}
      {heading(copy.person.chatDays)}
      {history(
        (who.chatDays ?? []).map((day) => ({
          text: day.text,
          at: day.day ?? '',
          current: day.current,
        })),
      )}
      {heading(copy.person.sore)}
      {sores(who.sore)}
      {heading(copy.person.threads)}
      {history(
        who.threads.map((thread) => ({
          text: thread.about ? `${thread.about}：${thread.then}` : thread.then,
          at: thread.at,
          current: thread.current,
          endedWhy: thread.endedWhy,
        })),
      )}
      {heading(copy.person.bits)}
      {bits(who.bits)}
    </SettingGroup>
  )

  const group = (it: MindGroup) => (
    <SettingGroup key={it.venue} title={it.venue} toc={false}>
      {it.guesses && it.guesses.length > 0 && (
        <>
          {heading(copy.group.guesses)}
          <ul className="space-y-2">
            {it.guesses.map((guess, index) => (
              <li key={`${guess.at}-${index}`} className="text-sm leading-relaxed">
                <strong>
                  {fill(copy.guess.mightBe, {
                    stranger: guess.stranger ?? '?',
                    candidate: guess.candidate,
                  })}
                </strong>
                {guess.sure && (
                  <span className="text-secondary ml-2">（{copy.guess.sure[guess.sure]}）</span>
                )}
                <div className="text-secondary">{guess.why}</div>
              </li>
            ))}
          </ul>
        </>
      )}
      {heading(copy.group.days)}
      {history(
        it.days.map((day) => ({
          text: day.text,
          at: day.day ?? '',
          current: day.current,
        })),
      )}
      {heading(copy.group.lands)}
      {history(it.lands ?? [])}
      {heading(copy.group.bits)}
      {bits(it.bits)}
      {heading(copy.group.sore)}
      {sores(it.sore)}
    </SettingGroup>
  )

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <div className="max-w-3xl mx-auto space-y-6">
        <header className="space-y-2">
          <h1 className="text-2xl font-semibold">{copy.title}</h1>
          <p className="text-sm text-secondary">{copy.desc}</p>
          <div className="flex items-center gap-3">
            <SettingsButton size="sm" loading={loading} onClick={() => void load()}>
              {copy.refresh}
            </SettingsButton>
            {mind && (
              <span className="text-xs text-secondary">
                {fill(copy.updated, {
                  time: new Date(mind.generatedAt).toLocaleString(locale),
                })}
              </span>
            )}
          </div>
          <SegmentedControl<Tab>
            size="md"
            columns={3}
            value={tab}
            onChange={setTab}
            options={[
              { value: 'her', label: copy.tabs.her },
              { value: 'people', label: copy.tabs.people, count: mind?.people.length },
              { value: 'groups', label: copy.tabs.groups, count: mind?.groups.length },
            ]}
          />
        </header>

        {failed && <p className="text-sm">{copy.error}</p>}
        {!mind && !failed && <p className="text-sm text-secondary">{copy.loading}</p>}

        {mind && tab === 'her' && (
          <div className="space-y-4">
            <SettingGroup title={copy.her.selfStory} toc={false}>
              {history(mind.her.selfStory)}
            </SettingGroup>
            <SettingGroup title={copy.her.wants} toc={false}>
              {wants(mind.her.wants)}
            </SettingGroup>
            <SettingGroup title={copy.her.wantsEnded} toc={false}>
              {history(mind.her.wantsEnded)}
            </SettingGroup>
            <SettingGroup title={copy.her.views} toc={false}>
              {history(mind.her.views)}
            </SettingGroup>
            <SettingGroup title={copy.her.questions} toc={false}>
              {history(mind.her.questions)}
            </SettingGroup>
            <SettingGroup title={copy.her.doing} toc={false}>
              {history(mind.her.doingThisWeek.map((done) => ({ ...done, current: true })))}
            </SettingGroup>
            <SettingGroup title={copy.her.vitals} description={copy.vitals.desc} descriptionVisible toc={false}>
              {vitalsView(mind.her.vitals)}
            </SettingGroup>
            <SettingGroup title={copy.her.pace} description={copy.pace.desc} descriptionVisible toc={false}>
              {paceView(mind.her.pace)}
            </SettingGroup>
            <SettingGroup title={copy.her.voice} description={copy.voice.desc} descriptionVisible toc={false}>
              {voice(mind.her.voice)}
            </SettingGroup>
            <SettingGroup title={copy.her.days} toc={false}>
              {history(mind.her.days)}
            </SettingGroup>
            <SettingGroup title={copy.her.corrected} toc={false}>
              {history(mind.her.corrected)}
            </SettingGroup>
          </div>
        )}

        {mind && tab === 'people' && (
          <div className="space-y-4">
            {mind.people.length === 0 ? (
              <p className="text-sm text-secondary">{copy.noPeople}</p>
            ) : (
              mind.people.map(person)
            )}
          </div>
        )}

        {mind && tab === 'groups' && (
          <div className="space-y-4">
            {mind.groups.length === 0 ? (
              <p className="text-sm text-secondary">{copy.noGroups}</p>
            ) : (
              mind.groups.map(group)
            )}
          </div>
        )}
      </div>
    </AnimatedView>
  )
}
