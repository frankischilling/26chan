// The owned matrix shares one loopback peer. Keep production's 30 writes/minute
// limit intact, including the browser's automatic CAPTCHA requests.
export function passWritePacer({ now = () => performance.now(), sleep = ms => new Promise(resolve => setTimeout(resolve, ms)) } = {}) {
  let started = null, used = 0, inCase = 0;
  return {
    async beginCase() {
      if (started !== null && now() - started >= 61000) { started = null; used = 0; }
      if (used + 12 > 30) {
        await sleep(Math.max(0, 61000 - (now() - started)));
        started = null; used = 0;
      }
      inCase = 0;
    },
    write() { started ??= now(); ++used; ++inCase; },
    endCase() {
      if (inCase > 12 || used > 30) throw new Error('Owned Pass case exceeded its reserved write budget.');
    },
  };
}
