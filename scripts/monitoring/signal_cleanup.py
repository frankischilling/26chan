"""Cooperative SIGTERM handling for owned monitoring qualification processes."""

import signal

_termination_signal = None


def install_signal_cleanup():
    global _termination_signal
    _termination_signal = None

    def terminate(signum, _frame):
        # Exceptions raised here can be swallowed by an interrupted finalizer,
        # or interrupt resource registration/cleanup. Only record the request.
        global _termination_signal
        _termination_signal = signum

    signal.signal(signal.SIGTERM, terminate)


def check_signal_cleanup():
    """Unwind at a main-thread checkpoint after owned cleanup is registered."""
    if _termination_signal is not None:
        raise SystemExit(128 + _termination_signal)
