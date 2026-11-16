import type { BackgroundRequirement } from '@myriad/tapp-runtime'
import { useState } from 'react'
import { I18nNamespace, useI18n } from '../contexts/I18nContext'
import {
  stopBackgroundResident,
  useBackgroundResidents,
} from '../tapp/runtime/backgroundResidentStore'
import { NotificationSourceIcon } from './notifications/NotificationIcons'

export function TappResidentNotice() {
  const residents = useBackgroundResidents()
  if (residents.length === 0) return null
  return (
    <I18nNamespace names={['tapp']}>
      <TappResidentNoticeBody residents={residents} />
    </I18nNamespace>
  )
}

function TappResidentNoticeBody({
  residents,
}: {
  residents: ReturnType<typeof useBackgroundResidents>
}) {
  const { t, format } = useI18n()
  const [expanded, setExpanded] = useState(false)

  const reasonLabels: Record<BackgroundRequirement, string> = {
    media: t.tapp.backgroundReasonMedia,
    sync: t.tapp.backgroundReasonSync,
    notification: t.tapp.backgroundReasonNotification,
    scheduler: t.tapp.backgroundReasonScheduler,
    'event-listener': t.tapp.backgroundReasonEventListener,
    realtime: t.tapp.backgroundReasonRealtime,
  }

  return (
    <section className="mb-2.5 overflow-hidden rounded-2xl border border-emerald-500/20 bg-emerald-500/7 dark:border-emerald-400/15 dark:bg-emerald-400/7">
      <button
        type="button"
        className="flex w-full items-center gap-2.5 px-2.5 py-2.5 text-left"
        aria-expanded={expanded}
        onClick={() => setExpanded((current) => !current)}
      >
        <span className="relative shrink-0">
          <NotificationSourceIcon
            source="tapp"
            className="h-10 w-10 object-contain"
          />
          <span
            className="absolute right-0 bottom-0 h-2.5 w-2.5 rounded-full border-2 border-white bg-emerald-500 dark:border-gray-900"
            aria-hidden
          />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-semibold text-gray-800 dark:text-gray-100">
            {format(t.tapp.residentRunning, { count: residents.length })}
          </span>
          <span className="mt-0.5 block text-xs text-gray-500 dark:text-gray-400">
            {t.tapp.residentHint}
          </span>
        </span>
        <svg
          className={`h-4 w-4 shrink-0 text-gray-400 transition-transform ${expanded ? 'rotate-180' : ''}`}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          viewBox="0 0 24 24"
          aria-hidden
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="m6 9 6 6 6-6" />
        </svg>
      </button>

      {expanded && (
        <div className="border-t border-emerald-500/15 px-2.5 py-1.5 dark:border-emerald-400/10">
          {residents.map((resident) => (
            <div
              key={resident.id}
              className="flex items-center gap-2 border-b border-black/5 py-2 last:border-b-0 dark:border-white/6"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-semibold text-gray-700 dark:text-gray-200">
                  {resident.name}
                </span>
                <span className="mt-0.5 block truncate text-[11px] text-gray-500 dark:text-gray-400">
                  {format(t.tapp.residentReason, {
                    reasons: resident.requirements
                      .map((requirement) => reasonLabels[requirement])
                      .join(' · '),
                  })}
                </span>
              </span>
              <button
                type="button"
                className="shrink-0 rounded-full bg-black/5 px-3 py-1 text-[11px] font-medium text-gray-600 transition-colors hover:bg-black/10 dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15"
                onClick={() => stopBackgroundResident(resident.id)}
              >
                {t.tapp.stopResident}
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  )
}
