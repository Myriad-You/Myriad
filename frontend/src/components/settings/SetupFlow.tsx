import type { ReactNode } from 'react'
import { FaExternalLinkAlt } from '@lib/icons'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { SettingsButton } from './items/SettingsButton'
import './SetupFlow.css'

export interface SetupFlowStep {
  key: string
  title: ReactNode
  description?: ReactNode
  href?: string
  onAction?: () => void
  actionLabel?: ReactNode
  optional?: boolean
  done?: boolean
  icon?: ReactNode
}

export interface SetupFlowProps {
  steps: SetupFlowStep[]
  title?: ReactNode
  optionalLabel?: ReactNode
  className?: string
  orientation?: 'auto' | 'horizontal' | 'vertical'
}

export const SetupFlow: React.FC<SetupFlowProps> = ({
  steps,
  title,
  optionalLabel: optionalLabelProp,
  className = '',
  orientation = 'auto',
}) => {
  const { t } = useI18n()
  const optionalLabel = optionalLabelProp ?? t.common.optional
  const defaultActionLabel = t.common.open
  if (!steps.length) return null

  const orientClass =
    orientation === 'horizontal'
      ? 'is-horizontal'
      : orientation === 'vertical'
        ? 'is-vertical'
        : 'is-auto'

  return (
    <section
      className={['setup-flow', orientClass, className].filter(Boolean).join(' ')}
      aria-label={typeof title === 'string' ? title : undefined}
    >
      {title != null && title !== false && title !== '' ? (
        <h3 className="setup-flow-title">{title}</h3>
      ) : null}

      <ol className="setup-flow-track">
        {steps.map((step, index) => {
          const hasAction = Boolean(step.href || step.onAction)
          const actionLabel = step.actionLabel
          const isLast = index === steps.length - 1

          return (
            <li
              key={step.key}
              className={[
                'setup-flow-step',
                step.done ? 'is-done' : '',
                step.optional ? 'is-optional' : '',
              ]
                .filter(Boolean)
                .join(' ')}
            >
              <div className="setup-flow-step-card">
                <div className="setup-flow-step-head">
                  <span className="setup-flow-step-index" aria-hidden>
                    {step.icon ?? index + 1}
                  </span>
                  <div className="setup-flow-step-head-text">
                    <span className="setup-flow-step-title">{step.title}</span>
                    {step.optional ? (
                      <span className="setup-flow-step-optional">
                        {optionalLabel}
                      </span>
                    ) : null}
                  </div>
                </div>

                {step.description != null &&
                step.description !== false &&
                step.description !== '' ? (
                  <p className="setup-flow-step-desc">{step.description}</p>
                ) : null}

                {hasAction ? (
                  <div className="setup-flow-step-actions">
                    {step.href ? (
                      <SettingsButton
                        variant="secondary"
                        size="sm"
                        icon={<FaExternalLinkAlt />}
                        onClick={() =>
                          window.open(step.href, '_blank', 'noopener,noreferrer')
                        }
                      >
                        {actionLabel ?? defaultActionLabel}
                      </SettingsButton>
                    ) : (
                      <SettingsButton
                        variant="secondary"
                        size="sm"
                        onClick={() => step.onAction?.()}
                      >
                        {actionLabel ?? defaultActionLabel}
                      </SettingsButton>
                    )}
                  </div>
                ) : null}
              </div>

              {!isLast ? (
                <span className="setup-flow-connector" aria-hidden>
                  <span className="setup-flow-connector-line" />
                  <span className="setup-flow-connector-arrow" />
                </span>
              ) : null}
            </li>
          )
        })}
      </ol>
    </section>
  )
}

SetupFlow.displayName = 'SetupFlow'

export default SetupFlow
