import { ArrowRight, Clock3, Database, ShieldCheck, X } from 'lucide-react'
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../contexts/I18nContext'
import {
  decideDataExchangeConsent,
  getDataExchangeConsentSnapshot,
  subscribeDataExchangeConsent,
} from '../runtime/DataExchangeConsent'
import './TappDataExchangeConsentHost.css'

const MAX_SCOPE_PREVIEW_CHARS = 2_000

function formatScope(params: unknown, emptyLabel: string): string {
  if (
    params == null ||
    (typeof params === 'object' &&
      !Array.isArray(params) &&
      Object.keys(params).length === 0)
  ) {
    return emptyLabel
  }
  try {
    const value = JSON.stringify(params, null, 2)
    if (!value) return emptyLabel
    return value.length > MAX_SCOPE_PREVIEW_CHARS
      ? `${value.slice(0, MAX_SCOPE_PREVIEW_CHARS)}…`
      : value
  } catch {
    return emptyLabel
  }
}

function formatLimit(maxBytes: number, maxRecords?: number): string {
  const bytes = `${Math.ceil(maxBytes / 1024)} KiB`
  return maxRecords ? `${maxRecords} / ${bytes}` : bytes
}

export function TappDataExchangeConsentHost() {
  const { t, format } = useI18n()
  const { current } = useSyncExternalStore(
    subscribeDataExchangeConsent,
    getDataExchangeConsentSnapshot,
    getDataExchangeConsentSnapshot,
  )
  const denyButtonRef = useRef<HTMLButtonElement>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  const [now, setNow] = useState(Date.now)

  const prepared = current?.prepared
  const scope = useMemo(
    () =>
      prepared ? formatScope(prepared.params, t.tapp.dataExchangeNoScope) : '',
    [prepared, t.tapp.dataExchangeNoScope],
  )
  const remainingSeconds = prepared
    ? Math.max(0, Math.ceil((Date.parse(prepared.expiresAt) - now) / 1000))
    : 0

  useEffect(() => {
    if (!prepared) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1_000)
    return () => window.clearInterval(timer)
  }, [prepared])

  useEffect(() => {
    if (!prepared) return
    const previouslyFocused = document.activeElement as HTMLElement | null
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    denyButtonRef.current?.focus()

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        decideDataExchangeConsent(prepared.requestId, false)
        return
      }
      if (event.key !== 'Tab' || !dialogRef.current) return
      const focusable = Iterator.from(dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        )).toArray()
      if (focusable.length === 0) {
        event.preventDefault()
        return
      }
      const first = focusable[0]
      const last = focusable.at(-1)!
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('keydown', handleKeyDown)
      document.body.style.overflow = previousOverflow
      previouslyFocused?.focus()
    }
  }, [prepared])

  if (!prepared || typeof document === 'undefined') return null

  const deny = () => decideDataExchangeConsent(prepared.requestId, false)
  const allow = () => decideDataExchangeConsent(prepared.requestId, true)

  return createPortal(
    <div className="tapp-data-consent-overlay" onMouseDown={deny}>
      <div
        ref={dialogRef}
        className="tapp-data-consent-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="tapp-data-consent-title"
        aria-describedby="tapp-data-consent-description"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <button
          type="button"
          className="tapp-data-consent-close"
          onClick={deny}
          aria-label={t.common.close}
        >
          <X aria-hidden="true" />
        </button>

        <div className="tapp-data-consent-heading">
          <span className="tapp-data-consent-icon">
            <ShieldCheck aria-hidden="true" />
          </span>
          <div>
            <h2 id="tapp-data-consent-title">
              {t.tapp.dataExchangeConsentTitle}
            </h2>
            <p id="tapp-data-consent-description">
              {t.tapp.dataExchangeConsentSubtitle}
            </p>
          </div>
        </div>

        <div
          className="tapp-data-consent-route"
          aria-label={t.tapp.dataExchangeRoute}
        >
          <div>
            <span>{t.tapp.dataExchangeRequester}</span>
            <strong>{prepared.requesterName}</strong>
          </div>
          <ArrowRight aria-hidden="true" />
          <div>
            <span>{t.tapp.dataExchangeProvider}</span>
            <strong>{prepared.providerName}</strong>
          </div>
        </div>

        <dl className="tapp-data-consent-details">
          <div>
            <dt>
              <Database aria-hidden="true" />
              {t.tapp.dataExchangeData}
            </dt>
            <dd>{prepared.exportDescription || prepared.exportId}</dd>
          </div>
          <div>
            <dt>{t.tapp.dataExchangePurpose}</dt>
            <dd>{prepared.purpose}</dd>
          </div>
          <div>
            <dt>{t.tapp.dataExchangeScope}</dt>
            <dd>
              <pre>{scope}</pre>
            </dd>
          </div>
          <div className="tapp-data-consent-meta">
            <span>
              {t.tapp.dataExchangeLimit}:{' '}
              {formatLimit(prepared.maxBytes, prepared.maxRecords)}
            </span>
            <span>
              <Clock3 aria-hidden="true" />
              {t.tapp.dataExchangeExpires}: {remainingSeconds}s
            </span>
          </div>
        </dl>

        {current.queuedCount > 0 && (
          <p className="tapp-data-consent-queued">
            {format(t.tapp.dataExchangeQueued, {
              count: current.queuedCount,
            })}
          </p>
        )}

        <p className="tapp-data-consent-once-hint">
          {t.tapp.dataExchangeOnceHint}
        </p>
        <div className="tapp-data-consent-actions">
          <button ref={denyButtonRef} type="button" onClick={deny}>
            {t.tapp.dataExchangeDeny}
          </button>
          <button type="button" className="primary" onClick={allow}>
            {t.tapp.dataExchangeAllowOnce}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}

export default TappDataExchangeConsentHost
