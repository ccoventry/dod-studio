"""One connection to one bridge VEGAS's pipe, with a time limit on each call (#1050).

A call writes one JSON request line and waits for the one reply line. The
wait happens on a reader thread started for that call, so the caller stops
waiting at its time limit while the read goes on: the reply of a call that
ran past its limit (a render that hangs, a dialog nobody answers) arrives
later, or never. Until it does, the pipe has half a conversation on it, so
the VEGAS is busy: a new call is refused, not sent, because its reply would
be read as the old one's. The late reply clears the mark, and so does
ending the VEGAS (its pipe closes and the read returns).

No request is written while a read is pending: on Windows a synchronous
pipe handle runs one operation at a time, so a write would wait behind the
read.

Standard library only.
"""

import errno
import json
import threading
import time

import vegas_instances

# How long close() waits for a pending read to return after its VEGAS has
# been ended (its pipe then breaks at once).
READER_EXIT_SECONDS = 2


class BridgeUnavailable(Exception):
    pass


class BridgeBusy(BridgeUnavailable):
    """The VEGAS is still running an earlier call that ran past its limit."""


class BridgeTimeout(BridgeUnavailable):
    """No reply within the call's time limit; the call goes on in VEGAS."""


def _close_quietly(pipe):
    if pipe is not None:
        try:
            pipe.close()
        except OSError:
            pass


class Pending:
    """A call whose reply has not been read yet."""

    def __init__(self, request_id, cmd, limit):
        self.id = request_id
        self.cmd = cmd
        self.limit = limit
        self.sent = time.monotonic()
        self.done = threading.Event()
        self.reply = None  # the reply line; b"" when the pipe closed instead
        self.timed_out = False
        self.reader = None

    def age(self):
        return time.monotonic() - self.sent


class Bridge:
    """One connection to one bridge VEGAS's pipe. Safe to call from several
    threads: calls take turns, each waiting at most its own limit."""

    def __init__(self, pid):
        self.pid = pid
        self.path = vegas_instances.pipe_path(pid)
        self.pipe = None
        self.next_id = 0
        self.cond = threading.Condition()
        self.pending = None
        # What a call that ran past its limit came back with, for the next reply.
        self.late = None

    def _open(self):
        try:
            self.pipe = open(self.path, "r+b", buffering=0)
        except OSError as e:
            # The pipe takes one client at a time; a second one gets EINVAL (pipe busy).
            why = "another client is connected to it" if e.errno == errno.EINVAL else str(e)
            raise BridgeUnavailable("the bridge VEGAS (PID %d) is not reachable at %s: %s" % (self.pid, self.path, why))

    def close(self):
        """Drop the connection. Every caller closes it once its VEGAS has gone
        (or after a reply, with no read pending), so a pending read has
        returned or is about to. Should a read still be pending, closing the
        handle could wait on it (a synchronous pipe handle runs one operation
        at a time), so the close is left to a thread of its own and this
        never blocks."""
        with self.cond:
            pending, pipe = self.pending, self.pipe
            self.pending, self.pipe = None, None
            self.cond.notify_all()
        if pending is not None and pending.reader is not None:
            pending.reader.join(READER_EXIT_SECONDS)
            if pending.reader.is_alive() and pipe is not None:
                threading.Thread(target=_close_quietly, args=(pipe,), daemon=True,
                                 name="vegas %d close" % self.pid).start()
                return
        _close_quietly(pipe)

    def busy(self):
        """What the VEGAS is still running past its limit, or None."""
        with self.cond:
            self._collect()
            p = self.pending
            return None if p is None or not p.timed_out else "%s, sent %.0f s ago (limit %g s)" % (p.cmd, p.age(), p.limit)

    def take_late(self):
        """How a call that ran past its limit ended, once; else None."""
        with self.cond:
            self._collect()
            late, self.late = self.late, None
            return late

    def _collect(self):
        """Clear a pending call whose reply has come. Call holding cond."""
        p = self.pending
        if p is None or not p.done.is_set():
            return
        self.pending = None
        self.cond.notify_all()
        if not p.timed_out:
            return  # its own caller reads it
        if not p.reply:
            self.late = "%s, which ran past its %g s limit, never replied: the pipe closed" % (p.cmd, p.limit)
            self._drop_pipe()
            return
        try:
            reply = json.loads(p.reply.decode("utf-8"))
            outcome = "ok" if reply.get("ok") else "failed: %s" % reply.get("error")
        except (ValueError, AttributeError):
            outcome = "sent an unreadable reply"
            self._drop_pipe()  # out of step: the next call starts on a fresh connection
        self.late = "%s, which ran past its %g s limit, finished %.0f s after it was sent: %s" % (p.cmd, p.limit, p.age(), outcome)

    def _drop_pipe(self):
        _close_quietly(self.pipe)
        self.pipe = None

    def _read(self, pending, pipe):
        try:
            line = pipe.readline()
        except (OSError, ValueError):
            line = b""  # a read on a pipe VEGAS has closed, or that close() closed
        with self.cond:
            pending.reply = line
            pending.done.set()
            self.cond.notify_all()

    def _busy_text(self, p):
        return ("the bridge VEGAS (PID %d) is still running %s, sent %.0f s ago, past its %g s limit, so this call was not "
                "sent: try again once it is done, or end it with end%s"
                % (self.pid, p.cmd, p.age(), p.limit, vegas_instances.open_windows_note(self.pid)))

    def call(self, cmd, args, limit):
        """Send one command and return its reply, waiting at most `limit`
        seconds in all (behind a call another thread made, then for this one)."""
        deadline = time.monotonic() + limit
        with self.cond:
            while True:
                self._collect()
                p = self.pending
                if p is None:
                    break
                if p.timed_out:
                    raise BridgeBusy(self._busy_text(p))
                left = deadline - time.monotonic()
                if left <= 0:
                    raise BridgeTimeout("the bridge VEGAS (PID %d) was still running %s (another call) after this call's "
                                        "%g s limit; this one was not sent" % (self.pid, p.cmd, limit))
                self.cond.wait(left)
            if self.pipe is None:
                self._open()
            self.next_id += 1
            pending = Pending(self.next_id, cmd, limit)
            line = json.dumps({"id": pending.id, "cmd": cmd, "args": args or {}}) + "\n"
            try:
                self.pipe.write(line.encode("utf-8"))
            except OSError:
                self._drop_pipe()
                raise BridgeUnavailable("the bridge VEGAS (PID %d) closed its pipe" % self.pid)
            self.pending = pending
            pending.reader = threading.Thread(target=self._read, args=(pending, self.pipe), daemon=True,
                                              name="vegas %d %s" % (self.pid, cmd))
            pending.reader.start()
            while not pending.done.is_set():
                left = deadline - time.monotonic()
                if left <= 0:
                    break
                self.cond.wait(left)
            if not pending.done.is_set():
                pending.timed_out = True
                # A caller queued behind this one is refused now (busy), not
                # left asleep until its own deadline.
                self.cond.notify_all()
                raise BridgeTimeout(
                    "%s got no reply from the bridge VEGAS (PID %d) within its %g s limit (%s) and is still running there. "
                    "That VEGAS is marked busy: later calls to it are refused until the reply comes, or until end ends it%s"
                    % (cmd, self.pid, limit, "timeout_seconds sets it", vegas_instances.open_windows_note(self.pid)))
            if self.pending is pending:
                self.pending = None
                self.cond.notify_all()
            reply = pending.reply
        if reply:
            try:
                parsed = json.loads(reply.decode("utf-8"))
            except ValueError:
                parsed = None
            if isinstance(parsed, dict) and parsed.get("id", pending.id) == pending.id:
                return parsed
            self.close()
            raise BridgeUnavailable("the bridge VEGAS (PID %d) sent an unreadable reply: %r" % (self.pid, reply[:200]))
        # The pipe closed: that VEGAS has gone (crashed, or ended elsewhere).
        # No reconnect to another VEGAS: a command meant for this one never
        # goes anywhere else.
        self.close()
        raise BridgeUnavailable("the bridge VEGAS (PID %d) closed its pipe" % self.pid)
