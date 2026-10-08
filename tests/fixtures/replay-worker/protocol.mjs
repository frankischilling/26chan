// Bounded bookkeeping only. Node tests of this module make no native-pixel claim.
export class WorkerCommandGate {
  constructor() { this.generation = null; this.nextSequence = 1; this.inFlight = null; this.closed = false; }
  initialize(generation) {
    if (this.generation !== null || !Number.isSafeInteger(generation) || generation < 1) throw new Error('invalid initialization');
    this.generation = generation;
  }
  begin(message) {
    if (this.closed || this.generation === null || message.generation !== this.generation) throw new Error('invalid generation');
    if (this.inFlight !== null) throw new Error('frame acknowledgement required');
    if (message.sequence !== this.nextSequence || !['initial', 'step'].includes(message.type)) throw new Error('invalid command sequence');
    this.inFlight = message.sequence; this.nextSequence++;
  }
  acknowledge(message) {
    if (this.closed || this.generation === null || message.generation !== this.generation || message.sequence !== this.inFlight || this.inFlight === null) throw new Error('invalid acknowledgement');
    this.inFlight = null;
  }
  close() { this.closed = true; this.inFlight = null; }
}
export function exactMessage(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).sort().join(',') !== [...keys].sort().join(',')) throw new Error('invalid protocol record');
}
