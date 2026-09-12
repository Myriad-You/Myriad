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
  mono?: boolean
  copyText?: string
  copyable?: boolean
}

export interface InfoActionButton {
  key: string
  label: ReactNode
  onClick: (event?: React.MouseEvent<HTMLButtonElement>) => void
  disabled?: boolean
  loading?: boolean
  variant?: SettingsButtonVariant
  icon?: ReactNode
  confirm?: string
  title?: string
  ariaLabel?: string
}

export interface InfoActionCardProps {
  title?: ReactNode
  icon?: ReactNode
  fields?: InfoActionField[]
  emptyText?: ReactNode
  empty?: boolean
  children?: ReactNode
  actions?: InfoActionButton[]
  footer?: ReactNode
  preview?: ReactNode
  tone?: InfoActionCardTone
  copyable?: boolean
  copyLabel?: string
  copiedLabel?: string
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

function CardHeader({
  title,
  icon,
}: {
  title?: ReactNode
  icon?: ReactNode
}) {
  if ((title == null || title === '') && icon == null) return null
  return (
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
  )
}

function CardActions({ actions }: { actions: InfoActionButton[] }) {
  return (
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
  )
}

export const InfoActionCard = React.memo(({
  title,
  icon,
  fields,
  emptyText,
  empty = false,
  children,
  actions,
  footer,
  preview,
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
  const hasChildren = children != null && children !== false && children !== ''
  const showEmpty =
    empty || (!hasChildren && !hasFields && emptyText != null && emptyText !== '')
  const hasPreview = preview != null
  const hasActions = !!(actions && actions.length > 0)
  const header = <CardHeader title={title} icon={icon} />

  let body: ReactNode = null
  if (showEmpty) {
    body = (
      <div className="info-action-card-empty" role="status">
        {emptyText}
      </div>
    )
  } else if (hasChildren) {
    body = children
  } else if (hasFields) {
    body = (
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
    )
  }

  return (
    <div
      className={[
        'info-action-card',
        toneClass(tone),
        embedded ? 'is-embedded' : '',
        hasPreview ? 'has-preview' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {hasPreview ? null : header}

      <div
        className={`info-action-card-main${hasPreview ? ' has-preview' : ''}`}
      >
        {hasPreview ? (
          <div className="info-action-card-preview">{preview}</div>
        ) : null}
        <div className="info-action-card-body">
          {hasPreview ? header : null}
          {body}
          {hasPreview && hasActions ? <CardActions actions={actions!} /> : null}
        </div>
      </div>

      {!hasPreview && hasActions ? <CardActions actions={actions!} /> : null}

      {footer != null && (
        <div className="info-action-card-footer">{footer}</div>
      )}
    </div>
  )
})

export default InfoActionCard
