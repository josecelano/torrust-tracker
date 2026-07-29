---
doc-type: issue
issue-type: bug
status: draft
priority: p1
github-issue: null
spec-path: docs/issues/drafts/normalize-per-instance-event-metrics-policy/ISSUE.md
branch: "{issue-number}-normalize-per-instance-event-metrics-policy"
related-pr: null
last-updated-utc: 2026-07-29 00:00
semantic-links:
  skill-links:
    - create-issue
    - write-unit-test
  related-artifacts:
    - .github/skills/dev/planning/create-issue/SKILL.md
    - docs/events-architecture.md
    - evidence.md
    - docs/adrs/20260727000000_events_are_objective_facts.md
    - docs/adrs/20260727180000_shared_services_across_tracker_instances.md
    - docs/issues/open/2035-fix-duplicate-port-zero-tracker-instance-bootstrap/ISSUE.md
    - packages/events/src/bus.rs
    - packages/http-core/src/container.rs
    - packages/udp-core/src/container.rs
    - packages/udp-server/src/container.rs
    - packages/rest-api-runtime-adapter/src/v1/adapters/stats.rs
    - src/container.rs
    - src/bootstrap/jobs/
  related-issues:
    - 1263
    - 1401
    - 1419
    - 2035
    - 2036
---

<!-- skill-link: create-issue -->

# Issue #[To be assigned] - Normalize Per-Instance Event Metrics Policy

## Goal

Make `tracker_usage_statistics` consistently control only metrics processing for
one public HTTP or UDP listener. Objective events must remain available to
non-metrics consumers, including UDP IP-ban enforcement.

## Background

Epic [#1263](https://github.com/torrust/torrust-tracker/issues/1263) established
the product intent: retain aggregate metrics while allowing operators to enable
or disable metrics for each public `[[http_trackers]]` or `[[udp_trackers]]`
listener. Issue [#1401](https://github.com/torrust/torrust-tracker/issues/1401)
added the per-instance configuration option.

The current runtime applies that policy by suppressing producers: an `EventBus`
returns no sender when statistics are disabled. HTTP and UDP-core metrics happen
to work because their enabled instances publish into a shared broadcaster and
aggregate repository.

UDP exposes a gap. UDP server metrics are emitted through one application-wide
`UdpTrackerServerContainer` event bus and repository, whose producer is gated by
the old global `core.tracker_usage_statistics` setting. The REST API maps
`udp4_announces_handled` from this server repository, so a metrics-disabled UDP
listener still increments public UDP metrics. Conversely, disabling the global
producer suppresses cookie-error events required by the separate banning
listener.

The issue is not repository sharing: one aggregate repository per layer is
compatible with per-listener policy. The issue is applying metrics configuration
to event production instead of metrics consumption.

See [events-architecture.md](../../events-architecture.md#event-layer-asymmetry-http-vs-udp)
for the current topology and analysis.

## Scope

### In Scope

- Normalize HTTP core, UDP core, and UDP server event publication so objective
  facts are emitted independently of per-listener metrics configuration.
- Carry stable runtime listener identity on metric-relevant HTTP and UDP events.
- Make metrics listeners filter events by the originating listener's
  `tracker_usage_statistics` policy before updating aggregate repositories.
- Keep the UDP banning listener independent from metrics policy and able to
  consume relevant cookie-error events from every UDP listener.
- Preserve aggregate REST API metrics and UDP server operational metrics.
- Add focused unit, integration, and manual regressions for enabled and disabled
  listener instances, including repeated `0.0.0.0:0` bindings.
- Update events architecture documentation and related ADRs.

### Out of Scope

- Per-listener metrics repositories or a public per-listener metrics API.
- Changing the public tracker TOML shape solely to mirror internal containers.
- Replacing the `Registar` runtime service registry work tracked by #2036.
- Changing shared-ban-service semantics or making connection-ID validation
  per-instance.

## Design Direction

```text
tracker listener instance N
  → always emits objective event with runtime identity N
  → shared layer broadcaster
       ├─ metrics listener: ignores N when metrics are disabled
       │    → shared aggregate repository
       └─ banning listener: receives all relevant UDP events
            → shared BanService
```

The event identity must not use configured `SocketAddr`, because multiple
instances can have equal configured `0.0.0.0:0` addresses. Use a stable runtime
configuration-instance identifier; the configuration index is acceptable as an
interim implementation, while #2036 may provide durable runtime service
metadata.

## Implementation Plan

Status values: `TODO`, `IN_PROGRESS`, `BLOCKED`, `DONE`.

| ID  | Status | Task                               | Notes / Expected Output                                                                                                                                               |
| --- | ------ | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | TODO   | Map all event producer gates       | Inventory current `SenderStatus` / optional sender use in HTTP core, UDP core, and UDP server; distinguish metric-only from security consumers.                       |
| T2  | TODO   | Define runtime listener identity   | Introduce a typed stable instance identity propagated from application bootstrap to HTTP/UDP event producers. Do not use configured socket address.                   |
| T3  | TODO   | Always emit HTTP core facts        | Remove per-instance statistics producer suppression in HTTP core; pass identity on emitted events.                                                                    |
| T4  | TODO   | Always emit UDP core facts         | Remove per-instance statistics producer suppression in UDP core; pass identity on emitted events.                                                                     |
| T5  | TODO   | Always emit UDP server facts       | Decouple the UDP server producer from global `core.tracker_usage_statistics`; propagate listener identity to every server event.                                      |
| T6  | TODO   | Filter metrics in listeners        | Add immutable per-listener metrics policy lookup to HTTP-core, UDP-core, and UDP-server metrics listeners. Ignore disabled-instance events before repository updates. |
| T7  | TODO   | Preserve banning independence      | Verify the UDP banning listener consumes cookie-error events regardless of metrics policy and preserves shared ban semantics.                                         |
| T8  | TODO   | Update REST metrics adapter        | Preserve aggregate API values while taking counts from policy-filtered repositories; retain UDP server operational metrics.                                           |
| T9  | TODO   | Add focused tests                  | Unit tests for producer independence and listener filtering; include ban-listener behavior.                                                                           |
| T10 | TODO   | Add application integration tests  | Two HTTP and two UDP listeners, one enabled and one disabled; each aggregate announce count must be one. Add duplicate-port-zero coverage.                            |
| T11 | TODO   | Run validation and record evidence | Run `linter all`, focused tests, relevant full tests, and manual tracker verification. Update docs/ADRs/spec evidence.                                                |

### Progressive Manual Verification Protocol

For every task that changes code (T2-T10), follow this sequence before moving
to the next task:

1. **Plan the probe** — identify the externally observable behaviour affected
   by the task and choose the smallest local tracker scenario that exercises it.
2. **Record a baseline** — run the probe against the current implementation and
   record the isolated configuration, bound endpoints, commands, and relevant
   output in [evidence.md](evidence.md).
3. **Implement and automatically test** — make the smallest change, then run
   the focused automated tests for the changed package(s).
4. **Repeat the same probe** — run the baseline scenario unchanged and record
   post-change output in `evidence.md`.
5. **Compare before proceeding** — stop and diagnose unexpected behaviour
   differences. For intentional behaviour changes, state the expected delta and
   add a regression test before marking the task done.

Metrics and UDP banning are the only current event consumers. Every affected
task must explicitly verify that both continue to behave as intended:

- Metrics policy: a disabled listener does not update aggregate metrics, while
  an enabled listener does.
- Security policy: metrics configuration never prevents the UDP banning listener
  from observing relevant cookie-error events or updating the shared ban state.

Exact commands are deliberately selected task by task because the affected
layer, expected delta, and smallest safe probe depend on the code change. The
evidence record format and task evidence matrix are maintained in
[evidence.md](evidence.md).

## Progress Tracking

### Workflow Checkpoints

- [x] Spec drafted in `docs/issues/drafts/`
- [ ] Spec reviewed and approved by user/maintainer
- [ ] GitHub issue created and issue number added to this spec
- [ ] Spec-only PR merged into `develop` before implementation
- [ ] Implementation completed
- [ ] Automatic verification completed (`linter all`, relevant tests, and any pre-push checks)
- [ ] Manual verification scenarios executed and recorded (status + evidence)
- [ ] Acceptance criteria reviewed after implementation and updated with evidence
- [ ] Reviewer validated acceptance criteria and updated checkboxes
- [ ] Committer verified spec progress is up to date before commit
- [ ] Issue closed and spec moved from `docs/issues/open/` to `docs/issues/closed/`

### Progress Log

- 2026-07-28 20:30 UTC - agent - Drafted from the #2035 manual verification finding and #1263/#1401 historical intent. Awaiting user review before GitHub issue creation.
- 2026-07-29 00:00 UTC - agent - Converted to folder-style draft specification and added `evidence.md`. Added mandatory baseline/post-change manual verification after every code-changing task.

## Acceptance Criteria

- [ ] AC1: A metrics-disabled HTTP listener emits objective events, but its
      events do not update aggregate HTTP metrics.
- [ ] AC2: A metrics-disabled UDP listener emits UDP-core and UDP-server
      objective events, but its events do not update aggregate UDP metrics.
- [ ] AC3: A metrics-disabled UDP listener still contributes cookie-error events
      to shared ban enforcement.
- [ ] AC4: A metrics-enabled listener updates the same aggregate metrics
      repositories as before.
- [ ] AC5: Metrics filtering uses stable listener identity and works when two
      listeners use the same configured `0.0.0.0:0` address.
- [ ] AC6: The REST API retains aggregate HTTP/UDP metrics and UDP operational
      metrics (discard, response, error, processing-time, and ban metrics).
- [ ] AC7: `linter all` exits with code `0`.
- [ ] AC8: Relevant tests pass.
- [ ] AC9: Manual verification scenarios are executed and documented.
- [ ] AC10: Acceptance criteria are re-reviewed after implementation and reflect
      observed behavior.
- [ ] AC11: Documentation is updated when behavior/workflow changes.
- [ ] AC12: Every code-changing task has a baseline and post-change manual
      verification record in `evidence.md`, with any intentional behaviour
      difference explained and covered by a regression test.

## Verification Plan

### Automatic Checks

- Focused unit tests for HTTP core, UDP core, UDP server metrics listeners, and
  UDP banning listener.
- Application-level tests with two listener instances per protocol, including a
  duplicate `0.0.0.0:0` scenario.
- `cargo test --test stats -- --test-threads=1` until #1419 resolves shared test
  process environment configuration.
- `linter all`.
- Relevant pre-push checks before opening the implementation PR.

### Per-Task Baseline and Post-Change Checks

Before and after every code-changing task (T2-T10), run the task-specific
manual probe selected under the Progressive Manual Verification Protocol and
record it in [evidence.md](evidence.md). Do not advance to the next task until
the comparison is accepted.

### Manual Verification Scenarios

Status values: `TODO`, `IN_PROGRESS`, `DONE`, `FAILED`, `BLOCKED`.

| ID  | Scenario                           | Command/Steps                                                                                                                           | Expected Result                                                   | Status | Evidence |
| --- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------ | -------- |
| M1  | HTTP policy filtering              | Start two HTTP listeners, one metrics-disabled and one enabled. Announce to both and query stats API.                                   | Aggregate TCP announce count is `1`; both requests are served.    | TODO   |          |
| M2  | UDP policy filtering               | Start two UDP listeners, one metrics-disabled and one enabled. Connect and announce to both and query stats API.                        | Aggregate UDP announce count is `1`; both requests are served.    | TODO   |          |
| M3  | UDP banning independent of metrics | Send invalid cookies through the metrics-disabled UDP listener until the threshold, then send a strict request through either listener. | Shared ban enforcement is active despite disabled metrics.        | TODO   |          |
| M4  | Duplicate port-zero identity       | Repeat M1 and M2 using repeated `0.0.0.0:0` config blocks.                                                                              | Metrics policy follows listener identity, not configured address. | TODO   |          |

### Acceptance Verification

| AC ID | Status (`TODO`/`DONE`) | Evidence |
| ----- | ---------------------- | -------- |
| AC1   | TODO                   |          |
| AC2   | TODO                   |          |
| AC3   | TODO                   |          |
| AC4   | TODO                   |          |
| AC5   | TODO                   |          |
| AC6   | TODO                   |          |
| AC7   | TODO                   |          |
| AC8   | TODO                   |          |
| AC9   | TODO                   |          |
| AC10  | TODO                   |          |
| AC11  | TODO                   |          |
| AC12  | TODO                   |          |

## Risks and Trade-offs

- Adding listener identity to event payloads changes cross-package event types;
  keep the identity typed and internal, and avoid exposing it as a user-supplied
  configuration ID.
- A metrics listener policy lookup must be immutable for the application
  lifetime or explicitly synchronized if runtime configuration is introduced.
- Metrics filtering must occur before repository writes; filtering only REST API
  output would preserve incorrect internal counters and omit no-cost metrics
  consumers.
- Security consumers must remain independent of metrics policy; any producer
  gate retained for resource control requires separate review.

## References

- Epic #1263: https://github.com/torrust/torrust-tracker/issues/1263
- Issue #1401: https://github.com/torrust/torrust-tracker/issues/1401
- Issue #2035: `docs/issues/open/2035-fix-duplicate-port-zero-tracker-instance-bootstrap/ISSUE.md`
- Issue #2036: `docs/issues/open/2036-add-runtime-service-registry-metadata/ISSUE.md`
- Issue #1419: `docs/issues/open/1419-allow-multiple-integration-tests-at-main-app-level/ISSUE.md`
- `docs/events-architecture.md`
- `docs/adrs/20260727000000_events_are_objective_facts.md`
- `docs/adrs/20260727180000_shared_services_across_tracker_instances.md`
