/**
 * A drawing the enhanced rig can go in without (the redrawn expressions, the
 * gesture arms): when it fails, the failure is told and not thrown, so the
 * split made beside it (a quarter of an hour of work) is still preflighted,
 * with the drawings there are.
 */
export function optionalDrawing(
  drawing: Promise<void> | undefined,
  failed: (reason: unknown) => void,
): Promise<void> {
  return drawing ? drawing.catch(failed) : Promise.resolve()
}
