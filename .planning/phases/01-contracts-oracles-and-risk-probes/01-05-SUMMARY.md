---
phase: 01-contracts-oracles-and-risk-probes
plan: 05
subsystem: oracle
tags: [docker, buildx, sympy, mpmath, python, uv, jcs, sha256, rlimit, sandbox]

requires:
  - phase: 01-02
    provides: "xtask spine: store put/fetch_blob, host::heavy_job, workspace::root, pinned-tool resolution by absolute path"
  - phase: 01-01
    provides: "ksci_wire::to_jcs and ksci_wire::sha256_hex"
provides:
  - "cargo xtask oracle run: the one hardened container runner for untrusted programs (D-01)"
  - "cargo xtask oracle build-image <name> --platform <p>... [--from-store]: pull or build, pin identity, store the image tar"
  - "cargo xtask oracle introspect: SymPy introspection table, identical on linux/arm64 and linux/amd64, stored by digest"
  - "conformance/oracles/run_programs.py: per-case isolated executor; record schema case_id, status, stdout, stdout_truncated, exception"
  - "conformance/oracles/sympy: Containerfile plus the approved hash lock (sympy 1.14.0, mpmath 1.3.0)"
  - "conformance/oracles/images.json (ksci.oracle-images.v1): image identities, tar digests, introspection_sha256"
  - "testdata/programs/smoke.jsonl: 8 own-authored smoke cases (ok=4 error=2 timeout=2)"
  - "Operator clearance of pyarrow 26.0.0 for plan 01-07 (7-day rule waived)"
affects: [01-07 strata and extraction image, 01-10 goldens, 01-12 stand-in name table, 01-13 stand-in baseline, 01-17 census]

actuals:
  tokens: 21800
  tasks: 3
  commits: 7
plan_head_before: 46c5bc6e6e4a686f4fa0ee7726e84e465f0112d7
plan_head_after: 497c00ddd6480b26137baa5ee4202b2f8f63e960

tech-stack:
  added: [docker 28.3.3 (pinned system tool), docker buildx, "python:3.13-slim-trixie@sha256:70729b46…", sympy 1.14.0 (oracle image only), mpmath 1.3.0 (oracle image only)]
  patterns:
    - "Untrusted code runs only through oracle::run_script; every flag lives in the HARDENING const and a unit test asserts it"
    - "Image identity = the content-addressed local image ID; docker run executes exactly that ID, after checking it resolves to the pinned platform manifest (review W1)"
    - "Case containment: run_programs and every case wrapper are subreapers; a finished case's descendants are swept, and a case is classified only from channels that reached EOF (review W3)"
    - "Container outputs are re-encoded with ksci_wire::to_jcs on the host and stored by digest; only regular files are admitted"
    - "System tools (git, docker) resolve through host::resolve_system from fixed absolute paths, never PATH"

key-files:
  created:
    - xtask/src/oracle.rs
    - conformance/oracles/run_programs.py
    - conformance/oracles/images.json
    - conformance/oracles/sympy/Containerfile
    - conformance/oracles/sympy/requirements.in
    - conformance/oracles/sympy/requirements.lock
    - conformance/oracles/sympy/introspect.py
    - testdata/programs/smoke.jsonl
    - testdata/programs/review.jsonl
    - testdata/programs/review-concurrency.jsonl
    - xtask/tests/oracle_review.rs
  modified:
    - xtask/src/main.rs
    - xtask/src/host.rs

key-decisions:
  - "docker is a pinned system tool (floor 28.3.3) run by absolute path, like git; tools check and therefore xtask ci now need a docker client"
  - "Resource limits are set by the trusted wrapper before the program is read, not in a preexec_fn (unsafe with a threaded parent)"
  - "A timeout record carries empty stdout: what a killed program had flushed depends on timing"
  - "images.json: schema tag, and reference per platform pin (built images are tagged per platform)"
  - "The SymPy table asserts the observed 921 runtime __all__ names (902 literal + 19 subpackages), not the plan's 902"
  - "pyarrow 26.0.0 cleared for plan 01-07 by operator decision, waiving the 7-day freshness rule"
  - "Review W1: docker run executes the pinned content-addressed image ID, never the tag; pins carry identity (image ID) and manifest (per-platform manifest digest)"
  - "Review W3: containment by two-level PR_SET_CHILD_SUBREAPER plus sweep; PID/user namespaces are EPERM in the hardened container (measured), so not used"
  - "Review W2: per-case harness failures are recorded as HarnessError; batch-fatal only for invalid input (2), unmet env (3), no subreaper (4), unwritable outputs"

patterns-established:
  - "Witness by probe: own-authored adversarial programs (network, rootfs, mounts, limits, symlink in /out) run through the real runner, kept out of git"

requirements-completed: [CONF-01]

coverage:
  - id: D1
    description: "One hardened container runner; every case recorded; canonical records stored by digest and stable across reruns and worker counts"
    requirement: CONF-01
    verification:
      - kind: integration
        ref: "cargo xtask oracle run --image base --platform linux/arm64 --script conformance/oracles/run_programs.py --input testdata/programs/smoke.jsonl --cpu-seconds 2 (ok=4 error=2 timeout=2, records sha256=13992589…5c90 on every run, also with --jobs 1)"
        status: pass
      - kind: unit
        ref: "xtask oracle::tests::oracle_run_flags_are_hardened (seen RED with --network=bridge)"
        status: pass
      - kind: other
        ref: "Boundary probes: ENETUNREACH, EROFS on / and on the conformance mount, EFBIG, MemoryError, uid 65534 with CapEff 0, symlink in /out refused"
        status: pass
    human_judgment: false
  - id: D2
    description: "SymPy oracle image: digest-pinned base, approved hash lock, pinned env, identity and stored tar per platform, env asserted at run time"
    requirement: CONF-01
    verification:
      - kind: integration
        ref: "cargo xtask oracle run --image sympy --platform linux/arm64 --require-sympy-env ... smoke.jsonl --cpu-seconds 2 (ok=4 error=2 timeout=2, twice)"
        status: pass
      - kind: other
        ref: "cargo xtask oracle build-image sympy --platform linux/arm64 --from-store after docker rmi: loaded from the store, identity b468cfd6…e140 ok"
        status: pass
      - kind: other
        ref: "Tampered identity in images.json -> run refused, exit 1; --require-sympy-env without sympy -> exit 3"
        status: pass
    human_judgment: false
  - id: D3
    description: "SymPy introspection table identical on linux/arm64 and linux/amd64, stored by digest and pinned"
    requirement: CONF-01
    verification:
      - kind: integration
        ref: "cargo xtask oracle introspect -> introspection sha256=011f497f2f95d8ffdcd59528df1caf992f2cf3a65a2cdfa43c90c7c717a81da9"
        status: pass
    human_judgment: false

duration: about 50 min active (Task 1 and the checkpoint, then Task 3 after the operator's approval)
completed: 2026-10-11
status: complete
---

# Phase 1 Plan 05: Offline SymPy oracle and the hardened runner Summary

**Untrusted Python now runs in exactly one place: `cargo xtask oracle run`. It is a digest-pinned container with no network, a read-only root, no capabilities and uid 65534, with per-case CPU, memory, file-size and wall-clock limits. Its records are JCS-canonical and stored by digest. On top of it sits a hash-locked SymPy 1.14.0 / mpmath 1.3.0 image, pinned per platform with its tar in the store. The SymPy introspection table (921 names) is byte-identical on arm64 and amd64.**

## Performance

- **Duration:** about 50 min of active work. Task 1 ran first; the plan then waited at the Task 2 package checkpoint, and Task 3 ran after the operator approved it.
- **Tasks:** 3 of 3. Task 2 was the operator checkpoint.
- **Files:** 8 created, 2 modified (1,595 insertions).

## Accomplishments

- **`cargo xtask oracle run`** (D-01, T-01-13)
  - The container runs with `--rm --pull=never --init --network=none --read-only`.
  - `/tmp` is a tmpfs: `rw,size=1g,mode=1777`.
  - `--cap-drop=ALL --security-opt=no-new-privileges --pids-limit=512 --memory=8g --user 65534:65534 --hostname ksci-oracle`.
  - `conformance/` (and `standin/` on request) and every input are bind-mounted read-only. A fresh `target/oracle/run-<pid>` directory is the only writable mount.
  - The run holds `host::heavy_job()`.
  - Before running, it checks the local image identity against `images.json`.
  - Afterwards it re-encodes every `*.jsonl` line and every `*.json` file with `ksci_wire::to_jcs`, stores each one, and prints `<file> sha256=<hex>`. For `records.jsonl` it also prints `ok=N error=N timeout=N`.
  - `records.jsonl` must be strictly ascending by `case_id`. Non-regular files in `/out` are refused, so a symlink is never followed on the host.
  - A failing container exit code is passed through, and nothing is stored.
- **`run_programs.py`**
  - Each case runs in its own `python -s -c WRAPPER` process, in a new session.
  - The trusted wrapper sets RLIMIT_CPU (S, hard limit S+1), RLIMIT_AS and RLIMIT_FSIZE (64 MiB) before it reads the program.
  - The program text arrives on a pipe, and the exception record leaves on a second dedicated pipe.
  - The whole process group is killed at 4×S of wall-clock time.
  - stdout is capped at 1 MiB and flagged `stdout_truncated`.
  - Status is `timeout` for SIGXCPU, the wall-clock kill, or SIGKILL at the CPU limit. It is `error` for an exception record, any other signal, or a nonzero exit. Otherwise it is `ok`.
  - Records are sorted by `case_id`; `run.json` records the interpreter, machine, hash seed, environment pins, limits, input digests, counts and (in the sympy image) the versions and backends.
  - `--require-sympy-env` exits 3 unless `sympy.external.gmpy.GROUND_TYPES` and `mpmath.libmp.backend.BACKEND` are both `python`. Both attribute names were confirmed in the installed sources: `gmpy.py` line 310 sets `GROUND_TYPES = 'python'`, and `backend.py` defines `BACKEND`.
- **`cargo xtask oracle build-image`**
  - `base` is pulled by digest.
  - Any other name is built with `docker buildx build --load --provenance=false --sbom=false` and tagged `ksci-oracle-<name>:<os-arch>`. The identity is pinned, and the image is saved with `docker save` and `store put`.
  - `--from-store` runs `docker load` on the pinned tar when the image is absent locally, then checks the identity.
- **`cargo xtask oracle introspect`** runs `introspect.py` with `--require-sympy-env` on linux/arm64 and linux/amd64, requires the canonical `names.json` to be byte-identical, stores it, and writes `introspection_sha256`.
- **SymPy image**
  - Built `FROM` the pinned index digest, with `SYMPY_GROUND_TYPES=python MPMATH_NOGMPY=1 SYMPY_USE_CACHE=yes LC_ALL=C.UTF-8 TZ=UTC`.
  - Installed with `pip install --no-deps --require-hashes --only-binary=:all:` from the approved lock.
  - The build fails if gmpy2, python-flint or numpy is present; the distributions installed are exactly mpmath, pip and sympy.
  - It runs as `USER 65534:65534`.

## Pins (conformance/oracles/images.json, after the review W1 re-pin)

`identity` is the content-addressed local image ID that `docker run` executes. `manifest` is the platform manifest that ID resolves to, and it is checked before every run.

| Image | Platform | Identity (runs) | Manifest | Stored tar sha256 | Emulated |
|---|---|---|---|---|---|
| base | linux/arm64 | `sha256:70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f` (the index) | `sha256:cf97c3b79da1c706532c7042f851654d97d3b88c8ca6520acda9cf4479ea9b14` (matches RESEARCH.md's arm64 digest) | — | no |
| sympy | linux/arm64 | `sha256:b468cfd6ec79408995fe9b8506e47952296a5fe78f9285d57517dff5b1d7e140` | same as the identity | `37c964f01f1a163522a422d6c07449f7bf58ae705714601a402e3542571cac57` | no |
| sympy | linux/amd64 | `sha256:4f33ed0632da2fe22a365386f7ddae072ccc332b940e2943b0b5567c5cabbfa9` | same as the identity | `7d1446620e1a3bfc2a125f80e1491400a3e91cb9d16f2b5a9f49fcb1ea88e3ec` | yes |

The re-pin rebuilt sympy. Buildx reused cached layers, so the identities are unchanged. The tars are new (`34738054…` became `37c964f0…` for arm64, `774bd1ed…` became `7d144662…` for amd64), because `docker save` now saves by identity instead of by tag. The introspection table was re-run, and its digest is unchanged.

- **Introspection table:** `introspection_sha256` = `011f497f2f95d8ffdcd59528df1caf992f2cf3a65a2cdfa43c90c7c717a81da9`.
  - It holds 921 names: 457 functions, 392 classes, 52 instances and 20 modules.
  - `sympy.abc` contributes 76 Symbol names.
  - Versions: sympy 1.14.0, mpmath 1.3.0, CPython 3.13.16. Ground types and the mpmath backend are both `python`.
- **Where the bytes are:** the tars and tables live in the local store (`$KSCI_CACHE`). Nothing was pushed to GHCR, because that needs the operator's go-ahead (D-05). A plan in another worktree on this Mac gets the exact images with `cargo xtask oracle build-image sympy --platform <p> --from-store`.

## Operator approvals (Task 2, 2026-10-11, relayed by the orchestrator)

- **sympy 1.14.0 and mpmath 1.3.0 approved.**
  - The lock was regenerated from the repository root with the pinned uv 0.13.0:
    `uv pip compile --no-config --index-url https://pypi.org/simple conformance/oracles/sympy/requirements.in --generate-hashes --python-version 3.13 --python-platform linux -o conformance/oracles/sympy/requirements.lock`.
  - Its hashes equal the approved set exactly. They are also PyPI's published digests for every file of each version.

  | Package | File | sha256 |
  |---|---|---|
  | sympy 1.14.0 | sympy-1.14.0-py3-none-any.whl (installed) | `e091cc3e99d2141a0ba2847328f5479b05d94a6635cb96148ccb3f34671bd8f5` |
  | sympy 1.14.0 | sympy-1.14.0.tar.gz | `d3d3fe8df1e5a0b42f0e7bdf50541697dbe7d23746e894990c030e2b05e72517` |
  | mpmath 1.3.0 | mpmath-1.3.0-py3-none-any.whl (installed) | `a0b2b9fe80bbcd81a6647ff13108738cfb482d481d826cc0e02f5b35e5c88d2c` |
  | mpmath 1.3.0 | mpmath-1.3.0.tar.gz | `7a28eb2a9774d00c7bc92411c19a89209d5da7c4c9a9e227be8330a23a25b91f` |

- **pyarrow: version 26.0.0, not 25.0.1, for plan 01-07.**
  - The operator chose 26.0.0 (released 2026-10-09), explicitly waiving the plan's 7-day freshness rule. **Plan 01-07 uses pyarrow 26.0.0.**
  - Facts verified by the orchestrator: Apache-2.0, apache/arrow, not yanked, no runtime dependencies, requires_python >=3.11.
  - The cp313 wheels are cleared with these hashes:
    - `pyarrow-26.0.0-cp313-cp313-manylinux_2_28_aarch64.whl`: `e3b190ba1d3d22a5a8758597f797111b77d433473744352a184a5ee0a42d672e`
    - `pyarrow-26.0.0-cp313-cp313-manylinux_2_28_x86_64.whl`: `240bd18a7487f8767616a948a69dd4e740a8bc36a1c9da49e4dc9a32c5c2faed`
  - pyarrow is not in this plan's lock and never enters the golden image.
- **Lookalikes:** `sympy-core`, `mp-math`, `py-arrow`, `sympy1`, `mpmath2` and `pyarow` all return 404 on PyPI.

## Task Commits

1. **Task 1: tracer, the hardened runner and the smoke programs through the pinned base image**: `bfb62ef` (feat)
2. **Task 2: package-legitimacy checkpoint**: no commit. Approved by the operator (see above).
3. **Task 3: hash-locked SymPy image pinned per platform, and the introspection table**: `8002345` (feat)

**Tracer gate.** The mode was interactive, `end-of-phase`, with an automated-only verify. The verify was re-run on the committed tree: same records digest, `ok=4 error=2 timeout=2`, clippy clean. Then the plan expanded.

## Witnesses (each guard was seen to fail)

1. **Determinism**
   - Four base-image runs and two sympy-image runs all gave records sha256 `13992589…5c90`, and `--jobs 1` gave the same bytes. Records do not depend on the worker count.
   - `--seed 1` changes exactly one record, the hash-seed case: `{'alpha', 'beta', 'gamma'}` became `{'beta', 'gamma', 'alpha'}`. This is the witness plan 01-10 needs.
2. **Identity pin.** With the identity in `images.json` altered, the run refused ("…is sha256:cf97… locally but pinned as sha256:0000…; refusing to run"), exit 1. File restored.
3. **Hardening unit test.** Changing `--network=none` to `--network=bridge` in `HARDENING` makes `oracle_run_flags_are_hardened` fail ("--network=none missing"). It passes again after the restore, confirmed with `cmp`.
4. **Container boundary.** Own-authored probe programs were run through the real runner; they are kept under `target/` and not committed.
   - An outbound connection fails with `[Errno 101] Network is unreachable`.
   - Writes to `/etc` and to `/opt/ksci/conformance` fail with `[Errno 30] Read-only file system`.
   - A 65 MiB file write fails with `[Errno 27] File too large`.
   - A 3 GiB allocation raises `MemoryError`.
   - The program runs as uid/gid 65534 with `CapEff` 0000000000000000.
   - `os._exit(5)` is recorded as error `ExitCode 5`, and SIGSEGV as error `Signal SIGSEGV`.
   - A forked child that sleeps forever is killed with the process group, so the case completes.
   - A symlink planted in `/out` is refused: "output evil is not a regular file; refusing it", exit 1, nothing stored, output dir removed.
5. **Input integrity.** A case whose `case_id` is not the sha256 of its code makes the script exit 2. xtask passes the 2 through and stores nothing.
6. **Environment assertion.** `--require-sympy-env` in the base image, where sympy is absent, exits 3.
7. **`--from-store`.** After `docker rmi ksci-oracle-sympy:linux-arm64`, the command loaded the stored tar (`34738054…`) and reported identity `b468cfd6…e140 ok`. So `docker save`/`load` preserves the pinned identity.

## Files Created/Modified

- `xtask/src/oracle.rs`: `OracleCmd` (`build-image`, `run`, `introspect`), `Pins` (the only writer of `images.json`), `HARDENING`, `run_argv`, `run_script` (which returns the stored outputs), `store_outputs`, and 4 unit tests.
- `xtask/src/main.rs`: the `Oracle` variant.
- `xtask/src/host.rs`: a `docker` row in `TOOLS`. `resolve_git` was generalised to `resolve_system(name)`, and `host::docker()` added.
- `conformance/oracles/run_programs.py`, `conformance/oracles/sympy/introspect.py`: both start with `# provenance: original`; standard library only.
- `conformance/oracles/sympy/{Containerfile, requirements.in, requirements.lock}`.
- `conformance/oracles/images.json`: JCS, schema `ksci.oracle-images.v1`.
- `testdata/programs/smoke.jsonl`: 8 cases, 1.0 KiB.

## Decisions Made

- **One reference per platform pin.** A built image is tagged per platform (`ksci-oracle-sympy:linux-arm64`), so `reference` lives in the platform pin. For `base` it is the digest reference on every platform.
- **A rebuild clears `introspection_sha256`.** A rebuilt image need not be byte-identical to the one the table came from.
- **No image is pushed.** The tars stay in the local store until the operator approves a push (D-05).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Security] docker runs by absolute path, as a pinned system tool**
- **Found during:** Task 1.
- **Issue:** the plan's runner would have found `docker` by a PATH lookup. Review G (plan 01-02) forbids that for git, for the same reason.
- **Fix:**
  - `docker` is added to `host::TOOLS` as `Install::System` with fixed paths: `/usr/local/bin`, `/opt/homebrew/bin`, the Docker.app bundle, `/usr/bin`.
  - The floor is 28.3.3, the version every flag was verified against.
  - The probe is `docker version --format {{.Client.Version}}`, because `--version` prints `28.3.3,` with a trailing comma.
  - `resolve_git` was generalised to `resolve_system(name)`. It is private: ast-grep found 2 sites and the compiler confirmed the rename.
- **Impact:** `tools check`, and therefore `xtask ci`, now needs a docker client at one of those paths.
- **Commit:** `bfb62ef`.

**2. [Rule 1 - Bug] Limits are set in the wrapper, not a `preexec_fn`**
- **Found during:** Task 1.
- **Issue:** the subprocess documentation calls `preexec_fn` unsafe in a threaded parent, and `run_programs.py` runs cases on threads.
- **Fix:** the trusted wrapper sets RLIMIT_CPU/AS/FSIZE before it reads or compiles the program. The acceptance greps for RLIMIT_CPU and RLIMIT_AS still hold.
- **Commit:** `bfb62ef`.

**3. [Rule 1 - Bug] Exits the plan's status rules would have called ok**
- **Found during:** Task 1.
- **Issue:** the plan's rule "otherwise ok" would record `os._exit(5)` or a segfault as ok.
- **Fix:**
  - any other signal is error `Signal <NAME>`;
  - a nonzero exit with no exception record is error `ExitCode <n>`;
  - SIGKILL with CPU time at or above the limit (the hard CPU limit) is `timeout`.
- **Commit:** `bfb62ef`.

**4. [Rule 1 - Determinism] A timeout record carries empty stdout**
- **Issue:** what a killed program had flushed depends on timing, which would break "two runs give the same digest".
- **Commit:** `bfb62ef`.

**5. [Rule 2 - Security] Extra container flags and output checks**
- **Flags:** `--pull=never`, so what runs is what was inspected. `--init`, to reap orphaned processes. `--hostname ksci-oracle`, so a program sees a deterministic hostname.
- **Checks:** xtask refuses any output that is not a regular file. `records.jsonl` must be strictly ascending by `case_id`.
- **Pipes:** program text and the exception record travel on pipes whose fd numbers are passed in argv. The plan said the record goes on fd 3.
- **Commit:** `bfb62ef`.

**6. [Schema] images.json shape**
- The plan's shape was name → platform → `{identity, tar_sha256}`.
- What was built: `{"schema":"ksci.oracle-images.v1","images":{name:{"platforms":{p:{"reference","identity","emulated","tar_sha256"?}},"introspection_sha256"?}}}`.
- The `platforms` level keeps the image-level `introspection_sha256` apart from the platform keys.
- Task 1 committed the schema with an image-level `reference`. Task 3 moved it into the platform pin, because built tags are per platform. The base pin was regenerated; its identity is unchanged.
- **Commits:** `bfb62ef`, `8002345`.

**7. [Rule 1 - Bug in the plan's constant] `sympy.__all__` has 921 names, not 902**
- **Found during:** Task 3 (`oracle introspect`).
- **Issue:** the plan's `assert len(sympy.__all__) == 902` failed with "921 names". Measured in the oracle image:
  - `sympy/__init__.py` assigns a 902-name `__all__` literal. RESEARCH.md counted that literal from the AST.
  - It then calls `__all__.extend((...))` with 19 subpackage names: algebras, assumptions, calculus, concrete, discrete, external, functions, geometry, interactive, multipledispatch, ntheory, parsing, plotting, polys, printing, release, strategies, tensor, utilities.
  - The runtime list, which is what `from sympy import *` binds, has 921 names. The plan's `kind: module` category exists only because of them.
- **Fix:** `introspect.py` asserts 902 + 19 = 921, and that the 19-name extended tail is all modules.
- **Second finding:** one literal name is also a module. `sympy.evalf` is bound at run time to the module `sympy.core.evalf`, which gives 20 modules in total.
- **For plan 01-12:** the stand-in name table must reproduce all 921 names, including the 20 modules.
- **Commit:** `8002345`.

---

**Total deviations:** 7: 4 bugs (one of them the plan's constant), 2 security hardenings, 1 schema clarification. Every acceptance criterion holds, with the 921 count as the corrected fact.
**Impact on plan:** no scope change. Plan 01-12 must use 921 names, not 902.

## Threat Flags

| Flag | File | Description |
|---|---|---|
| threat_flag: shared-uid /out | xtask/src/oracle.rs, conformance/oracles/run_programs.py | Cases run as the same uid as `run_programs.py`, so a program can write into the writable `/out` bind mount. Each file is capped by RLIMIT_FSIZE (64 MiB), but the total is not bounded by the container. xtask admits only regular files and re-encodes JSON. A program could also add or alter files in `/out`; record integrity against an adversarial program is not in T-01-13, which protects the host. Closing it fully needs per-case uid separation or Landlock. |

## Issues Encountered

- **No rust-analyzer.** The LSP returned "No LSP server available" all session. As CLAUDE.md's fallback, every-site checks used ast-grep (structural) plus compiler enumeration (`cargo clippy --all-targets`).
- **Shell commands rejected by the worktree guard.** The sandbox refused compound commands and some `curl` lines. PyPI facts were gathered with a stdlib script in the scratchpad, and fixtures were generated the same way.
- **The ground-type check is only seen RED by its absence branch.** An image whose ground types are not `python` would need gmpy2 or python-flint installed, which the build forbids. The exit-3 path was witnessed via "sympy is not importable".

## Verification

- `cargo xtask oracle run --image base … --cpu-seconds 2`: `ok=4 error=2 timeout=2`, records `13992589…5c90`, stable across runs.
- `cargo xtask oracle run --image sympy --platform linux/arm64 --require-sympy-env …`: `ok=4 error=2 timeout=2` (twice).
- `cargo xtask oracle introspect`: `introspection sha256=011f497f2f95d8ffdcd59528df1caf992f2cf3a65a2cdfa43c90c7c717a81da9`, the same on both platforms.
- `cargo clippy -p xtask --all-targets --locked -- -D warnings` and `cargo fmt --all --check` are clean.
- `cargo test -p xtask`: 30 unit and 15 integration tests pass.
- `cargo xtask check no-corpus` reports no violations. `cargo xtask ci` reports "all 6 steps passed".
- Acceptance greps all pass:
  - `--network=none`, `--read-only`, `--cap-drop=ALL`, `no-new-privileges`, `--pids-limit` and `--user` each appear in `oracle.rs` (2 each).
  - RLIMIT_CPU and RLIMIT_AS each appear in `run_programs.py` (2 each).
  - The Containerfile's `FROM`, `SYMPY_GROUND_TYPES=python` and `MPMATH_NOGMPY=1` each appear once.
  - The lock has `sympy==1.14.0` once, `mpmath==1.3.0` once, and four `--hash=sha256:` lines.
  - `images.json` names `introspection_sha256`, and the sympy entry has both linux/arm64 and linux/amd64.
  - Both new `.py` files start with `# provenance: original`.
  - `Cargo.lock` is unchanged.

## Review fixes (cross-model review: Codex gpt-6-astra, FAIL with 3 must-fix warnings, confirmed by the orchestrator)

Each fix went test-first. The RED witnesses were committed first in `2969217`, failing on the unfixed code, then fixed.

| Commit | What |
|---|---|
| `2969217` | test(01-05): RED witnesses for W1, W2 and W3 |
| `f67bf6c` | fix(01-05): run the verified content-addressed image identity, never the tag (W1) |
| `497c00d` | fix(01-05): contain every case's descendants and never discard the batch (W2, W3) |

- **No rust-analyzer.** The LSP was unavailable this session. As CLAUDE.md's fallback, every-site checks used ast-grep plus compiler enumeration (`cargo clippy --all-targets`).
- **The new witnesses are ignored tests.** Both tests in `xtask/tests/oracle_review.rs` are `#[ignore]` because they need docker and the pinned base image, and because `cargo xtask ci` holds the heavy-job lock they would take. Run them with `cargo test -p xtask --test oracle_review -- --ignored`.

### W1: the pin was checked on a tag that was then executed

- **Finding.** `run_script` checked `inspect --platform <p> <reference>` against the pin, then passed `pinned.reference`, a mutable tag, to `docker run`. A retag between the check and the run would execute a different image.
- **Measured before the fix:**
  - docker runs a built image by its ID;
  - for the base image, the runnable ID is the index digest `sha256:70729b46…`;
  - the platform manifest `sha256:cf97c3b7…` is not runnable, either as `sha256:cf97…` or as `python@sha256:cf97…` ("No such image").
- **Fix (one path).**
  - `PlatformPin.identity` is now the local image ID (`inspect .Id`), which is content-addressed. `run_image(pin)` returns it, and `docker run` executes it.
  - The new `PlatformPin.manifest` holds the platform manifest that the ID resolves to. `verify_pin` checks it before every run and after `--from-store`.
  - The reference (tag) is only a name; it is never executed. `docker save` also saves by identity.
- **RED.** `oracle_runs_the_pinned_identity_not_the_tag`: `left: "ksci-oracle-sympy:linux-arm64"`, `right: "sha256:aaaa…"`.
- **GREEN, the same test.** Live witnesses:
  - With `ksci-oracle-sympy:linux-arm64` retagged to the base image, a `--require-sympy-env` run still ran sympy. Its `run.json` is `90b24ba6…`, which carries the sympy facts; the base image has no sympy and would have exited 3.
  - A tampered `manifest` in `images.json` is refused: "resolves to manifest sha256:cf97… locally but is pinned to sha256:0000…; refusing to run".
  - After `docker rmi`, `--from-store` loads the untagged image by ID and runs.

### W2: an unnamed signal discarded the whole batch

- **Finding.** `signal.Signals(sig).name` raised `ValueError: 36 is not a valid Signals` for SIGRTMIN+2. `run_programs.py` exited 1, and xtask stored nothing.
- **Fix.**
  - `signal_name` falls back to the number, deterministically.
  - `run_case_recorded` turns any harness failure inside one case into that case's `HarnessError` record (`"<ExceptionType>: <message>"`), with no stdout.
  - `run_case` ends the case on any failure: it kills the wrapper, reaps it and sweeps.
- **Batch-fatal by decision:**
  - invalid input or arguments (exit 2);
  - `--require-sympy-env` unmet (exit 3);
  - `run_programs.py` cannot become a subreaper (exit 4, new);
  - failing to write the outputs.

### W3: an escaped child outlived its case and hid its exception

- **Finding.** A child that called `os.setsid()` escaped the `killpg`. It held the exception pipe open, and the buffered `f.read(65536)` kept the record inside the reader. The case was recorded `ok` with no exception, and the child outlived the case.
- **Mechanism chosen: a two-level subreaper and a sweep.**
  - `run_programs.py` and each case wrapper call `prctl(PR_SET_CHILD_SUBREAPER)`.
  - While a case runs, its orphans are adopted by its own wrapper. Once the wrapper is gone, everything left is adopted by `run_programs.py`, whatever session or process group it moved to.
  - When a wrapper exits, `sweep_strays` SIGKILLs and reaps every child of `run_programs.py` that is not a live wrapper. It repeats in rounds, because a killed process's children are re-parented in turn.
  - Spawning and sweeping share `SPAWN_LOCK`, so a just-forked wrapper is never swept.
  - A per-case PID namespace would be the stronger primitive, but inside the hardened container `unshare(CLONE_NEWPID)` and `unshare(CLONE_NEWUSER)` both return `EPERM` (measured). Granting them would weaken the sandbox.
- **Classification needs complete channels.**
  - The exception channel is read with raw `os.read`.
  - After the sweep, stdout and the exception channel must both reach EOF within `CLOSE_GRACE` (2 s). Otherwise the case is `HarnessError`, never `ok`.
- **Residual.** The wrapper's subreaper flag lives in the program's own process, so a program that clears it with ctypes only exposes its own orphans to other cases' sweeps. Containment of finished cases does not depend on that flag.

### Per case, before and after

| Case | Before the fix | After |
|---|---|---|
| orchestrator w2 `33174b22` (`os.kill(getpid(), SIGRTMIN+1)`) | run_programs crashed, batch discarded, nothing stored | error `Signal "35"`; the batch is kept (`b8079233` hello is ok); records `d39e402a…` |
| orchestrator w3 `7320fca9` (setsid child holds the pipes, parent raises) | ok, exception null (records `e82da2cd…` per the orchestrator) | error `ValueError "boom"`; records `5605d7fb…` |
| own `abe29b00` (SIGRTMIN+2) | batch aborted (`ValueError: 36 is not a valid Signals`) | error `Signal "36"` |
| own `5231a540` (escape, then raise) | ok, exception null | error `ValueError "escaped"` |
| own `42099a76` + `34fc6f01` (escaped child; next case checks it) | `spawned`, then `stray alive` | `spawned`, then `stray gone` |
| own concurrency `1d69cd63` (a running case's orphan) + `d979b41a` (a running case's child), with 24 fast cases | (new guard) | `orphan done` and `child done 0`. With the wrapper subreaper disabled, the orphan was killed by another case's sweep and printed `"\n"` (RED) |
| smoke suite, base and sympy | records `13992589…5c90` | records `13992589…5c90` (unchanged); run.json `0750a3b9…` and `90b24ba6…` (unchanged) |
| boundary probes (`probe.jsonl`) | records `923d797a…` | records `923d797a…` (unchanged) |

### Determinism of record bytes (note for plans 01-07 and 01-10)

- **What the runner guarantees.** It removes the nondeterminism it introduces: the hash seed is fixed per run, the hostname is fixed, the worker count does not change record bytes, timeout records carry no stdout, and harness errors carry no stdout.
- **What it cannot guarantee.** Raw `stdout` and exception messages are recorded verbatim, so a program that prints object addresses (`<… at 0x7f…>`), times, pids, random draws or hash-ordered containers under a varied seed gives different bytes on every run. Records are byte-stable only for programs that are deterministic themselves.
- **Admission policy for later plans (open item for 01-07 and 01-10).** Plans 01-07 and 01-10 must admit a case as a golden only after the same records come out on two or more independent runs, at least across two seeds and both architectures (Pitfall J). A disagreeing case is adjudicated or excluded, never normalised silently. Any normalisation (for example `0x…` addresses) must be an explicit, versioned rule applied in the scorer, not in the runner.

## Next Phase Readiness

- **01-07, 01-10, 01-13 and 01-17** run untrusted programs only through `cargo xtask oracle run`. They pass `--image sympy --require-sympy-env`, get records by digest, and should use `--seed` sweeps for adjudication.
- **01-07:** use pyarrow **26.0.0** with the hashes above (operator decision).
- **01-12:** generate the stand-in name table from introspection blob `011f497f…1da9` (921 names, 20 modules, 76 abc symbols).
- **Other worktrees** run `cargo xtask oracle build-image sympy --platform linux/arm64 --from-store` before their first run.
- **01-07 and 01-10** apply the record-admission policy in "Determinism of record bytes" above.

---
*Phase: 01-contracts-oracles-and-risk-probes*
*Completed: 2026-10-11*

## Self-Check: PASSED

- All 11 created key files exist on disk.
- The task commits `bfb62ef` and `8002345`, and the review commits `2969217`, `f67bf6c` and `497c00d`, are ancestors of HEAD.
- After the review fixes:
  - `cargo test -p xtask` passes (31 unit, 15 integration, 2 ignored docker witnesses);
  - both docker witnesses pass with `-- --ignored`;
  - clippy `-D warnings`, fmt and `check no-corpus` are clean;
  - `cargo xtask ci` reports "all 6 steps passed".
- STATE.md, ROADMAP.md and REQUIREMENTS.md were not touched (parallel worktree; the orchestrator owns them).
