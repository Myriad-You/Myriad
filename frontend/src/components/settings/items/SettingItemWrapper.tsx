import React from 'react';
import type { BaseSettingItemConfig } from '../types';
import './SettingItem.css';

export interface SettingItemWrapperProps extends Partial<BaseSettingItemConfig> {
  children: React.ReactNode;
  className?: string;
  id?: string;
  contentRight?: boolean;
}

export const SettingItemWrapper: React.FC<SettingItemWrapperProps> = ({
  label,
  description,
  hint,
  error,
  required,
  layout = 'vertical',
  size = 'md',
  className = '',
  id,
  children,
  contentRight = false,
  disabled = false,
}) => {
  const labelContent = label && (
    <div className="setting-label">
      <span className="setting-label-text">
        {label}
        {required && <span className="required">*</span>}
      </span>
      {description && (
        <span className="setting-description">{description}</span>
      )}
    </div>
  );

  if (layout === 'horizontal') {
    return (
      <div
        className={`setting-item setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
      >
        <div className="setting-item-content">
          {contentRight ? (
            <>
              {labelContent}
              <div className="setting-control">
                {children}
              </div>
            </>
          ) : (
            <>
              {labelContent}
              <div className="setting-control">
                {children}
              </div>
            </>
          )}
        </div>
        {hint && <p className="setting-hint">{hint}</p>}
        {error && <p className="setting-error">{error}</p>}
      </div>
    );
  }

  return (
    <div
      className={`setting-item setting-${layout} setting-${size} ${className} ${disabled ? 'disabled' : ''}`}
    >
      {label && (
        <label htmlFor={id} className="setting-label">
          <span className="setting-label-text">
            {label}
            {required && <span className="required">*</span>}
          </span>
          {description && (
            <span className="setting-description">{description}</span>
          )}
        </label>
      )}

      <div className="setting-control">
        {children}
      </div>

      {hint && <p className="setting-hint">{hint}</p>}
      {error && <p className="setting-error">{error}</p>}
    </div>
  );
};
