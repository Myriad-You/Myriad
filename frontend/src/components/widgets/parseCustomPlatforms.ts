/**
 * Normalize ui_config.custom_platforms into an array.
 *
 * The field is stored as a JSON string. Historical writes can be "null",
 * a non-array object, or already-parsed JSON — JSON.parse("null") is null
 * and would crash later .find() / .map() calls.
 */
export function parseCustomPlatforms(raw: unknown): unknown[] {
  if (raw == null || raw === '') return []
  try {
    const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}
