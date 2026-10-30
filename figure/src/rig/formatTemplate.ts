/** `{name}` interpolation with no locale catalog imports. Safe for workers. */
export function formatTemplate(
  template: string,
  params: Record<string, string | number> = {},
): string {
  if (!template) return ''
  return template.replaceAll(/\{(\w+)\}/g, (_, key: string) => {
    const value = params[key]
    return value == null ? `{${key}}` : String(value)
  })
}
