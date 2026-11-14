use super::helpers::{
    catalog_error_response, catalog_search, json_error, search_type_for_category, SEARCH_MAX_LIMIT,
};
use crate::catalog::CatalogError;
use crate::models::{ProviderSearchQuery, SearchQuery};
use crate::spotify::SpotifyClient;
use crate::state::AppState;
use std::convert::Infallible;
use std::sync::Arc;
use warp::{http::StatusCode, Filter, Reply};

pub enum SearchRequest {
    Common(String, u32, SearchQuery),
    Official(String, u32, ProviderSearchQuery),
    Internal(u32, ProviderSearchQuery),
}

pub fn search_request() -> impl Filter<Extract = (SearchRequest,), Error = warp::Rejection> + Clone
{
    warp::path!("search" / "official" / String / u32)
        .and(warp::query::<ProviderSearchQuery>())
        .map(SearchRequest::Official)
        .or(warp::path!("search" / "internal" / u32)
            .and(warp::query::<ProviderSearchQuery>())
            .map(SearchRequest::Internal))
        .unify()
        .or(warp::path!("search" / String / u32)
            .and(warp::query::<SearchQuery>())
            .map(SearchRequest::Common))
        .unify()
}

pub async fn handle_search(
    request: SearchRequest,
    state: Arc<AppState>,
) -> Result<warp::reply::Response, Infallible> {
    Ok(search_response(request, &state.spotify).await)
}

async fn search_response(request: SearchRequest, client: &SpotifyClient) -> warp::reply::Response {
    let (category, limit, max_limit) = match &request {
        SearchRequest::Common(category, limit, _) | SearchRequest::Official(category, limit, _) => {
            (Some(category), *limit, SEARCH_MAX_LIMIT)
        }
        SearchRequest::Internal(limit, _) => (None, *limit, 50),
    };
    if !(1..=max_limit).contains(&limit) {
        return json_error(
            StatusCode::BAD_REQUEST,
            format!("Search limit must be between 1 and {max_limit}"),
        );
    }
    let category = match category {
        Some(category) => match search_type_for_category(category) {
            Some(category) => Some(category),
            None => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "Search category must be 'track', 'album', 'artist', or 'playlist'",
                )
            }
        },
        None => None,
    };

    match request {
        SearchRequest::Common(_, _, params) => {
            // Only the common endpoint selects providers and normalizes results.
            match catalog_search(client, &params.q, category.unwrap(), limit).await {
                Ok(result) => search_json_response(&result.items, result.source),
                Err(error) => catalog_error_response(&error),
            }
        }
        SearchRequest::Official(_, _, params) => {
            if params.offset > 1000 {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "Official search offset must be between 0 and 1000",
                );
            }
            if !client.web_api_enabled() {
                return json_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Spotify Web API is disabled; configure rspotify_client_id to enable it",
                );
            }
            match client
                .search_raw(&params.q, category.unwrap(), limit, params.offset)
                .await
            {
                Ok(result) => search_json_response(&result, "spotify_web_api"),
                Err(error) => catalog_error_response(&CatalogError::WebApi(error)),
            }
        }
        SearchRequest::Internal(_, params) => {
            match client
                .catalog
                .search_raw(&params.q, limit, params.offset)
                .await
            {
                Ok(result) => search_json_response(&result, "spotify_anonymous"),
                Err(error) => catalog_error_response(&CatalogError::Anonymous(error)),
            }
        }
    }
}

fn search_json_response(
    result: &impl serde::Serialize,
    source: &'static str,
) -> warp::reply::Response {
    warp::reply::with_header(warp::reply::json(result), "x-spotify-source", source).into_response()
}

#[cfg(test)]
mod tests;
