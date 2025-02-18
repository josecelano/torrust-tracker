//! UDP tracker scrape handler.
use std::net::SocketAddr;
use std::ops::Range;
use std::sync::Arc;

use aquatic_udp_protocol::{
    NumberOfDownloads, NumberOfPeers, Response, ScrapeRequest, ScrapeResponse, TorrentScrapeStatistics, TransactionId,
};
use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
use torrust_tracker_primitives::core::ScrapeData;
use tracing::{instrument, Level};
use zerocopy::network_endian::I32;

use crate::packages::udp_tracker_core;
use crate::servers::udp::error::Error;

/// It handles the `Scrape` request. Refer to [`Scrape`](crate::servers::udp#scrape)
/// request for more information.
///
/// # Errors
///
/// This function does not ever return an error.
#[instrument(fields(transaction_id, connection_id), skip(scrape_handler, opt_udp_stats_event_sender),  ret(level = Level::TRACE))]
pub async fn handle_scrape(
    remote_addr: SocketAddr,
    request: &ScrapeRequest,
    scrape_handler: &Arc<ScrapeHandler>,
    opt_udp_stats_event_sender: &Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>>,
    cookie_valid_range: Range<f64>,
) -> Result<Response, (Error, TransactionId)> {
    tracing::Span::current()
        .record("transaction_id", request.transaction_id.0.to_string())
        .record("connection_id", request.connection_id.0.to_string());

    tracing::trace!("handle scrape");

    let scrape_data = udp_tracker_core::services::scrape::handle_scrape(
        remote_addr,
        request,
        scrape_handler,
        opt_udp_stats_event_sender,
        cookie_valid_range,
    )
    .await
    .map_err(|e| (e.into(), request.transaction_id))?;

    Ok(build_response(request, &scrape_data))
}

fn build_response(request: &ScrapeRequest, scrape_data: &ScrapeData) -> Response {
    let mut torrent_stats: Vec<TorrentScrapeStatistics> = Vec::new();

    for file in &scrape_data.files {
        let swarm_metadata = file.1;

        #[allow(clippy::cast_possible_truncation)]
        let scrape_entry = {
            TorrentScrapeStatistics {
                seeders: NumberOfPeers(I32::new(i64::from(swarm_metadata.complete) as i32)),
                completed: NumberOfDownloads(I32::new(i64::from(swarm_metadata.downloaded) as i32)),
                leechers: NumberOfPeers(I32::new(i64::from(swarm_metadata.incomplete) as i32)),
            }
        };

        torrent_stats.push(scrape_entry);
    }

    let response = ScrapeResponse {
        transaction_id: request.transaction_id,
        torrent_stats,
    };

    Response::from(response)
}

#[cfg(test)]
mod tests {

    mod scrape_request {
        use std::net::SocketAddr;
        use std::sync::Arc;

        use aquatic_udp_protocol::{
            InfoHash, NumberOfDownloads, NumberOfPeers, PeerId, Response, ScrapeRequest, ScrapeResponse, TorrentScrapeStatistics,
            TransactionId,
        };
        use bittorrent_tracker_core::scrape_handler::ScrapeHandler;
        use bittorrent_tracker_core::torrent::repository::in_memory::InMemoryTorrentRepository;

        use crate::packages;
        use crate::packages::udp_tracker_core::connection_cookie::{gen_remote_fingerprint, make};
        use crate::servers::udp::handlers::handle_scrape;
        use crate::servers::udp::handlers::tests::{
            initialize_core_tracker_services_for_public_tracker, sample_cookie_valid_range, sample_ipv4_remote_addr,
            sample_issue_time, TorrentPeerBuilder,
        };

        fn zeroed_torrent_statistics() -> TorrentScrapeStatistics {
            TorrentScrapeStatistics {
                seeders: NumberOfPeers(0.into()),
                completed: NumberOfDownloads(0.into()),
                leechers: NumberOfPeers(0.into()),
            }
        }

        #[tokio::test]
        async fn should_return_no_stats_when_the_tracker_does_not_have_any_torrent() {
            let (core_tracker_services, core_udp_tracker_services) = initialize_core_tracker_services_for_public_tracker();

            let remote_addr = sample_ipv4_remote_addr();

            let info_hash = InfoHash([0u8; 20]);
            let info_hashes = vec![info_hash];

            let request = ScrapeRequest {
                connection_id: make(gen_remote_fingerprint(&remote_addr), sample_issue_time()).unwrap(),
                transaction_id: TransactionId(0i32.into()),
                info_hashes,
            };

            let response = handle_scrape(
                remote_addr,
                &request,
                &core_tracker_services.scrape_handler,
                &core_udp_tracker_services.udp_stats_event_sender,
                sample_cookie_valid_range(),
            )
            .await
            .unwrap();

            let expected_torrent_stats = vec![zeroed_torrent_statistics()];

            assert_eq!(
                response,
                Response::from(ScrapeResponse {
                    transaction_id: request.transaction_id,
                    torrent_stats: expected_torrent_stats
                })
            );
        }

        async fn add_a_seeder(
            in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
            remote_addr: &SocketAddr,
            info_hash: &InfoHash,
        ) {
            let peer_id = PeerId([255u8; 20]);

            let peer = TorrentPeerBuilder::new()
                .with_peer_id(peer_id)
                .with_peer_address(*remote_addr)
                .with_number_of_bytes_left(0)
                .into();

            let () = in_memory_torrent_repository.upsert_peer(&info_hash.0.into(), &peer);
        }

        fn build_scrape_request(remote_addr: &SocketAddr, info_hash: &InfoHash) -> ScrapeRequest {
            let info_hashes = vec![*info_hash];

            ScrapeRequest {
                connection_id: make(gen_remote_fingerprint(remote_addr), sample_issue_time()).unwrap(),
                transaction_id: TransactionId::new(0i32),
                info_hashes,
            }
        }

        async fn add_a_sample_seeder_and_scrape(
            in_memory_torrent_repository: Arc<InMemoryTorrentRepository>,
            scrape_handler: Arc<ScrapeHandler>,
        ) -> Response {
            let (udp_stats_event_sender, _udp_stats_repository) = packages::udp_tracker_core::statistics::setup::factory(false);
            let udp_stats_event_sender = Arc::new(udp_stats_event_sender);

            let remote_addr = sample_ipv4_remote_addr();
            let info_hash = InfoHash([0u8; 20]);

            add_a_seeder(in_memory_torrent_repository.clone(), &remote_addr, &info_hash).await;

            let request = build_scrape_request(&remote_addr, &info_hash);

            handle_scrape(
                remote_addr,
                &request,
                &scrape_handler,
                &udp_stats_event_sender,
                sample_cookie_valid_range(),
            )
            .await
            .unwrap()
        }

        fn match_scrape_response(response: Response) -> Option<ScrapeResponse> {
            match response {
                Response::Scrape(scrape_response) => Some(scrape_response),
                _ => None,
            }
        }

        mod with_a_public_tracker {
            use aquatic_udp_protocol::{NumberOfDownloads, NumberOfPeers, TorrentScrapeStatistics};

            use crate::servers::udp::handlers::scrape::tests::scrape_request::{
                add_a_sample_seeder_and_scrape, match_scrape_response,
            };
            use crate::servers::udp::handlers::tests::initialize_core_tracker_services_for_public_tracker;

            #[tokio::test]
            async fn should_return_torrent_statistics_when_the_tracker_has_the_requested_torrent() {
                let (core_tracker_services, _core_udp_tracker_services) = initialize_core_tracker_services_for_public_tracker();

                let torrent_stats = match_scrape_response(
                    add_a_sample_seeder_and_scrape(
                        core_tracker_services.in_memory_torrent_repository.clone(),
                        core_tracker_services.scrape_handler.clone(),
                    )
                    .await,
                );

                let expected_torrent_stats = vec![TorrentScrapeStatistics {
                    seeders: NumberOfPeers(1.into()),
                    completed: NumberOfDownloads(0.into()),
                    leechers: NumberOfPeers(0.into()),
                }];

                assert_eq!(torrent_stats.unwrap().torrent_stats, expected_torrent_stats);
            }
        }

        mod with_a_whitelisted_tracker {
            use aquatic_udp_protocol::{InfoHash, NumberOfDownloads, NumberOfPeers, TorrentScrapeStatistics};

            use crate::servers::udp::handlers::handle_scrape;
            use crate::servers::udp::handlers::scrape::tests::scrape_request::{
                add_a_seeder, build_scrape_request, match_scrape_response, zeroed_torrent_statistics,
            };
            use crate::servers::udp::handlers::tests::{
                initialize_core_tracker_services_for_listed_tracker, sample_cookie_valid_range, sample_ipv4_remote_addr,
            };

            #[tokio::test]
            async fn should_return_the_torrent_statistics_when_the_requested_torrent_is_whitelisted() {
                let (core_tracker_services, core_udp_tracker_services) = initialize_core_tracker_services_for_listed_tracker();

                let remote_addr = sample_ipv4_remote_addr();
                let info_hash = InfoHash([0u8; 20]);

                add_a_seeder(
                    core_tracker_services.in_memory_torrent_repository.clone(),
                    &remote_addr,
                    &info_hash,
                )
                .await;

                core_tracker_services.in_memory_whitelist.add(&info_hash.0.into()).await;

                let request = build_scrape_request(&remote_addr, &info_hash);

                let torrent_stats = match_scrape_response(
                    handle_scrape(
                        remote_addr,
                        &request,
                        &core_tracker_services.scrape_handler,
                        &core_udp_tracker_services.udp_stats_event_sender,
                        sample_cookie_valid_range(),
                    )
                    .await
                    .unwrap(),
                )
                .unwrap();

                let expected_torrent_stats = vec![TorrentScrapeStatistics {
                    seeders: NumberOfPeers(1.into()),
                    completed: NumberOfDownloads(0.into()),
                    leechers: NumberOfPeers(0.into()),
                }];

                assert_eq!(torrent_stats.torrent_stats, expected_torrent_stats);
            }

            #[tokio::test]
            async fn should_return_zeroed_statistics_when_the_requested_torrent_is_not_whitelisted() {
                let (core_tracker_services, core_udp_tracker_services) = initialize_core_tracker_services_for_listed_tracker();

                let remote_addr = sample_ipv4_remote_addr();
                let info_hash = InfoHash([0u8; 20]);

                add_a_seeder(
                    core_tracker_services.in_memory_torrent_repository.clone(),
                    &remote_addr,
                    &info_hash,
                )
                .await;

                let request = build_scrape_request(&remote_addr, &info_hash);

                let torrent_stats = match_scrape_response(
                    handle_scrape(
                        remote_addr,
                        &request,
                        &core_tracker_services.scrape_handler,
                        &core_udp_tracker_services.udp_stats_event_sender,
                        sample_cookie_valid_range(),
                    )
                    .await
                    .unwrap(),
                )
                .unwrap();

                let expected_torrent_stats = vec![zeroed_torrent_statistics()];

                assert_eq!(torrent_stats.torrent_stats, expected_torrent_stats);
            }
        }

        fn sample_scrape_request(remote_addr: &SocketAddr) -> ScrapeRequest {
            let info_hash = InfoHash([0u8; 20]);
            let info_hashes = vec![info_hash];

            ScrapeRequest {
                connection_id: make(gen_remote_fingerprint(remote_addr), sample_issue_time()).unwrap(),
                transaction_id: TransactionId(0i32.into()),
                info_hashes,
            }
        }

        mod using_ipv4 {
            use std::future;
            use std::sync::Arc;

            use mockall::predicate::eq;

            use super::sample_scrape_request;
            use crate::packages::udp_tracker_core;
            use crate::servers::udp::handlers::handle_scrape;
            use crate::servers::udp::handlers::tests::{
                initialize_core_tracker_services_for_default_tracker_configuration, sample_cookie_valid_range,
                sample_ipv4_remote_addr, MockUdpStatsEventSender,
            };

            #[tokio::test]
            async fn should_send_the_upd4_scrape_event() {
                let mut udp_stats_event_sender_mock = MockUdpStatsEventSender::new();
                udp_stats_event_sender_mock
                    .expect_send_event()
                    .with(eq(udp_tracker_core::statistics::event::Event::Udp4Scrape))
                    .times(1)
                    .returning(|_| Box::pin(future::ready(Some(Ok(())))));
                let udp_stats_event_sender: Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>> =
                    Arc::new(Some(Box::new(udp_stats_event_sender_mock)));

                let remote_addr = sample_ipv4_remote_addr();

                let (core_tracker_services, _core_udp_tracker_services) =
                    initialize_core_tracker_services_for_default_tracker_configuration();

                handle_scrape(
                    remote_addr,
                    &sample_scrape_request(&remote_addr),
                    &core_tracker_services.scrape_handler,
                    &udp_stats_event_sender,
                    sample_cookie_valid_range(),
                )
                .await
                .unwrap();
            }
        }

        mod using_ipv6 {
            use std::future;
            use std::sync::Arc;

            use mockall::predicate::eq;

            use super::sample_scrape_request;
            use crate::packages::udp_tracker_core;
            use crate::servers::udp::handlers::handle_scrape;
            use crate::servers::udp::handlers::tests::{
                initialize_core_tracker_services_for_default_tracker_configuration, sample_cookie_valid_range,
                sample_ipv6_remote_addr, MockUdpStatsEventSender,
            };

            #[tokio::test]
            async fn should_send_the_upd6_scrape_event() {
                let mut udp_stats_event_sender_mock = MockUdpStatsEventSender::new();
                udp_stats_event_sender_mock
                    .expect_send_event()
                    .with(eq(udp_tracker_core::statistics::event::Event::Udp6Scrape))
                    .times(1)
                    .returning(|_| Box::pin(future::ready(Some(Ok(())))));
                let udp_stats_event_sender: Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>> =
                    Arc::new(Some(Box::new(udp_stats_event_sender_mock)));

                let remote_addr = sample_ipv6_remote_addr();

                let (core_tracker_services, _core_udp_tracker_services) =
                    initialize_core_tracker_services_for_default_tracker_configuration();

                handle_scrape(
                    remote_addr,
                    &sample_scrape_request(&remote_addr),
                    &core_tracker_services.scrape_handler,
                    &udp_stats_event_sender,
                    sample_cookie_valid_range(),
                )
                .await
                .unwrap();
            }
        }
    }
}
