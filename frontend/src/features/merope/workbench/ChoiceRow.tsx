/** A labelled single choice among a few options, in the wardrobe drawer. */
export function ChoiceRow<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string
  value: T
  options: Array<{ value: T; label: string }>
  disabled: boolean
  onChange: (value: T) => void
}) {
  return (
    <>
      <p className="merope-wardrobe__drawer-label">{label}</p>
      <div className="merope-wardrobe__kinds" role="radiogroup" aria-label={label}>
        {options.map((option) => (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={value === option.value}
            className={`merope-wardrobe__family${value === option.value ? ' is-on' : ''}`}
            disabled={disabled}
            onClick={() => onChange(option.value)}
          >
            <span>{option.label}</span>
          </button>
        ))}
      </div>
    </>
  )
}
