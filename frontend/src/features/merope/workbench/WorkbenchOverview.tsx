import type { ReactNode } from 'react'
import type { AgentPersona } from '../../../services/agent/agentApi'
import type { Addressee } from './useAddressee'
import { LuClock, LuRefreshCw, LuSparkles } from '@lib/icons'
import { useState } from 'react'
import { SettingsButton, ToggleSwitch } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'

interface Props {
  persona: AgentPersona | null
  status: { name: string; mood: string; activity: string }
  rows: Array<{ key: string; label: string; value: string }>
  sticker: { url: string | null; busy: boolean; make: () => Promise<void> }
  hasPortrait: boolean
  addressee: Addressee
  /** The names she had before, shown under hers while open; no button without it. */
  renderNameHistory?: (close: () => void) => ReactNode
}

/** Overview tab: sticker avatar, her status, and do-not-disturb. */
export function WorkbenchOverview({
  persona,
  status,
  rows,
  sticker,
  hasPortrait,
  addressee,
  renderNameHistory,
}: Props) {
  const { t } = useI18n()
  const [nameHistoryOpen, setNameHistoryOpen] = useState(false)
  if (!persona) {
    return <p className="merope-motion-home__help">{t.merope.overviewEmpty}</p>
  }
  const { doNotDisturb, dndStart, dndEnd, busy: dndBusy } = addressee
  return (
    <div className="merope-ob-persona-groups">
      <div className="merope-motion-avatar">
        <section
          className="merope-motion-avatar__pane"
          aria-label={t.merope.avatarTitle}
        >
          <div className="merope-motion-avatar__frame">
            {sticker.url ? (
              <img
                className="merope-motion-avatar__preview"
                src={siteMediaUrl(sticker.url)}
                alt={t.merope.avatarTitle}
                width={96}
                height={96}
                decoding="async"
              />
            ) : (
              <span className="merope-motion-avatar__preview is-empty" aria-hidden />
            )}
          </div>
          <div className="merope-motion-asset__actions">
            <SettingsButton
              type="button"
              size="sm"
              icon={sticker.url ? <LuRefreshCw /> : <LuSparkles />}
              disabled={sticker.busy || !hasPortrait}
              loading={sticker.busy}
              confirm={t.merope.avatarConfirm}
              title={hasPortrait ? undefined : t.merope.avatarNeedsPortrait}
              onClick={() => void sticker.make()}
            >
              {sticker.busy
                ? t.merope.avatarGenerating
                : sticker.url
                  ? t.merope.avatarRegenerate
                  : t.merope.avatarGenerate}
            </SettingsButton>
          </div>
        </section>
        <section
          className="merope-motion-avatar__pane"
          aria-label={t.merope.statusGroup}
        >
          <div className="merope-motion-avatar__field">
            <div className="merope-ob-persona-group__head">
              <h2 className="merope-ob-persona-group__title">
                {t.merope.overviewName}
              </h2>
              {renderNameHistory ? (
                <button
                  type="button"
                  className="merope-ob-persona-view__edit"
                  title={t.merope.personaHistory}
                  aria-label={`${t.merope.personaHistory} · ${t.merope.overviewName}`}
                  aria-expanded={nameHistoryOpen}
                  onClick={() => setNameHistoryOpen((open) => !open)}
                >
                  <LuClock aria-hidden />
                </button>
              ) : null}
            </div>
            <p className="merope-motion-avatar__value">{status.name}</p>
            {nameHistoryOpen && renderNameHistory
              ? renderNameHistory(() => setNameHistoryOpen(false))
              : null}
          </div>
          <div className="merope-motion-avatar__field">
            <h2 className="merope-ob-persona-group__title">
              {t.merope.statusGroup}
            </h2>
            <p className="merope-motion-avatar__value">
              {status.mood}
              <span aria-hidden> · </span>
              {status.activity}
            </p>
          </div>
        </section>
      </div>
      <section
        className="merope-ob-persona-group"
        aria-label={t.merope.overviewGroup}
      >
        <h2 className="merope-ob-persona-group__title">{t.merope.overviewGroup}</h2>
        <dl className="merope-ob-persona-view">
          {rows.map((row) => (
            <div key={row.key} className="merope-ob-persona-view__row">
              <div className="merope-ob-persona-view__copy">
                <dt>{row.label}</dt>
                <dd>{row.value}</dd>
              </div>
            </div>
          ))}
          <div className="merope-ob-persona-view__row">
            <div className="merope-ob-persona-view__copy">
              <dt>{t.merope.overviewDoNotDisturb}</dt>
              <dd>
                {doNotDisturb
                  ? t.merope.overviewOn
                  : persona.doNotDisturbActive
                    ? t.merope.overviewDndScheduled
                    : t.merope.overviewOff}
              </dd>
            </div>
            <ToggleSwitch
              checked={doNotDisturb}
              disabled={dndBusy}
              aria-label={t.merope.overviewDoNotDisturb}
              onChange={(next) => void addressee.saveDoNotDisturb(next)}
            />
          </div>
          <div className="merope-ob-persona-view__row merope-motion-overview__hours-row">
            <div className="merope-ob-persona-view__copy">
              <dt>{t.merope.overviewDndWindow}</dt>
              <dd className="merope-motion-overview__hours">
                <input
                  type="time"
                  className="merope-motion-overview__clock"
                  value={dndStart}
                  disabled={dndBusy}
                  aria-label={t.merope.overviewDndStart}
                  onChange={(event) => addressee.setDndStart(event.target.value)}
                  onBlur={addressee.commitHours}
                />
                <span aria-hidden>–</span>
                <input
                  type="time"
                  className="merope-motion-overview__clock"
                  value={dndEnd}
                  disabled={dndBusy}
                  aria-label={t.merope.overviewDndEnd}
                  onChange={(event) => addressee.setDndEnd(event.target.value)}
                  onBlur={addressee.commitHours}
                />
              </dd>
            </div>
          </div>
        </dl>
      </section>
    </div>
  )
}
