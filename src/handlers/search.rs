use super::helpers::{catalog_error_response, catalog_search, search_type_for_category};
use crate::models::SearchQuery;
use crate::state::AppState;
use std::convert::Infallible;
use std::sync::Arc;
use warp::{http::StatusCode, Reply};

pub async fn handle_search_category(
    category: String,
    limit: u32,
    query: SearchQuery,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    println!("[API] GET /search/{}?q={}&limit={limit}", category, query.q);
    if !(1..=50).contains(&limit) {
        return Ok(super::helpers::json_error(
            StatusCode::BAD_REQUEST,
            "Search limit must be between 1 and 50",
        ));
    }
    let Some(search_type) = search_type_for_category(&category) else {
        return Ok(super::helpers::json_error(
            StatusCode::BAD_REQUEST,
            "Search category must be 'track', 'album', 'artist', or 'playlist'",
        ));
    };

    // The catalogue helper handles refresh failures and selects a backup.
    // A separate token preflight here would prevent anonymous search recovery.
    match catalog_search(&state.spotify, &query.q, search_type, limit).await {
        Ok(result) => Ok(warp::reply::with_header(
            warp::reply::json(&result.items),
            "x-spotify-source",
            result.source,
        )
        .into_response()),
        Err(error) => Ok(catalog_error_response(&error)),
    }
}
