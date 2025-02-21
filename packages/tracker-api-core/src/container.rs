use std::sync::Arc;

use bittorrent_tracker_core::authentication::handler::KeysHandler;
use bittorrent_tracker_core::container::TrackerCoreContainer;
use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
use bittorrent_tracker_core::whitelist::manager::WhitelistManager;
use bittorrent_udp_tracker_core::services::banning::BanService;
use bittorrent_udp_tracker_core::{self, MAX_CONNECTION_ID_ERRORS_PER_IP};
use tokio::sync::RwLock;
use torrust_tracker_configuration::{Core, HttpApi};

pub struct HttpApiContainer {
    pub core_config: Arc<Core>,
    pub http_api_config: Arc<HttpApi>,
    pub in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
    pub keys_handler: Arc<KeysHandler>,
    pub whitelist_manager: Arc<WhitelistManager>,
    pub ban_service: Arc<RwLock<BanService>>,
    pub http_stats_repository: Arc<bittorrent_http_tracker_core::statistics::repository::Repository>,
    pub udp_stats_repository: Arc<bittorrent_udp_tracker_core::statistics::repository::Repository>,
}

#[must_use]
pub fn initialize_http_api_container(core_config: &Arc<Core>, http_api_config: &Arc<HttpApi>) -> Arc<HttpApiContainer> {
    let tracker_core_container = TrackerCoreContainer::initialize(core_config);

    // HTTP stats
    let (_http_stats_event_sender, http_stats_repository) =
        bittorrent_http_tracker_core::statistics::setup::factory(core_config.tracker_usage_statistics);
    let http_stats_repository = Arc::new(http_stats_repository);

    // UDP stats
    let (_udp_stats_event_sender, udp_stats_repository) =
        bittorrent_udp_tracker_core::statistics::setup::factory(core_config.tracker_usage_statistics);
    let udp_stats_repository = Arc::new(udp_stats_repository);

    let ban_service = Arc::new(RwLock::new(BanService::new(MAX_CONNECTION_ID_ERRORS_PER_IP)));

    Arc::new(HttpApiContainer {
        http_api_config: http_api_config.clone(),
        core_config: core_config.clone(),
        in_memory_torrent_repository: tracker_core_container.in_memory_torrent_repository.clone(),
        keys_handler: tracker_core_container.keys_handler.clone(),
        whitelist_manager: tracker_core_container.whitelist_manager.clone(),
        ban_service: ban_service.clone(),
        http_stats_repository: http_stats_repository.clone(),
        udp_stats_repository: udp_stats_repository.clone(),
    })
}
