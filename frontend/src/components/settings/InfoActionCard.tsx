/**
 * Generic info surface + action buttons for settings pages.
 *
 * Use for “show status / identity / summary, then act” panels
 * (e.g. federation keys, about version, export status).
 * Callers own data; this only provides chrome and layout.
 * Field rows support one-click copy (default on when text is available).
 */

import type { ReactNode } from 'react'
import type { SettingsButtonVariant } from './items/SettingsButton'
import { FaCheck, FaCopy } from '@lib/icons'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { SettingsButton } from './items/SettingsButton'
import './InfoActionCard.css'

export type InfoActionCardTone =
  | 'default'
  | 'info'
  | 'success'
  | 'warn'
  | 'danger'
  | 'muted'

export interface InfoActionField {
  key: string
  label: ReactNode
  value: ReactNode
  /** Monospace + break-all (URLs, key ids). */
  mono?: boolean
  /**
   * Text written to the clipboard. Defaults to string/number `value`.
   * Required for non-string ReactNode values if copy is enabled.
   */
  copyText?: string
  /**
   * Show a copy control on this row.
   * Default: true when `copyText` or a string/number `value` is available
   * (and the card has not disabled copy).
   */
  copyable?: boolean
}

export interface InfoActionButton {
  key: string
  label: ReactNode
  /** 可接收点击事件（用于锚定 popover / tooltip） */
  onClick: (event?: React.MouseEvent<HTMLButtonElement>) => void
  disabled?: boolean
  loading?: boolean
  variant?: SettingsButtonVariant
  /** Leading icon (passed to SettingsButton). */
  icon?: ReactNode
  /** Optional window.confirm before onClick. */
  confirm?: string
  /** Native tooltip / longer description. */
  title?: string
  ariaLabel?: string
}

export interface InfoActionCardProps {
  /** Optional heading inside the card. */
  title?: ReactNode
  icon?: ReactNode
  /** Structured label/value rows (preferred for identity-style data). */
  fields?: InfoActionField[]
  /**
   * Shown when there are no fields (or fields empty) and no `children`.
   * Also used when `empty` is true.
   */
  emptyText?: ReactNode
  /** Force empty state even if fields/children exist. */
  empty?: boolean
  /** Free-form body (used when not empty; takes precedence over fields). */
  children?: ReactNode
  /** Primary action row under the body. */
  actions?: InfoActionButton[]
  /** Extra footer slot (e.g. secondary links). */
  footer?: ReactNode
  tone?: InfoActionCardTone
  /**
   * Master switch for per-field copy controls. Default `true`.
   * Individual fields can still set `copyable={false}`.
   */
  copyable?: boolean
  /** Accessible name for copy buttons. Default “Copy”. */
  copyLabel?: string
  /** Title after a successful copy. Default “Copied”. */
  copiedLabel?: string
  /**
   * Nested layout (e.g. inside ManagedList expand): no card border/fill,
   * avoids double chrome with the parent surface.
   */
  embedded?: boolean
  className?: string
}

function toneClass(tone: InfoActionCardTone | undefined): string {
  return `info-action-card--${tone ?? 'default'}`
}

function resolveCopyText(field: InfoActionField): string | null {
  if (typeof field.copyText === 'string') {
    const t = field.copyText.trim()
    return t.length > 0 ? field.copyText : null
  }
  if (typeof field.value === 'string') {
    const t = field.value.trim()
    return t.length > 0 && field.value !== '—' ? field.value : null
  }
  if (typeof field.value === 'number' && Number.isFinite(field.value)) {
    return String(field.value)
  }
  return null
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    /* fall through */
  }
  try {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.setAttribute('readonly', '')
    ta.style.position = 'fixed'
    ta.style.left = '-9999px'
    document.body.appendChild(ta)
    ta.select()
    const ok = document.execCommand('copy')
    document.body.removeChild(ta)
    return ok
  } catch {
    return false
  }
}

const FieldRow = React.memo(({
  field,
  allowCopy,
  copyLabel,
  copiedLabel,
}: {
  field: InfoActionField
  allowCopy: boolean
  copyLabel: string
  copiedLabel: string
}) => {
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const text = resolveCopyText(field)
  const canCopy =
    allowCopy && field.copyable !== false && text != null && text !== ''

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [])

  const handleCopy = useCallback(async () => {
    if (!text) return
    const ok = await writeClipboard(text)
    if (!ok) return
    setCopied(true)
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(setCopied, 1600, false)
  }, [text])

  return (
    <div className="info-action-card-field">
      <dt className="info-action-card-field-label">{field.label}</dt>
      <dd
        className={[
          'info-action-card-field-value',
          field.mono ? 'is-mono' : '',
          canCopy ? 'has-copy' : '',
        ]
          .filter(Boolean)
          .join(' ')}
      >
        <span className="info-action-card-field-text">{field.value}</span>
        {canCopy && (
          <button
            type="button"
            className={`info-action-card-copy${copied ? ' is-copied' : ''}`}
            onClick={() => void handleCopy()}
            title={copied ? copiedLabel : copyLabel}
            aria-label={
              copied
                ? copiedLabel
                : `${copyLabel}${typeof field.label === 'string' ? `: ${field.label}` : ''}`
            }
          >
            {copied ? <FaCheck aria-hidden /> : <FaCopy aria-hidden />}
          </button>
        )}
      </dd>
    </div>
  )
})

export const InfoActionCard = React.memo(({
  title,
  icon,
  fields,
  emptyText,
  empty = false,
  children,
  actions,
  footer,
  tone = 'default',
  copyable = true,
  copyLabel: copyLabelProp,
  copiedLabel: copiedLabelProp,
  embedded = false,
  className = '',
}: InfoActionCardProps) => {
  const { t } = useI18n()
  const copyLabel = copyLabelProp ?? t.common.copy
  const copiedLabel = copiedLabelProp ?? t.common.copied
  const hasFields = !!(fields && fields.length > 0)
  const showEmpty =
    empty || (!children && !hasFields && emptyText != null && emptyText !== '')

  return (
    <div
      className={[
        'info-action-card',
        toneClass(tone),
        embedded ? 'is-embedded' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {(title != null && title !== '') || icon != null ? (
        <div className="info-action-card-header">
          {icon != null && (
            <span className="info-action-card-icon" aria-hidden>
              {icon}
            </span>
          )}
          {title != null && title !== '' && (
            <div className="info-action-card-title">{title}</div>
          )}
        </div>
      ) : null}

      <div className="info-action-card-body">
        {showEmpty ? (
          <div className="info-action-card-empty" role="status">
            {emptyText}
          </div>
        ) : children ?? hasFields ? (
          <dl className="info-action-card-fields">
            {fields!.map((f) => (
              <FieldRow
                key={f.key}
                field={f}
                allowCopy={copyable}
                copyLabel={copyLabel}
                copiedLabel={copiedLabel}
              />
            ))}
          </dl>
        ) : null}
      </div>

      {actions && actions.length > 0 && (
        <div className="info-action-card-actions" role="group">
          {actions.map((a) => (
            <SettingsButton
              key={a.key}
              variant={a.variant ?? 'secondary'}
              size="sm"
              disabled={a.disabled}
              loading={a.loading}
              icon={a.icon}
              confirm={a.confirm}
              title={a.title}
              aria-label={
                a.ariaLabel ??
                (typeof a.label === 'string' ? a.label : undefined)
              }
              onClick={a.onClick}
            >
              {a.label}
            </SettingsButton>
          ))}
        </div>
      )}

      {footer != null && (
        <div className="info-action-card-footer">{footer}</div>
      )}
    </div>
  )
})

export default InfoActionCard
