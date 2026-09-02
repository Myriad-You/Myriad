/**
 * 站点地址：InputItem clickToEdit + 域名 API / 运维清单
 */

import type {
  ChangeSiteDomainResponse,
  DomainChecklistItem,
} from '../../services/siteDomainApi'
import React, { useCallback, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  changeSiteDomain,
  checklistItems,
} from '../../services/siteDomainApi'
import { userFacingError } from '../../utils/userFacingError'
import { InputItem } from '../settings'
import './SiteUrlField.css'

export interface SiteUrlFieldProps {
  value: string
  /** 应用成功后同步展示（应 silent） */
  onApplied: (url: string) => void
}

function normalizeOrigin(url: string): string {
  return url.trim().replace(/\/$/, '')
}

export const SiteUrlField: React.FC<SiteUrlFieldProps> = ({
  value,
  onApplied,
}) => {
  const { t } = useI18n()
  const [error, setError] = useState<string | undefined>()
  const [resultOk, setResultOk] = useState<string | null>(null)
  const [applied, setApplied] = useState<
    ChangeSiteDomainResponse['applied'] | null
  >(null)
  const [checklist, setChecklist] = useState<DomainChecklistItem[]>([])

  const current = normalizeOrigin(value)

  /** 仅清错误（进编辑/取消时）；成功结果与清单保留到下次提交 */
  const clearError = useCallback(() => {
    setError(undefined)
  }, [])

  const checklistLabel = useCallback(
    (key: string) => {
      const map = t.config.domainChecklist as Record<string, string> | undefined
      return map?.[key] || key
    },
    [t.config.domainChecklist],
  )

  const onCommit = useCallback(
    async (nextRaw: string) => {
      const next = normalizeOrigin(nextRaw)
      if (!next) {
        const msg = t.config.domainChangeEmpty
        setError(msg)
        setResultOk(null)
        throw new Error(msg)
      }

      // 未改动：直接收起
      if (current && next === current) {
        setError(undefined)
        return
      }

      if (
        !window.confirm(t.config.domainChangeConfirm.replace('{origin}', next))
      ) {
        // 取消确认：保持编辑态，不写 error
        throw new Error('cancelled')
      }

      // 新一次提交：清掉上次结果
      setError(undefined)
      setResultOk(null)
      setChecklist([])
      setApplied(null)

      try {
        const res = await changeSiteDomain({
          new_origin: next,
          previous_origin: current || undefined,
        })
        // 200 正常返回；4xx/5xx 由 apiService 抛 ApiError
        if (res.success === false || !res.applied) {
          const fail = userFacingError(res.message, t.config.domainChangeFailed)
          setError(fail)
          throw new Error(fail)
        }

        onApplied(res.applied.base_url)
        setApplied(res.applied)
        setChecklist(checklistItems(res.checklist))
        setResultOk(t.config.domainChangeSuccess)
      } catch (err) {
        if (err instanceof Error && err.message === 'cancelled') throw err
        const message = userFacingError(err, t.config.domainChangeFailed)
        setError(message)
        throw err instanceof Error ? err : new Error(message)
      }
    },
    [current, onApplied, t],
  )

  const hasMeta = Boolean(resultOk || applied || checklist.length > 0)

  return (
    <div className="site-url-field">
      <InputItem
        itemKey="site-url"
        label={t.config.baseUrl}
        value={value}
        onChange={() => {
          // 编辑中输入时清掉上次错误，避免旧红字一直挂着
          clearError()
        }}
        variant="clickToEdit"
        inputType="url"
        placeholder={t.config.baseUrlPlaceholder}
        emptyLabel={t.config.siteUrlUnset}
        editLabel={t.config.siteUrlEdit}
        saveLabel={t.config.siteUrlSave}
        cancelLabel={t.config.siteUrlCancel}
        onCommit={onCommit}
        onEditStart={clearError}
        onEditCancel={clearError}
        error={error}
      />

      {hasMeta && (
        <div className="site-url-meta">
          {resultOk && (
            <p className="site-url-result is-ok" role="status">
              {resultOk}
            </p>
          )}

          {applied && (
            <div className="site-url-applied">
              <div className="site-url-applied-row">
                <span className="site-url-applied-key">
                  BASE_URL / FRONTEND_URL
                </span>
                <span className="site-url-applied-val">{applied.base_url}</span>
              </div>
              <div className="site-url-applied-row">
                <span className="site-url-applied-key">CORS_ORIGINS</span>
                <span className="site-url-applied-val">
                  {applied.cors_origins}
                </span>
              </div>
            </div>
          )}

          {checklist.length > 0 && (
            <div className="site-url-checklist-wrap">
              <p className="site-url-checklist-title">
                {t.config.domainChecklistTitle}
              </p>
              <ul className="site-url-checklist">
                {checklist.map((item) => {
                  const statusClass =
                    item.status === 'auto'
                      ? 'is-auto'
                      : item.status === 'manual'
                        ? 'is-manual'
                        : ''
                  return (
                    <li key={item.key}>
                      <span className="site-url-checklist-item-name">
                        {checklistLabel(item.key)}
                      </span>
                      <span
                        className={`site-url-checklist-item-status ${statusClass}`.trim()}
                      >
                        {item.status === 'auto'
                          ? t.config.domainStatusAuto
                          : item.status === 'manual'
                            ? t.config.domainStatusManual
                            : item.status}
                      </span>
                      {item.summary ? (
                        <span className="site-url-checklist-item-summary">
                          {item.summary}
                        </span>
                      ) : (
                        <span />
                      )}
                    </li>
                  )
                })}
              </ul>
              <p className="site-url-note">{t.config.domainFederationNote}</p>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export default React.memo(SiteUrlField)
