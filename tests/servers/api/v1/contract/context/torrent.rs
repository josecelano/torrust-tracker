use std::str::FromStr;

use bittorrent_primitives::info_hash::InfoHash;
use torrust_tracker::servers::apis::v1::context::torrent::resources::peer::Peer;
use torrust_tracker::servers::apis::v1::context::torrent::resources::torrent::{self, Torrent};
use torrust_tracker_primitives::peer::fixture::PeerBuilder;
use torrust_tracker_test_helpers::configuration;

use crate::common::http::{Query, QueryParam};
use crate::servers::api::connection_info::{connection_with_invalid_token, connection_with_no_token};
use crate::servers::api::v1::asserts::{
    assert_bad_request, assert_invalid_infohash_param, assert_not_found, assert_token_not_valid, assert_torrent_info,
    assert_torrent_list, assert_torrent_not_known, assert_unauthorized,
};
use crate::servers::api::v1::client::Client;
use crate::servers::api::v1::contract::fixtures::{
    invalid_infohashes_returning_bad_request, invalid_infohashes_returning_not_found,
};
use crate::servers::api::Started;

#[tokio::test]
async fn should_allow_getting_all_torrents() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let info_hash = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();

    env.add_torrent_peer(&info_hash, &PeerBuilder::default().into());

    let response = Client::new(env.get_connection_info()).get_torrents(Query::empty()).await;

    assert_torrent_list(
        response,
        vec![torrent::ListItem {
            info_hash: "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d".to_string(),
            seeders: 1,
            completed: 0,
            leechers: 0,
        }],
    )
    .await;

    env.stop().await;
}

#[tokio::test]
async fn should_allow_limiting_the_torrents_in_the_result() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    // torrents are ordered alphabetically by infohashes
    let info_hash_1 = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();
    let info_hash_2 = InfoHash::from_str("0b3aea4adc213ce32295be85d3883a63bca25446").unwrap();

    env.add_torrent_peer(&info_hash_1, &PeerBuilder::default().into());
    env.add_torrent_peer(&info_hash_2, &PeerBuilder::default().into());

    let response = Client::new(env.get_connection_info())
        .get_torrents(Query::params([QueryParam::new("limit", "1")].to_vec()))
        .await;

    assert_torrent_list(
        response,
        vec![torrent::ListItem {
            info_hash: "0b3aea4adc213ce32295be85d3883a63bca25446".to_string(),
            seeders: 1,
            completed: 0,
            leechers: 0,
        }],
    )
    .await;

    env.stop().await;
}

#[tokio::test]
async fn should_allow_the_torrents_result_pagination() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    // torrents are ordered alphabetically by infohashes
    let info_hash_1 = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();
    let info_hash_2 = InfoHash::from_str("0b3aea4adc213ce32295be85d3883a63bca25446").unwrap();

    env.add_torrent_peer(&info_hash_1, &PeerBuilder::default().into());
    env.add_torrent_peer(&info_hash_2, &PeerBuilder::default().into());

    let response = Client::new(env.get_connection_info())
        .get_torrents(Query::params([QueryParam::new("offset", "1")].to_vec()))
        .await;

    assert_torrent_list(
        response,
        vec![torrent::ListItem {
            info_hash: "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d".to_string(),
            seeders: 1,
            completed: 0,
            leechers: 0,
        }],
    )
    .await;

    env.stop().await;
}

#[tokio::test]
async fn should_allow_getting_a_list_of_torrents_providing_infohashes() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let info_hash_1 = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap(); // DevSkim: ignore DS173237
    let info_hash_2 = InfoHash::from_str("0b3aea4adc213ce32295be85d3883a63bca25446").unwrap(); // DevSkim: ignore DS173237

    env.add_torrent_peer(&info_hash_1, &PeerBuilder::default().into());
    env.add_torrent_peer(&info_hash_2, &PeerBuilder::default().into());

    let response = Client::new(env.get_connection_info())
        .get_torrents(Query::params(
            [
                QueryParam::new("info_hash", "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d"), // DevSkim: ignore DS173237
                QueryParam::new("info_hash", "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d"), // DevSkim: ignore DS173237
            ]
            .to_vec(),
        ))
        .await;

    assert_torrent_list(
        response,
        vec![
            torrent::ListItem {
                info_hash: "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d".to_string(), // DevSkim: ignore DS173237
                seeders: 1,
                completed: 0,
                leechers: 0,
            },
            torrent::ListItem {
                info_hash: "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d".to_string(), // DevSkim: ignore DS173237
                seeders: 1,
                completed: 0,
                leechers: 0,
            },
        ],
    )
    .await;

    env.stop().await;
}

#[tokio::test]
async fn should_fail_getting_torrents_when_the_offset_query_parameter_cannot_be_parsed() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let invalid_offsets = [" ", "-1", "1.1", "INVALID OFFSET"];

    for invalid_offset in &invalid_offsets {
        let response = Client::new(env.get_connection_info())
            .get_torrents(Query::params([QueryParam::new("offset", invalid_offset)].to_vec()))
            .await;

        assert_bad_request(response, "Failed to deserialize query string: invalid digit found in string").await;
    }

    env.stop().await;
}

#[tokio::test]
async fn should_fail_getting_torrents_when_the_limit_query_parameter_cannot_be_parsed() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let invalid_limits = [" ", "-1", "1.1", "INVALID LIMIT"];

    for invalid_limit in &invalid_limits {
        let response = Client::new(env.get_connection_info())
            .get_torrents(Query::params([QueryParam::new("limit", invalid_limit)].to_vec()))
            .await;

        assert_bad_request(response, "Failed to deserialize query string: invalid digit found in string").await;
    }

    env.stop().await;
}

#[tokio::test]
async fn should_fail_getting_torrents_when_the_info_hash_parameter_is_invalid() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let invalid_info_hashes = [" ", "-1", "1.1", "INVALID INFO_HASH"];

    for invalid_info_hash in &invalid_info_hashes {
        let response = Client::new(env.get_connection_info())
            .get_torrents(Query::params([QueryParam::new("info_hash", invalid_info_hash)].to_vec()))
            .await;

        assert_bad_request(
            response,
            &format!("Invalid URL: invalid infohash param: string \"{invalid_info_hash}\", expected a 40 character long string"),
        )
        .await;
    }

    env.stop().await;
}

#[tokio::test]
async fn should_not_allow_getting_torrents_for_unauthenticated_users() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let response = Client::new(connection_with_invalid_token(env.get_connection_info().bind_address.as_str()))
        .get_torrents(Query::empty())
        .await;

    assert_token_not_valid(response).await;

    let response = Client::new(connection_with_no_token(env.get_connection_info().bind_address.as_str()))
        .get_torrents(Query::default())
        .await;

    assert_unauthorized(response).await;

    env.stop().await;
}

#[tokio::test]
async fn should_allow_getting_a_torrent_info() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let info_hash = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();

    let peer = PeerBuilder::default().into();

    env.add_torrent_peer(&info_hash, &peer);

    let response = Client::new(env.get_connection_info())
        .get_torrent(&info_hash.to_string())
        .await;

    assert_torrent_info(
        response,
        Torrent {
            info_hash: "9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d".to_string(),
            seeders: 1,
            completed: 0,
            leechers: 0,
            peers: Some(vec![Peer::from(peer)]),
        },
    )
    .await;

    env.stop().await;
}

#[tokio::test]
async fn should_fail_while_getting_a_torrent_info_when_the_torrent_does_not_exist() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let info_hash = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();

    let response = Client::new(env.get_connection_info())
        .get_torrent(&info_hash.to_string())
        .await;

    assert_torrent_not_known(response).await;

    env.stop().await;
}

#[tokio::test]
async fn should_fail_getting_a_torrent_info_when_the_provided_infohash_is_invalid() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    for invalid_infohash in &invalid_infohashes_returning_bad_request() {
        let response = Client::new(env.get_connection_info()).get_torrent(invalid_infohash).await;

        assert_invalid_infohash_param(response, invalid_infohash).await;
    }

    for invalid_infohash in &invalid_infohashes_returning_not_found() {
        let response = Client::new(env.get_connection_info()).get_torrent(invalid_infohash).await;

        assert_not_found(response).await;
    }

    env.stop().await;
}

#[tokio::test]
async fn should_not_allow_getting_a_torrent_info_for_unauthenticated_users() {
    let env = Started::new(&configuration::ephemeral().into()).await;

    let info_hash = InfoHash::from_str("9e0217d0fa71c87332cd8bf9dbeabcb2c2cf3c4d").unwrap();

    env.add_torrent_peer(&info_hash, &PeerBuilder::default().into());

    let response = Client::new(connection_with_invalid_token(env.get_connection_info().bind_address.as_str()))
        .get_torrent(&info_hash.to_string())
        .await;

    assert_token_not_valid(response).await;

    let response = Client::new(connection_with_no_token(env.get_connection_info().bind_address.as_str()))
        .get_torrent(&info_hash.to_string())
        .await;

    assert_unauthorized(response).await;

    env.stop().await;
}
