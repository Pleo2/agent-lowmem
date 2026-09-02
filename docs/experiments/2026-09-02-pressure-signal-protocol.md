# Agent Lowmem Pressure-Signal Experiment

**Status:** Approved measurement gate before design Revision 4

**Reference host:** Apple M2, 8 GiB unified memory, 16 KiB pages, macOS 26.x

**Probe:** `tools/pressure-probe`

## Question

Does the public macOS Dispatch memory-pressure signal reach a user-space supervisor early and consistently enough to help preserve interactivity during real JavaScript and TypeScript workloads on the reference Mac?

This experiment does not choose a production threshold in advance and does not validate the 1,024 MiB Node heap guardrail. Revision 4 will remove that unverified guardrail independently of the pressure result.

## Safety boundary

The probe is observational. It never sends a signal because of memory pressure, changes a macOS setting, modifies `NODE_OPTIONS`, or retries a failed workload.

- Do not allocate memory synthetically or attempt to force OOM.
- Keep the editor, browser, agent client, and normal MCP processes in their ordinary working state.
- Stop a run if the Mac becomes unsafe or substantially unusable. An interrupted trace is diagnostic evidence but not a qualifying completed run.
- Use the project's documented clean command when a cold repository cache is required. Do not delete guessed build or package-manager paths.
- Run one measured workload at a time.
- Do not commit raw traces or place secrets in the label. Labels accept only ASCII letters, digits, dots, underscores, and hyphens.

## What the probe records

Every JSONL record has schema version `1`, a monotonic timestamp, a wall-clock timestamp, an event name, and an event-specific payload.

The trace contains:

- the safe experiment label;
- exact host profile and probe intervals;
- command executable basename and argument count, but not arguments;
- current `kern.memorystatus_vm_pressure_level` every 250 ms;
- public Dispatch memory-pressure events as delivered to the process;
- timer scheduling delay and coalesced 250 ms intervals;
- swap used and best-effort launched-process-tree footprint every second;
- child start, termination reason, exit status, and session completion.

It does not contain command arguments, environment-variable values, repository paths, usernames, or child output. Child stdin, stdout, and stderr remain attached directly to the terminal and therefore are not copied into the trace.

Process-tree footprint is contextual rather than authoritative. A process that exits between enumeration and inspection, or that is reparented, can be missed.

## Build once

From the repository root:

```bash
cd tools/pressure-probe
swift run -j 1 pressure-probe-core-tests
swift run -j 1 pressure-probe-macos-tests
swift build -j 1 -c release
cd ../..
mkdir -p artifacts/pressure-probe/raw
```

Do not rebuild between measured runs unless probe source changes. Record the probe commit in the experiment ledger.

## Baseline

Before counting real workloads, collect two five-minute baseline traces while the normal desktop applications are open but no build or test is running:

```bash
tools/pressure-probe/.build/release/agent-lowmem-pressure-probe \
  --output artifacts/pressure-probe/raw/baseline-01.jsonl \
  --label baseline-01 \
  -- /bin/sleep 300
```

Repeat as `baseline-02`. Baselines characterize probe timer jitter and pressure events under ordinary use. They do not count toward the 20 real workloads and cannot establish that pressure protection is effective.

## Measured workload command

Wrap the exact argument array that would otherwise be executed directly:

```bash
tools/pressure-probe/.build/release/agent-lowmem-pressure-probe \
  --output artifacts/pressure-probe/raw/next-build-warm-01.jsonl \
  --label next-build-warm-01 \
  -- pnpm build
```

The probe returns the child's normal exit code. It refuses to overwrite an existing trace.

## Campaign composition

Collect at least 20 qualifying completed workloads.

- Cover at least three available classes among focused test, full test, typecheck, Next.js build, and NestJS build.
- Collect at least five repetitions for each selected class. Distribute remaining runs across the most memory-intensive selected classes.
- Within each class, alternate warm-cache runs with cold-cache runs created only through a documented project command or a naturally fresh checkout.
- Keep the primary 20-run campaign on AC power to reduce power-policy variance. Record a separate battery campaign later when measuring long-run supervisor overhead.
- Record the repository pseudonym, workload class, warm/cold state, child result, subjective degradation, and trace filename in a local ledger. Do not put client names or absolute paths in committed summaries.

If the available repositories do not provide three workload classes, record that limitation instead of fabricating fixtures and extend passive collection as qualifying projects become available.

## Qualifying a run

A run counts toward 20 only when all of these are true:

1. Exactly one `session_start`, `child_start`, `child_exit`, and `session_end` exists.
2. `session_end.data.complete` is `true`.
3. At least four `sample` records exist.
4. Every sample has an empty `measurementErrors` array.
5. The ledger records workload class, cache state, power source, outcome, subjective degradation, and trace filename.
6. The probe commit and macOS product version match the campaign being analyzed.

Validate an individual trace with:

```bash
jq -e -s '
  (map(select(.event == "session_start")) | length) == 1 and
  (map(select(.event == "child_start")) | length) == 1 and
  (map(select(.event == "child_exit")) | length) == 1 and
  (map(select(.event == "session_end" and .data.complete == true)) | length) == 1 and
  (map(select(.event == "sample")) | length) >= 4 and
  ([.[] | select(.event == "sample") | .data.measurementErrors[]] | length) == 0
' artifacts/pressure-probe/raw/next-build-warm-01.jsonl
```

A nonzero build or test exit may still qualify: the experiment measures signal timing, not application correctness. A launch failure, malformed JSONL, missing end event, measurement error, probe-source change, or intentionally induced memory exhaustion does not qualify.

## Timing definitions

For each trace, derive:

- `dispatchWarning`: first `dispatch_pressure` containing `warning`;
- `dispatchCritical`: first `dispatch_pressure` containing `critical`;
- `polledWarning`: first sample whose normalized pressure is `warning`;
- `polledCritical`: first sample whose normalized pressure is `critical`;
- `objectiveDegradation`: first sample with at least one coalesced 250 ms interval;
- `subjectiveDegradation`: approximate wall time at which the operator first noticed sustained UI or input degradation, when any;
- `warningLead`: `objectiveDegradation - dispatchWarning`;
- `criticalLead`: `objectiveDegradation - dispatchCritical`.

A positive lead means the event reached the probe before objective degradation. A missing degradation marker means the run is useful for detecting possible false-positive pressure events but cannot prove that the signal arrives early enough.

The one-full-interval objective marker is deliberately mechanical: it means the lightweight probe missed at least one entire 250 ms scheduling deadline. It is an experiment marker, not a proposed runtime threshold.

## Information sufficiency

Twenty completed runs are necessary but not automatically sufficient. The campaign must also contain at least five informative runs where either:

- Dispatch reports warning or critical pressure; or
- objective or sustained subjective degradation is observed.

If the 20 runs stay entirely normal, the result is **inconclusive**, not successful. Continue passive measurement during ordinary heavy work; do not manufacture pressure.

## Revision 4 decision gate

### Outcome A — actionable warning signal

Choose this only if every degradation-positive informative run receives a Dispatch warning before objective degradation, the smallest observed warning lead is at least two seconds, and warning episodes in non-degraded runs can be separated by one fixed sustained-duration rule.

Revision 4 then:

- uses the public Dispatch source for in-run events;
- uses the private sysctl only for the explicitly compatibility-sensitive initial snapshot;
- fixes one sustained-warning duration from this campaign rather than learning it at runtime;
- retains immediate critical reaction as an emergency best-effort path;
- documents the tested macOS build and observed lead-time distribution.

### Outcome B — late or ambiguous warning

Choose this if warning overlaps or follows degradation, false-positive warning episodes cannot be separated cleanly, or only critical pressure distinguishes unsafe runs.

Revision 4 then:

- leads with serialization, no-watch, one-worker support, focused validation, and timeouts;
- treats warning as informational;
- may retain critical termination only as best-effort emergency protection;
- does not claim that pressure monitoring prevents the Mac from becoming unresponsive.

### Outcome C — no useful separation

Choose this if degraded runs occur before both warning and critical events, or pressure behavior is inconsistent enough that it cannot support a stable policy.

Revision 4 then removes preventive pressure termination from the v1 promise. Any future swap/compressor derivative begins as observation-only and requires its own prospective evidence before enforcement.

## Claims this experiment cannot support

- A successful build under normal pressure does not prove that the pressure signal is timely.
- Absence of a pressure event does not prove the host was safe.
- Correlation between high swap and a failed build does not establish a safe swap threshold.
- The process-tree footprint is not a hard cap or complete accounting boundary.
- Results from one macOS major version, memory size, or chip do not validate another profile.
- The campaign cannot justify a Node heap limit; that requires separate workload-level evidence.

## Post-campaign artifact

Commit only an aggregate report containing:

- probe commit and host profile;
- qualifying and excluded run counts with exclusion reasons;
- workload-class coverage;
- pressure/degradation contingency counts;
- warning and critical lead-time distributions for informative runs;
- false-positive episode summary;
- selected Outcome A, B, or C with exceptions;
- exact resulting Revision 4 changes.

Raw JSONL remains local and ignored unless the owner explicitly reviews and authorizes sharing it.
