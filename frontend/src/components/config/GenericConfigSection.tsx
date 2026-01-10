import React from 'react'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface GenericConfigSectionProps {
  title: string
  icon: string
  description: string
  fields: ConfigField[]
  onUpdateField: (fieldKey: string, value: string) => void
  providerField?: ConfigField
  providerOptions?: Array<{ value: string, label: string, icon: string }>
}

const GenericConfigSection = React.memo<GenericConfigSectionProps>(({
  title,
  icon,
  description,
  fields,
  onUpdateField,
  providerField,
  providerOptions,
}) => {
  const otherFields = providerField ? fields.filter(f => f.key !== providerField.key) : fields

  return (
    <div className="config-section">
      <div className="section-header">
        <div className="section-header-left">
          <span className="section-icon">{icon}</span>
          <div>
            <h2 className="section-title">{title}</h2>
            <p className="section-description">{description}</p>
          </div>
        </div>
      </div>

      <div className="config-form">
        {providerField && providerOptions && (
          <div className="form-group">
            <label className="form-label">
              {providerField.label}
              {providerField.required && <span className="required-mark">*</span>}
            </label>
            <div className="provider-selector">
              {providerOptions.map(option => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => onUpdateField(providerField.key, option.value)}
                  className={`provider-option ${providerField.value === option.value ? 'active' : ''}`}
                >
                  <span className="provider-icon">{option.icon}</span>
                  <span className="provider-name">{option.label}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {otherFields.map((field) => {
          if (field.field_type === 'number') {
            return (
              <div key={field.key} className="form-group">
                <label className="form-label">
                  {field.label}
                  {field.required && <span className="required-mark">*</span>}
                </label>
                <input
                  type="number"
                  value={field.value}
                  onChange={e => onUpdateField(field.key, e.target.value)}
                  placeholder={field.placeholder}
                  className="form-input"
                />
              </div>
            )
          }

          if (field.field_type === 'checkbox') {
            return (
              <div key={field.key} className="form-group">
                <label className="form-label-checkbox">
                  <input
                    type="checkbox"
                    checked={field.value === 'true'}
                    onChange={e => onUpdateField(field.key, e.target.checked ? 'true' : 'false')}
                    className="form-checkbox"
                  />
                  <span>{field.label}</span>
                </label>
              </div>
            )
          }

          return (
            <div key={field.key} className="form-group">
              <label className="form-label">
                {field.label}
                {field.required && <span className="required-mark">*</span>}
              </label>
              <input
                type={field.field_type === 'password' ? 'password' : 'text'}
                value={field.value}
                onChange={e => onUpdateField(field.key, e.target.value)}
                placeholder={field.placeholder}
                className="form-input"
                autoComplete="off"
              />
            </div>
          )
        })}
      </div>
    </div>
  )
})

GenericConfigSection.displayName = 'GenericConfigSection'

export default GenericConfigSection
