/**
 * 文本输入设置项
 * - default：常规输入
 * - clickToEdit：只读展示 → 点击编辑 → 框内保存（复用 .field-input 壳）
 * - imageUpload：URL 输入 + 本地上传（data URL）+ 左侧预览
 */

import type { InputSettingConfig } from '../types'
import {
  FaCheck,
  FaCopy,
  FaEdit,
  FaImage,
  FaTimes,
  FaUpload,
  LuCheck,
} from '@lib/icons'
import React, { useCallback, useEffect, useId, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { guideDomProps } from '../guides/guideAnchor'
import { SettingDefaultChangeTag } from '../SettingDefaultChangeTag'
import { SettingFieldErrorTag } from '../SettingFieldErrorTag'
import { SettingTitleGuideEntry } from '../SettingTitleGuideEntry'
import { SettingsButton } from './SettingsButton'
import './SettingItem.css'

export interface InputItemProps extends Omit<InputSettingConfig, 'type'> {}

const DEFAULT_MAX_IMAGE_BYTES = 512 * 1024

function isDataImageUrl(v: string | undefined | null): boolean {
  return Boolean(v?.startsWith('data:image/'))
}

export const InputItem = React.memo<InputItemProps>(
  ({
    itemKey,
    label,
    guide,
    guidePath,
    description,
    hint,
    value,
    onChange,
    onFocus,
    onBlur,
    disabled = false,
    loading = false,
    required = false,
    error,
    size = 'md',
    layout = 'vertical',
    placeholder,
    inputType = 'text',
    multiline = false,
    rows = 3,
    autoComplete = 'one-time-code',
    autoSelectOnMask = true,
    copyable = false,
    className = '',
    variant = 'default',
    emptyLabel,
    editLabel,
    saveLabel,
    cancelLabel,
    onCommit,
    onEditStart,
    onEditCancel,
    accept = 'image/*',
    maxImageBytes = DEFAULT_MAX_IMAGE_BYTES,
    uploadLabel,
    clearImageLabel,
    localImageLabel,
    previewAlt,
    clearable = true,
    imageTypeError,
    imageSizeError,
    imageReadError,
  }) => {
    const { t } = useI18n()
    const [isCopied, setIsCopied] = useState(false)
    const [editing, setEditing] = useState(false)
    const [draft, setDraft] = useState(value)
    const [committing, setCommitting] = useState(false)
    const [uploadError, setUploadError] = useState<string | undefined>()
    const [previewBroken, setPreviewBroken] = useState(false)
    const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement>(null)
    const fileInputRef = useRef<HTMLInputElement>(null)
    const fileInputId = useId()

    const isClickToEdit = variant === 'clickToEdit'
    const isImageUpload = variant === 'imageUpload'
    const busy = loading || committing
    const displayValue = value?.trim() ?? ''
    const hasValue = Boolean(displayValue)
    const isLocalImage = isDataImageUrl(value)
    const shownError = error || (isImageUpload ? uploadError : undefined)

    useEffect(() => {
      if (!editing) setDraft(value)
    }, [value, editing])

    useEffect(() => {
      if (editing && isClickToEdit) {
        const el = inputRef.current
        el?.focus()
        if (el && 'select' in el) el.select()
      }
    }, [editing, isClickToEdit])

    useEffect(() => {
      setPreviewBroken(false)
      setUploadError(undefined)
    }, [value])

    const handleChange = useCallback(
      (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
        if (disabled || busy) return
        let newValue = e.target.value
        if (newValue.includes('••') || newValue.includes('**')) {
          newValue = newValue.replace(/[•*]+/g, '')
        }
        if (isImageUpload) {
          setUploadError(undefined)
        }
        if (isClickToEdit && editing) {
          setDraft(newValue)
          // 通知父级（可清 error）；不代表已提交
          onChange(newValue)
          return
        }
        onChange(newValue)
      },
      [busy, disabled, editing, isClickToEdit, isImageUpload, onChange],
    )

    const handleFocus = useCallback(
      (e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => {
        if (autoSelectOnMask) {
          const isMasked =
            e.target.value === '••••••••' || e.target.value === '********'
          if (isMasked) e.target.select()
        }
        onFocus?.()
      },
      [onFocus, autoSelectOnMask],
    )

    const handleCopy = useCallback(async () => {
      if (value) {
        await navigator.clipboard.writeText(value)
        setIsCopied(true)
        setTimeout(setIsCopied, 2000, false)
      }
    }, [value])

    const startEdit = useCallback(() => {
      if (disabled || busy) return
      setDraft(value ?? '')
      setEditing(true)
      onEditStart?.()
    }, [busy, disabled, onEditStart, value])

    const cancelEdit = useCallback(() => {
      setDraft(value ?? '')
      setEditing(false)
      onEditCancel?.()
    }, [onEditCancel, value])

    const commitEdit = useCallback(async () => {
      if (!onCommit || busy) return
      try {
        setCommitting(true)
        await onCommit(draft.trim())
        setEditing(false)
      } catch {
        // 保持编辑态，由父级通过 error / 外部反馈说明
      } finally {
        setCommitting(false)
      }
    }, [busy, draft, onCommit])

    const onKeyDown = useCallback(
      (e: React.KeyboardEvent) => {
        if (!isClickToEdit || !editing) return
        if (e.key === 'Enter' && !multiline) {
          e.preventDefault()
          void commitEdit()
        } else if (e.key === 'Escape') {
          e.preventDefault()
          cancelEdit()
        }
      },
      [cancelEdit, commitEdit, editing, isClickToEdit, multiline],
    )

    const handleFileChange = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0]
        // 允许重复选择同一文件
        e.target.value = ''
        if (!file || disabled || busy) return

        if (!file.type.startsWith('image/')) {
          setUploadError(
            imageTypeError || t.config.imageUploadTypeError,
          )
          return
        }
        if (file.size > maxImageBytes) {
          const kb = Math.round(maxImageBytes / 1024)
          setUploadError(
            imageSizeError ||
              t.config.imageUploadSizeError.replace('{kb}', String(kb)),
          )
          return
        }

        const reader = new FileReader()
        reader.onload = () => {
          const result = reader.result
          if (typeof result === 'string') {
            setUploadError(undefined)
            onChange(result)
          } else {
            setUploadError(imageReadError || t.config.imageUploadReadError)
          }
        }
        reader.onerror = () => {
          setUploadError(imageReadError || t.config.imageUploadReadError)
        }
        reader.readAsDataURL(file)
      },
      [
        busy,
        disabled,
        imageReadError,
        imageSizeError,
        imageTypeError,
        maxImageBytes,
        onChange,
        t.config.imageUploadReadError,
        t.config.imageUploadSizeError,
        t.config.imageUploadTypeError,
      ],
    )

    const triggerUpload = useCallback(() => {
      if (disabled || busy) return
      fileInputRef.current?.click()
    }, [busy, disabled])

    const clearImage = useCallback(() => {
      if (disabled || busy) return
      setUploadError(undefined)
      onChange('')
    }, [busy, disabled, onChange])

    const id = `setting-input-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`
    const inputName = `myriad-setting-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`
    const inputClassName = `field-input ${shownError ? 'has-error' : ''}`

    const renderDefaultControl = () => (
      <div className="input-wrapper">
        {multiline ? (
          <textarea
            id={id}
            name={inputName}
            value={value}
            onChange={handleChange}
            onFocus={handleFocus}
            onBlur={onBlur}
            placeholder={placeholder}
            rows={rows}
            disabled={disabled || busy}
            className={`${inputClassName} resizable-textarea`}
            autoComplete={autoComplete}
            data-form-type="other"
            data-lpignore="true"
          />
        ) : (
          <input
            id={id}
            name={inputName}
            type={inputType}
            value={value}
            onChange={handleChange}
            onFocus={handleFocus}
            onBlur={onBlur}
            placeholder={placeholder}
            disabled={disabled || busy}
            className={inputClassName}
            autoComplete={autoComplete}
            data-form-type="other"
            data-lpignore="true"
            data-1p-ignore="true"
          />
        )}
        {copyable && value && (
          <button
            type="button"
            className="copy-btn"
            onClick={handleCopy}
            title={isCopied ? t.common.copied : t.common.copy}
          >
            {isCopied ? <FaCheck /> : <FaCopy />}
          </button>
        )}
      </div>
    )

    const renderClickToEditControl = () => {
      if (!editing) {
        return (
          <button
            type="button"
            id={id}
            className={`${inputClassName} field-input--static`}
            onClick={startEdit}
            disabled={disabled || busy}
          >
            <span
              className={`field-input-static-text${hasValue ? '' : ' is-empty'}`}
            >
              {hasValue ? displayValue : emptyLabel || placeholder || '—'}
            </span>
            <span className="field-input-static-action" aria-hidden>
              <FaEdit />
              {editLabel || t.common.edit}
            </span>
          </button>
        )
      }

      return (
        <div
          className={`field-input field-input--composite${busy ? ' is-busy' : ''}${shownError ? ' has-error' : ''}`}
        >
          <input
            ref={inputRef as React.RefObject<HTMLInputElement>}
            name={inputName}
            type={inputType}
            value={draft}
            onChange={handleChange}
            onFocus={handleFocus}
            onKeyDown={onKeyDown}
            placeholder={placeholder}
            disabled={disabled || busy}
            className="field-input-inner"
            autoComplete="off"
            data-form-type="other"
            data-lpignore="true"
            data-1p-ignore="true"
            aria-label={label}
          />
          <div className="field-input-actions">
            <SettingsButton
              variant="primary"
              size="sm"
              loading={busy}
              disabled={disabled || busy || !draft.trim()}
              icon={<LuCheck size={14} aria-hidden />}
              onClick={() => void commitEdit()}
            >
              {saveLabel || t.common.save}
            </SettingsButton>
            <SettingsButton
              variant="ghost"
              size="sm"
              className="btn-icon"
              disabled={busy}
              icon={<FaTimes size={12} aria-hidden />}
              onClick={cancelEdit}
              aria-label={cancelLabel || t.common.cancel}
            />
          </div>
        </div>
      )
    }

    const renderImageUploadControl = () => {
      const showPreview = hasValue && !previewBroken
      const actionPadClass =
        hasValue && clearable
          ? 'field-input--image-actions-2'
          : 'field-input--image-actions-1'

      return (
        <div
          className={`field-input field-input--image ${actionPadClass}${busy ? ' is-busy' : ''}${shownError ? ' has-error' : ''}`}
        >
          <div
            className={`field-input-preview${showPreview ? '' : ' is-empty'}`}
            aria-hidden={!showPreview}
          >
            {showPreview ? (
              <img
                src={value}
                alt={previewAlt || label}
                className="field-input-preview-img"
                onError={() => setPreviewBroken(true)}
              />
            ) : (
              <FaImage className="field-input-preview-placeholder" />
            )}
          </div>

          {isLocalImage ? (
            <span
              id={id}
              className="field-input-local-label"
              title={localImageLabel || t.config.imageUploadLocal}
            >
              {localImageLabel || t.config.imageUploadLocal}
            </span>
          ) : (
            <input
              ref={inputRef as React.RefObject<HTMLInputElement>}
              id={id}
              name={inputName}
              type={inputType === 'password' ? 'text' : inputType}
              value={value}
              onChange={handleChange}
              onFocus={handleFocus}
              onBlur={onBlur}
              placeholder={placeholder}
              disabled={disabled || busy}
              className="field-input-inner"
              autoComplete="off"
              data-form-type="other"
              data-lpignore="true"
              data-1p-ignore="true"
              aria-label={label}
            />
          )}

          <div className="field-input-actions">
            <input
              ref={fileInputRef}
              id={fileInputId}
              type="file"
              accept={accept}
              className="field-input-file-hidden"
              disabled={disabled || busy}
              onChange={handleFileChange}
              tabIndex={-1}
              aria-hidden
            />
            <SettingsButton
              variant="secondary"
              size="sm"
              disabled={disabled || busy}
              loading={busy}
              icon={<FaUpload size={12} aria-hidden />}
              onClick={triggerUpload}
              aria-label={uploadLabel || t.config.imageUpload}
            >
              {uploadLabel || t.config.imageUpload}
            </SettingsButton>
            {hasValue && clearable && (
              <SettingsButton
                variant="ghost"
                size="sm"
                className="btn-icon"
                disabled={disabled || busy}
                icon={<FaTimes size={12} aria-hidden />}
                onClick={clearImage}
                aria-label={clearImageLabel || t.config.imageUploadClear}
                title={clearImageLabel || t.config.imageUploadClear}
              />
            )}
          </div>
        </div>
      )
    }

    const control = isClickToEdit
      ? renderClickToEditControl()
      : isImageUpload
        ? renderImageUploadControl()
        : renderDefaultControl()

    const anchorProps = guideDomProps(guidePath)

    return (
      <div
        {...anchorProps}
        className={`setting-item setting-item-input setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      >
        <label
          htmlFor={isClickToEdit && editing ? undefined : id}
          className="setting-label"
        >
          <span className="setting-label-text">
            {label}
            {required && <span className="required">*</span>}
            <SettingTitleGuideEntry title={label} guide={guide} />
            <SettingDefaultChangeTag
              fieldKey={itemKey}
              onApply={(next) => onChange(next)}
            />
            <SettingFieldErrorTag>{shownError}</SettingFieldErrorTag>
          </span>
          {description && layout === 'vertical' && (
            <span className="setting-description">{description}</span>
          )}
        </label>

        <div className="setting-control">
          {control}
          {hint && !shownError && <p className="setting-hint">{hint}</p>}
        </div>
      </div>
    )
  },
)

InputItem.displayName = 'InputItem'
