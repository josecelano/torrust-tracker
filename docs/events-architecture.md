---
semantic-links:
  related-artifacts:
    - packages/events/src/
    - packages/http-core/src/event.rs
    - packages/udp-core/src/event.rs
    - packages/udp-server/src/event.rs
    - packages/swarm-coordination-registry/src/event.rs
    - src/bootstrap/jobs/
    - docs/adrs/20260727000000_events_are_objective_facts.md
    - docs/adrs/20260727180000_shared_services_across_tracker_instances.md
---

# Events Architecture

## Overview

The tracker uses a publish/subscribe event system for asynchronous communication
between protocol handlers and cross-cutting concerns (statistics, banning,
metrics). Events are objective facts about what happened — they never encode
consumer-specific policy decisions (see
[ADR 20260727000000](adrs/20260727000000_events_are_objective_facts.md)).

## Core Infrastructure (`packages/events/`)

The events package provides generic, protocol-agnostic building blocks:

| Type                 | Purpose                                                                                                                                                                                  |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Sender` trait       | Async event emitter. Returns `Option<Result<usize, _>>` — `None` means disabled.                                                                                                         |
| `Receiver` trait     | Async event consumer. Returns `RecvError::Lagged(n)` or `RecvError::Closed`.                                                                                                             |
| `Broadcaster<Event>` | Wraps `tokio::sync::broadcast` (capacity 65 536). Implements `Sender` and allows `subscribe()` → `broadcast::Receiver` (implements `Receiver`). Supports multiple subscribers (fan-out). |
| `EventBus<Event>`    | Wraps a `Broadcaster` + `SenderStatus` flag. `sender()` returns `Some` if enabled, `None` if disabled. This is the per-instance gate.                                                    |
| `SenderStatus`       | `Enabled` / `Disabled`, derived from the `tracker_usage_statistics` config flag.                                                                                                         |

### Flow Pattern

```text
Service (announce/scrape/connect)
  → sender.send(event)
    → Broadcaster (tokio::sync::broadcast channel)
      → N subscribers (each listener subscribes once)
        → handler.handle_event()
          → repository.increase_counter() / set_gauge()
```

## Event Types by Package

### HTTP Core (`packages/http-core/src/event.rs`)

```rust
pub enum Event {
    TcpAnnounce { connection: ConnectionContext, info_hash, announcement },
    TcpScrape  { connection: ConnectionContext },
}
```

Emitted by `AnnounceService` and `ScrapeService` in each HTTP tracker instance.

### UDP Core (`packages/udp-core/src/event.rs`)

```rust
pub enum Event {
    UdpConnect  { connection: ConnectionContext },
    UdpAnnounce { connection: ConnectionContext, info_hash, announcement },
    UdpScrape   { connection: ConnectionContext },
}
```

Emitted by `ConnectService`, `AnnounceService`, and `ScrapeService` in each UDP
tracker instance.

### UDP Server (`packages/udp-server/src/event.rs`)

A separate, finer-grained event layer for the UDP server's request lifecycle:

```rust
pub enum Event {
    UdpRequestReceived  { context },
    UdpRequestDiscarded { context },   // client port is 0
    UdpRequestAborted   { context },   // processing aborted
    UdpRequestBanned    { context },   // IP is banned
    UdpRequestAccepted  { context, kind },
    UdpResponseSent     { context, kind, req_processing_time },
    UdpError            { context, kind, error },
}
```

### Swarm Coordination Registry (`packages/swarm-coordination-registry/src/event.rs`)

Domain-level events about swarm state changes:

```rust
pub enum Event {
    TorrentAdded, TorrentRemoved,
    PeerAdded, PeerRemoved, PeerUpdated,
    PeerDownloadCompleted,
}
```

## Shared vs Per-Instance Event Buses

This is a key architectural decision documented in
[ADR 20260727180000](adrs/20260727180000_shared_services_across_tracker_instances.md).

### The Shared Broadcaster Pattern

For HTTP and UDP **core** events, a single `Broadcaster` (one broadcast channel)
is created per protocol type. Each tracker instance gets a per-instance
`EventBus` that wraps the **same** shared `Broadcaster` but with its own
`SenderStatus` flag:

```text
                    ┌────────────────────────────┐
                    │ HttpTrackerCoreServices    │
                    │ ┌────────────────────────┐ │
                    │ │ Broadcaster (1 ch)     │ │
                    │ └────────────┬───────────┘ │
                    └──────────────┼─────────────┘
                                   │ cloned into each instance
                ┌──────────────────┼────────────────────┐
                │                  │                    │
          ┌─────▼────────┐   ┌─────▼─────────┐    ┌─────▼────────┐
          │ Instance 0   │   │ Instance 1    │    │ Instance 2   │
          │ EventBus     │   │ EventBus      │    │ EventBus     │
          │ sender=Some  │   │ sender=None   │    │ sender=Some  │
          │ (stats=true) │   │ (stats=false) │    │ (stats=true) │
          └─────┬────────┘   └───────────────┘    └──────┬───────┘
                │ send()                                 │ send()
                └──────────────────┬─────────────────────┘
                                   ▼
                        ┌────────────────────┐
                        │ Global Listener    │
                        │ (1 subscription)   │
                        │ → stats_repository │
                        └────────────────────┘
```

**Key properties:**

- All instances emit into the **same** broadcast channel.
- A single global listener subscribes once and receives events from **all**
  enabled instances.
- **Current behaviour:** the per-instance `EventBus` is a producer-side gate —
  when `SenderStatus::Disabled`, `sender()` returns `None` and the service
  skips event emission entirely.
- **Target behaviour:** events are objective facts and must be emitted
  independently of metrics configuration. A metrics listener should filter
  by listener identity before updating the aggregate repository. Other
  consumers, such as banning, continue to receive the facts.

### UDP Server Event Bus

The UDP **server** layer currently uses one application-wide `EventBus` in one
`UdpTrackerServerContainer`. Every UDP listener passes that same container to
its launcher and request processor. Server-level events (request received,
accepted, banned, error, and response sent) therefore share one event stream
and one metrics repository.

This is an internal implementation choice, not a consequence of the global
`[udp_tracker_server]` configuration section in schema v3. That TOML section
expresses settings shared by all public UDP listener instances; it does not
require a singleton runtime server container.

### Event Bus Inventory

| Event Bus                              | Scope                            | Created In                                           | Subscribers                                    |
| -------------------------------------- | -------------------------------- | ---------------------------------------------------- | ---------------------------------------------- |
| HTTP core `Broadcaster`                | Shared across all HTTP instances | `HttpTrackerCoreServices::initialize_from`           | 1 (global stats listener)                      |
| HTTP per-instance `EventBus`           | Per HTTP tracker instance        | `HttpTrackerCoreContainer::initialize_from_services` | 0 (uses shared broadcaster)                    |
| UDP core `Broadcaster`                 | Shared across all UDP instances  | `UdpTrackerCoreServices::initialize_from`            | 1 (global stats listener)                      |
| UDP per-instance `EventBus`            | Per UDP tracker instance         | `UdpTrackerCoreContainer::initialize_from_services`  | 0 (uses shared broadcaster)                    |
| UDP server `EventBus`                  | Single application-wide bus      | `UdpTrackerServerContainer::initialize`              | 2 (stats listener + ban listener)              |
| Swarm coordination registry `EventBus` | Single (global)                  | `SwarmCoordinationRegistryContainer::initialize`     | 2 (SCR stats listener + tracker-core listener) |

## Event Listeners (Consumer Side)

Each listener runs as a Tokio task, started in `src/bootstrap/jobs/`. The
current bootstrap gates metrics-listener startup with
`config.core.tracker_usage_statistics`; the UDP banning listener is started
independently. This remains incomplete because the UDP server **producer** is
also currently gated by that same global metrics setting.
**Note**: The REST API adapter (`packages/rest-api-runtime-adapter/src/v1/adapters/stats.rs`)
maps these stats to the public API:

- `tcp4_announces_handled` ← HTTP core stats
- `udp4_announces_handled` ← UDP **server** stats (not core stats)

  | Bootstrap Job                 | Subscribes To                          | Listener Package                                           | Purpose                                           |
  | ----------------------------- | -------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------- |
  | `http_tracker_core.rs`        | HTTP core `Broadcaster`                | `http-core::statistics::event::listener`                   | TCP request counters                              |
  | `udp_tracker_core.rs`         | UDP core `Broadcaster`                 | `udp-core::statistics::event::listener`                    | UDP core request counters                         |
  | `udp_tracker_server.rs`       | UDP server `EventBus`                  | `udp-server::statistics::event::listener`                  | Server request/response counters, processing time |
  | `udp_tracker_server.rs` (ban) | UDP server `EventBus` (2nd subscriber) | `udp-server::banning::event::listener`                     | IP ban enforcement                                |
  | `tracker_core.rs`             | SCR `EventBus`                         | `tracker-core::statistics::event::listener`                | Persistent torrent download counters              |
  | `torrent_repository.rs`       | SCR `EventBus` (2nd subscriber)        | `swarm-coordination-registry::statistics::event::listener` | Torrent/peer gauges                               |

### Multiple Subscribers on One Channel

The `tokio::sync::broadcast` channel supports multiple independent subscribers.
Each `subscribe()` call creates an independent receiver that gets its own copy
of every message. For example, the UDP server event bus has two subscribers:
one for statistics and one for ban enforcement. Both receive every event
independently.

## End-to-End Flow Examples

### HTTP Announce

```text
HTTP GET /announce → Axum server
  → HttpTrackerCoreContainer.announce_service.handle_announce()
    → announce_handler.handle_announcement() (peer repo)
    → sender.send(TcpAnnounce { connection, info_hash, announcement })
      → Broadcaster (shared channel)
        → Global stats listener
          → handler increases http_tracker_core_requests_received_total{request_kind=announce, ...}
```

### UDP Announce (Two Event Layers)

```text
UDP packet arrives → UDP server
  → Server emits UdpRequestReceived { context }
    → Server stats listener → UDP_TRACKER_SERVER_REQUESTS_RECEIVED_TOTAL
  → Server emits UdpRequestAccepted { context, kind: Announce }
    → Server stats listener → UDP_TRACKER_SERVER_REQUESTS_ACCEPTED_TOTAL
  → ConnectService / AnnounceService processes request
    → Core emits UdpAnnounce { connection, info_hash, announcement }
      → Broadcaster (shared channel)
        → Global stats listener → udp_tracker_core_requests_received_total{request_kind=announce, ...}
  → Server emits UdpResponseSent { context, kind, req_processing_time }
    → Server stats listener → UDP_TRACKER_SERVER_RESPONSES_SENT_TOTAL + avg processing time
```

## Event Layer Asymmetry (HTTP vs UDP)

There is an important architectural asymmetry between HTTP and UDP trackers in
how events are used for metrics collection.

### Metrics Repository Architecture

Each event bus feeds events into a `stats_repository` (a metrics counter store).
The key question is: **how many repositories exist, and who controls the gate?**

#### HTTP: One shared repo, per-instance gate

```text
HttpTrackerCoreServices::initialize_from()
  → stats_repository = Arc::new(Repository::new())  // ONE shared repo

For each HTTP tracker instance:
  HttpTrackerCoreContainer::initialize_from_services(..., stats_repository)
    → per_instance_event_bus = EventBus::new(tracker_usage_statistics.into(), shared_broadcaster)
    → announce_service uses per_instance_event_bus.sender()  // Some or None
    → stats_repository: stats_repository.clone()            // same Arc
```

- **ONE** `stats_repository` shared by all HTTP instances (via `Arc::clone`)
- **Per-instance** `EventBus` gates event sending based on `tracker_usage_statistics`
- When disabled, `sender()` returns `None` → announce service skips `send_event()` entirely
- The shared `stats_repository` only receives events from **enabled** instances
- REST API reads from this shared repo → `tcp4_announces_handled` is correct ✓

#### UDP Core: Same pattern as HTTP

```text
UdpTrackerCoreServices::initialize_from()
  → stats_repository = Arc::new(Repository::new())  // ONE shared repo

For each UDP tracker instance:
  UdpTrackerCoreContainer::initialize_from_services(..., stats_repository)
    → per_instance_event_bus = EventBus::new(tracker_usage_statistics.into(), shared_broadcaster)
    → announce_service uses per_instance_event_bus.sender()  // Some or None
    → stats_repository: stats_repository.clone()            // same Arc
```

- **ONE** `stats_repository` shared by all UDP core instances
- **Per-instance** `EventBus` gates event sending
- Works correctly at the core level ✓

#### UDP Server: One shared repo, GLOBAL gate

```text
UdpTrackerServerServices::initialize(core_config)
  → stats_repository = Arc::new(Repository::new())  // ONE shared repo
  → event_bus = EventBus::new(core_config.tracker_usage_statistics.into(), ...)  // GLOBAL config!
```

- **ONE** `stats_repository` for the entire UDP server (all instances share it)
- **GLOBAL** `EventBus` controlled by `core_config.tracker_usage_statistics` (not per-instance)
- `udp-server` handlers always call `send()` on this global sender
- No per-instance gate exists at the server level
- REST API reads from this repo → `udp4_announces_handled` always counts ALL instances ✗

### Why HTTP Works and UDP Doesn't

| Aspect                          | HTTP                                             | UDP Core                                        | UDP Server                                        |
| ------------------------------- | ------------------------------------------------ | ----------------------------------------------- | ------------------------------------------------- |
| Stats repositories              | 1 shared (per `*CoreServices`)                   | 1 shared (per `*CoreServices`)                  | 1 shared (for entire server)                      |
| EventBus per instance?          | ✓ Yes                                            | ✓ Yes                                           | ✗ No (single global)                              |
| Gate reads per-instance config? | ✓ `http_tracker_config.tracker_usage_statistics` | ✓ `udp_tracker_config.tracker_usage_statistics` | ✗ `core_config.tracker_usage_statistics` (global) |
| REST API stats source           | Core stats                                       | Core stats                                      | **Server stats**                                  |
| Per-instance control works?     | ✓                                                | ✓                                               | ✗                                                 |

The root cause is **not** that the repos are shared — sharing repos is fine as long
as the gate prevents events from reaching them. The root cause is that the UDP
server level has no per-instance `EventBus` gate. It uses the **global**
`core_config.tracker_usage_statistics` instead of the per-instance
`udp_tracker_config.tracker_usage_statistics`.

### Why the REST API Reads from Server Stats (Not Core)

The REST API adapter (`packages/rest-api-runtime-adapter/src/v1/adapters/stats.rs`) maps:

- `tcp4_announces_handled` ← `http_stats.tcp4_announces_handled()` — HTTP **core** stats
- `udp4_announces_handled` ← `udp_server_stats.udp4_announce_requests_accepted_total()` — UDP **server** stats

This is an intentional design choice: the UDP server layer provides richer
operational metrics (request received/accepted/discarded/banned/response sent,
processing time) that the core layer does not track. The HTTP tracker does not
have this two-layer split because its Axum server layer does not emit its own
statistics events — all metrics flow through `http-core` events.

### Historical Context: Epic #1263 and Issue #1401

Epic [#1263](https://github.com/torrust/torrust-tracker/issues/1263) records
the original product intent:

- keep aggregate/global metrics;
- move `tracker_usage_statistics` from `[core]` to each public tracker block;
- let operators enable or disable metrics for each HTTP or UDP listener.

Issue #1401 added the per-instance configuration fields. Its implementation
predated the present split between UDP core and UDP server event streams, so it
did not complete the corresponding server-layer metric gating. The current
asymmetry is therefore an incomplete migration from global to per-instance
metrics configuration, rather than evidence that public UDP listener settings
should be global.

### Proposed Normalization: Always Emit Facts, Filter Metrics per Instance

The desired semantics, originally established by epic
[#1263](https://github.com/torrust/torrust-tracker/issues/1263) and issue
[#1401](https://github.com/torrust/torrust-tracker/issues/1401), are:

| UDP listener setting               | Metrics listener                  | Banning/security listener      |
| ---------------------------------- | --------------------------------- | ------------------------------ |
| `tracker_usage_statistics = true`  | Updates aggregate metrics         | Receives security events       |
| `tracker_usage_statistics = false` | Does not update aggregate metrics | Still receives security events |

The same policy must apply to every HTTP/UDP event layer. The recommended
design is a shared, always-emitting event stream with independent consumer
policy:

```text
tracker listener instance N
  → emits objective event with instance identity N
  → shared layer broadcaster
     ├─ metrics listener: ignores events from metrics-disabled instances
     │    → shared aggregate metrics repository
     └─ banning listener: processes every relevant event
        → shared BanService
```

#### Required changes

1. **Keep event publication independent of statistics.** HTTP core, UDP core,
   and UDP server producers must not use `tracker_usage_statistics` as a sender
   gate. Objective request and error events remain available to every consumer.
2. **Carry stable listener identity on emitted events.** Use a runtime
   configuration-instance identity propagated from application bootstrap—not
   configured `SocketAddr`, which is not unique for duplicate `0.0.0.0:0`.
   The current configuration index can be an interim identity; #2036 can later
   provide formal runtime service metadata.
3. **Move the per-instance decision to metrics consumers.** Each HTTP/UDP
   statistics listener needs a lookup of metrics-enabled listener identities
   and must ignore disabled-instance events before updating its shared
   repository.
4. **Keep banning independent.** The UDP banning listener remains an
   unconditional subscriber, so a disabled-metrics listener cannot hide cookie
   errors or weaken the intentionally shared ban state.

The design preserves one aggregate UDP server repository and one aggregate
REST API response. It does not require one repository—or one configuration
section—per internal server object.

#### Alternatives rejected or deferred

- **Use UDP core metrics in the REST API.** This restores per-instance
  filtering for announce/connect/scrape counts but loses the richer UDP server
  operational metrics, such as discards, response timing, and errors.
- **Make every UDP server container/repository per instance.** This may be
  useful for future per-listener metrics APIs, but is unnecessary for the
  current aggregate API and increases orchestration complexity.
- **Add `metrics_enabled` to every event payload.** This embeds consumer policy
  in an objective event. Listener-owned policy keyed by stable instance identity
  is clearer and follows ADR 20260727000000.

#### Validation required after implementation

1. Two HTTP listeners and two UDP listeners, one enabled and one disabled per
   protocol: valid announces to both produce aggregate counters of `1` per
   protocol.
2. Invalid cookies through the UDP metrics-disabled listener still update shared
   ban state; after the threshold, strict requests through either listener are
   rejected.
3. A duplicate-`0.0.0.0:0` case proves filtering uses configuration/runtime
   instance identity rather than configured socket address.

## Trade-offs

### Shared Broadcaster (current design)

**Advantages:**

- Single listener per protocol type — simpler bootstrap, less resource usage.
- Natural aggregation: global statistics see all instances.
- Consistent with the tracker being a single logical service.

**Disadvantages:**

- Cannot filter events by instance at the listener level (all events from all
  instances are mixed in the same channel).
- Per-instance statistics are not available through the event system (they would
  require instance metadata in the event or a separate per-instance channel).

### Per-Instance Broadcasters (alternative)

**Advantages:**

- Instance-level filtering at the listener.
- Per-instance statistics without extra metadata.

**Disadvantages:**

- N listeners for N instances — more tasks, more subscriptions.
- Aggregation requires combining N repositories.
- More complex bootstrap and configuration.
