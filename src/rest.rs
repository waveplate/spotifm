use actix_web::{web::{self, Path, Data}, rt, middleware, App, HttpServer, HttpRequest, HttpResponse};
use std::{env, thread, sync::mpsc::{SyncSender}};
use std::sync::{Arc,Mutex};
use chrono::{Duration as ChronoDuration, Utc};
use librespot::core::{keymaster, session::Session, spotify_id::SpotifyId};
use librespot::playback::player::{PlayerEvent};
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::FromIterator;
use rspotify::{
    model::{SearchType, SearchResult, TrackId},
    prelude::*, AuthCodeSpotify,
};

use crate::db::{SpotifyDatabase, SpotifyTrack};

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const SCOPES: &str =
    "streaming,user-read-playback-state,user-modify-playback-state,user-read-currently-playing";

//const SCOPES: = scopes!("user-read-playback-state user-modify-playback-state user-read-currently-playing");

#[get("/search/{type}/{num}")]
pub async fn search(req: HttpRequest, path: Path<(String,u32)>, session: Data<Arc<Mutex<Session>>>) -> HttpResponse {
    let query = web::Query::<HashMap<String,String>>::from_query(req.query_string()).unwrap();
    let search_type = match path.0.0.as_str() {
        "track" => SearchType::Track,
        "artist" => SearchType::Artist,
        "album" => SearchType::Album,
        "playlist" => SearchType::Playlist,
        _ => SearchType::Track,
    };

    return match api(session).await {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err)])),
        Ok(spotify) => {
            return match spotify.search(query.get("q").unwrap(), search_type, None, None, Some(path.1), None) {
                Ok(result) => {
                    match result {
                        SearchResult::Tracks(track) => return HttpResponse::Ok().json(track.items),
                        SearchResult::Artists(artist) => return HttpResponse::Ok().json(artist.items),
                        SearchResult::Albums(album) => return HttpResponse::Ok().json(album.items),
                        SearchResult::Playlists(playlist) => return HttpResponse::Ok().json(playlist.items),
                        _ => return HttpResponse::NotFound().json(HashMap::from([("error", "no results")])),
                    }
                },
                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
            };
        },
    }
}

#[get("/np")]
pub async fn np(db: Data<Arc<Mutex<SpotifyDatabase>>>) -> HttpResponse {
    return match db.lock().unwrap().current_track() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.unwrap().to_string())])),
        Ok(track) => HttpResponse::Ok().json(track),
    }
}

#[get("/skip")]
pub async fn skip(data: Data<SyncSender<PlayerEvent>>, db: Data<Arc<Mutex<SpotifyDatabase>>>) -> HttpResponse {
    let next_playing = db.lock().unwrap().next_track().unwrap();
    return match data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: SpotifyId::from_base62("0").unwrap() }) {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(_) => {
            return HttpResponse::Ok().json(next_playing);
        },
    };
}

#[get("/queue/{id}")]
pub async fn queue(path: Path<String>, data: Data<SyncSender<PlayerEvent>>, session: Data<Arc<Mutex<Session>>>, db: Data<Arc<Mutex<SpotifyDatabase>>>) -> HttpResponse {
    let now_playing = db.lock().unwrap().current_track().unwrap();
    let next_playing = db.lock().unwrap().next_track().unwrap();

    if now_playing.id == path.0.as_str() {
        return HttpResponse::Ok().json(now_playing);
    } else if next_playing.id == path.0.as_str() {
        return HttpResponse::Ok().json(next_playing);
    }

    return match api(session).await {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.unwrap().to_string())])),
        Ok(spotify) => {
            return match spotify.track(TrackId::from_id(path.0).unwrap()) {
                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                Ok(track) => {
                    let spotify_track = SpotifyTrack::new(track.id.unwrap().to_string().split(":").collect::<Vec<&str>>().get(2).unwrap().to_string(), track.name, track.artists.iter().map(|x| x.clone().name).collect());
                    return match db.lock().unwrap().queue_track(spotify_track.clone()) {
                        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                        Ok(_) => {
                            return match data.send(PlayerEvent::Changed { old_track_id: now_playing.spotify_id(), new_track_id: spotify_track.spotify_id() }) {
                                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                                Ok(_) => {
                                    return HttpResponse::Ok().json(spotify_track);
                                },
                            }
                        }
                    }
                },
            }
        },
    }

}

#[get("/play/{id}")]
pub async fn play(path: Path<String>, data: Data<SyncSender<PlayerEvent>>, session: Data<Arc<Mutex<Session>>>, db: Data<Arc<Mutex<SpotifyDatabase>>>) -> HttpResponse {
    let now_playing = db.lock().unwrap().current_track().unwrap();
    let next_playing = db.lock().unwrap().next_track().unwrap();

    if now_playing.id == path.0.as_str() {
        return HttpResponse::Ok().json(now_playing);
    } else if next_playing.id == path.0.as_str() {
        return HttpResponse::Ok().json(next_playing);
    }

    return match api(session).await {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.unwrap().to_string())])),
        Ok(spotify) => {
            return match spotify.track(TrackId::from_id(path.0).unwrap()) {
                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                Ok(track) => {
                    let spotify_track = SpotifyTrack::new(track.id.unwrap().to_string().split(":").collect::<Vec<&str>>().get(2).unwrap().to_string(), track.name, track.artists.iter().map(|x| x.clone().name).collect());
                    return match db.lock().unwrap().queue_track(spotify_track.clone()) {
                        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                        Ok(_) => {
                            return match data.send(PlayerEvent::Changed { old_track_id: now_playing.spotify_id(), new_track_id: spotify_track.spotify_id() }) {
                                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                                Ok(_) => {
                                    return match data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: spotify_track.spotify_id() }) {
                                        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                                        Ok(_) => HttpResponse::Ok().json(spotify_track),
                                    }
                                },
                            }
                        },
                    }
                },
            }
        },
    }
}

#[get("/playlist")]
pub async fn show_playlist(db: Data<Arc<Mutex<SpotifyDatabase>>>) -> HttpResponse {
    return match db.lock().unwrap().read() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(state) => HttpResponse::Ok().json(state.queue),
    }
}

async fn api(session: Data<Arc<Mutex<Session>>>) -> Result<AuthCodeSpotify, Option<String>> {
    return match keymaster::get_token(&session.clone().lock().unwrap(), CLIENT_ID, SCOPES).await {
        Err(_) => Err(Some("could not get token".to_string())),
        Ok(search_token) => {
            let token = rspotify::Token {
                access_token: search_token.access_token.clone(),
                expires_in: ChronoDuration::seconds(search_token.expires_in.into()),
                expires_at: Some(Utc::now() + ChronoDuration::seconds(search_token.expires_in.into())),
                refresh_token: None,
                scopes: HashSet::from_iter(SCOPES.split(",").into_iter().map(|x| x.to_string())),
            };

            let mut spotify = rspotify::AuthCodeSpotify::from_token(token.clone());

            spotify.creds.id = CLIENT_ID.to_string();

            Ok(spotify)
        }
    }
}

#[actix_rt::main]
pub async fn start(tx: SyncSender<PlayerEvent>, session: Arc<Mutex<Session>>, db: Arc<Mutex<SpotifyDatabase>>) {
    thread::spawn(move || {
        env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
        env_logger::init();

        match rt::System::new("rest-api").block_on(
            HttpServer::new(move || {
                let tx = web::Data::new(tx.clone());
                let session = web::Data::new(session.clone());
                let db = web::Data::new(db.clone());
                App::new()
                    .wrap(middleware::Logger::default())
                    .app_data(tx)
                    .app_data(session)
                    .app_data(db)
                    .service(np)
                    .service(skip)
                    .service(queue)
                    .service(play)
                    .service(search)
                    .service(show_playlist)
            })
            .bind("0.0.0.0:9090")
            .unwrap()
            .run()
        )
        {
            Ok(_) => {},
            Err(err) => panic!("{}", err.to_string()),
        };
    });
}