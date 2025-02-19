use std::sync::Arc;

use bittorrent_tracker_core::announce_handler::AnnounceHandler;
use bittorrent_tracker_core::authentication::service::AuthenticationService;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use bittorrent_tracker_core::whitelist;
use torrust_tracker_configuration::{Core, HttpTracker};

pub struct HttpTrackerContainer {
    pub core_config: Arc<Core>,
    pub http_tracker_config: Arc<HttpTracker>,
    pub announce_handler: Arc<AnnounceHandler>,
    pub scrape_handler: Arc<ScrapeHandler>,
    pub whitelist_authorization: Arc<whitelist::authorization::WhitelistAuthorization>,
    pub http_stats_event_sender: Arc<Option<Box<dyn bittorrent_http_tracker_core::statistics::event::sender::Sender>>>,
    pub authentication_service: Arc<AuthenticationService>,
}
