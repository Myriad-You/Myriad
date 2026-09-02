import type { MotionChannel } from './channels'

/**
 * Body-semantic resources used by behavior planning and realization.
 *
 * The dotted names form a hierarchy: a future `body.arm` claim conflicts with
 * either arm, while `body.arm.left` and `body.arm.right` may run together.
 * MotionChannel remains the coarse production ownership compatibility layer.
 */
export type BehaviorResource =
  | 'face.mouth'
  | 'face.expression'
  | 'face.gaze'
  | 'body.head'
  | 'body.torso'
  | 'body.arm.left'
  | 'body.arm.right'
  | 'body.hand.left'
  | 'body.hand.right'
  | 'secondary.hair'
  | 'secondary.clothing'
  | 'secondary.bust'

export type BehaviorResourceGroup =
  | 'face'
  | 'face.mouth'
  | 'face.expression'
  | 'face.gaze'
  | 'body'
  | 'body.head'
  | 'body.torso'
  | 'body.arm'
  | 'body.arm.left'
  | 'body.arm.right'
  | 'body.hand'
  | 'body.hand.left'
  | 'body.hand.right'
  | 'secondary'
  | 'secondary.hair'
  | 'secondary.clothing'
  | 'secondary.bust'

export function resourceInGroup(
  resource: BehaviorResource,
  group: BehaviorResourceGroup,
): boolean {
  return resource === group || resource.startsWith(`${group}.`)
}

export function resourcesConflict(
  left: BehaviorResourceGroup,
  right: BehaviorResourceGroup,
): boolean {
  return (
    left === right ||
    left.startsWith(`${right}.`) ||
    right.startsWith(`${left}.`)
  )
}

/**
 * Projects a behavior's resources onto what this rig can actually drive.
 *
 * The dotted vocabulary anticipates a richer body; `rig/README.md` is the
 * boundary, and it lists no shoulder, elbow, wrist, leg or foot. Of the body
 * resources only head and torso reach a real transform, and this rig composes
 * them through one shared upper-body pose — so `headBody` is not a leftover
 * coarse channel, it is the granularity the artwork has.
 *
 * Splitting head from torso is the one refinement the rig could support (a nod
 * could then survive a groove). That changes arbitration outcomes per cue, so
 * it belongs in its own change with a running face to look at — not here.
 */
export function rigChannelsForResources(
  resources: readonly BehaviorResource[],
): MotionChannel[] {
  const channels = new Set<MotionChannel>()
  for (const resource of resources) {
    if (resource === 'face.mouth') channels.add('mouth')
    else if (resource === 'face.expression') channels.add('expression')
    else if (resource === 'face.gaze') channels.add('gaze')
    else channels.add('headBody')
  }
  return [...channels]
}

/** The inverse: the resources a rig channel stands for. */
export function resourcesForRigChannels(
  channels: readonly MotionChannel[],
): BehaviorResource[] {
  const resources = new Set<BehaviorResource>()
  for (const channel of channels) {
    if (channel === 'mouth') resources.add('face.mouth')
    if (channel === 'expression') resources.add('face.expression')
    if (channel === 'gaze') resources.add('face.gaze')
    if (channel === 'headBody') {
      resources.add('body.head')
      resources.add('body.torso')
      resources.add('body.arm.left')
      resources.add('body.arm.right')
    }
  }
  return [...resources]
}
