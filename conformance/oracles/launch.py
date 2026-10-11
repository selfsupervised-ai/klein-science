# provenance: original
"""The trusted entry point of every oracle container: `python3 launch.py <script> <args>...`.

The container starts as root holding exactly CAP_KILL, CAP_SETGID and CAP_SETUID (xtask:
--cap-drop=ALL plus those three, no-new-privileges). Before any script code runs, this drops to
the harness identity, uid and gid 65534 with no supplementary groups, and keeps exactly those three
capabilities, effective and permitted, with nothing inheritable or ambient. It then runs the script
in this same process (runpy), because an exec would drop the capabilities.

Why each capability (operator decision 2026-10-11, review R1: a distinct uid per case):
- CAP_SETUID, CAP_SETGID: run_programs.py starts each case as its own unprivileged uid and gid
  with supplementary groups cleared (subprocess user=/group=/extra_groups=[]). The case's
  exec then drops every capability (non-root, no file capabilities, no ambient set).
- CAP_KILL: signalling a process of another uid needs it. Without it the harness could not kill
  and sweep a case's processes.

Exit status 4 when the drop cannot be done or verified; otherwise the script's own.
"""

import ctypes
import os
import runpy
import sys

HARNESS_ID = 65534
PR_SET_KEEPCAPS = 8
LINUX_CAPABILITY_VERSION_3 = 0x20080522
CAP_KILL, CAP_SETGID, CAP_SETUID = 5, 6, 7
KEPT = (1 << CAP_KILL) | (1 << CAP_SETGID) | (1 << CAP_SETUID)


class _CapHeader(ctypes.Structure):
    _fields_ = [("version", ctypes.c_uint32), ("pid", ctypes.c_int)]


class _CapData(ctypes.Structure):
    _fields_ = [
        ("effective", ctypes.c_uint32),
        ("permitted", ctypes.c_uint32),
        ("inheritable", ctypes.c_uint32),
    ]


def fail(message):
    print("launch: " + message, file=sys.stderr)
    sys.exit(4)


def status():
    fields = {}
    with open("/proc/self/status") as f:
        for line in f:
            key, _, value = line.partition(":")
            fields[key] = value.split()
    return fields


def drop_to_harness():
    if os.getuid() != 0:
        fail(f"must start as root holding the harness capabilities, not uid {os.getuid()}")
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0) != 0:
        fail(f"PR_SET_KEEPCAPS failed: errno {ctypes.get_errno()}")
    os.setgroups([])
    os.setresgid(HARNESS_ID, HARNESS_ID, HARNESS_ID)
    os.setresuid(HARNESS_ID, HARNESS_ID, HARNESS_ID)
    data = (_CapData * 2)()
    data[0].effective = data[0].permitted = KEPT
    header = _CapHeader(LINUX_CAPABILITY_VERSION_3, 0)
    if libc.capset(ctypes.byref(header), data) != 0:
        fail(f"capset failed: errno {ctypes.get_errno()}")
    if libc.prctl(PR_SET_KEEPCAPS, 0, 0, 0, 0) != 0:
        fail(f"clearing PR_SET_KEEPCAPS failed: errno {ctypes.get_errno()}")
    # Verify, rather than trust, the resulting identity.
    s = status()
    expected = {
        "Uid": [str(HARNESS_ID)] * 4,
        "Gid": [str(HARNESS_ID)] * 4,
        "Groups": [],
        "CapEff": [f"{KEPT:016x}"],
        "CapPrm": [f"{KEPT:016x}"],
        "CapInh": ["0" * 16],
        "CapAmb": ["0" * 16],
    }
    for key, want in expected.items():
        if s.get(key) != want:
            fail(f"after the drop {key} is {s.get(key)}, expected {want}")


def main():
    if len(sys.argv) < 2:
        fail("usage: launch.py <script> [args...]")
    drop_to_harness()
    script = sys.argv[1]
    sys.argv = sys.argv[1:]
    runpy.run_path(script, run_name="__main__")


if __name__ == "__main__":
    main()
