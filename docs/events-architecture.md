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
- The per-instance `EventBus` is just a **gate** — when `SenderStatus::Disabled`,
  `sender()` returns `None` and the service skips the `send()` call entirely.
- Filtering by instance is not done at the listener level; events from all
  instances are aggregated into shared statistics repositories.

### Per-Instance Event Buses (UDP Server)

The UDP **server** layer uses a per-instance `EventBus` in
`UdpTrackerServerContainer`. This is because server-level events (request
received, banned, error) are specific to one listener's network binding and
processing pipeline.

### Event Bus Inventory

| Event Bus                              | Scope                            | Created In                                           | Subscribers                                    |
| -------------------------------------- | -------------------------------- | ---------------------------------------------------- | ---------------------------------------------- |
| HTTP core `Broadcaster`                | Shared across all HTTP instances | `HttpTrackerCoreServices::initialize_from`           | 1 (global stats listener)                      |
| HTTP per-instance `EventBus`           | Per HTTP tracker instance        | `HttpTrackerCoreContainer::initialize_from_services` | 0 (uses shared broadcaster)                    |
| UDP core `Broadcaster`                 | Shared across all UDP instances  | `UdpTrackerCoreServices::initialize_from`            | 1 (global stats listener)                      |
| UDP per-instance `EventBus`            | Per UDP tracker instance         | `UdpTrackerCoreContainer::initialize_from_services`  | 0 (uses shared broadcaster)                    |
| UDP server `EventBus`                  | Single (server-level)            | `UdpTrackerServerContainer::initialize`              | 2 (stats listener + ban listener)              |
| Swarm coordination registry `EventBus` | Single (global)                  | `SwarmCoordinationRegistryContainer::initialize`     | 2 (SCR stats listener + tracker-core listener) |

## Event Listeners (Consumer Side)

Each listener runs as a Tokio task, started in `src/bootstrap/jobs/`. All are
gated by `config.core.tracker_usage_statistics`.

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
