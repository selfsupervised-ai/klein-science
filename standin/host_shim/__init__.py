# provenance: original
"""CPython-only tool transport for the stand-in: frames requests to the R2 native worker (`ksci-worker`).

This package lives outside `sympy` and is never part of a bundle (GUEST-02). It exists so the stand-in can run under
CPython in tests and oracle containers, where Klein is not the host.

Frames are `ksci_wire::frame`: a request is a big-endian u32 length and the JCS bytes; a response is a u8 tag, a
big-endian u32 length and the payload.

`calls` counts the tool calls answered since the last `reset()`, so a test can assert that none happened.
"""

import os
import struct
import subprocess

from sympy import _tool

# ksci_wire::MAX_DOCUMENT_BYTES: a response frame longer than this is refused before its payload is read.
MAX_DOCUMENT_BYTES = 64 * 1024 * 1024

calls = 0

_worker_path = None
_process = None


def install(worker_path=None):
    """Install the worker transport as `sympy._tool._transport`.

    The worker is `worker_path`, or else the `KSCI_WORKER` environment variable. It must be an absolute path to an
    existing file; anything else raises ValueError. With neither set, the installed transport refuses every call:
    that is the default inside oracle containers.
    """
    global _worker_path
    path = worker_path if worker_path is not None else os.environ.get("KSCI_WORKER")
    reset()
    _worker_path = None
    _tool._transport = None
    if path is None:
        _tool._transport = _no_worker
        return
    if not os.path.isabs(path):
        raise ValueError("KSCI_WORKER must be an absolute path, got " + repr(path))
    if not os.path.isfile(path):
        raise ValueError("KSCI_WORKER is not an existing file: " + repr(path))
    _worker_path = path
    _tool._transport = _transport


def reset():
    """Stop the worker child, if one is running, and set `calls` back to zero."""
    global _process, calls
    process = _process
    _process = None
    calls = 0
    if process is None:
        return
    try:
        process.stdin.close()
    except OSError:
        pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    process.stdout.close()


def _no_worker(request):
    raise RuntimeError("no klein tool transport: set KSCI_WORKER")


def _worker():
    global _process
    if _process is None:
        # An argument list and no shell: the path is the whole command line.
        _process = subprocess.Popen([_worker_path], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    return _process


def _read_exact(stream, count):
    chunks = []
    remaining = count
    while remaining > 0:
        chunk = stream.read(remaining)
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
    data = b"".join(chunks)
    if len(data) != count:
        code = _process.poll() if _process is not None else None
        raise RuntimeError(
            "ksci-worker closed its output inside a frame (wanted "
            + str(count)
            + " bytes, got "
            + str(len(data))
            + ", exit status "
            + repr(code)
            + ")"
        )
    return data


def _transport(request):
    global calls
    if len(request) > MAX_DOCUMENT_BYTES:
        raise ValueError("request of " + str(len(request)) + " bytes is above the frame cap")
    process = _worker()
    process.stdin.write(struct.pack(">I", len(request)))
    process.stdin.write(request)
    process.stdin.flush()
    head = _read_exact(process.stdout, 5)
    tag = head[0]
    length = struct.unpack(">I", head[1:5])[0]
    if tag > 3:
        raise RuntimeError("ksci-worker sent unknown frame tag " + str(tag))
    if length > MAX_DOCUMENT_BYTES:
        raise RuntimeError("ksci-worker frame length " + str(length) + " is above the cap")
    payload = _read_exact(process.stdout, length)
    calls += 1
    return (tag, payload)
