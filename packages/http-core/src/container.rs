use std::sync::Arc;

use torrust_tracker_configuration::{Core, HttpTracker};
use torrust_tracker_core::container::TrackerCoreContainer;
use torrust_tracker_swarm_coordination_registry::container::SwarmCoordinationRegistryContainer;

use crate::event::bus::EventBus;
use crate::event::sender::Broadcaster;
use crate::services::announce::AnnounceService;
use crate::services::scrape::ScrapeService;
use crate::statistics::repository::Repository;
use crate::{event, statistics};

pub struct HttpTrackerCoreContainer {
    pub http_tracker_config: Arc<HttpTracker>,

    pub tracker_core_container: Arc<TrackerCoreContainer>,

    // Per-instance services
    pub event_bus: Arc<event::bus::EventBus>,
    pub stats_event_sender: event::sender::Sender,
    pub stats_repository: Arc<statistics::repository::Repository>,
    pub announce_service: Arc<AnnounceService>,
    pub scrape_service: Arc<ScrapeService>,
}

impl HttpTrackerCoreContainer {
    #[must_use]
    pub async fn initialize(core_config: &Arc<Core>, http_tracker_config: &Arc<HttpTracker>) -> Arc<Self> {
        let swarm_coordination_registry_container = Arc::new(SwarmCoordinationRegistryContainer::initialize(
            core_config.tracker_usage_statistics.into(),
        ));

        let tracker_core_container =
            Arc::new(TrackerCoreContainer::initialize_from(core_config, &swarm_coordination_registry_container).await);

        Self::initialize_from_tracker_core(&tracker_core_container, http_tracker_config)
    }

    #[must_use]
    pub fn initialize_from_tracker_core(
        tracker_core_container: &Arc<TrackerCoreContainer>,
        http_tracker_config: &Arc<HttpTracker>,
    ) -> Arc<Self> {
        let shared_services = HttpTrackerCoreServices::initialize_from(tracker_core_container);

        Self::initialize_from_services(
            tracker_core_container,
            &shared_services.broadcaster,
            &shared_services.stats_repository,
            http_tracker_config,
        )
    }

    /// Creates a per-instance container with its own EventBus and services.
    ///
    /// The `broadcaster` is shared across all instances so the global event
    /// listener receives events from every instance. The per-instance
    /// `EventBus` uses the individual `tracker_usage_statistics` setting to
    /// control whether events are actually sent.
    #[must_use]
    pub fn initialize_from_services(
        tracker_core_container: &Arc<TrackerCoreContainer>,
        broadcaster: &Broadcaster,
        stats_repository: &Arc<statistics::repository::Repository>,
        http_tracker_config: &Arc<HttpTracker>,
    ) -> Arc<Self> {
        let per_instance_event_bus = Arc::new(EventBus::new(
            http_tracker_config.tracker_usage_statistics.into(),
            broadcaster.clone(),
        ));

        let per_instance_stats_event_sender = per_instance_event_bus.sender();

        let announce_service = Arc::new(AnnounceService::new(
            tracker_core_container.core_config.clone(),
            tracker_core_container.announce_handler.clone(),
            tracker_core_container.authentication_service.clone(),
            tracker_core_container.whitelist_authorization.clone(),
            per_instance_stats_event_sender.clone(),
        ));

        let scrape_service = Arc::new(ScrapeService::new(
            tracker_core_container.core_config.clone(),
            tracker_core_container.scrape_handler.clone(),
            tracker_core_container.authentication_service.clone(),
            per_instance_stats_event_sender.clone(),
        ));

        Arc::new(Self {
            tracker_core_container: tracker_core_container.clone(),
            http_tracker_config: http_tracker_config.clone(),
            event_bus: per_instance_event_bus,
            stats_event_sender: per_instance_stats_event_sender,
            stats_repository: stats_repository.clone(),
            announce_service,
            scrape_service,
        })
    }
}

/// Shared infrastructure across all HTTP tracker instances.
///
/// Contains only the resources that are genuinely shared: the broadcaster
/// channel (so the global event listener receives events from all instances)
/// and the statistics repository (aggregated across all instances).
///
/// Per-instance services (announce, scrape) are created in
/// [`HttpTrackerCoreContainer::initialize_from_services`].
pub struct HttpTrackerCoreServices {
    pub broadcaster: Broadcaster,
    pub stats_repository: Arc<statistics::repository::Repository>,
}

impl HttpTrackerCoreServices {
    #[must_use]
    pub fn initialize_from(_tracker_core_container: &Arc<TrackerCoreContainer>) -> Arc<Self> {
        let broadcaster = Broadcaster::default();
        let stats_repository = Arc::new(Repository::new());

        Arc::new(Self {
            broadcaster,
            stats_repository,
        })
    }
}
