use actix_web::web::{Json, Path, Data};
use actix_web::{HttpResponse};
use std::sync::mpsc::{SyncSender};
use librespot::core::spotify_id::SpotifyId;
use librespot::playback::player::{PlayerEvent};

/// find a tweet by its id `/tweets/{id}`
#[get("/skip")]
pub async fn skip(data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: SpotifyId::from_base62("0").unwrap() }).expect("error skipping track");
    HttpResponse::Ok().content_type("application/json").body("{ok:'skip'}")
}

#[get("/queue/{id}")]
pub async fn queue(path: Path<String>, data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    data.send(PlayerEvent::Changed { old_track_id: SpotifyId::from_base62("0").unwrap(), new_track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }).expect("error queueing track");
    HttpResponse::Ok().content_type("application/json").body("{ok:'queue'}")

}

#[get("/play/{id}")]
pub async fn play(path: Path<String>, data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    data.send(PlayerEvent::Changed { old_track_id: SpotifyId::from_base62("0").unwrap(), new_track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }).expect("error queueing track");
    data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }).expect("error skipping track");
    HttpResponse::Ok().content_type("application/json").body("{ok:'play'}")
}