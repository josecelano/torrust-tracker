---
semantic-links:
  related-artifacts:
    - docs/adrs/index.md
    - docs/events-architecture.md
    - packages/udp-core/src/container.rs
    - packages/udp-core/src/services/banning.rs
    - packages/http-core/src/container.rs
    - packages/tracker-core/src/container.rs
    - packages/configuration/src/v3_0_0/udp_tracker_server.rs
    - src/container.rs
---

# Shared Services Across Tracker Instances

## Description

The tracker can run multiple UDP and HTTP tracker listeners in a single process.
Each listener binds to a different address/port but shares core infrastructure:

- **Peer repository** (`TrackerCoreContainer`) — all instances share the same
  swarm data (torrents, peers, statistics). This is the primary reason to run
  multiple listeners: they serve the same swarm.
- **Ban service** (`BanService` in `UdpTrackerCoreServices`) — all UDP instances
  share the same IP-ban state. An IP banned on one UDP listener is banned on all.
- **Event buses** — core-layer events are shared via a single `Broadcaster`
  channel per protocol type; server-layer events are per-instance.
- **Statistics repositories** — each protocol layer has a single shared
  repository that aggregates metrics from all instances.

## Agreement

### Shared services

The following services are created once and shared across all instances of the
same type:

| Service                                       | Location                                      | Shared? | Rationale                                                                                                                                                                                                       |
| --------------------------------------------- | --------------------------------------------- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Peer repository                               | `TrackerCoreContainer`                        | Yes     | All listeners serve the same swarm                                                                                                                                                                              |
| Swarm coordination registry                   | `SwarmCoordinationRegistryContainer`          | Yes     | Single source of truth for swarm state                                                                                                                                                                          |
| UDP ban service                               | `UdpTrackerCoreServices::ban_service`         | Yes     | Resource protection: an attacker should not be able to consume N× resources by attacking N listeners independently                                                                                              |
| UDP core event bus                            | `UdpTrackerCoreServices::broadcaster`         | Yes     | Core events (connect, announce, scrape) are objective facts about the swarm, not about a specific listener. A single `Broadcaster` channel is shared; per-instance `EventBus` gates sending via `SenderStatus`. |
| UDP core services (connect, announce, scrape) | `UdpTrackerCoreContainer` (per instance)      | No      | Per-instance; each container creates its own services with a per-instance `EventBus` that wraps the shared `Broadcaster`. Services are stateless but need per-instance `tracker_usage_statistics` gating.       |
| HTTP core event bus                           | `HttpTrackerCoreServices::broadcaster`        | Yes     | Same pattern as UDP core. Single `Broadcaster` shared across all HTTP instances.                                                                                                                                |
| HTTP core services (announce, scrape)         | `HttpTrackerCoreContainer` (per instance)     | No      | Per-instance; each container creates its own services with a per-instance `EventBus`.                                                                                                                           |
| UDP server event bus                          | `UdpTrackerServerContainer::event_bus`        | **No**  | Per-instance; server events (request accepted, banned, error) are specific to one listener                                                                                                                      |
| UDP server stats repository                   | `UdpTrackerServerContainer::stats_repository` | **No**  | Per-instance; each listener has its own statistics                                                                                                                                                              |

### Why the ban service is shared

The ban service protects server resources by rate-limiting misbehaving IPs.
If each UDP listener had its own independent ban service, an attacker could
send `max_connection_id_errors_per_ip` invalid requests to each listener
independently, consuming N× the allowed error budget. A shared ban service
ensures that the total error rate across all listeners is bounded.

This is consistent with the principle that the tracker is a single logical
service, even when it exposes multiple network endpoints.

### Consequences for per-listener configuration

Settings that affect shared services must themselves be global. For example:

- `connection_id_validation` (issue #1136) controls whether the shared ban
  service's enforcement is active. It must be a global setting because the
  ban service is global — a per-instance policy would create an inconsistency
  where one listener's traffic pollutes the shared ban counter that another
  listener enforces against.

Settings that are inherently per-listener (bind address, cookie lifetime,
public URL, network topology) remain on the per-instance config struct.

### Alternatives Considered

**Per-instance ban service.**

Rejected because it allows an attacker to multiply resource consumption by
the number of listeners. It also complicates the operator's mental model:
"why did I ban this IP on port 6969 but not on port 6970?"

**Per-instance peer repository.**

Rejected because the primary reason to run multiple listeners is to serve
the same swarm through different protocols or addresses. Isolated peer
repositories would defeat this purpose.

**Per-instance event broadcasters (one broadcast channel per listener).**

Considered but rejected for core-layer events. With per-instance
broadcasters, each listener would need its own event listener task, and
aggregating statistics across instances would require combining N
repositories. The shared broadcaster pattern keeps the bootstrap simple
(one listener per protocol type) and naturally aggregates metrics.

The trade-off is that events from all instances are mixed in the same
channel. Instance-level filtering is not possible at the listener level.
This is acceptable because core-layer events are objective facts about the
swarm, not about a specific listener — the listener's job is to aggregate,
not to distinguish. If per-instance statistics are needed in the future,
instance metadata can be added to the event payload without changing the
broadcasting topology.

The UDP **server** layer uses per-instance event buses because server-level
events (request received, banned, error) are specific to one listener's
network binding and are not aggregated across instances. This is documented
in [events-architecture.md](../events-architecture.md).

### Consequences

#### Positive

- Resource protection scales with the number of listeners.
- Operators have a single ban list to reason about.
- Configuration for shared services is naturally global, avoiding
  per-instance inconsistencies.

#### Negative

- Per-listener policies that interact with shared services (like
  `connection_id_validation`) must be global, reducing flexibility.
- A misconfigured listener on one port can affect the ban state for all
  listeners.
