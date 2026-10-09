// Native Windows ownership-test fixture. Never run outside the owned test job.
const fs = require('node:fs');
const { spawn } = require('node:child_process');
if (process.platform !== 'win32') process.exit(2);

function publish(name, value) {
  fs.writeFileSync(`${name}.tmp`, JSON.stringify(value), { flag: 'wx' });
  fs.renameSync(`${name}.tmp`, name);
}

if (process.argv[2] === 'child') {
  // This acknowledgement proves the descendant ran before the parent may exit.
  publish('child-ready.json', { schema: 1, pid: process.pid });
  setTimeout(() => process.exit(0), 30000);
} else if (process.argv[2] === 'parent') {
  // Node's non-detached Windows children join libuv's kill-on-parent-exit job.
  // Detached avoids that inner job; libuv does not request job breakaway, so
  // our enclosing Job Object still owns the descendant.
  const child = spawn(process.execPath, [__filename, 'child'], {
    detached: true,
    windowsHide: true,
    stdio: 'ignore',
  });
  child.on('error', () => process.exit(1));
  child.on('exit', () => process.exit(1));
  const deadline = Date.now() + 10000;
  let acknowledged = false;
  const poll = setInterval(() => {
    if (Date.now() >= deadline) process.exit(1);
    if (!acknowledged && fs.existsSync('child-ready.json')) {
      const ready = JSON.parse(fs.readFileSync('child-ready.json', 'utf8'));
      if (ready.schema !== 1 || ready.pid !== child.pid) process.exit(1);
      publish('parent-ready.json', { schema: 1, child_ready: true, pid: child.pid });
      acknowledged = true;
    }
    // PowerShell releases the parent only after checking live job membership.
    if (acknowledged && fs.existsSync('release-parent')) {
      clearInterval(poll);
      child.unref();
      process.exit(0);
    }
  }, 10);
} else {
  process.exit(2);
}
