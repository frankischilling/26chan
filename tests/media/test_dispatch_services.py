#!/usr/bin/env python3
"""Exercise the candidate systemd profiles on an owned disposable Linux host."""
import os
import signal

from dispatch_service_fixture import SystemdExercise


if __name__ == '__main__':
    os.umask(0o077)

    def cancelled(_signal, _frame):
        raise KeyboardInterrupt('owned service qualification cancelled')

    signal.signal(signal.SIGTERM, cancelled)
    exercise = SystemdExercise()
    try:
        exercise.setup()
        exercise.service_boundaries()
        exercise.production_denial()
        exercise.exercise()
        exercise.retention_recovery()
    finally:
        exercise.cleanup()
    print('PASS all owned candidate units, processes, queue fixtures and private files cleaned')
