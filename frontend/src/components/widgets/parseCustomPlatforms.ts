export function parseCustomPlatforms(raw: unknown): unknown[] {
  if (raw == null || raw === '') return []
  try {
    const parsed = typeof raw === 'string' ? JSON.parse(raw) : raw
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}
