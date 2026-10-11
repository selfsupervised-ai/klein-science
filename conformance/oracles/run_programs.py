# provenance: original
"""Run untrusted Python programs, one isolated subprocess per case, and record every outcome.

This script runs only inside the oracle container that `cargo xtask oracle run` starts
(network-less, read-only root, capped tmpfs), under launch.py, as the harness: uid and gid 65534
holding exactly CAP_KILL, CAP_SETGID and CAP_SETUID. It is the one executor shared by the goldens,
the stand-in baseline and the census, so their records compare directly. Standard library only.

Case identity (review R1, operator decision 2026-10-11). Each case runs as its own unprivileged
uid and gid: CASE_ID_BASE plus an offset derived from its case_id, unique within the batch
(assign_case_ids). So no two cases of a batch share a uid, none shares the harness's, and a case
gets the same uid on every run, so its records stay byte-stable. Supplementary groups are
cleared, and the exec into the wrapper drops every capability. A case therefore cannot signal or
ptrace the harness or a sibling, and cannot read their /proc/<pid>/environ, fd or mem. Inputs
and outputs live under /harness, a tmpfs of mode 0700 owned by the harness, which cases cannot
even traverse. /proc itself stays enumerable: remounting it with hidepid=2 needs CAP_SYS_ADMIN
(EPERM, measured), so cases still see other pids, their command lines (the wrapper argv; program
text travels on a pipe) and /proc/<pid>/stat. /tmp is shared (mode 1777, sticky): a case
cannot change or remove another case's files there, but can read the world-readable ones.

Task budget (review round 3). Every task counts against the container's --pids-limit, which
xtask passes as KSCI_PIDS_LIMIT. Each case uid gets a fixed RLIMIT_NPROC (NPROC_PER_CASE), and
--jobs is clamped so the harness's own threads plus every concurrent case's maximum always fit
(pid_budget). So cases cannot starve each other or the harness of tasks, and what a case may
start never depends on --jobs. The requested and effective worker counts go into run.json.

Input: JSON Lines, one object per line, exactly {"case_id": <sha256 hex of the UTF-8 code>,
"code": <str>}. Several --input files are unioned by case_id.

Output, in --out:
- records.jsonl: one object per case, sorted by case_id, with the keys case_id, status
  ("ok" | "error" | "timeout"), stdout, stdout_truncated and exception ({type, message} or null).
  No case is ever dropped.
- run.json: the interpreter, platform, hash seed, environment pins, limits, input digests and
  counts by status (and the SymPy and mpmath versions and backends when sympy is importable).

Each case runs in a fresh `python -s -c WRAPPER`. The wrapper is trusted code: before it reads
the program it lowers RLIMIT_CPU, RLIMIT_AS and RLIMIT_FSIZE, so the limits bind the program and
everything it starts. (They are set there rather than in a `preexec_fn`, which the subprocess
documentation calls unsafe in a threaded parent, and this parent runs cases on threads.) The
program text arrives on a pipe. The parent kills the case's whole process group at 4x the CPU
limit of wall-clock time.

Outcome protocol (review R2). `ok` needs positive evidence. The wrapper reports on a dedicated
status channel. Every line carries a per-case random token and the wrapper's pid, so lines a
forked child or stray output writes are ignored. It writes `ok` only after the program's code
returned normally (or raised SystemExit with code 0, None or False). For an uncaught exception
it first writes `raised`, then `type` (module and qualname, read through type's own descriptors
so no user code runs), then the message (str() of the exception, untrusted: bounded, and
`unprintable` if it raises or is not a str), then `end`. stdout is flushed before either marker.
Classification:
- `raised` with `type` is an error with that type. The message is `<message unavailable>`
  when the report was cut short (the process exited, crashed or was killed while str() ran),
  and `<unprintable>` when str() failed.
- Without `raised`: a timeout as before; a signal or nonzero exit is an error; `ok` requires the
  `ok` marker; a clean exit without it is the error IncompleteExit.
A program that deliberately forges marker lines can still misreport its own outcome: it runs in
the same process as the reporter. That is outside the threat model (programs are untrusted for
safety, not adversarial toward their own records).

Containment (review W3). A case ends when its wrapper ends, and everything it started ends with
it. This process and each wrapper are subreapers (PR_SET_CHILD_SUBREAPER): while a case runs,
its orphans are adopted by its own wrapper; once the wrapper is gone they are adopted here,
however they detached (a new session or process group does not escape). When a wrapper exits,
every child of this process that is not a live wrapper is SIGKILLed and reaped, in rounds, so
nothing a finished case started survives into another case. A per-case PID namespace would be
the stronger primitive, but unshare(CLONE_NEWPID) and unshare(CLONE_NEWUSER) are EPERM in the
hardened container (measured), and granting them would weaken the sandbox.

A case is classified only from complete channels: if stdout or the exception channel has not
reached EOF after the sweep, the case is an error (HarnessError), never ok.

Failures. Per case, any failure of the harness itself (spawn, thread start, read,
classification) ends the case and everything it started, and is recorded as that case's
HarnessError; one case never discards the batch (review W2, round 3). Batch-fatal, with no
records written: invalid input or arguments (exit 2), --require-sympy-env unmet (exit 3), this
process cannot become a subreaper, lacks the harness identity, or has no usable KSCI_PIDS_LIMIT
(exit 4), and failing to write the outputs.

Exit status: 0 after a run (whatever the cases did), 2 for invalid input or arguments, 3 when
--require-sympy-env is given and the ground types or the mpmath backend are not "python", 4
when cases cannot be contained or isolated.
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import platform
import select
import signal
import subprocess
import sys
import threading
import time

STDOUT_CAP = 1 << 20
FSIZE_BYTES = 64 << 20
WALL_FACTOR = 4
# Environment variables a case inherits from this process when they are set (PYTHONPATH only
# when xtask set it). Everything else is dropped.
COPIED_ENV = ("SYMPY_GROUND_TYPES", "MPMATH_NOGMPY", "SYMPY_USE_CACHE", "LC_ALL", "TZ")
# prctl(2) option; Linux UAPI value.
PR_SET_CHILD_SUBREAPER = 36
# Seconds a finished case's stdout and exception channel get to reach EOF after the sweep.
CLOSE_GRACE = 2.0
# Sweep rounds before a case's descendants count as uncontained.
REAP_ROUNDS = 1000
# Pids of the case wrappers started and not yet reaped. SPAWN_LOCK guards it and serialises
# spawning against the stray sweep, so a just-forked wrapper is never taken for a stray.
SPAWN_LOCK = threading.Lock()
LIVE = set()
# The harness identity launch.py establishes, and the capabilities it must hold.
HARNESS_ID = 65534
HARNESS_CAPS = (1 << 5) | (1 << 6) | (1 << 7)  # CAP_KILL, CAP_SETGID, CAP_SETUID
# Case uid = gid = CASE_ID_BASE + (first 32 bits of case_id mod CASE_ID_SPAN), probed past
# collisions in case_id order (assign_case_ids).
CASE_ID_BASE = 100000
CASE_ID_SPAN = 1 << 24
# The container's task budget (review round 3). Every task in the container counts against
# --pids-limit, which xtask passes as KSCI_PIDS_LIMIT (one source). The budget is an invariant
# checked at startup:
#   HARNESS_TASKS_FIXED + jobs * (HARNESS_TASKS_PER_JOB + NPROC_PER_CASE) <= KSCI_PIDS_LIMIT
# - HARNESS_TASKS_FIXED: the init process, the harness's main thread, and headroom.
# - HARNESS_TASKS_PER_JOB: a pool worker thread, plus the case's feeder and status-reader
#   threads.
# - NPROC_PER_CASE: RLIMIT_NPROC of a case's uid (the wrapper, its threads and descendants).
#   It is a constant, so what a case may start never depends on --jobs, and neither do its
#   records.
# A larger --jobs is clamped to the largest that fits; the requested and effective values are
# recorded in run.json.
HARNESS_TASKS_FIXED = 8
HARNESS_TASKS_PER_JOB = 3
NPROC_PER_CASE = 64
# Longest exception message recorded, in characters.
MESSAGE_CAP = 65536
# Placeholder messages (review R2).
MESSAGE_UNAVAILABLE = "<message unavailable>"
MESSAGE_UNPRINTABLE = "<unprintable>"

WRAPPER = r"""
import json, os, resource, sys, types
code_fd, chan_fd, cpu, mem, fsize, nproc = (int(a) for a in sys.argv[1:7])
prelude = sys.argv[7]
resource.setrlimit(resource.RLIMIT_CPU, (cpu, cpu + 1))
resource.setrlimit(resource.RLIMIT_AS, (mem, mem))
resource.setrlimit(resource.RLIMIT_FSIZE, (fsize, fsize))
resource.setrlimit(resource.RLIMIT_NPROC, (nproc, nproc))
with os.fdopen(code_fd, "rb") as f:
    _token = f.readline().strip().decode("ascii")
    source = f.read()
_chan = os.fdopen(chan_fd, "w", encoding="utf-8", errors="backslashreplace")
sys.argv = ["<program>"]

# Captured before the program runs, so rebinding builtins or json does not reach the reporter.
_dumps, _getpid, _type, _str, _int = json.dumps, os.getpid, type, str, int
_SystemExit, _BaseException, _exit = SystemExit, BaseException, os._exit
_qualname = type.__dict__["__qualname__"].__get__
_module = type.__dict__["__module__"].__get__
_exit_code = SystemExit.__dict__["code"].__get__
_stdout = sys.stdout

def _mark(kind, **fields):
    fields["t"], fields["pid"], fields["k"] = _token, _getpid(), kind
    _chan.write(_dumps(fields) + "\n")
    _chan.flush()

def _flush():
    try:
        _stdout.flush()
    except Exception:
        pass

# Adopt the case's orphans while the case runs, so they stay inside this case.
import ctypes
_libc = ctypes.CDLL(None, use_errno=True)
if _libc.prctl(36, 1, 0, 0, 0) != 0:
    _mark("harness", message="PR_SET_CHILD_SUBREAPER failed: errno %d" % ctypes.get_errno())
    _exit(0)
del _libc

def _raised(e, message=None):
    _flush()
    _mark("raised")
    t = _type(e)
    qualname, module = _qualname(t, _type), _module(t, _type)
    _mark("type",
          qualname=qualname if _type(qualname) is _str else "<unnamed>",
          module=module if _type(module) is _str else "<unknown>")
    try:
        if message is None:
            message = _str(e)
        if _type(message) is not _str:
            raise TypeError
        _mark("message", text=message[:MESSAGE_CAP])
    except _BaseException:
        _mark("unprintable")
    _mark("end")

try:
    if prelude:
        import importlib
        importlib.import_module(prelude).install()
    program = compile(source, "<program>", "exec")
    main = types.ModuleType("__main__")
    main.__dict__["__builtins__"] = __builtins__
    sys.modules["__main__"] = main
    exec(program, main.__dict__)
except _SystemExit as e:
    code = _exit_code(e)
    if code is None or code is False or (_type(code) is _int and code == 0):
        _flush()
        _mark("ok")
    elif _type(code) is _int:
        _raised(e, _int.__repr__(code))
    else:
        _raised(e)
except _BaseException as e:
    _raised(e)
else:
    _flush()
    _mark("ok")
""".replace("MESSAGE_CAP", str(MESSAGE_CAP))


def fail(code, message):
    print("run_programs: " + message, file=sys.stderr)
    sys.exit(code)


def clean(text):
    """`text` with any lone surrogate escaped, so it encodes as UTF-8 and parses as JSON."""
    return text.encode("utf-8", "backslashreplace").decode("utf-8")


def load_cases(paths):
    cases = {}
    inputs = []
    for path in paths:
        try:
            with open(path, "rb") as f:
                data = f.read()
        except OSError as e:
            fail(2, f"{path}: {e}")
        count = 0
        for number, line in enumerate(data.split(b"\n"), 1):
            if not line.strip():
                continue
            where = f"{path}:{number}"
            try:
                case = json.loads(line)
            except ValueError as e:
                fail(2, f"{where}: not JSON: {e}")
            if not isinstance(case, dict) or set(case) != {"case_id", "code"}:
                fail(2, f"{where}: a case is exactly {{case_id, code}}")
            case_id, code = case["case_id"], case["code"]
            if not isinstance(case_id, str) or not isinstance(code, str):
                fail(2, f"{where}: case_id and code must be strings")
            try:
                code_bytes = code.encode("utf-8")
            except UnicodeEncodeError as e:
                fail(2, f"{where}: code is not valid Unicode: {e}")
            actual = hashlib.sha256(code_bytes).hexdigest()
            if actual != case_id:
                fail(2, f"{where}: case_id {case_id} is not the sha256 of its code ({actual})")
            cases[case_id] = code_bytes
            count += 1
        inputs.append(
            {
                "name": os.path.basename(path),
                "sha256": hashlib.sha256(data).hexdigest(),
                "cases": count,
            }
        )
    return cases, inputs


def case_env():
    seed = os.environ.get("PYTHONHASHSEED")
    if seed is None:
        fail(2, "PYTHONHASHSEED must be set (xtask passes --seed)")
    env = {
        "PYTHONHASHSEED": seed,
        "PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin"),
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    for name in COPIED_ENV + ("PYTHONPATH",):
        if name in os.environ:
            env[name] = os.environ[name]
    return env


def set_subreaper():
    """Make this process adopt its orphaned descendants. Returns 0, or the errno of the failure."""
    import ctypes

    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
        return ctypes.get_errno()
    return 0


def harness_identity_problem():
    """None when this process is the harness launch.py establishes (uid and gid 65534, no
    groups, holding CAP_KILL, CAP_SETGID and CAP_SETUID); otherwise what is wrong."""
    fields = {}
    with open("/proc/self/status") as f:
        for line in f:
            key, _, value = line.partition(":")
            fields[key] = value.split()
    if fields.get("Uid") != [str(HARNESS_ID)] * 4 or fields.get("Gid") != [str(HARNESS_ID)] * 4:
        return f"uid {fields.get('Uid')} gid {fields.get('Gid')}, expected {HARNESS_ID} (run under launch.py)"
    if fields.get("Groups") != []:
        return f"supplementary groups {fields.get('Groups')}"
    effective = int(fields.get("CapEff", ["0"])[0], 16)
    if effective & HARNESS_CAPS != HARNESS_CAPS:
        return f"effective capabilities {effective:#x} lack CAP_KILL, CAP_SETGID or CAP_SETUID"
    return None


def children_of(pid):
    """Pids whose parent is `pid`, from /proc (the container's own pid namespace)."""
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat", "rb") as f:
                stat = f.read()
        except OSError:
            continue
        # After the parenthesised command name: state, then ppid.
        fields = stat.rsplit(b")", 1)[-1].split()
        if len(fields) > 1 and int(fields[1]) == pid:
            found.append(int(entry))
    return found


def sweep_strays():
    """Kill and reap every child of this process that is not a live case wrapper.

    This process is a subreaper, so a descendant of a finished case (one whose wrapper has
    exited) is re-parented here, however it detached: a new session or process group does not
    escape. Killing a stray re-parents its own children here, hence the rounds. Returns False
    when strays remained after REAP_ROUNDS rounds.
    """
    me = os.getpid()
    with SPAWN_LOCK:
        for _ in range(REAP_ROUNDS):
            strays = [p for p in children_of(me) if p not in LIVE]
            if not strays:
                return True
            for p in strays:
                try:
                    os.kill(p, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for p in strays:
                try:
                    os.waitpid(p, 0)
                except ChildProcessError:
                    pass
    return False


def kill_group(pid):
    try:
        os.killpg(pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass


def signal_name(sig):
    """`SIGSEGV` for a named signal; the number (`36`) for one without a `signal.Signals`
    member, such as a real-time signal."""
    try:
        return signal.Signals(sig).name
    except ValueError:
        return str(sig)


def harness_error(message):
    """The record of a case the harness could not run or observe completely. Partial stdout is
    timing-dependent, so none is recorded."""
    return {
        "status": "error",
        "stdout": "",
        "stdout_truncated": False,
        "exception": {"type": "HarnessError", "message": clean(message)},
    }


def assign_case_ids(case_ids):
    """case_id -> the case's uid (and gid), unique within the batch (review R1).

    Derived from the case_id, so a case gets the same uid on every run whatever the worker count
    or the order cases finish in (a program that prints os.getuid() stays byte-stable), and no
    uid is reused within a batch. A collision moves the later case_id, in sorted order, to the
    next free uid.
    """
    taken = set()
    assigned = {}
    for case_id in sorted(case_ids):
        offset = int(case_id[:8], 16) % CASE_ID_SPAN
        while CASE_ID_BASE + offset in taken:
            offset = (offset + 1) % CASE_ID_SPAN
        taken.add(CASE_ID_BASE + offset)
        assigned[case_id] = CASE_ID_BASE + offset
    return assigned


def run_case_recorded(code, env, limits, case_uid):
    """`run_case` under the case's own identity, with any failure of the harness itself
    recorded as this case's error: one case never aborts the batch (review W2)."""
    try:
        record, _ = run_case(code, env, limits, case_uid)
        return record
    except Exception as e:
        return harness_error(f"{type(e).__name__}: {e}")


def channel_events(data, token, pid):
    """The wrapper's own status lines: JSON objects carrying this case's token and the wrapper's
    pid. Lines from forked children, partial lines and anything else are ignored."""
    events = []
    for line in data.split(b"\n"):
        if not line:
            continue
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if (
            isinstance(event, dict)
            and event.get("t") == token
            and event.get("pid") == pid
            and isinstance(event.get("k"), str)
        ):
            events.append(event)
    return events


def reported_exception(events):
    """The exception the wrapper reported, or None when it reported none. A report cut short
    after the type still names the type (review R2)."""
    kinds = [e["k"] for e in events]
    if "raised" not in kinds:
        return None
    after = events[kinds.index("raised") :]
    kind = next((e for e in after if e["k"] == "type"), None)
    if kind is None or not isinstance(kind.get("qualname"), str):
        return {"type": "HarnessError", "message": "the exception report ended before its type"}
    text = next((e.get("text") for e in after if e["k"] == "message"), None)
    if isinstance(text, str):
        message = text
    elif any(e["k"] == "unprintable" for e in after):
        message = MESSAGE_UNPRINTABLE
    else:
        message = MESSAGE_UNAVAILABLE
    return {"type": clean(kind["qualname"]), "message": clean(message)}


def run_case(code, env, limits, case_uid):
    """Run one case to completion as `case_uid`. Returns (record, contained); when it returns,
    every process the case started is dead unless contained is False."""
    cpu, mem, prelude = limits["cpu_seconds"], limits["mem_bytes"], limits["prelude"]
    token = os.urandom(16).hex()
    code_r, code_w = os.pipe()
    exc_r, exc_w = os.pipe()
    argv = [sys.executable, "-s", "-c", WRAPPER, str(code_r), str(exc_w)]
    argv += [str(cpu), str(mem), str(FSIZE_BYTES), str(limits["nproc_per_case"])]
    argv += [prelude or ""]
    try:
        with SPAWN_LOCK:
            proc = subprocess.Popen(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                pass_fds=(code_r, exc_w),
                env=env,
                start_new_session=True,
                user=case_uid,
                group=case_uid,
                extra_groups=[],
            )
            LIVE.add(proc.pid)
    except BaseException:
        os.close(code_w)
        os.close(exc_r)
        raise
    finally:
        os.close(code_r)
        os.close(exc_w)

    def feed():
        try:
            with os.fdopen(code_w, "wb") as f:
                f.write(token.encode("ascii") + b"\n" + code)
        except OSError:
            pass

    exc = {"chunks": [], "size": 0, "eof": False}

    def drain_exc():
        # Raw reads, so whatever has arrived is kept even if the channel never closes.
        try:
            while True:
                chunk = os.read(exc_r, 65536)
                if not chunk:
                    exc["eof"] = True
                    return
                if exc["size"] < STDOUT_CAP:
                    exc["chunks"].append(chunk)
                    exc["size"] += len(chunk)
        finally:
            os.close(exc_r)

    feeder = threading.Thread(target=feed, daemon=True)
    exc_reader = threading.Thread(target=drain_exc, daemon=True)
    started = {"feeder": False, "exc_reader": False}
    captured = bytearray()
    truncated = False
    eof = False
    killed = False
    contained = True
    wait_status = None
    rusage = None
    closed_by = None
    # From here on every failure, including a thread that cannot start, ends the case and
    # everything it started, and is recorded as this case's HarnessError (review round 3).
    try:
        feeder.start()
        started["feeder"] = True
        exc_reader.start()
        started["exc_reader"] = True
        out_fd = proc.stdout.fileno()
        os.set_blocking(out_fd, False)
        deadline = time.monotonic() + WALL_FACTOR * cpu
        while True:
            if wait_status is None:
                pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
                if pid:
                    wait_status, rusage = status, usage
                    with SPAWN_LOCK:
                        LIVE.discard(proc.pid)
                    # The case is over: everything it started dies with it (review W3).
                    contained = sweep_strays()
                    closed_by = time.monotonic() + CLOSE_GRACE
            if wait_status is not None and (eof or time.monotonic() >= closed_by):
                break
            if not killed and time.monotonic() >= deadline:
                kill_group(proc.pid)
                killed = True
            if eof:
                time.sleep(0.01)
                continue
            ready, _, _ = select.select([out_fd], [], [], 0.05)
            if not ready:
                continue
            while True:
                try:
                    chunk = os.read(out_fd, 65536)
                except BlockingIOError:
                    break
                if not chunk:
                    eof = True
                    break
                room = STDOUT_CAP - len(captured)
                if len(chunk) > room:
                    truncated = True
                captured += chunk[: max(room, 0)]
    finally:
        if wait_status is None:
            # The harness failed mid-case: end the case and everything it started anyway.
            kill_group(proc.pid)
            try:
                os.kill(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            _, wait_status, rusage = os.wait4(proc.pid, 0)
            with SPAWN_LOCK:
                LIVE.discard(proc.pid)
            contained = sweep_strays()
        # A thread that never started never took ownership of its pipe end.
        if not started["feeder"]:
            os.close(code_w)
        if not started["exc_reader"]:
            os.close(exc_r)
        proc.returncode = os.waitstatus_to_exitcode(wait_status)
        proc.stdout.close()
    # Every writer is dead, so the status channel reaches EOF unless containment failed.
    exc_reader.join(timeout=CLOSE_GRACE)
    feeder.join(timeout=CLOSE_GRACE)
    return classify(
        wait_status=wait_status,
        cpu_used=rusage.ru_utime + rusage.ru_stime,
        cpu=cpu,
        killed=killed,
        contained=contained,
        stdout_eof=eof,
        channel_eof=exc["eof"],
        events=channel_events(b"".join(exc["chunks"]), token, proc.pid),
        stdout=captured.decode("utf-8", "backslashreplace"),
        truncated=truncated,
    ), contained


def classify(
    *,
    wait_status,
    cpu_used,
    cpu,
    killed,
    contained,
    stdout_eof,
    channel_eof,
    events,
    stdout,
    truncated,
):
    """The record of a finished case. Only complete channels are trusted (review W3), and `ok`
    needs the wrapper's completion marker (review R2)."""
    if not contained:
        return harness_error("processes started by the case survived the sweep")
    if not stdout_eof:
        return harness_error("stdout stayed open after the case ended")
    if not channel_eof:
        return harness_error("the status channel stayed open after the case ended")
    harness = next((e for e in events if e["k"] == "harness"), None)
    if harness is not None:
        return harness_error(str(harness.get("message")))

    def error(kind, message):
        exception = {"type": kind, "message": message}
        return {"status": "error", "stdout": stdout, "stdout_truncated": truncated, "exception": exception}

    exception = reported_exception(events)
    if exception is not None:
        # The program raised; stdout was flushed before the report, so it is complete.
        return error(exception["type"], exception["message"])
    signalled = os.WIFSIGNALED(wait_status)
    sig = os.WTERMSIG(wait_status) if signalled else None
    if killed or sig == signal.SIGXCPU or (sig == signal.SIGKILL and cpu_used >= cpu):
        # What a killed program had flushed depends on timing, so a timeout records no stdout.
        return {"status": "timeout", "stdout": "", "stdout_truncated": False, "exception": None}
    if signalled:
        return error("Signal", signal_name(sig))
    if os.WEXITSTATUS(wait_status) != 0:
        return error("ExitCode", str(os.WEXITSTATUS(wait_status)))
    if any(e["k"] == "ok" for e in events):
        return {"status": "ok", "stdout": stdout, "stdout_truncated": truncated, "exception": None}
    return error("IncompleteExit", "exit status 0 without the completion marker")


def sympy_facts():
    """Versions and effective backends, or None when sympy is not importable."""
    try:
        import mpmath
        import mpmath.libmp.backend
        import sympy
        import sympy.external.gmpy
    except ImportError:
        return None
    return {
        "sympy_version": sympy.__version__,
        "mpmath_version": mpmath.__version__,
        "ground_types": sympy.external.gmpy.GROUND_TYPES,
        "mpmath_backend": mpmath.libmp.backend.BACKEND,
    }


def pid_budget(jobs_requested):
    """The task budget for this run (see HARNESS_TASKS_FIXED): the container limit from
    KSCI_PIDS_LIMIT and the effective worker count, `--jobs` clamped to the largest that fits.
    Batch-fatal (exit 4) when the limit is missing or not even one worker fits."""
    raw = os.environ.get("KSCI_PIDS_LIMIT")
    if raw is None or not raw.isdigit():
        fail(4, f"KSCI_PIDS_LIMIT must give the container's --pids-limit (got {raw!r})")
    limit = int(raw)
    per_job = HARNESS_TASKS_PER_JOB + NPROC_PER_CASE
    jobs_max = (limit - HARNESS_TASKS_FIXED) // per_job
    if jobs_max < 1:
        fail(4, f"a pids limit of {limit} fits no worker ({HARNESS_TASKS_FIXED} + {per_job} needed)")
    jobs = min(jobs_requested, jobs_max)
    assert HARNESS_TASKS_FIXED + jobs * per_job <= limit
    return {
        "limit": limit,
        "harness_tasks_fixed": HARNESS_TASKS_FIXED,
        "harness_tasks_per_job": HARNESS_TASKS_PER_JOB,
        "nproc_per_case": NPROC_PER_CASE,
        "jobs_requested": jobs_requested,
        "jobs_max": jobs_max,
        "jobs_effective": jobs,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--input", action="append", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--cpu-seconds", type=int, default=60)
    parser.add_argument("--mem-bytes", type=int, default=2147483648)
    parser.add_argument("--prelude")
    parser.add_argument("--require-sympy-env", action="store_true")
    args = parser.parse_args()
    if args.jobs < 1 or args.cpu_seconds < 1 or args.mem_bytes < 1:
        fail(2, "--jobs, --cpu-seconds and --mem-bytes must be positive")
    if args.prelude is not None and not all(p.isidentifier() for p in args.prelude.split(".")):
        fail(2, f"--prelude must be a dotted module name: {args.prelude!r}")

    sympy = sympy_facts()
    if args.require_sympy_env:
        if sympy is None:
            fail(3, "--require-sympy-env: sympy is not importable")
        if sympy["ground_types"] != "python" or sympy["mpmath_backend"] != "python":
            fail(
                3,
                "--require-sympy-env: ground types "
                f"{sympy['ground_types']!r} and mpmath backend {sympy['mpmath_backend']!r}, "
                "both must be 'python'",
            )

    cases, inputs = load_cases(args.input)
    env = case_env()
    errno = set_subreaper()
    if errno:
        fail(4, f"cannot contain cases: PR_SET_CHILD_SUBREAPER failed with errno {errno}")
    problem = harness_identity_problem()
    if problem:
        fail(4, f"cannot isolate cases: {problem}")
    case_uids = assign_case_ids(cases)
    pids = pid_budget(args.jobs)
    jobs = pids["jobs_effective"]
    limits = {
        "cpu_seconds": args.cpu_seconds,
        "wall_seconds": WALL_FACTOR * args.cpu_seconds,
        "mem_bytes": args.mem_bytes,
        "fsize_bytes": FSIZE_BYTES,
        "stdout_cap_bytes": STDOUT_CAP,
        "nproc_per_case": NPROC_PER_CASE,
        "prelude": args.prelude,
    }

    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {
            pool.submit(run_case_recorded, code, env, limits, case_uids[case_id]): case_id
            for case_id, code in cases.items()
        }
        for future in concurrent.futures.as_completed(futures):
            results[futures[future]] = future.result()

    counts = {"ok": 0, "error": 0, "timeout": 0}
    with open(os.path.join(args.out, "records.jsonl"), "w", encoding="utf-8") as f:
        for case_id in sorted(results):
            record = {"case_id": case_id, **results[case_id]}
            counts[record["status"]] += 1
            f.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")

    run = {
        "python_version": platform.python_version(),
        "machine": platform.machine(),
        "python_hash_seed": env["PYTHONHASHSEED"],
        "env": {name: os.environ.get(name) for name in COPIED_ENV},
        "jobs": jobs,
        "pids": pids,
        "limits": limits,
        "inputs": inputs,
        "cases": len(results),
        "counts": counts,
        "sympy": sympy,
    }
    with open(os.path.join(args.out, "run.json"), "w", encoding="utf-8") as f:
        f.write(json.dumps(run, ensure_ascii=False, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
