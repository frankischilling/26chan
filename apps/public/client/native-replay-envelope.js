// Numeric preflight for full-vocabulary worker preparation. No decompression,
// rendering, logical-history validation or publication authority lives here.
import { isReplayCandidate, prepareFullReplayCandidate, ReplayCandidateError } from './native-replay-data.js';
const duration = 86400000;
const steps = [.01, .05, .1, 100, .01, 100, .25, .1].map(Math.fround);
const unit = value => Number.isFinite(value) && value >= 0 && value <= 1;
const reject = () => { throw new ReplayCandidateError('numeric-envelope'); };

export function prepareBoundedReplayCandidate(candidate, provider) {
  // Check the private brand before touching caller fields or source constructors.
  if (!isReplayCandidate(candidate)) throw new ReplayCandidateError('candidate-brand');
  const m = candidate.metadata;
  if (m.endedAtSeconds < m.startedAtSeconds || m.endedAtSeconds - m.startedAtSeconds > duration / 1000) reject();
  for (let index = 0; index < candidate.tools.length; index++) {
    const tool = candidate.tools[index];
    if (tool.size < 1 || tool.size > 64 || !unit(tool.alpha) || !unit(tool.flow)
      || !Object.is(tool.step, steps[index])
      || tool.usePreserveAlpha !== [1,2,3,5].includes(tool.id)
      || (tool.id === 8 ? tool.tipId < 0 || tool.tipId > 2 : tool.tipId !== 0)) reject();
  }
  const events = candidate.events, first = events[0].timestampMs, last = events.at(-1).timestampMs;
  if (last < first || last - first > duration) reject();
  let previous = first, tool = m.toolId;
  for (const event of events) {
    if (event.timestampMs < previous) reject();
    previous = event.timestampMs;
    switch (event.tag) {
      case 10: tool = event.value; break;
      case 11: if (event.value < 1 || event.value > 64) reject(); break;
      case 12: case 18: case 27: if (!unit(event.value)) reject(); break;
      case 15: if (tool === 8 ? event.value > 2 : event.value !== 0) reject(); break;
      case 1: case 2: case 7: case 8:
        if (event.x < -m.width || event.x > 2 * m.width || event.y < -m.height || event.y > 2 * m.height) reject();
        break;
    }
  }
  return prepareFullReplayCandidate(candidate, provider);
}
