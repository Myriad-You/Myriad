import { installFigureHost } from '@myriad/figure/figureHost'
import { API_URL } from '../../../config'
import { currentCopy } from '../../../i18n/localeCopy'
import {
  markTurnTrace,
  noteTurnTraceDrop,
  noteTurnTraceFrame,
  noteTurnTraceLeaseExpiry,
} from '../events/turnTrace'

// The figure speaks the site's language, fetches from its API and reports
// what it did with a turn into the turn trace.
installFigureHost({
  copy: () => currentCopy().merope,
  apiUrl: () => API_URL,
  trace: {
    mark: markTurnTrace,
    drop: noteTurnTraceDrop,
    leaseExpiry: noteTurnTraceLeaseExpiry,
    frame: noteTurnTraceFrame,
  },
})
