import type { IconType } from 'react-icons'

/**
 * Decorative-by-default a11y for react-icons SVGs.
 *
 * Simple Icons bake role="img" into SVG attrs. Without title/aria-label that
 * fails Lighthouse "svg-img-alt". Most call sites use icons next to visible
 * labels, so default to aria-hidden and strip role=img. When the caller
 * provides an accessible name (title / aria-label / aria-labelledby), keep
 * role="img". Explicit role / aria-hidden from the caller always win.
 */
export function withIconA11y(Icon: IconType): IconType {
  const AccessibleIcon: IconType = (props) => {
    const hasAccessibleName =
      (props.title != null && props.title !== '') ||
      (props['aria-label'] != null && props['aria-label'] !== '') ||
      (props['aria-labelledby'] != null && props['aria-labelledby'] !== '')

    const explicitAriaHidden = Object.hasOwn(props, 'aria-hidden')
    const explicitRole = Object.hasOwn(props, 'role')

    if (hasAccessibleName) {
      return (
        <Icon
          {...props}
          role={explicitRole ? props.role : 'img'}
          aria-hidden={explicitAriaHidden ? props['aria-hidden'] : undefined}
        />
      )
    }

    return (
      <Icon
        {...props}
        role={explicitRole ? props.role : undefined}
        aria-hidden={explicitAriaHidden ? props['aria-hidden'] : true}
      />
    )
  }

  return AccessibleIcon
}
