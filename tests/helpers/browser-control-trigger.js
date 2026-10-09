export const CONTROL_TRIGGER = '[owned-browser-control] primary-no-buffer-space';

export function validControlShard(shard) {
  return typeof shard === 'string' && shard.length === 1 && /^[1-8]$/.test(shard);
}

export function browserControlEnabled(platform, env) {
  return platform === 'win32' && env.WINDOWS_BROWSER_CONTROL === '1' &&
    env.WINDOWS_BROWSER_CONTROL_OWNED === '1' && env.VISUAL_FIXTURE_SERVER === '1' &&
    env.WINDOWS_VISUAL_NETLOG === '1' && env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS === '1' && validControlShard(env.THEME_SHARD);
}

// The coordinator has a second latch covering replacement test workers.
export function createBrowserControlTrigger(emit) {
  let emitted = false;
  return request => {
    if (emitted) return;
    try {
      if (request.failure()?.errorText !== 'net::ERR_NO_BUFFER_SPACE') return;
      const url = new URL(request.url());
      if (url.origin !== 'http://127.0.0.1:3000' || url.username || url.password) return;
      emitted = true;
      emit(CONTROL_TRIGGER + '\n');
    } catch { /* Collection cannot replace the original request failure. */ }
  };
}
