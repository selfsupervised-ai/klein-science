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

Exit status: 0 after a run (whatever the cases did), 2 for invalid input or arguments, 3 when
--require-sympy-env is given and the ground types or the mpmath backend are not "python".
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


def kill_group(pid):
    try:
        os.killpg(pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass


def run_case(code, env, limits):
    cpu, mem, prelude = limits["cpu_seconds"], limits["mem_bytes"], limits["prelude"]
    code_r, code_w = os.pipe()
    exc_r, exc_w = os.pipe()
    argv = [sys.executable, "-s", "-c", WRAPPER, str(code_r), str(exc_w)]
    argv += [str(cpu), str(mem), str(FSIZE_BYTES), prelude or ""]
    proc = subprocess.Popen(
        argv,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        pass_fds=(code_r, exc_w),
        env=env,
        start_new_session=True,
    )
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

    exc_chunks = []

    def drain_exc():
        with os.fdopen(exc_r, "rb") as f:
            while True:
                chunk = f.read(65536)
                if not chunk:
                    return
                if sum(len(c) for c in exc_chunks) < STDOUT_CAP:
                    exc_chunks.append(chunk)

    exc_reader = threading.Thread(target=drain_exc, daemon=True)
    exc_reader.start()

    out_fd = proc.stdout.fileno()
    os.set_blocking(out_fd, False)
    captured = bytearray()
    truncated = False
    eof = False
    killed = False
    wait_status = None
    rusage = None
    deadline = time.monotonic() + WALL_FACTOR * cpu
    exited_at = None
    while True:
        if wait_status is None:
            pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                wait_status, rusage = status, usage
                exited_at = time.monotonic()
                # Anything the program left running in its group goes with it.
                kill_group(proc.pid)
        if wait_status is not None and (eof or time.monotonic() - exited_at > 1.0):
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
    proc.stdout.close()
    proc.returncode = os.waitstatus_to_exitcode(wait_status)
    exc_reader.join(timeout=1.0)
    feeder.join(timeout=1.0)

    exception = None
    lines = b"".join(exc_chunks).decode("utf-8", "backslashreplace").splitlines()
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
    if exception is None and signalled:
        exception = {"type": "Signal", "message": signal.Signals(sig).name}
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
            pool.submit(run_case, code, env, limits): case_id for case_id, code in cases.items()
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
