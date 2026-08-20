/**
 * AraelInput - 输入区域组件
 *
 * 现代化设计，SVG 图标按钮
 */

import React, { useEffect, useRef } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { Spinner } from '../../Spinner'

export interface AraelInputProps {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  onKeyDown: (e: React.KeyboardEvent) => void
  isLoading?: boolean
  /** 任务执行中仍允许提交转向指令 */
  allowSubmitWhileLoading?: boolean
  disabled?: boolean
  placeholder?: string
  autoFocus?: boolean
  isRecording?: boolean
  isProcessingVoice?: boolean
  onToggleRecording?: () => void
  onInterrupt?: () => void
  inputRef?: React.RefObject<HTMLInputElement | null>
}

export const AraelInput: React.FC<AraelInputProps> = ({
  value,
  onChange,
  onSubmit,
  onKeyDown,
  isLoading = false,
  allowSubmitWhileLoading = false,
  disabled = false,
  placeholder = '',
  autoFocus = false,
  isRecording = false,
  isProcessingVoice = false,
  onToggleRecording,
  onInterrupt,
  inputRef: externalInputRef,
}) => {
  const { t } = useI18n()
  const internalRef = useRef<HTMLInputElement>(null)
  const inputRef = externalInputRef || internalRef

  useEffect(() => {
    if (autoFocus && inputRef.current) {
      inputRef.current.focus()
    }
  }, [autoFocus, inputRef])

  const dynamicPlaceholder = isRecording
    ? t.arael.recording
    : isProcessingVoice
      ? t.arael.recognizing
      : placeholder || t.arael.inputPlaceholder

  return (
    <div className="arael-input-section">
      <div className="arael-input-wrapper">
        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={dynamicPlaceholder}
          maxLength={32680}
          className="arael-input"
          disabled={
            (isLoading && !allowSubmitWhileLoading) ||
            isRecording ||
            isProcessingVoice ||
            disabled
          }
        />
      </div>
      <div className="arael-input-actions">
        {onToggleRecording && (
          <button
            className={`arael-voice-btn${isRecording ? ' arael-voice-recording' : ''}${isProcessingVoice ? ' arael-voice-processing' : ''}`}
            onClick={onToggleRecording}
            disabled={isLoading || isProcessingVoice}
            title={
              isRecording
                ? t.arael.stopRecording
                : isProcessingVoice
                  ? t.arael.recognizing
                  : t.arael.voiceInput
            }
          >
            {isProcessingVoice ? (
              <Spinner size="xs" color="primary" />
            ) : (
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" x2="12" y1="19" y2="22" />
              </svg>
            )}
          </button>
        )}
        {isLoading && onInterrupt && (
          <button
            className="arael-send-btn arael-stop-btn"
            onClick={onInterrupt}
            title={t.arael.stopConversation}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
              <rect x="4" y="4" width="16" height="16" rx="2" />
            </svg>
            {t.arael.stop}
          </button>
        )}
        {(!isLoading || allowSubmitWhileLoading) && (
          <button
            className="arael-send-btn"
            onClick={onSubmit}
            disabled={!value.trim() || (isLoading && !allowSubmitWhileLoading)}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <line x1="12" y1="19" x2="12" y2="5" />
              <polyline points="5 12 12 5 19 12" />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}

export default AraelInput
