# Archived Implementation Attempt

## Status

This document records the implementation attempt preserved on the reference
branch `archive/2035-bootstrap-identity-attempt`.

The attempt is intentionally not proposed for merge. It discovered that the
original #2035 plan did not account for an incomplete migration of
`tracker_usage_statistics` from global to per-public-listener configuration.

## What the Attempt Implemented

The branch replaced address-keyed HTTP and UDP instance container storage with
ordered collections indexed by configuration position. It also changed startup
to select the matching container by that position, so repeated configured
`0.0.0.0:0` bindings no longer overwrite a container during bootstrap.

The key implementation commit was:

```text
e9a82156 fix(core): implement per-instance container storage and event bus
```

The branch also added regressions and architecture documentation:

```text
e7944be1 test(2035): add UDP bootstrap regression test for duplicate port-zero
adf79415 test(core): add regression tests for duplicate port-zero bootstrap
3a1c278c docs: add events architecture and update ADRs
1d831a46 docs(events): document per-listener metrics normalization
```

## What Manual Verification Found

Two HTTP listeners with the same configured `0.0.0.0:0` address and different
`tracker_usage_statistics` settings behaved as expected: both served announces,
while only the enabled listener contributed to `tcp4_announces_handled`.

The equivalent UDP scenario exposed an architectural gap. The REST API maps
`udp4_announces_handled` from the UDP server metrics repository, whose current
event producer is global rather than per listener. As a result, a
metrics-disabled UDP listener still increments that public counter.

## Why Work Stopped

The attempted change used producer-side event suppression: a disabled listener
received no event sender. That model is unsafe now that events have consumers
beyond metrics. In particular, UDP server cookie-error events are also consumed
by the independent banning listener.

The revised design is:

1. Preserve configuration-instance identity during bootstrap.
2. Implement #2036 to define canonical runtime service identity and registry
   metadata.
3. Implement the dedicated per-instance event-metrics normalization issue:
   always emit objective facts, then filter only metrics consumers by stable
   listener identity while keeping banning independent.
4. Reimplement and complete #2035 on top of those merged prerequisites.

This order avoids creating a temporary event-only identity that #2036 would
later replace, and avoids using a metrics setting to suppress security-relevant
facts.

## Use as Reference Only

The branch is a research and implementation reference, not a source for a
blind cherry-pick. The future implementation should reuse its evidence and
bootstrap observations, but must be designed against the canonical identity and
listener-side policy architecture established by the prerequisite issues.

See also:

- [#2035 issue specification](ISSUE.md)
- [Events architecture](../../../events-architecture.md)
- [Runtime service registry metadata (#2036)](../../2036-add-runtime-service-registry-metadata/ISSUE.md)
- Draft per-instance event-metrics normalization specification:
  `docs/issues/drafts/normalize-per-instance-event-metrics-policy/ISSUE.md`
