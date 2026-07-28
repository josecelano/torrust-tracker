use serde::Deserialize;
use torrust_tracker_rest_api_client::connection_info::{ConnectionInfo, Origin};
use torrust_tracker_rest_api_client::v1::client::ApiHttpClient as TrackerApiClient;
use url::Url;

use crate::common::{self, EphemeralTrackerWorkspace};

/// The stats API endpoint should aggregate announces across multiple HTTP tracker instances.
///
/// This is an application-level integration test. It verifies that announces
/// sent to two separate HTTP tracker instances are both counted in the global
/// tracker statistics. This behavior cannot be tested at the package level
/// because it requires the full application container coordinating multiple
/// HTTP tracker instances.
///
/// Single-instance announce and scrape behavior is tested in the
/// `axum-http-server` package.
///
/// TODO: Replace the temporary bind-IP endpoint discovery used by this suite after
/// `fix-duplicate-port-zero-tracker-instance-bootstrap` and
/// `add-runtime-service-registry-metadata` are implemented.
#[tokio::test]
async fn the_stats_api_endpoint_should_return_the_global_stats() {
    // ── 1. Configuration ──────────────────────────────────────────────
    let config_toml = r#"
        [metadata]
        app = "torrust-tracker"
        purpose = "configuration"
        schema_version = "2.0.0"

        [logging]
        threshold = "off"

        [core]
        listed = false
        private = false

        [core.database]
        driver = "sqlite3"
        path = "{STORAGE_PATH}/sqlite3.db"

        [[http_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = true

        [[http_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = true

        [http_api]
        bind_address = "127.0.0.1:0"

        [http_api.access_tokens]
        admin = "MyAccessToken"

        [health_check_api]
        bind_address = "127.0.0.2:0"
    "#;

    // ── 2. Start tracker on isolated workspace ───────────────────────
    let workspace = EphemeralTrackerWorkspace::new(config_toml);
    let (app_container, _jobs) = common::start_tracker_with_config(&workspace).await;

    let tracker_urls = common::http_tracker_urls(&app_container).await;
    assert_eq!(tracker_urls.len(), 2, "expected two HTTP trackers");

    let api_url = common::http_api_url(&app_container).await.expect("expected an HTTP API URL");

    // ── 3. Announce to both tracker instances ────────────────────────
    let client = reqwest::Client::new();
    for url in &tracker_urls {
        let announce_url = url
            .join("/announce?info_hash=%9c8b%22%13%e3%0b%ff%21%2b0%c3%60%d2o%9a%02%13d%22&peer_id=-qB00000000000000001&port=17548&ip=127.0.0.1&event=started&compact=0")
            .expect("announce URL should be valid");
        let resp = client.get(announce_url.as_str()).send().await.unwrap();
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            panic!("announce to {url} failed: status {status}, body: {body}");
        }
    }

    // ── 4. Verify both announces are aggregated ──────────────────────
    let global_stats = get_tracker_statistics(&api_url, "MyAccessToken").await;
    assert_eq!(global_stats.tcp4_announces_handled, 2);

    // The tracker application and its temporary workspace are cleaned up
    // when `workspace` and `_jobs` are dropped at the end of this scope.
}

/// A disabled tracker must not contribute to global statistics.
///
/// This regression is ignored until
/// `fix-duplicate-port-zero-tracker-instance-bootstrap` preserves an individual container for
/// every repeated `0.0.0.0:0` configuration entry. At present, the address-keyed bootstrap map
/// makes both listeners use the later enabled configuration, so both announces are counted.
#[ignore = "blocked by fix-duplicate-port-zero-tracker-instance-bootstrap"]
#[tokio::test]
async fn the_stats_api_endpoint_should_exclude_announces_from_a_tracker_with_statistics_disabled() {
    let config_toml = r#"
        [metadata]
        app = "torrust-tracker"
        purpose = "configuration"
        schema_version = "2.0.0"

        [logging]
        threshold = "off"

        [core]
        listed = false
        private = false

        [core.database]
        driver = "sqlite3"
        path = "{STORAGE_PATH}/sqlite3.db"

        [[http_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = false

        [[http_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = true

        [http_api]
        bind_address = "127.0.0.1:0"

        [http_api.access_tokens]
        admin = "MyAccessToken"

        [health_check_api]
        bind_address = "127.0.0.2:0"
    "#;

    let workspace = EphemeralTrackerWorkspace::new(config_toml);
    let (app_container, _jobs) = common::start_tracker_with_config(&workspace).await;

    let tracker_urls = common::http_tracker_urls(&app_container).await;
    assert_eq!(tracker_urls.len(), 2, "expected two HTTP trackers");

    let api_url = common::http_api_url(&app_container).await.expect("expected an HTTP API URL");

    let client = reqwest::Client::new();
    for url in &tracker_urls {
        let announce_url = url
            .join("/announce?info_hash=%9c8b%22%13%e3%0b%ff%21%2b0%c3%60%d2o%9a%02%13d%22&peer_id=-qB00000000000000001&port=17548&ip=127.0.0.1&event=started&compact=0")
            .expect("announce URL should be valid");
        let response = client.get(announce_url.as_str()).send().await.unwrap();
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            panic!("announce to {url} failed: status {status}, body: {body}");
        }
    }

    let global_stats = get_tracker_statistics(&api_url, "MyAccessToken").await;
    assert_eq!(global_stats.tcp4_announces_handled, 1);
}

/// A disabled UDP tracker must not contribute to global statistics.
///
/// This regression verifies the same defect as the HTTP test above, but for
/// UDP tracker instances. When two UDP blocks share `0.0.0.0:0` and differ
/// only in `tracker_usage_statistics`, the disabled instance must not count
/// announces in global stats.
#[ignore = "blocked by fix-duplicate-port-zero-tracker-instance-bootstrap"]
#[tokio::test]
async fn udp_stats_should_exclude_announces_from_a_tracker_with_statistics_disabled() {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use torrust_peer_id::PeerId;
    use torrust_tracker_client::udp::client::UdpTrackerClient;
    use torrust_tracker_udp_protocol::{
        AnnounceActionPlaceholder, AnnounceEvent, AnnounceRequest, ConnectionId, InfoHash, NumberOfBytes, NumberOfPeers, PeerKey,
        Port, TransactionId,
    };

    let config_toml = r#"
        [metadata]
        app = "torrust-tracker"
        purpose = "configuration"
        schema_version = "2.0.0"

        [logging]
        threshold = "off"

        [core]
        listed = false
        private = false

        [core.database]
        driver = "sqlite3"
        path = "{STORAGE_PATH}/sqlite3.db"

        [[udp_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = false

        [[udp_trackers]]
        bind_address = "0.0.0.0:0"
        tracker_usage_statistics = true

        [http_api]
        bind_address = "127.0.0.1:0"

        [http_api.access_tokens]
        admin = "MyAccessToken"

        [health_check_api]
        bind_address = "127.0.0.2:0"
    "#;

    let workspace = EphemeralTrackerWorkspace::new(config_toml);
    let (app_container, _jobs) = common::start_tracker_with_config(&workspace).await;

    let udp_addresses = common::udp_tracker_addresses(&app_container).await;
    assert_eq!(udp_addresses.len(), 2, "expected two UDP trackers");

    let api_url = common::http_api_url(&app_container).await.expect("expected an HTTP API URL");

    let timeout = Duration::from_secs(5);
    // 20-byte info hash (same as the HTTP test uses in its announce URL)
    let info_hash = InfoHash([
        0x9c, 0x8b, 0x22, 0x13, 0xe3, 0x0b, 0xff, 0x21, 0x2b, 0x30, 0xc3, 0x60, 0xd2, 0x6f, 0x9a, 0x02, 0x13, 0x64, 0x22, 0x00,
    ]);

    for addr in &udp_addresses {
        let client = UdpTrackerClient::new(*addr, timeout)
            .await
            .expect("failed to create UDP client");

        // Connect
        let connect_request = torrust_tracker_udp_protocol::ConnectRequest {
            transaction_id: TransactionId::new(1),
        };
        let connection_id = match client.send(connect_request.into()).await {
            Ok(_) => match client.receive().await {
                Ok(torrust_tracker_udp_protocol::Response::Connect(resp)) => resp.connection_id,
                other => panic!("expected connect response, got {other:?}"),
            },
            Err(e) => panic!("connect failed: {e}"),
        };

        // Announce
        let announce_request = AnnounceRequest {
            connection_id: ConnectionId(connection_id.0),
            action_placeholder: AnnounceActionPlaceholder::default(),
            transaction_id: TransactionId::new(2),
            info_hash,
            peer_id: PeerId([255u8; 20]),
            bytes_downloaded: NumberOfBytes(0i64.into()),
            bytes_uploaded: NumberOfBytes(0i64.into()),
            bytes_left: NumberOfBytes(0i64.into()),
            event: AnnounceEvent::Started.into(),
            ip_address: Ipv4Addr::UNSPECIFIED.into(),
            key: PeerKey::new(0i32),
            peers_wanted: NumberOfPeers(1i32.into()),
            port: Port(17548u16.into()),
        };
        match client.send(announce_request.into()).await {
            Ok(_) => match client.receive().await {
                Ok(torrust_tracker_udp_protocol::Response::AnnounceIpv4(_)) => {}
                other => panic!("expected announce response, got {other:?}"),
            },
            Err(e) => panic!("announce failed: {e}"),
        }
    }

    let global_stats = get_tracker_statistics(&api_url, "MyAccessToken").await;
    assert_eq!(
        global_stats.tcp4_announces_handled, 0,
        "UDP announces should not count in tcp4 stats"
    );
}

/// Global statistics with only metrics relevant to the test.
#[derive(Deserialize)]
struct PartialGlobalStatistics {
    tcp4_announces_handled: u64,
}

async fn get_tracker_statistics(api_url: &Url, token: &str) -> PartialGlobalStatistics {
    let response = TrackerApiClient::new(ConnectionInfo::authenticated(Origin::new(api_url.as_str()).unwrap(), token))
        .unwrap()
        .get_tracker_statistics(None)
        .await
        .expect("failed to get tracker statistics");

    response
        .json::<PartialGlobalStatistics>()
        .await
        .expect("Failed to parse JSON response")
}
