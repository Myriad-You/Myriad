/**
 * 文本输入设置项组件
 */

import React, { useCallback, useState } from 'react';
import type { InputSettingConfig } from '../types';
import { FaCheck, FaCopy } from '@lib/icons';
import './SettingItem.css';

export interface InputItemProps extends Omit<InputSettingConfig, 'type'> {}

export const InputItem = React.memo<InputItemProps>(({
  itemKey,
  label,
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
}) => {
  const [isCopied, setIsCopied] = useState(false);

  const handleChange = useCallback((
    e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>
  ) => {
    if (!disabled && !loading) {
      let newValue = e.target.value;
      // 安全措施：如果新值包含掩码字符，移除掩码
      if (newValue.includes('••') || newValue.includes('**')) {
        newValue = newValue.replace(/[•*]+/g, '');
      }
      onChange(newValue);
    }
  }, [onChange, disabled, loading]);

  const handleFocus = useCallback((
    e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>
  ) => {
    // 如果是掩码值，自动选中全部内容
    if (autoSelectOnMask) {
      const isMasked = e.target.value === '••••••••' || e.target.value === '********';
      if (isMasked) {
        e.target.select();
      }
    }
    onFocus?.();
  }, [onFocus, autoSelectOnMask]);

  const handleCopy = useCallback(async () => {
    if (value) {
      await navigator.clipboard.writeText(value);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    }
  }, [value]);

  const id = `setting-input-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`;
  // 生成稳定的 name 属性，防止密码管理器识别
  const inputName = `myriad-setting-${itemKey || label.replace(/\s+/g, '-').toLowerCase()}`;

  const inputClassName = `field-input ${error ? 'has-error' : ''}`;

  return (
    <div
      className={`setting-item setting-item-input setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
    >
      <label htmlFor={id} className="setting-label">
        <span className="setting-label-text">
          {label}
          {required && <span className="required">*</span>}
        </span>
        {description && layout === 'vertical' && (
          <span className="setting-description">{description}</span>
        )}
      </label>

      <div className="setting-control">
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
              disabled={disabled || loading}
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
              disabled={disabled || loading}
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
              title={isCopied ? 'Copied!' : 'Copy'}
            >
              {isCopied ? <FaCheck /> : <FaCopy />}
            </button>
          )}
        </div>
        {error && <p className="setting-error">{error}</p>}
        {hint && !error && <p className="setting-hint">{hint}</p>}
      </div>
    </div>
  );
});

InputItem.displayName = 'InputItem';
