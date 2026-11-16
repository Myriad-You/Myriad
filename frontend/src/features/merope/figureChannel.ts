import { FigureChannel } from '@myriad/figure'
import {
  markTurnTrace,
  noteTurnTraceDrop,
  noteTurnTraceFrame,
  noteTurnTraceLeaseExpiry,
} from './events/turnTrace'

// The one live figure this site hosts. Replies, speech, her state and typing
// reach it on this channel; what it did with a turn goes into the turn trace.
export const figureChannel = new FigureChannel({
  mark: markTurnTrace,
  drop: noteTurnTraceDrop,
  leaseExpiry: noteTurnTraceLeaseExpiry,
  frame: noteTurnTraceFrame,
})
