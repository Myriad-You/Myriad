export async function formatUserFacingError(
  reason: unknown,
  fallback?: string,
): Promise<string> {
  const { userFacingError } = await import('./userFacingError')
  return userFacingError(reason, fallback)
}
