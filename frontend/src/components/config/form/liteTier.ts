/** Lite has no switch: it is in use when its own model is filled in. */
export function liteInUse(getFieldValue: (key: string) => string): boolean {
  const model =
    getFieldValue('lite_provider') === 'gemini'
      ? 'lite_gemini_model'
      : 'lite_openai_model'
  return getFieldValue(model).trim() !== ''
}
