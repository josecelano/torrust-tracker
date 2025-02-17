//! The `scrape` service.
//!
//! The service is responsible for handling the `scrape` requests.
//!
//! It delegates the `scrape` logic to the [`ScrapeHandler`] and it returns the
//! [`ScrapeData`].
//!
//! It also sends an [`http_tracker_core::statistics::event::Event`]
//! because events are specific for the HTTP tracker.
use std::net::IpAddr;
use std::sync::Arc;

use bittorrent_http_protocol::v1::requests::scrape::Scrape;
use bittorrent_http_protocol::v1::responses;
use bittorrent_http_protocol::v1::services::peer_ip_resolver::{self, ClientIpSources};
use bittorrent_primitives::info_hash::InfoHash;
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use torrust_tracker_configuration::Core;
use torrust_tracker_primitives::core::ScrapeData;

use crate::packages::http_tracker_core;

/// The HTTP tracker `scrape` service.
///
/// The service sends an statistics event that increments:
///
/// - The number of TCP connections handled by the HTTP tracker.
/// - The number of TCP `scrape` requests handled by the HTTP tracker.
///
/// > **NOTICE**: as the HTTP tracker does not requires a connection request
/// > like the UDP tracker, the number of TCP connections is incremented for
/// > each `scrape` request.
///
/// # Errors
///
/// This function will return an error if:
///
/// - There is an error when resolving the client IP address.
#[allow(clippy::too_many_arguments)]
pub async fn handle_scrape(
    core_config: &Arc<Core>,
    scrape_handler: &Arc<ScrapeHandler>,
    opt_http_stats_event_sender: &Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>>,
    scrape_request: &Scrape,
    client_ip_sources: &ClientIpSources,
    return_fake_scrape_data: bool,
) -> Result<ScrapeData, responses::error::Error> {
    // Authorization for scrape requests is handled at the `bittorrent-_racker_core`
    // level for each torrent.

    let peer_ip = match peer_ip_resolver::invoke(core_config.net.on_reverse_proxy, client_ip_sources) {
        Ok(peer_ip) => peer_ip,
        Err(error) => return Err(responses::error::Error::from(error)),
    };

    if return_fake_scrape_data {
        return Ok(
            http_tracker_core::services::scrape::fake(opt_http_stats_event_sender, &scrape_request.info_hashes, &peer_ip).await,
        );
    }

    let scrape_data = scrape_handler.scrape(&scrape_request.info_hashes).await?;

    send_scrape_event(&peer_ip, opt_http_stats_event_sender).await;

    Ok(scrape_data)
}

/// The HTTP tracker fake `scrape` service. It returns zeroed stats.
///
/// When the peer is not authenticated and the tracker is running in `private` mode,
/// the tracker returns empty stats for all the torrents.
///
/// > **NOTICE**: tracker statistics are not updated in this case.
pub async fn fake(
    opt_http_stats_event_sender: &Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>>,
    info_hashes: &Vec<InfoHash>,
    original_peer_ip: &IpAddr,
) -> ScrapeData {
    send_scrape_event(original_peer_ip, opt_http_stats_event_sender).await;

    ScrapeData::zeroed(info_hashes)
}

async fn send_scrape_event(
    original_peer_ip: &IpAddr,
    opt_http_stats_event_sender: &Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>>,
) {
    if let Some(http_stats_event_sender) = opt_http_stats_event_sender.as_deref() {
        match original_peer_ip {
            IpAddr::V4(_) => {
                http_stats_event_sender
                    .send_event(http_tracker_core::statistics::event::Event::Tcp4Scrape)
                    .await;
            }
            IpAddr::V6(_) => {
                http_stats_event_sender
                    .send_event(http_tracker_core::statistics::event::Event::Tcp6Scrape)
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;

    use aquatic_udp_protocol::{AnnounceEvent, NumberOfBytes, PeerId};
    use bittorrent_primitives::info_hash::InfoHash;
    use bittorrent_tracker_core::announce_handler::AnnounceHandler;
    use bittorrent_tracker_core::databases::setup::initialize_database;
    use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
    use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;
    use bittorrent_tracker_core::torrent::repository::persisted::DatabasePersistentTorrentRepository;
    use bittorrent_tracker_core::whitelist::authorization::WhitelistAuthorization;
    use bittorrent_tracker_core::whitelist::repository::in_memory::InMemoryWhitelist;
    use futures::future::BoxFuture;
    use mockall::mock;
    use tokio::sync::mpsc::error::SendError;
    use torrust_tracker_configuration::Configuration;
    use torrust_tracker_primitives::{peer, DurationSinceUnixEpoch};
    use torrust_tracker_test_helpers::configuration;

    use crate::packages::http_tracker_core;
    use crate::servers::http::test_helpers::tests::sample_info_hash;

    fn initialize_announce_and_scrape_handlers_for_public_tracker() -> (Arc<AnnounceHandler>, Arc<ScrapeHandler>) {
        initialize_announce_and_scrape_handlers_with_configuration(&configuration::ephemeral_public())
    }

    fn initialize_announce_and_scrape_handlers_with_configuration(
        config: &Configuration,
    ) -> (Arc<AnnounceHandler>, Arc<ScrapeHandler>) {
        let database = initialize_database(&config.core);
        let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
        let whitelist_authorization = Arc::new(WhitelistAuthorization::new(&config.core, &in_memory_whitelist.clone()));
        let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());
        let db_torrent_repository = Arc::new(DatabasePersistentTorrentRepository::new(&database));
        let announce_handler = Arc::new(AnnounceHandler::new(
            &config.core,
            &whitelist_authorization,
            &in_memory_torrent_repository,
            &db_torrent_repository,
        ));
        let scrape_handler = Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository));

        (announce_handler, scrape_handler)
    }

    fn sample_info_hashes() -> Vec<InfoHash> {
        vec![sample_info_hash()]
    }

    fn sample_peer() -> peer::Peer {
        peer::Peer {
            peer_id: PeerId(*b"-qB00000000000000000"),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(126, 0, 0, 1)), 8080),
            updated: DurationSinceUnixEpoch::new(1_669_397_478_934, 0),
            uploaded: NumberOfBytes::new(0),
            downloaded: NumberOfBytes::new(0),
            left: NumberOfBytes::new(0),
            event: AnnounceEvent::Started,
        }
    }

    fn initialize_scrape_handler_with_config(config: &Configuration) -> Arc<ScrapeHandler> {
        let in_memory_whitelist = Arc::new(InMemoryWhitelist::default());
        let whitelist_authorization = Arc::new(WhitelistAuthorization::new(&config.core, &in_memory_whitelist.clone()));
        let in_memory_torrent_repository = Arc::new(InMemoryTorrentRepository::default());

        Arc::new(ScrapeHandler::new(&whitelist_authorization, &in_memory_torrent_repository))
    }

    mock! {
        HttpStatsEventSender {}
        impl http_tracker_core::statistics::event::sender::Sender for HttpStatsEventSender {
             fn send_event(&self, event: http_tracker_core::statistics::event::Event) -> BoxFuture<'static,Option<Result<(),SendError<http_tracker_core::statistics::event::Event> > > > ;
        }
    }

    mod with_real_data {

        use std::future;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use std::sync::Arc;

        use bittorrent_http_protocol::v1::requests::scrape::Scrape;
        use bittorrent_http_protocol::v1::services::peer_ip_resolver::ClientIpSources;
        use bittorrent_tracker_core::announce_handler::PeersWanted;
        use mockall::predicate::eq;
        use torrust_tracker_primitives::core::ScrapeData;
        use torrust_tracker_primitives::swarm_metadata::SwarmMetadata;
        use torrust_tracker_test_helpers::configuration;

        use crate::packages::http_tracker_core::services::scrape::handle_scrape;
        use crate::packages::http_tracker_core::services::scrape::tests::{
            initialize_announce_and_scrape_handlers_with_configuration, initialize_scrape_handler_with_config,
            sample_info_hashes, sample_peer, MockHttpStatsEventSender,
        };
        use crate::packages::{self, http_tracker_core};
        use crate::servers::http::test_helpers::tests::sample_info_hash;

        #[tokio::test]
        async fn it_should_return_the_scrape_data_for_a_torrent() {
            let configuration = configuration::ephemeral_public();
            let core_config = Arc::new(configuration.core.clone());

            let (http_stats_event_sender, _http_stats_repository) =
                packages::http_tracker_core::statistics::setup::factory(false);
            let http_stats_event_sender = Arc::new(http_stats_event_sender);

            let (announce_handler, scrape_handler) = initialize_announce_and_scrape_handlers_with_configuration(&configuration);

            let info_hash = sample_info_hash();
            let info_hashes = vec![info_hash];

            // Announce a new peer to force scrape data to contain non zeroed data
            let mut peer = sample_peer();
            let original_peer_ip = peer.ip();
            announce_handler
                .announce(&info_hash, &mut peer, &original_peer_ip, &PeersWanted::AsManyAsPossible)
                .await
                .unwrap();

            let scrape_request = Scrape {
                info_hashes: info_hashes.clone(),
            };

            let client_ip_sources = ClientIpSources {
                right_most_x_forwarded_for: None,
                connection_info_ip: Some(original_peer_ip),
            };

            let scrape_data = handle_scrape(
                &core_config,
                &scrape_handler,
                &http_stats_event_sender,
                &scrape_request,
                &client_ip_sources,
                false,
            )
            .await
            .unwrap();

            let mut expected_scrape_data = ScrapeData::empty();
            expected_scrape_data.add_file(
                &info_hash,
                SwarmMetadata {
                    complete: 1,
                    downloaded: 0,
                    incomplete: 0,
                },
            );

            assert_eq!(scrape_data, expected_scrape_data);
        }

        #[tokio::test]
        async fn it_should_send_the_tcp_4_scrape_event_when_the_peer_uses_ipv4() {
            let config = configuration::ephemeral();

            let mut http_stats_event_sender_mock = MockHttpStatsEventSender::new();
            http_stats_event_sender_mock
                .expect_send_event()
                .with(eq(http_tracker_core::statistics::event::Event::Tcp4Scrape))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let http_stats_event_sender: Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(http_stats_event_sender_mock)));

            let scrape_handler = initialize_scrape_handler_with_config(&config);

            let peer_ip = IpAddr::V4(Ipv4Addr::new(126, 0, 0, 1));

            let scrape_request = Scrape {
                info_hashes: sample_info_hashes(),
            };

            let client_ip_sources = ClientIpSources {
                right_most_x_forwarded_for: None,
                connection_info_ip: Some(peer_ip),
            };

            handle_scrape(
                &Arc::new(config.core),
                &scrape_handler,
                &http_stats_event_sender,
                &scrape_request,
                &client_ip_sources,
                false,
            )
            .await
            .unwrap();
        }

        #[tokio::test]
        async fn it_should_send_the_tcp_6_scrape_event_when_the_peer_uses_ipv6() {
            let config = configuration::ephemeral();

            let mut http_stats_event_sender_mock = MockHttpStatsEventSender::new();
            http_stats_event_sender_mock
                .expect_send_event()
                .with(eq(http_tracker_core::statistics::event::Event::Tcp6Scrape))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let http_stats_event_sender: Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(http_stats_event_sender_mock)));

            let scrape_handler = initialize_scrape_handler_with_config(&config);

            let peer_ip = IpAddr::V6(Ipv6Addr::new(0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969));

            let scrape_request = Scrape {
                info_hashes: sample_info_hashes(),
            };

            let client_ip_sources = ClientIpSources {
                right_most_x_forwarded_for: None,
                connection_info_ip: Some(peer_ip),
            };

            handle_scrape(
                &Arc::new(config.core),
                &scrape_handler,
                &http_stats_event_sender,
                &scrape_request,
                &client_ip_sources,
                false,
            )
            .await
            .unwrap();
        }
    }

    mod with_zeroed_data {

        use std::future;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use std::sync::Arc;

        use bittorrent_tracker_core::announce_handler::PeersWanted;
        use mockall::predicate::eq;
        use torrust_tracker_primitives::core::ScrapeData;

        use crate::packages::http_tracker_core::services::scrape::fake;
        use crate::packages::http_tracker_core::services::scrape::tests::{
            initialize_announce_and_scrape_handlers_for_public_tracker, sample_info_hashes, sample_peer, MockHttpStatsEventSender,
        };
        use crate::packages::{self, http_tracker_core};
        use crate::servers::http::test_helpers::tests::sample_info_hash;

        #[tokio::test]
        async fn it_should_always_return_the_zeroed_scrape_data_for_a_torrent() {
            let (http_stats_event_sender, _http_stats_repository) =
                packages::http_tracker_core::statistics::setup::factory(false);
            let http_stats_event_sender = Arc::new(http_stats_event_sender);

            let (announce_handler, _scrape_handler) = initialize_announce_and_scrape_handlers_for_public_tracker();

            let info_hash = sample_info_hash();
            let info_hashes = vec![info_hash];

            // Announce a new peer to force scrape data to contain not zeroed data
            let mut peer = sample_peer();
            let original_peer_ip = peer.ip();
            announce_handler
                .announce(&info_hash, &mut peer, &original_peer_ip, &PeersWanted::AsManyAsPossible)
                .await
                .unwrap();

            let scrape_data = fake(&http_stats_event_sender, &info_hashes, &original_peer_ip).await;

            let expected_scrape_data = ScrapeData::zeroed(&info_hashes);

            assert_eq!(scrape_data, expected_scrape_data);
        }

        #[tokio::test]
        async fn it_should_send_the_tcp_4_scrape_event_when_the_peer_uses_ipv4() {
            let mut http_stats_event_sender_mock = MockHttpStatsEventSender::new();
            http_stats_event_sender_mock
                .expect_send_event()
                .with(eq(http_tracker_core::statistics::event::Event::Tcp4Scrape))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let http_stats_event_sender: Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(http_stats_event_sender_mock)));

            let peer_ip = IpAddr::V4(Ipv4Addr::new(126, 0, 0, 1));

            fake(&http_stats_event_sender, &sample_info_hashes(), &peer_ip).await;
        }

        #[tokio::test]
        async fn it_should_send_the_tcp_6_scrape_event_when_the_peer_uses_ipv6() {
            let mut http_stats_event_sender_mock = MockHttpStatsEventSender::new();
            http_stats_event_sender_mock
                .expect_send_event()
                .with(eq(http_tracker_core::statistics::event::Event::Tcp6Scrape))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let http_stats_event_sender: Arc<Option<Box<dyn http_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(http_stats_event_sender_mock)));

            let peer_ip = IpAddr::V6(Ipv6Addr::new(0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969, 0x6969));

            fake(&http_stats_event_sender, &sample_info_hashes(), &peer_ip).await;
        }
    }
}
