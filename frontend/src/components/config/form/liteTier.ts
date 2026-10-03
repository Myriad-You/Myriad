/** Lite has no switch: it is in use when its own model is filled in. */
export function liteInUse(getFieldValue: (key: string) => string): boolean {
  return getFieldValue('lite_ai_model').trim() !== ''
}
