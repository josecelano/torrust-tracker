//! The `connect` service.
//!
//! The service is responsible for handling the `connect` requests.
use std::net::SocketAddr;
use std::sync::Arc;

use aquatic_udp_protocol::ConnectionId;

use crate::packages::udp_tracker_core;
use crate::packages::udp_tracker_core::connection_cookie::{gen_remote_fingerprint, make};

/// # Panics
///
/// IT will panic if there was an error making the connection cookie.
pub async fn handle_connect(
    remote_addr: SocketAddr,
    opt_udp_stats_event_sender: &Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>>,
    cookie_issue_time: f64,
) -> ConnectionId {
    // todo: return a UDP response like the HTTP tracker instead of raw ConnectionId.

    let connection_id = make(gen_remote_fingerprint(&remote_addr), cookie_issue_time).expect("it should be a normal value");

    if let Some(udp_stats_event_sender) = opt_udp_stats_event_sender.as_deref() {
        match remote_addr {
            SocketAddr::V4(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp4Connect)
                    .await;
            }
            SocketAddr::V6(_) => {
                udp_stats_event_sender
                    .send_event(udp_tracker_core::statistics::event::Event::Udp6Connect)
                    .await;
            }
        }
    }

    connection_id
}

#[cfg(test)]
mod tests {

    mod connect_request {

        use std::future;
        use std::sync::Arc;

        use mockall::predicate::eq;

        use crate::packages::udp_tracker_core::connection_cookie::make;
        use crate::packages::udp_tracker_core::services::connect::handle_connect;
        use crate::packages::udp_tracker_core::services::tests::{
            sample_ipv4_remote_addr, sample_ipv4_remote_addr_fingerprint, sample_ipv4_socket_address, sample_ipv6_remote_addr,
            sample_ipv6_remote_addr_fingerprint, sample_issue_time, MockUdpStatsEventSender,
        };
        use crate::packages::{self, udp_tracker_core};

        #[tokio::test]
        async fn a_connect_response_should_contain_the_same_transaction_id_as_the_connect_request() {
            let (udp_stats_event_sender, _udp_stats_repository) = packages::udp_tracker_core::statistics::setup::factory(false);
            let udp_stats_event_sender = Arc::new(udp_stats_event_sender);

            let response = handle_connect(sample_ipv4_remote_addr(), &udp_stats_event_sender, sample_issue_time()).await;

            assert_eq!(
                response,
                make(sample_ipv4_remote_addr_fingerprint(), sample_issue_time()).unwrap()
            );
        }

        #[tokio::test]
        async fn a_connect_response_should_contain_a_new_connection_id() {
            let (udp_stats_event_sender, _udp_stats_repository) = packages::udp_tracker_core::statistics::setup::factory(false);
            let udp_stats_event_sender = Arc::new(udp_stats_event_sender);

            let response = handle_connect(sample_ipv4_remote_addr(), &udp_stats_event_sender, sample_issue_time()).await;

            assert_eq!(
                response,
                make(sample_ipv4_remote_addr_fingerprint(), sample_issue_time()).unwrap(),
            );
        }

        #[tokio::test]
        async fn a_connect_response_should_contain_a_new_connection_id_ipv6() {
            let (udp_stats_event_sender, _udp_stats_repository) = packages::udp_tracker_core::statistics::setup::factory(false);
            let udp_stats_event_sender = Arc::new(udp_stats_event_sender);

            let response = handle_connect(sample_ipv6_remote_addr(), &udp_stats_event_sender, sample_issue_time()).await;

            assert_eq!(
                response,
                make(sample_ipv6_remote_addr_fingerprint(), sample_issue_time()).unwrap(),
            );
        }

        #[tokio::test]
        async fn it_should_send_the_upd4_connect_event_when_a_client_tries_to_connect_using_a_ip4_socket_address() {
            let mut udp_stats_event_sender_mock = MockUdpStatsEventSender::new();
            udp_stats_event_sender_mock
                .expect_send_event()
                .with(eq(udp_tracker_core::statistics::event::Event::Udp4Connect))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let udp_stats_event_sender: Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(udp_stats_event_sender_mock)));

            let client_socket_address = sample_ipv4_socket_address();

            handle_connect(client_socket_address, &udp_stats_event_sender, sample_issue_time()).await;
        }

        #[tokio::test]
        async fn it_should_send_the_upd6_connect_event_when_a_client_tries_to_connect_using_a_ip6_socket_address() {
            let mut udp_stats_event_sender_mock = MockUdpStatsEventSender::new();
            udp_stats_event_sender_mock
                .expect_send_event()
                .with(eq(udp_tracker_core::statistics::event::Event::Udp6Connect))
                .times(1)
                .returning(|_| Box::pin(future::ready(Some(Ok(())))));
            let udp_stats_event_sender: Arc<Option<Box<dyn udp_tracker_core::statistics::event::sender::Sender>>> =
                Arc::new(Some(Box::new(udp_stats_event_sender_mock)));

            handle_connect(sample_ipv6_remote_addr(), &udp_stats_event_sender, sample_issue_time()).await;
        }
    }
}
