use std::sync::Arc;

use tokio::sync::RwLock;
use torrust_tracker_configuration::{Core, UdpTracker};
use torrust_tracker_core::container::TrackerCoreContainer;
use torrust_tracker_swarm_coordination_registry::container::SwarmCoordinationRegistryContainer;

use crate::event::bus::EventBus;
use crate::event::sender::Broadcaster;
use crate::services::announce::AnnounceService;
use crate::services::banning::BanService;
use crate::services::connect::ConnectService;
use crate::services::scrape::ScrapeService;
use crate::statistics::repository::Repository;
use crate::{event, statistics};

pub struct UdpTrackerCoreContainer {
    pub udp_tracker_config: Arc<UdpTracker>,

    pub tracker_core_container: Arc<TrackerCoreContainer>,

    // Per-instance services
    pub event_bus: Arc<event::bus::EventBus>,
    pub stats_event_sender: crate::event::sender::Sender,
    pub stats_repository: Arc<statistics::repository::Repository>,
    pub ban_service: Arc<RwLock<BanService>>,
    pub connect_service: Arc<ConnectService>,
    pub announce_service: Arc<AnnounceService>,
    pub scrape_service: Arc<ScrapeService>,
}

impl UdpTrackerCoreContainer {
    #[must_use]
    pub async fn initialize(core_config: &Arc<Core>, udp_tracker_config: &Arc<UdpTracker>) -> Arc<UdpTrackerCoreContainer> {
        let swarm_coordination_registry_container = Arc::new(SwarmCoordinationRegistryContainer::initialize(
            core_config.tracker_usage_statistics.into(),
        ));

        let tracker_core_container =
            Arc::new(TrackerCoreContainer::initialize_from(core_config, &swarm_coordination_registry_container).await);

        Self::initialize_from_tracker_core(&tracker_core_container, udp_tracker_config)
    }

    #[must_use]
    pub fn initialize_from_tracker_core(
        tracker_core_container: &Arc<TrackerCoreContainer>,
        udp_tracker_config: &Arc<UdpTracker>,
    ) -> Arc<UdpTrackerCoreContainer> {
        let max_connection_id_errors_per_ip = udp_tracker_config.max_connection_id_errors_per_ip;
        let shared_services = UdpTrackerCoreServices::initialize_from(tracker_core_container, max_connection_id_errors_per_ip);

        Self::initialize_from_services(
            tracker_core_container,
            &shared_services.broadcaster,
            &shared_services.stats_repository,
            &shared_services.ban_service,
            udp_tracker_config,
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
        ban_service: &Arc<RwLock<BanService>>,
        udp_tracker_config: &Arc<UdpTracker>,
    ) -> Arc<Self> {
        let per_instance_event_bus = Arc::new(EventBus::new(
            udp_tracker_config.tracker_usage_statistics.into(),
            broadcaster.clone(),
        ));

        let per_instance_stats_event_sender = per_instance_event_bus.sender();

        let announce_service = Arc::new(AnnounceService::new(
            tracker_core_container.announce_handler.clone(),
            tracker_core_container.whitelist_authorization.clone(),
            per_instance_stats_event_sender.clone(),
        ));

        let scrape_service = Arc::new(ScrapeService::new(
            tracker_core_container.scrape_handler.clone(),
            per_instance_stats_event_sender.clone(),
        ));

        let connect_service = Arc::new(ConnectService::new(per_instance_stats_event_sender.clone()));

        Arc::new(Self {
            udp_tracker_config: udp_tracker_config.clone(),
            tracker_core_container: tracker_core_container.clone(),
            event_bus: per_instance_event_bus,
            stats_event_sender: per_instance_stats_event_sender,
            stats_repository: stats_repository.clone(),
            ban_service: ban_service.clone(),
            connect_service,
            announce_service,
            scrape_service,
        })
    }
}

/// Shared infrastructure across all UDP tracker instances.
///
/// Contains only the resources that are genuinely shared: the broadcaster
/// channel, statistics repository, and ban service.
///
/// Per-instance services (announce, scrape, connect) are created in
/// [`UdpTrackerCoreContainer::initialize_from_services`].
pub struct UdpTrackerCoreServices {
    pub broadcaster: Broadcaster,
    pub stats_repository: Arc<statistics::repository::Repository>,
    pub ban_service: Arc<RwLock<BanService>>,
}

impl UdpTrackerCoreServices {
    #[must_use]
    pub fn initialize_from(
        _tracker_core_container: &Arc<TrackerCoreContainer>,
        max_connection_id_errors_per_ip: u32,
    ) -> Arc<Self> {
        let broadcaster = Broadcaster::default();
        let stats_repository = Arc::new(Repository::new());
        let ban_service = Arc::new(RwLock::new(BanService::new(max_connection_id_errors_per_ip)));

        Arc::new(Self {
            broadcaster,
            stats_repository,
            ban_service,
        })
    }
}
