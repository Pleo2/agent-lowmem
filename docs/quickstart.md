# Test Agent Lowmem in 10 minutes

This guide lets you exercise the complete Agent Lowmem workflow in a disposable JavaScript project before using it in real work.

## What Agent Lowmem does

Agent Lowmem is a policy runner for heavy JavaScript and TypeScript validation on memory-constrained Apple Silicon Macs. It does not make arbitrary commands consume less memory. Instead, it:

- inspects a repository without executing its package scripts;
- recognizes only reviewed package-manager and tool versions;
- generates a small repository policy for supported operations;
- runs one heavy operation at a time under a global lock;
- injects reviewed low-concurrency arguments when required;
- supervises the child process group, deadline, signals, and cleanup;
- records a bounded machine-readable result when requested;
- removes only the files and `AGENTS.md` block it owns during restore.

The v0.1.0 MVP supports ARM64 Macs running macOS 14 or newer and npm/pnpm JavaScript projects. It intentionally rejects watch mode, UI mode, background execution, uncontrolled parallelism, unknown tool versions, and ambiguous scripts.

## 1. Install and verify

```sh
brew install Pleo2/agent-lowmem/agent-lowmem
agent-lowmem --version
agent-lowmem doctor
```

The version must be:

```text
agent-lowmem 0.1.0
```

`doctor` is read-only. Outside a Git repository it can validate the Mac, but it will report that no supported repository is available.

## 2. Create a disposable demo

The current policy matrix is deliberately exact. This demo uses Node `24.15.0`, npm `12.0.2`, and Vitest `4.1.11`. [Volta](https://volta.sh/) keeps those demo versions isolated from your normal defaults.

```sh
mkdir agent-lowmem-demo
cd agent-lowmem-demo
git init -b main
mkdir test
```

Create `package.json`:

```json
{
  "name": "agent-lowmem-demo",
  "private": true,
  "packageManager": "npm@12.0.2",
  "scripts": {
    "test": "vitest run"
  },
  "devDependencies": {
    "vitest": "4.1.11"
  }
}
```

Create `test/math.test.js`:

```js
import { describe, expect, it } from "vitest";

describe("math", () => {
  it("adds two values", () => {
    expect(2 + 2).toBe(4);
  });
});
```

Install the exact demo dependencies and create a baseline commit:

```sh
volta run --node 24.15.0 --npm 12.0.2 npm install --ignore-scripts --no-audit --no-fund
git add package.json package-lock.json test/math.test.js
git commit -m "test: create Agent Lowmem demo"
```

If Git asks for your identity, configure `user.name` and `user.email` before the commit.

## 3. Inspect before changing anything

Run Agent Lowmem inside the same Volta environment so its child receives npm `12.0.2`:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem doctor
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem doctor --json
```

The important fields are:

- `Runtime supported: yes`: the Mac can run the CLI;
- `Performance validated: yes`: it matches the tested 8 GiB reference profile;
- `root:test [candidate] runnable`: the test script is understood but not configured yet;
- `Init: available`: Agent Lowmem can prepare its managed files.

`doctor` does not execute `npm`, `pnpm`, your tests, or your build.

## 4. Preview and apply adoption

Always preview first:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem init --dry-run
```

The preview shows a bounded diff but writes nothing. If it looks correct, apply it:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem init
git status --short
sed -n '1,160p' .agent-lowmem.json
sed -n '1,200p' AGENTS.md
```

Agent Lowmem creates:

- `.agent-lowmem.json`: approved operations and their deadlines;
- an owned policy block in `AGENTS.md`: tells compatible coding agents to use the managed commands;
- `.git/agent-lowmem/restoration-v1.json`: private restoration metadata inside Git metadata, not project source.

After initialization, `doctor` should report managed runs as available.

## 5. Run the test through Agent Lowmem

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem run test
```

For this Vitest script, Agent Lowmem launches the equivalent of:

```text
npm run test -- --no-file-parallelism --maxWorkers=1
```

The executable is started directly, without `sh -c`. Agent Lowmem holds the global heavy-operation lock, supervises the process group, enforces the configured deadline, forwards termination signals, and verifies cleanup.

A successful run ends with:

```text
agent-lowmem: result origin=child code=0 reason=completed
```

To give another agent a durable result, request a repository-relative JSON file:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem run test \
  --json-file .agent-lowmem-result.json
jq '{origin, code, reason, childStarted, details}' .agent-lowmem-result.json
```

The JSON result records the operation, evidence hashes, applied controls, deadline, spawn state, elapsed time, and cleanup state. It does not record source contents, environment values, or credentials.

## 6. Restore the repository

Preview removal first:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem restore --dry-run
```

Then remove Agent Lowmem's managed configuration and owned `AGENTS.md` block:

```sh
volta run --node 24.15.0 --npm 12.0.2 agent-lowmem restore
```

Restore does not remove `.agent-lowmem-result.json`, because that file was explicitly requested by you rather than created by `init`. Remove the disposable demo when finished:

```sh
cd ..
mv agent-lowmem-demo ~/.Trash/
```

## Trying a real project

Start with read-only inspection and a dry run:

```sh
cd /path/to/your/project
agent-lowmem doctor --json
agent-lowmem init --dry-run
```

Do not edit `packageManager` or dependency versions merely to make the project appear compatible. If `doctor` reports an unsupported tool or version, keep the project unchanged and open an issue with the redacted JSON result. The initial matrix is intentionally narrow and will expand through reviewed, fixture-backed releases.

If the preview is correct:

```sh
agent-lowmem init
git status --short
git diff -- AGENTS.md
agent-lowmem run test
```

Only operation keys generated in `.agent-lowmem.json` are accepted. Depending on the project, these may include `test`, `lint`, `typecheck`, or `build`.

## Common results

| Reason | Meaning | What to do |
| --- | --- | --- |
| `operation-unsupported` | The requested key was not generated | Run `doctor` and inspect `.agent-lowmem.json` |
| `tool-version-unsupported` | A detected tool version is outside the reviewed matrix | Keep the real version and report the compatibility gap |
| `watch-denied`, `ui-denied`, `background-denied`, `parallel-denied` | The script requests an unsafe mode for this MVP | Add a separate bounded non-watch script to the project |
| `lock-held` | Another managed heavy operation owns the global lease | Wait for it to finish; do not start parallel retries |
| `evidence-changed` | A manifest, lockfile, or configuration changed after planning | Inspect the change and run the command again |
| `managed-file-conflict` | Managed content was edited or became ambiguous | Preserve your work and inspect before using force restore |
| `deadline-exceeded` | The configured operation deadline elapsed | Narrow the command or move the broad job to CI |

Use `restore --force-managed-block` only when the configuration is safe and the sole conflict is one structurally complete Agent Lowmem block whose body was edited. The force boundary never authorizes overwriting arbitrary files.

## Remove Agent Lowmem

Restore each adopted repository first, then uninstall the binary:

```sh
agent-lowmem restore --dry-run
agent-lowmem restore
brew uninstall Pleo2/agent-lowmem/agent-lowmem
```

Agent Lowmem installs no daemon, background service, telemetry, or automatic updater.
