import type { TranslationKeys } from '../../contexts/I18nContext'
import type { HerAlive, HerAliveEnd } from '../../services/agent/agentApi'
import { LuActivity } from '@lib/icons'

import { useEffect, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { InfoActionCard, SettingGroup } from '../settings'
import './AgentAliveGroup.css'

type Copy = TranslationKeys['config']
type CopyKey = Extract<keyof Copy, `agentAlive${string}`>

/** Each driver of her own life, by the name the site knows it by. */
const DRIVERS: Record<string, CopyKey> = {
  autonomy: 'agentAliveDriverAutonomy',
  'Work recovery': 'agentAliveDriverWorkRecovery',
  'memory jobs': 'agentAliveDriverMemoryJobs',
  'speech intents': 'agentAliveDriverSpeechIntents',
  'mind wandering': 'agentAliveDriverWandering',
  'her own time': 'agentAliveDriverOwnTime',
  'what they play': 'agentAliveDriverPlaying',
  'writing first': 'agentAliveDriverReach',
  'persona nights': 'agentAliveDriverNights',
  'her upkeep': 'agentAliveDriverUpkeep',
  'her intents': 'agentAliveDriverIntents',
  'scheduled tasks': 'agentAliveDriverScheduled',
  'what she takes in': 'agentAliveDriverTakesIn',
  'a look at sessions': 'agentAliveDriverSessionLook',
  "a session's change": 'agentAliveDriverSessionChange',
}

const SKIPPED: Record<string, CopyKey> = {
  moving: 'agentAliveWhyMoving',
  lives_elsewhere: 'agentAliveWhyElsewhere',
  vacant: 'agentAliveWhyElsewhere',
  moved_out: 'agentAliveWhyElsewhere',
  catching_up: 'agentAliveWhyCatchingUp',
  mind_elsewhere: 'agentAliveWhyMindElsewhere',
}

const SETTLED: Record<string, CopyKey> = {
  applied: 'agentAliveSettledApplied',
  no_change: 'agentAliveSettledNoChange',
  stale: 'agentAliveSettledStale',
}

const OUTCOMES: Record<string, CopyKey> = {
  asked: 'agentAliveOutcomeAsked',
  completed: 'agentAliveOutcomeCompleted',
  failed: 'agentAliveOutcomeFailed',
  cancelled: 'agentAliveOutcomeCancelled',
  timeout: 'agentAliveOutcomeTimeout',
  act_timeout: 'agentAliveOutcomeTimeout',
  ran_over: 'agentAliveOutcomeTimeout',
  act_off: 'agentAliveOutcomeOff',
  act_busy: 'agentAliveOutcomeBusy',
  act_unavailable: 'agentAliveOutcomeUnavailable',
  refused: 'agentAliveOutcomeRefused',
  not_picked_up: 'agentAliveOutcomeNotPickedUp',
  revoked: 'agentAliveOutcomeRevoked',
  host_frozen: 'agentAliveOutcomeAway',
  host_moved: 'agentAliveOutcomeAway',
  proposal_unknown: 'agentAliveOutcomeGone',
}

const CALLS: Record<string, CopyKey> = {
  'act.request': 'agentAliveCallAct',
}

/** What each capability an end may list is called on the page. */
const CAPS: Record<string, CopyKey> = {
  rig: 'agentAliveCapRig',
  "voice.out": 'agentAliveCapVoiceOut',
  "voice.in": 'agentAliveCapVoiceIn',
  notify: 'agentAliveCapNotify',
  panel: 'agentAliveCapPanel',
  pi: 'agentAliveCapPi',
  cua: 'agentAliveCapCua',
  "perception.window": 'agentAliveCapPerceptionWindow',
  "perception.pointer": 'agentAliveCapPerceptionPointer',
  "perception.music": 'agentAliveCapPerceptionMusic',
  "perception.screen": 'agentAliveCapPerceptionScreen',
  say: 'agentAliveCapSay',
  playback: 'agentAliveCapPlayback',
  touch: 'agentAliveCapTouch',
  wardrobe: 'agentAliveCapWardrobe',
  "wardrobe.list": 'agentAliveCapWardrobeList',
  experience: 'agentAliveCapExperience',
  conversations: 'agentAliveCapConversations',
  "conversations.live": 'agentAliveCapConversationsLive',
  proposals: 'agentAliveCapProposals',
  act: 'agentAliveCapAct',
  reminders: 'agentAliveCapReminders',
  moments: 'agentAliveCapMoments',
  images: 'agentAliveCapImages',
  lend: 'agentAliveCapLend',
  "now.sessions": 'agentAliveCapNowSessions',
  "now.choices": 'agentAliveCapNowChoices',
  cards: 'agentAliveCapCards',
  nodes: 'agentAliveCapNodes',
  streams: 'agentAliveCapStreams',
  calls: 'agentAliveCapCalls',
  glance: 'agentAliveCapGlance',
  versions: 'agentAliveCapVersions',
}

/**
 * Listed only to say which version of her log's sync an end speaks: shown
 * as that one version, not one by one.
 */
const SYNC_CAPS = /^(log\d+|log\.full|now\.sessions2|partner)$/

const REFRESH_MS = 15_000

/**
 * Whether she is living now, for the site's owner: her host, each driver of
 * her own life, her memory's line, the linked devices and what they asked of
 * each other. Times, counts, names and versions; nothing to turn on or off.
 */
export function AgentAliveGroup() {
  const { t, format, locale } = useI18n()
  const c = t.config
  const [alive, setAlive] = useState<HerAlive | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    let cancelled = false
    const read = () => {
      agentService
        .getHerAlive()
        .then((shown) => {
          if (cancelled) return
          setAlive(shown)
          setFailed(false)
        })
        .catch(() => {
          if (!cancelled) setFailed(true)
        })
    }
    read()
    const timer = window.setInterval(read, REFRESH_MS)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [])

  const now = alive ? new Date(alive.now).getTime() : 0
  const span = (seconds: number): string => {
    if (seconds < 90) {
      return format(c.agentAliveSeconds, { n: String(Math.max(0, Math.round(seconds))) })
    }
    if (seconds < 90 * 60) {
      return format(c.agentAliveMinutes, { n: String(Math.round(seconds / 60)) })
    }
    if (seconds < 36 * 3600) {
      return format(c.agentAliveHours, { n: String(Math.round(seconds / 3600)) })
    }
    return format(c.agentAliveDays, { n: String(Math.round(seconds / 86400)) })
  }
  const ago = (at: string | null): string => {
    if (!at) return c.agentAliveNever
    const seconds = (now - new Date(at).getTime()) / 1000
    if (seconds >= 36 * 3600) {
      return new Date(at).toLocaleString(locale || undefined)
    }
    return format(c.agentAliveAgo, { span: span(seconds) })
  }
  /** Capabilities by what they are called; a name not known is kept, marked. */
  const capabilities = (words: string[] | null | undefined): string => {
    if (words == null) return c.agentAliveUnknown
    const named = words
      .filter((word) => !SYNC_CAPS.test(word))
      .map((word) =>
        CAPS[word] ? c[CAPS[word]] : format(c.agentAliveCapUnknown, { name: word }),
      )
    if (words.some((word) => SYNC_CAPS.test(word))) {
      const version = Math.max(
        1,
        ...words.map((word) => Number(/^log(\d+)$/.exec(word)?.[1] ?? 0)),
      )
      named.push(format(c.agentAliveCapSyncVersion, { n: String(version) }))
    }
    return named.length ? named.join(' · ') : c.agentAliveNone
  }

  if (!alive) {
    return (
      <SettingGroup
        title={c.agentAliveTitle}
        icon={<LuActivity />}
        description={c.agentAliveDesc}
      >
        <p className="setting-hint">
          {failed ? c.agentAliveLoadFailed : c.agentAliveLoading}
        </p>
      </SettingGroup>
    )
  }

  const { life, host, memory } = alive
  const stopped =
    life.shouldLive &&
    life.quietSecs !== null &&
    life.quietSecs >= life.quietAfterSecs
  const verdict = !life.personaOn
    ? c.agentAliveOff
    : !life.shouldLive
      ? host.moving
        ? c.agentAliveMoving
        : c.agentAliveNotHere
      : life.background === 'failed'
        ? c.agentAliveDriversFailed
        : stopped
          ? format(c.agentAliveStopped, { span: span(life.quietSecs ?? 0) })
          : life.livedAt
            ? format(c.agentAliveLiving, { ago: ago(life.livedAt) })
            : c.agentAliveStarting
  const tone =
    stopped || (life.shouldLive && life.background === 'failed')
      ? 'danger'
      : life.shouldLive
        ? 'success'
        : 'muted'

  const end = (device: HerAliveEnd) => {
    const caps = device.capabilities
    return (
      <li key={device.id} className="agent-alive-end">
        <div className="agent-alive-end-head">
          <strong>{device.name}</strong>
          <span data-on={device.connected ? 'true' : 'false'}>
            {device.connected
              ? c.agentDevicesConnected
              : c.agentAliveNotConnected}
          </span>
        </div>
        <dl className="agent-alive-rows">
          <dt>{c.agentAliveLinkedAt}</dt>
          <dd>{ago(device.linkedAt ?? device.lastSeenAt)}</dd>
          <dt>{c.agentAliveVersion}</dt>
          <dd>
            {device.protocol === null
              ? c.agentAliveUnknown
              : [
                  device.app
                    ? `${device.app.name} ${device.app.version}`
                    : c.agentAliveAppUnsaid,
                  format(c.agentAliveProtocol, { n: String(device.protocol) }),
                  format(c.agentAliveCore, { n: String(device.coreAbi ?? 1) }),
                ].join(' · ')}
          </dd>
          <dt>{c.agentAliveDeclared}</dt>
          <dd>{capabilities(caps.declared)}</dd>
          <dt>{c.agentAliveApproved}</dt>
          <dd>
            {caps.approved === null
              ? c.agentAliveApprovedNone
              : capabilities(caps.approved)}
          </dd>
          <dt>{c.agentAliveGranted}</dt>
          <dd>{capabilities(caps.granted)}</dd>
          <dt>{c.agentAliveLog}</dt>
          <dd>
            {!device.log
              ? c.agentAliveLogUnheard
              : device.log.behindRows === 0
                ? format(c.agentAliveLogLevel, { ago: ago(device.log.heardAt) })
                : format(c.agentAliveLogBehind, {
                    rows: String(device.log.behindRows),
                    ago: ago(device.log.heardAt),
                  })}
          </dd>
          <dt>{c.agentAliveDropped}</dt>
          <dd>
            {device.dropped.length
              ? device.dropped.map((kind) => (
                  <div key={`${kind.t} ${kind.reason}`}>
                    {format(c.agentAliveDroppedItem, {
                      t: kind.t,
                      reason: kind.reason,
                      n: String(kind.links),
                      ago: ago(kind.lastAt),
                    })}
                  </div>
                ))
              : c.agentAliveNone}
          </dd>
          <dt>{c.agentAliveUnread}</dt>
          <dd>
            {device.unread.length
              ? device.unread.map((kind) => (
                  <div key={kind.t}>
                    {format(c.agentAliveUnreadItem, {
                      t: kind.t,
                      n: String(kind.count),
                      ago: ago(kind.lastAt),
                    })}
                  </div>
                ))
              : c.agentAliveNone}
          </dd>
        </dl>
      </li>
    )
  }

  return (
    <SettingGroup
      title={c.agentAliveTitle}
      icon={<LuActivity />}
      description={c.agentAliveDesc}
    >
      <InfoActionCard title={verdict} tone={tone}>
        <p className="info-action-card-lede">
          {host.holder === 'site'
            ? host.since
              ? format(c.agentAliveHostSiteSince, {
                  time: new Date(host.since).toLocaleString(locale || undefined),
                })
              : c.agentAliveHostSite
            : host.moving
              ? c.agentAliveHostMoving
              : c.agentAliveHostDevice}
          {life.gaps.count > 0
            ? ` ${format(c.agentAliveGaps, {
                count: String(life.gaps.count),
                span: span(life.gaps.secs),
              })}`
            : ` ${c.agentAliveNoGaps}`}
          {life.hostStops.count > 0
            ? ` ${format(c.agentAliveHostStops, {
                count: String(life.hostStops.count),
                span: span(life.hostStops.secs),
              })}`
            : null}
        </p>
      </InfoActionCard>

      <section className="agent-alive-part">
        <h4>{c.agentAliveLifeTitle}</h4>
        {life.drivers.length === 0 ? (
          <p className="setting-hint">{c.agentAliveStarting}</p>
        ) : (
          <table className="agent-alive-table">
            <thead>
              <tr>
                <th>{c.agentAliveColDriver}</th>
                <th>{c.agentAliveColRan}</th>
                <th>{c.agentAliveColEvery}</th>
                <th>{c.agentAliveColNote}</th>
              </tr>
            </thead>
            <tbody>
              {life.drivers.map((driver) => {
                const why = driver.skippedWhy
                  ? c[SKIPPED[driver.skippedWhy] ?? 'agentAliveWhyElsewhere']
                  : null
                return (
                  <tr key={driver.name} data-late={driver.overdue || why || driver.failures > 0 ? 'true' : 'false'}>
                    <td>{DRIVERS[driver.name] ? c[DRIVERS[driver.name]] : driver.name}</td>
                    <td>{ago(driver.ranAt)}</td>
                    <td>
                      {driver.everySecs
                        ? span(driver.everySecs)
                        : c.agentAliveEveryEvent}
                    </td>
                    <td>
                      {why
                        ? format(c.agentAliveSkipped, {
                            why,
                            n: String(driver.skips),
                          })
                        : driver.failures > 0 && driver.failedAt
                          ? format(c.agentAliveBroke, {
                              n: String(driver.failures),
                              ago: ago(driver.failedAt),
                            })
                          : driver.overdue
                            ? c.agentAliveOverdue
                            : driver.running
                              ? c.agentAliveRunning
                              : ''}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </section>

      <section className="agent-alive-part">
        <h4>{c.agentAliveMemoryTitle}</h4>
        <dl className="agent-alive-rows">
          <dt>{c.agentAliveSettled}</dt>
          <dd>
            {memory.settledAt
              ? `${ago(memory.settledAt)} · ${
                  c[SETTLED[memory.settledOutcome ?? ''] ?? 'agentAliveSettledFailed']
                }`
              : c.agentAliveNever}
          </dd>
          <dt>{c.agentAliveStanding}</dt>
          <dd>{ago(memory.standingWrittenAt)}</dd>
          <dt>{c.agentAliveStandingFailed}</dt>
          <dd>
            {memory.standingFailures
              ? format(c.agentAliveStandingFailures, {
                  unanswered: String(memory.standingFailures.unanswered),
                  unreadable: String(memory.standingFailures.unreadable),
                  notWritten: String(memory.standingFailures.not_written),
                  again: String(memory.standingFailures.asked_again),
                })
              : memory.standingFailed === 0
              ? c.agentAliveNone
              : format(c.agentAliveStandingFailedCount, {
                  n: String(memory.standingFailed),
                  ago: ago(memory.standingFailedAt),
                })}
          </dd>
        </dl>
      </section>

      <section className="agent-alive-part">
        <h4>{c.agentAliveEndsTitle}</h4>
        {alive.ends.length === 0 ? (
          <p className="setting-hint">{c.agentDevicesEmpty}</p>
        ) : (
          <ul className="agent-alive-ends">{alive.ends.map(end)}</ul>
        )}
      </section>

      <section className="agent-alive-part">
        <h4>{c.agentAliveCallsTitle}</h4>
        {alive.calls.length === 0 ? (
          <p className="setting-hint">{c.agentAliveCallsNone}</p>
        ) : (
          <dl className="agent-alive-rows">
            {alive.calls.map((call) => (
              <div key={call.kind} className="agent-alive-call">
                <dt>{CALLS[call.kind] ? c[CALLS[call.kind]] : call.kind}</dt>
                <dd>
                  {Object.entries(call.outcomes)
                    .map(
                      ([outcome, count]) =>
                        `${OUTCOMES[outcome] ? c[OUTCOMES[outcome]] : outcome} ${count}`,
                    )
                    .join(' · ')}
                </dd>
              </div>
            ))}
          </dl>
        )}
      </section>
    </SettingGroup>
  )
}
