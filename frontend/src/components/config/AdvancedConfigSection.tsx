import React, { useState } from 'react';
import { useI18n } from '../../contexts/I18nContext';
import { FaTimes } from '@lib/icons';
import { SettingSection, SettingGroup, ButtonItem } from '../settings';

interface AdvancedConfigSectionProps {
  onReset: () => void;
  title: string;
  icon: React.ReactNode;
  description: string;
}

export const AdvancedConfigSection: React.FC<AdvancedConfigSectionProps> = ({
  onReset,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n();
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false);

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      <SettingGroup>
        <ButtonItem
          itemKey="reset_config"
          label={t.config.resetConfig}
          description={t.config.resetConfigDesc || 'Reset all configurations to default values. This action cannot be undone.'}
          buttonText={t.config.resetConfig}
          onClick={() => setResetConfirmOpen(true)}
          variant="danger"
          layout="horizontal"
        />
      </SettingGroup>

      {/* Reset Confirmation Modal */}
      {resetConfirmOpen && (
        <div className="modal-overlay" onClick={() => setResetConfirmOpen(false)}>
          <div className="modal-content modal-small" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="modal-title text-danger">{t.config.resetConfig}</h3>
              <button onClick={() => setResetConfirmOpen(false)} className="modal-close-button">
                <FaTimes />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-base text-gray-600 dark:text-gray-300">
                {t.config.resetConfirmMessage || 'Are you sure you want to reset all configurations? This action cannot be undone and will restore all settings to their default values.'}
              </p>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => setResetConfirmOpen(false)}
                className="btn-base btn-secondary"
              >
                {t.common.cancel}
              </button>
              <button
                onClick={() => {
                  onReset();
                  setResetConfirmOpen(false);
                }}
                className="btn-base btn-danger"
              >
                {t.common.confirm}
              </button>
            </div>
          </div>
        </div>
      )}
    </SettingSection>
  );
};
