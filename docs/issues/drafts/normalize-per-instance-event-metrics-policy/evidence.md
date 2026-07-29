# Progressive Manual Verification Evidence

## Purpose

Record a manual baseline and matching post-change verification for every
implementation task that changes observable event, metrics, or banning
behaviour.

The implementation must not choose the exact probe in advance for every task.
Before each code-changing task, identify the affected external behaviour and
record the baseline command, configuration, and observed output. After the
change, run the same probe and record the result. The expected difference must
be documented explicitly when the task intentionally changes observable
behaviour.

## Evidence Rules

- Use an isolated local tracker configuration and storage directory for each
  scenario.
- Record tracker configuration, relevant bound endpoints, commands, and the
  relevant output or metric fields.
- Use distinct peer IDs and info hashes where a scenario needs more than one
  request.
- Preserve the baseline record even if the post-change check fails.
- Stop implementation and diagnose any unexpected external behaviour change.

## Task Evidence

| Task | Baseline Status | Post-change Status | Evidence                                                                             |
| ---- | --------------- | ------------------ | ------------------------------------------------------------------------------------ |
| T2   | TODO            | TODO               | Runtime listener identity does not alter public tracker protocol behaviour.          |
| T3   | TODO            | TODO               | HTTP announce delivery, aggregate metrics, and event observation.                    |
| T4   | TODO            | TODO               | UDP connect/announce delivery and aggregate core metrics.                            |
| T5   | TODO            | TODO               | UDP server request/response behaviour, server metrics, and cookie-error observation. |
| T6   | TODO            | TODO               | Per-instance metrics filtering for HTTP and UDP.                                     |
| T7   | TODO            | TODO               | Shared ban behaviour through a metrics-disabled UDP listener.                        |
| T8   | TODO            | TODO               | REST aggregate metrics and operational metrics.                                      |
| T9   | TODO            | TODO               | Regression tests and any manual probe affected by testable event wiring.             |
| T10  | TODO            | TODO               | Full application-level enabled/disabled and duplicate-port-zero scenarios.           |

## Scenario Records

Add one subsection per task as implementation proceeds. Use this structure:

```markdown
### T{N} - {Task title}

#### Baseline

- Date/time:
- Configuration:
- Endpoints:
- Commands:
- Observed behaviour:

#### Post-change

- Commit or working-tree revision:
- Commands:
- Observed behaviour:
- Comparison with baseline:
- Result: `DONE` / `FAILED`
```
