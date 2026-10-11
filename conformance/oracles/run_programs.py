# provenance: original
"""Run untrusted Python programs, one isolated subprocess per case, and record every outcome.

This script runs only inside the oracle container that `cargo xtask oracle run` starts
(network-less, read-only root, capped tmpfs, no capabilities, uid 65534). It is the one
executor shared by the goldens, the stand-in baseline and the census, so their records compare
directly. Standard library only.

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
program text arrives on a pipe, and an uncaught exception is reported as one JSON line on a
second, dedicated pipe. The parent kills the case's whole process group at 4x the CPU limit of
wall-clock time.

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

Failures. Per case, any failure of the harness itself (spawn, read, classification) is recorded
as that case's HarnessError; one case never discards the batch (review W2). Batch-fatal, with
no records written: invalid input or arguments (exit 2), --require-sympy-env unmet (exit 3),
this process cannot become a subreaper (exit 4), and failing to write the outputs.

Exit status: 0 after a run (whatever the cases did), 2 for invalid input or arguments, 3 when
--require-sympy-env is given and the ground types or the mpmath backend are not "python", 4
when cases cannot be contained.
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

WRAPPER = r"""
import json, os, resource, sys, types
code_fd, exc_fd, cpu, mem, fsize = (int(a) for a in sys.argv[1:6])
prelude = sys.argv[6]
resource.setrlimit(resource.RLIMIT_CPU, (cpu, cpu + 1))
resource.setrlimit(resource.RLIMIT_AS, (mem, mem))
resource.setrlimit(resource.RLIMIT_FSIZE, (fsize, fsize))
with os.fdopen(code_fd, "rb") as f:
    source = f.read()
exc_out = os.fdopen(exc_fd, "w", encoding="utf-8", errors="backslashreplace")
sys.argv = ["<program>"]

# Adopt the case's orphans while the case runs, so they stay inside this case.
import ctypes
_libc = ctypes.CDLL(None, use_errno=True)
if _libc.prctl(36, 1, 0, 0, 0) != 0:
    exc_out.write(json.dumps({"type": "HarnessError",
                              "message": "PR_SET_CHILD_SUBREAPER failed: errno %d" % ctypes.get_errno()}) + "\n")
    exc_out.flush()
    os._exit(0)
del _libc

def _report(e):
    try:
        message = str(e)
    except BaseException:
        message = "<unprintable " + type(e).__name__ + ">"
    exc_out.write(json.dumps({"type": type(e).__name__, "message": message}) + "\n")
    exc_out.flush()

try:
    if prelude:
        import importlib
        importlib.import_module(prelude).install()
    program = compile(source, "<program>", "exec")
    main = types.ModuleType("__main__")
    main.__dict__["__builtins__"] = __builtins__
    sys.modules["__main__"] = main
    exec(program, main.__dict__)
except SystemExit as e:
    if e.code not in (0, None):
        _report(e)
except BaseException as e:
    _report(e)
finally:
    try:
        sys.stdout.flush()
    except BaseException:
        pass
"""


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


def run_case_recorded(code, env, limits):
    """`run_case`, with any failure of the harness itself recorded as this case's error: one
    case never aborts the batch (review W2)."""
    try:
        return run_case(code, env, limits)
    except Exception as e:
        return harness_error(f"{type(e).__name__}: {e}")


def run_case(code, env, limits):
    """Run one case to completion. When it returns, every process the case started is dead."""
    cpu, mem, prelude = limits["cpu_seconds"], limits["mem_bytes"], limits["prelude"]
    code_r, code_w = os.pipe()
    exc_r, exc_w = os.pipe()
    argv = [sys.executable, "-s", "-c", WRAPPER, str(code_r), str(exc_w)]
    argv += [str(cpu), str(mem), str(FSIZE_BYTES), prelude or ""]
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
                f.write(code)
        except OSError:
            pass

    feeder = threading.Thread(target=feed, daemon=True)
    feeder.start()

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

    exc_reader = threading.Thread(target=drain_exc, daemon=True)
    exc_reader.start()

    captured = bytearray()
    truncated = False
    eof = False
    killed = False
    contained = True
    wait_status = None
    rusage = None
    closed_by = None
    try:
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
            sweep_strays()
        proc.returncode = os.waitstatus_to_exitcode(wait_status)
        proc.stdout.close()
    # Every writer is dead, so the exception channel reaches EOF unless containment failed.
    exc_reader.join(timeout=CLOSE_GRACE)
    feeder.join(timeout=CLOSE_GRACE)

    cpu_used = rusage.ru_utime + rusage.ru_stime
    signalled = os.WIFSIGNALED(wait_status)
    sig = os.WTERMSIG(wait_status) if signalled else None
    if (
        killed
        or sig == signal.SIGXCPU
        or (sig == signal.SIGKILL and cpu_used >= cpu)
    ):
        # What a killed program had flushed depends on timing, so a timeout records no stdout.
        return {"status": "timeout", "stdout": "", "stdout_truncated": False, "exception": None}
    if not contained:
        return harness_error("processes started by the case survived the sweep")
    if not eof:
        return harness_error("stdout stayed open after the case ended")
    if not exc["eof"]:
        return harness_error("the exception channel stayed open after the case ended")

    exception = None
    lines = b"".join(exc["chunks"]).decode("utf-8", "backslashreplace").splitlines()
    if lines:
        try:
            record = json.loads(lines[-1])
            if (
                not isinstance(record, dict)
                or set(record) != {"type", "message"}
                or not all(isinstance(v, str) for v in record.values())
            ):
                raise ValueError("bad shape")
            exception = {"type": clean(record["type"]), "message": clean(record["message"])}
        except ValueError:
            exception = {"type": "InvalidExceptionRecord", "message": clean(lines[-1][:200])}

    if exception is None and signalled:
        exception = {"type": "Signal", "message": signal_name(sig)}
    elif exception is None and os.WEXITSTATUS(wait_status) != 0:
        exception = {"type": "ExitCode", "message": str(os.WEXITSTATUS(wait_status))}
    return {
        "status": "error" if exception is not None else "ok",
        "stdout": captured.decode("utf-8", "backslashreplace"),
        "stdout_truncated": truncated,
        "exception": exception,
    }


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
    limits = {
        "cpu_seconds": args.cpu_seconds,
        "wall_seconds": WALL_FACTOR * args.cpu_seconds,
        "mem_bytes": args.mem_bytes,
        "fsize_bytes": FSIZE_BYTES,
        "stdout_cap_bytes": STDOUT_CAP,
        "prelude": args.prelude,
    }

    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {
            pool.submit(run_case_recorded, code, env, limits): case_id
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
        "jobs": args.jobs,
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
