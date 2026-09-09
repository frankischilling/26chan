"""Supervise only child processes created by these owned Linux tests."""
import contextlib
import os
import signal
import subprocess


def cancel_test(signum, frame):
    # Repeated ordinary cancellation must not interrupt bounded cleanup.
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    raise KeyboardInterrupt('owned test cancelled')


@contextlib.contextmanager
def protected_cleanup(handlers):
    # All callers run on the main thread. Change both dispositions while blocked,
    # then discard ordinary cancellation during bounded cleanup and post-checks.
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, set(handlers))
    try:
        for sig in handlers:
            signal.signal(sig, signal.SIG_IGN)
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)
    try:
        yield
    finally:
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, set(handlers))
        try:
            for sig, handler in handlers.items():
                signal.signal(sig, handler)
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)


def run_owned(args, *, timeout, post_check=lambda: None, **kwargs):
    process = None
    handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, set(handlers))
        try:
            # Popen inherits the parent's blocked mask. GNU env resets and
            # unblocks these signals before exec, without a threaded preexec hook.
            process = subprocess.Popen(['/usr/bin/env', '--default-signal=INT,TERM', '--', *args],
                                       start_new_session=True, **kwargs)
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)
        stdout, stderr = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(args, process.returncode, stdout, stderr)
    finally:
        with protected_cleanup(handlers):
            try:
                if process is not None and process.poll() is None:
                    # The direct child unwinds its own managed descendants, including
                    # separate-session launch monitors. A group signal alone is not proof.
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.communicate(timeout=12)
                    except subprocess.TimeoutExpired as error:
                        with contextlib.suppress(ProcessLookupError):
                            os.killpg(process.pid, signal.SIGKILL)
                        process.communicate(timeout=5)
                        raise RuntimeError('forced test termination; inspect retained fixture and media runner state') from error
            finally:
                post_check()
