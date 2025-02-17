//! The `scrape` service.
//!
//! The service is responsible for handling the `scrape` requests.
//!
//! It delegates the `scrape` logic to the [`ScrapeHandler`] and it returns the
//! [`ScrapeData`].
//!
//! It also sends an [`udp_tracker_core::statistics::event::Event`]
//! because events are specific for the UDP tracker.
use std::net::SocketAddr;
use std::sync::Arc;

use aquatic_udp_protocol::ScrapeRequest;
use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::error::ScrapeError;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use torrust_tracker_primitives::core::ScrapeData;

use crate::packages::udp_tracker_core;

/// It handles the `Scrape` request.
///
/// # Errors
///
/// It will return an error if the tracker core scrape handler returns an error.
pub async fn handle_scrape(
    remote_addr: SocketAddr,
    request: &ScrapeRequest,
    scrape_handler: &Arc<ScrapeHandler>,
    opt_udp_stats_event_sender: &Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>>,
) -> Result<ScrapeData, ScrapeError> {
    // Convert from aquatic infohashes
    let mut info_hashes: Vec<InfoHash> = vec![];
    for info_hash in &request.info_hashes {
        info_hashes.push((*info_hash).into());
    }

    let scrape_data = scrape_handler.scrape(&info_hashes).await?;

    if let Some(udp_stats_event_sender) = opt_udp_stats_event_sender.as_deref() {
        match remote_addr {
            SocketAddr::V4(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp4Scrape)
                    .await;
            }
            SocketAddr::V6(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp6Scrape)
                    .await;
            }
        }
    }

    Ok(scrape_data)
}
