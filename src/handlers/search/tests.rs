use super::*;
use crate::anonymous::AnonymousSearch;
use crate::catalog::CatalogFallback;
use rspotify::{AuthCodePkceSpotify, Config, Credentials, OAuth, Token};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

type Calls = Arc<Mutex<Vec<(String, HashMap<String, String>)>>>;

async fn mock_client(
    official: impl Fn(HashMap<String, String>) -> warp::reply::Response + Send + Sync + Clone + 'static,
    internal: Value,
) -> (SpotifyClient, Calls) {
    let calls: Calls = Default::default();
    let trace = calls.clone();
    let route = warp::path::param::<String>()
        .and(warp::query::<HashMap<String, String>>())
        .map(move |path: String, query: HashMap<String, String>| {
            trace.lock().unwrap().push((path.clone(), query.clone()));
            if path == "search" {
                official(query)
            } else {
                warp::reply::json(&internal).into_response()
            }
        });
    let (address, server) = warp::serve(route).bind_ephemeral(([127, 0, 0, 1], 0));
    tokio::spawn(server);
    let raw = AuthCodePkceSpotify::with_config(
        Credentials::new_pkce("test-client"),
        OAuth::default(),
        Config {
            api_base_url: format!("http://{address}/"),
            token_cached: false,
            ..Default::default()
        },
    );
    *raw.token.lock().await.unwrap() = Some(Token {
        access_token: "test-token".to_owned(),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        ..Default::default()
    });
    let mut client = SpotifyClient::new(raw);
    client.catalog = Arc::new(CatalogFallback::with_anonymous_for_test(
        AnonymousSearch::for_test(format!("http://{address}/internal")),
    ));
    (client, calls)
}

async fn request(client: &SpotifyClient, path: &str) -> warp::http::Response<bytes::Bytes> {
    let client = client.clone();
    let route = search_request().and(warp::get()).and_then(move |request| {
        let client = client.clone();
        async move { Ok::<_, Infallible>(search_response(request, &client).await) }
    });
    warp::test::request().path(path).reply(&route).await
}

fn json_body(response: &warp::http::Response<bytes::Bytes>) -> Value {
    serde_json::from_slice(response.body()).unwrap()
}

fn official_track(index: u32) -> Value {
    json!({
        "id": format!("{index:022}"), "uri": format!("spotify:track:{index:022}"),
        "name": format!("Track {index}"), "duration_ms": 123000, "explicit": true,
        "artists": [{"id":"1111111111111111111111", "name":"Artist"}],
        "album": {"id":"2222222222222222222222", "name":"Album", "images":[], "artists":[]},
        "new_provider_field": {"preserve": true}
    })
}

fn internal_search() -> Value {
    json!({"data":{"searchV2":{
        "tracksV2":{"totalCount":1,"items":[{"item":{"data":{
            "uri":"spotify:track:0000000000000000000001", "name":"Internal track",
            "duration":{"totalMilliseconds":123000}, "contentRating":{"label":"EXPLICIT"},
            "artists":{"items":[{"uri":"spotify:artist:1111111111111111111111", "profile":{"name":"Artist"}}]},
            "album":{"uri":"spotify:album:2222222222222222222222", "name":"Album"}
        }}}]}, "albums":{"items":[]}, "artists":{"items":[]}, "playlists":{"items":[]}
    }}, "extensions":{"native_field":true}})
}

#[tokio::test]
async fn native_routes_preserve_provider_json_and_pagination() {
    let official =
        json!({"tracks":{"items":[official_track(1)], "next":"native-next"}, "unknown":"keep"});
    let expected = official.clone();
    let internal = internal_search();
    let (client, calls) = mock_client(
        move |_| warp::reply::json(&official).into_response(),
        internal.clone(),
    )
    .await;
    let response = request(&client, "/search/official/track/10?q=hello&offset=20").await;
    assert_eq!(response.status(), 200);
    assert_eq!(json_body(&response), expected);
    assert_eq!(response.headers()["x-spotify-source"], "spotify_web_api");
    let response = request(&client, "/search/internal/20?q=hello&offset=40").await;
    assert_eq!(response.status(), 200);
    assert_eq!(json_body(&response), internal);
    assert_eq!(response.headers()["x-spotify-source"], "spotify_anonymous");
    assert_eq!(
        request(&client, "/search/internal/20?q=hello&offset=41")
            .await
            .status(),
        200
    );
    assert_eq!(
        request(&client, "/search/internal/20?q=hello&offset=40")
            .await
            .status(),
        200
    );
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        3,
        "identical offsets share cache entries; different offsets do not"
    );
    assert_eq!(calls[0].1["offset"], "20");
    assert_eq!(calls[0].1["limit"], "10");
    let variables: Value = serde_json::from_str(&calls[1].1["variables"]).unwrap();
    assert_eq!(variables["offset"], 40);
    assert_eq!(variables["limit"], 20);
    let variables: Value = serde_json::from_str(&calls[2].1["variables"]).unwrap();
    assert_eq!(variables["offset"], 41);
}

#[tokio::test]
async fn common_search_fetches_ten_results_in_one_provider_request() {
    let (client, calls) = mock_client(
        |query| {
            let offset: u32 = query["offset"].parse().unwrap();
            let limit: u32 = query["limit"].parse().unwrap();
            assert!(limit <= 10);
            let tracks: Vec<_> = (offset..offset + limit).map(official_track).collect();
            warp::reply::json(&json!({"tracks":{"items":tracks}})).into_response()
        },
        internal_search(),
    )
    .await;
    let response = request(&client, "/search/track/10?q=hello").await;
    assert_eq!(response.status(), 200);
    let body = json_body(&response);
    let tracks = body.as_array().unwrap();
    assert_eq!(tracks.len(), 10);
    assert_eq!(tracks[9]["track_name"], "Track 9");
    assert_eq!(tracks[0]["popularity"], Value::Null);
    assert_eq!(tracks[0]["artists"], json!(["Artist"]));
    assert!(tracks[0].get("new_provider_field").is_none());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["offset"], "0");
    assert_eq!(calls[0].1["limit"], "10");
}

#[tokio::test]
async fn explicit_official_errors_do_not_fall_back_but_common_search_does() {
    let (client, calls) = mock_client(
        |_| {
            warp::reply::with_header(
                warp::reply::with_status("rate limited", StatusCode::TOO_MANY_REQUESTS),
                "Retry-After",
                "17",
            )
            .into_response()
        },
        internal_search(),
    )
    .await;
    let response = request(&client, "/search/official/track/10?q=hello").await;
    assert_eq!(response.status(), 429);
    assert_eq!(response.headers()["retry-after"], "17");
    assert_eq!(json_body(&response)["source"], "spotify");
    assert_eq!(calls.lock().unwrap().len(), 1);
    let response = request(&client, "/search/track/10?q=hello").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-spotify-source"], "spotify_anonymous");
    assert_eq!(json_body(&response)[0]["track_name"], "Internal track");
    // A forced official request still uses the official provider during cooldown.
    assert_eq!(
        request(&client, "/search/official/track/10?q=hello")
            .await
            .status(),
        429
    );
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["search", "internal", "search"]
    );
}

#[tokio::test]
async fn common_search_falls_back_after_a_single_failed_official_request() {
    let (client, calls) = mock_client(
        |_| {
            warp::reply::with_status("unavailable", StatusCode::SERVICE_UNAVAILABLE).into_response()
        },
        internal_search(),
    )
    .await;
    let response = request(&client, "/search/track/10?q=hello").await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-spotify-source"], "spotify_anonymous");
    assert_eq!(json_body(&response).as_array().unwrap().len(), 1);
    assert_eq!(json_body(&response)[0]["track_name"], "Internal track");
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        ["search", "internal"]
    );
}

#[tokio::test]
async fn catalog_only_mode_keeps_common_search_working_and_disables_only_official_route() {
    let (mock, calls) = mock_client(
        |_| panic!("must not call official provider"),
        internal_search(),
    )
    .await;
    let mut client = SpotifyClient::catalog_only();
    client.catalog = mock.catalog;
    assert_eq!(
        request(&client, "/search/official/track/10?q=hello")
            .await
            .status(),
        503
    );
    assert!(calls.lock().unwrap().is_empty());
    for category in ["track", "album", "artist", "playlist"] {
        let response = request(&client, &format!("/search/{category}/10?q=hello")).await;
        assert_eq!(response.status(), 200);
        assert!(json_body(&response).is_array());
        assert_eq!(response.headers()["x-spotify-source"], "spotify_anonymous");
    }
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "common categories share the native response cache"
    );
}

#[tokio::test]
async fn internal_graphql_errors_are_native_but_not_empty_common_results() {
    let errors = json!({"errors":[{"message":"changed protocol"}],"data":null});
    let (client, calls) = mock_client(
        |_| panic!("must not call official provider"),
        errors.clone(),
    )
    .await;
    let mut fallback_client = SpotifyClient::catalog_only();
    fallback_client.catalog = client.catalog;
    let client = fallback_client;
    let response = request(&client, "/search/internal/20?q=hello").await;
    assert_eq!(response.status(), 200);
    assert_eq!(json_body(&response), errors);
    let response = request(&client, "/search/track/10?q=hello").await;
    assert_eq!(response.status(), 502);
    assert_eq!(json_body(&response)["source"], "spotify_anonymous");
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "GraphQL errors must not be cached"
    );
}

#[tokio::test]
async fn provider_validation_rejects_invalid_limits_categories_and_offsets_without_network() {
    let (official, calls) = mock_client(
        |_| panic!("invalid limits must not reach the official provider"),
        internal_search(),
    )
    .await;
    let mut anonymous = SpotifyClient::catalog_only();
    anonymous.catalog = official.catalog.clone();
    for client in [&official, &anonymous] {
        for path in [
            "/search/track/0?q=x",
            "/search/track/11?q=x",
            "/search/track/20?q=x",
            "/search/track/51?q=x",
            "/search/invalid/10?q=x",
            "/search/official/track/11?q=x",
            "/search/official/track/0?q=x",
            "/search/official/invalid/10?q=x",
            "/search/official/track/10?q=x&offset=1001",
            "/search/internal/0?q=x",
            "/search/internal/51?q=x",
            "/search/internal/20?q=x&offset=-1",
        ] {
            assert_eq!(request(client, path).await.status(), 400, "{path}");
        }
        for category in ["track", "album", "artist", "playlist"] {
            assert_eq!(
                request(client, &format!("/search/{category}/11?q=x"))
                    .await
                    .status(),
                400
            );
        }
    }
    assert!(calls.lock().unwrap().is_empty());
    let response = request(&anonymous, "/search/internal/50?q=hello").await;
    assert_eq!(
        response.status(),
        200,
        "the native internal endpoint retains its own limit"
    );
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let variables: Value = serde_json::from_str(&calls[0].1["variables"]).unwrap();
    assert_eq!(variables["limit"], 50);
}

#[tokio::test]
async fn common_track_contract_is_identical_across_providers() {
    let mut track = official_track(1);
    track["name"] = json!("Internal track");
    let (client, calls) = mock_client(
        move |_| warp::reply::json(&json!({"tracks":{"items":[track]}})).into_response(),
        internal_search(),
    )
    .await;
    let official = request(&client, "/search/track/1?q=hello").await;
    assert_eq!(official.status(), 200);
    assert_eq!(
        calls.lock().unwrap()[0].1["limit"],
        "5",
        "small common searches retain a stable candidate window"
    );
    let mut fallback = SpotifyClient::catalog_only();
    fallback.catalog = client.catalog;
    let internal = request(&fallback, "/search/track/1?q=hello").await;
    assert_eq!(internal.status(), 200);
    assert_eq!(json_body(&official), json_body(&internal));
}

#[tokio::test]
async fn common_search_accepts_current_and_legacy_official_fields_for_every_category() {
    let (client, _) = mock_client(|query| {
        let (key, row) = match query["type"].as_str() {
            "album" => ("albums", json!({"uri":"spotify:album:2222222222222222222222", "name":"Album", "artists":[], "images":[]})),
            "artist" => ("artists", json!({"uri":"spotify:artist:1111111111111111111111", "name":"Artist", "images":[]})),
            "playlist" => ("playlists", json!({"uri":"spotify:playlist:3333333333333333333333", "name":"Playlist", "owner":{}, "images":[], "items":{"total":42}})),
            _ => unreachable!(),
        };
        // Unavailable items may be null in a Spotify page.
        warp::reply::json(&json!({key:{"items":[null, row]}})).into_response()
    }, internal_search()).await;
    for (category, expected) in [
        ("album", "album_name"),
        ("artist", "artist_name"),
        ("playlist", "playlist_name"),
    ] {
        let response = request(&client, &format!("/search/{category}/10?q=hello")).await;
        assert_eq!(response.status(), 200);
        let body = json_body(&response);
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert!(body[0][expected].is_string());
        if category == "artist" {
            assert_eq!(body[0]["genres"], json!([]));
            assert_eq!(body[0]["popularity"], Value::Null);
        } else if category == "playlist" {
            assert_eq!(body[0]["total_tracks"], 42);
        }
    }
    let (client, _) = mock_client(|_| warp::reply::json(&json!({"playlists":{"items":[{
        "uri":"spotify:playlist:3333333333333333333333", "name":"Legacy playlist", "tracks":{"total":17}
    }]}})).into_response(), internal_search()).await;
    let response = request(&client, "/search/playlist/10?q=hello").await;
    assert_eq!(json_body(&response)[0]["total_tracks"], 17);
}

#[tokio::test]
async fn official_playlist_counts_distinguish_unknown_from_empty_and_keep_legacy_counts() {
    let cases = [
        (json!({}), Value::Null),
        (json!({"items":{"total":0}}), json!(0)),
        (json!({"items":{"total":1}}), json!(1)),
        (json!({"items":{"total":42}}), json!(42)),
        (json!({"tracks":{"total":17}}), json!(17)),
        (
            json!({"items":{"total":null},"tracks":{"total":9}}),
            json!(9),
        ),
        (json!({"items":{"total":null}}), Value::Null),
        (json!({"items":{"total":-1}}), Value::Null),
        (json!({"items":{"total":4294967296_u64}}), Value::Null),
    ];
    let rows: Vec<_> = cases
        .iter()
        .enumerate()
        .map(|(index, (count, _))| {
            let mut row = json!({
                "uri":format!("spotify:playlist:{index:022}"),
                "name":format!("Playlist {index}"), "owner":{"display_name":"Owner"}
            });
            row.as_object_mut()
                .unwrap()
                .extend(count.as_object().unwrap().clone());
            row
        })
        .collect();
    let (client, _) = mock_client(
        move |_| warp::reply::json(&json!({"playlists":{"items":rows}})).into_response(),
        internal_search(),
    )
    .await;
    let response = request(&client, "/search/playlist/10?q=memphis").await;
    assert_eq!(response.status(), 200);
    let body = json_body(&response);
    assert_eq!(body.as_array().unwrap().len(), cases.len());
    for (row, (_, expected)) in body.as_array().unwrap().iter().zip(cases) {
        assert_eq!(row.get("total_tracks"), Some(&expected));
    }
}

#[tokio::test]
async fn internal_playlist_counts_preserve_missing_counts_from_live_search_shape() {
    // The metadata-only row matches the live searchDesktop response for this
    // playlist: name, URI, description, images, and owner, without a count.
    let metadata = json!({
        "name":"MEMPHIS 66.6 FM", "uri":"spotify:playlist:21uWsBc6t70kbHzWRiBUPy",
        "description":"Playlist", "images":{"items":[]}, "owner":{"name":"Pushed"}
    });
    let cases = [
        (json!({}), Value::Null),
        (json!({"content":{"totalCount":0}}), json!(0)),
        (json!({"content":{"totalCount":1}}), json!(1)),
        (json!({"content":{"totalCount":42}}), json!(42)),
        (json!({"content":{"totalCount":null}}), Value::Null),
        (json!({"content":{"totalCount":-1}}), Value::Null),
        (
            json!({"content":{"totalCount":4294967296_u64}}),
            Value::Null,
        ),
    ];
    let rows: Vec<_> = cases
        .iter()
        .map(|(count, _)| {
            let mut row = metadata.clone();
            row.as_object_mut()
                .unwrap()
                .extend(count.as_object().unwrap().clone());
            row
        })
        .collect();
    let internal = json!({"data":{"searchV2":{"playlists":{"totalCount":999,"items":rows}}}});
    let (mock, _) = mock_client(|_| panic!("must use internal provider"), internal).await;
    let mut client = SpotifyClient::catalog_only();
    client.catalog = mock.catalog;
    let response = request(&client, "/search/playlist/10?q=memphis").await;
    assert_eq!(response.status(), 200);
    let body = json_body(&response);
    assert_eq!(body.as_array().unwrap().len(), cases.len());
    assert_eq!(body[0]["playlist_name"], "MEMPHIS 66.6 FM");
    assert_eq!(body[0]["owner"], "Pushed");
    for (row, (_, expected)) in body.as_array().unwrap().iter().zip(cases) {
        assert_eq!(row.get("total_tracks"), Some(&expected));
    }
}
