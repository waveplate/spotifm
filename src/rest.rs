use serde::{Deserialize, Serialize};

use actix_web::{
    middleware, rt,
    web::{self, Data, Path, Form},
    App, HttpRequest, HttpResponse, HttpServer,
};
use std::sync::{Arc, Mutex};
use chrono::{Duration as ChronoDuration, Utc};
use librespot::core::{keymaster, session::Session, spotify_id::SpotifyId};
use librespot::playback::player::PlayerEvent;
use rspotify::{
    model::{SearchResult, SearchType, TrackId},
    prelude::*,
    AuthCodeSpotify,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::{sync::mpsc::SyncSender, thread};

use crate::db::{SpotifyDatabase, SpotifyTrack};
use crate::config::SpotifmConfig;
use crate::announce::{espeak, get_elevenlabs_tts, play_elevenlabs};

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const SCOPES: &str =
    "streaming,user-read-playback-state,user-modify-playback-state,user-read-currently-playing";

#[derive(Serialize, Deserialize)]
pub struct AnnounceBumper {
    pub enable: Option<bool>,
    pub tag: Option<String>,
    pub freq: Option<usize>,
    pub speed: Option<u32>,
    pub amplitude: Option<u32>,
    pub voice: Option<String>,
    pub pitch: Option<u32>,
    pub gap: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct AnnounceSong {
    pub enable: Option<bool>,
    pub speed: Option<u32>,
    pub amplitude: Option<u32>,
    pub voice: Option<String>,
    pub pitch: Option<u32>,
    pub gap: Option<u32>,
}

#[get("/elevenlabs")]
pub async fn do_elevenlabs_say(
    req: HttpRequest,
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    let query = web::Query::<HashMap<String, String>>::from_query(req.query_string()).unwrap();

    let text = query.get("text").unwrap().clone();

    get_elevenlabs_tts(text.as_str(), config.lock().unwrap().elevenlabs.clone());
    play_elevenlabs();

    return HttpResponse::Ok().json(HashMap::from([("text", text)]));
}

#[get("/espeak")]
pub async fn do_espeak_say(
    req: HttpRequest,
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    let query = web::Query::<HashMap<String, String>>::from_query(req.query_string()).unwrap();
    espeak(query.get("text").unwrap().clone(), config.lock().unwrap().announce.song.espeak.clone());
    return HttpResponse::Ok().json(HashMap::from([("text", query.get("text").unwrap())]));
}

#[delete("/announce/bumper/tags")]
pub async fn delete_announce_bumper_tags(
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    config.lock().unwrap().announce.bumper.clear_tags();
    return HttpResponse::Ok().json(config.lock().unwrap().announce.bumper.clone());
}

#[post("/announce/song")]
pub async fn edit_announce_song(
    form: Form<AnnounceSong>,
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    if form.enable.is_some() {
        config.lock().unwrap().announce.song.enable = form.enable.unwrap();
    }

    if form.speed.is_some() {
        config.lock().unwrap().announce.song.espeak.speed = form.speed.unwrap();
    }

    if form.amplitude.is_some() {
        config.lock().unwrap().announce.song.espeak.amplitude = form.amplitude.unwrap();
    }

    if form.voice.is_some() {
        config.lock().unwrap().announce.song.espeak.voice = form.voice.clone().unwrap();
    }

    if form.pitch.is_some() {
        config.lock().unwrap().announce.song.espeak.pitch = form.pitch.unwrap();
    }

    if form.gap.is_some() {
        config.lock().unwrap().announce.song.espeak.gap = form.gap.unwrap();
    }    

    return HttpResponse::Ok().json(config.lock().unwrap().announce.song.clone());
}

#[post("/announce/bumper")]
pub async fn edit_announce_bumper(
    form: Form<AnnounceBumper>,
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    if form.enable.is_some() {
        config.lock().unwrap().announce.bumper.enable = form.enable.unwrap();
    }

    if form.tag.is_some() {
        config.lock().unwrap().announce.bumper.add_tag(form.tag.clone().unwrap());
    }

    if form.freq.is_some() {
        config.lock().unwrap().announce.bumper.freq = form.freq.unwrap();
    }

    if form.speed.is_some() {
        config.lock().unwrap().announce.bumper.espeak.speed = form.speed.unwrap();
    }

    if form.amplitude.is_some() {
        config.lock().unwrap().announce.bumper.espeak.amplitude = form.amplitude.unwrap();
    }

    if form.voice.is_some() {
        config.lock().unwrap().announce.bumper.espeak.voice = form.voice.clone().unwrap();
    }

    if form.pitch.is_some() {
        config.lock().unwrap().announce.bumper.espeak.pitch = form.pitch.unwrap();
    }

    if form.gap.is_some() {
        config.lock().unwrap().announce.bumper.espeak.gap = form.gap.unwrap();
    }    

    return HttpResponse::Ok().json(config.lock().unwrap().announce.bumper.clone());
}

#[get("/announce/{type}")]
pub async fn get_announce(
    path: Path<String>,
    config: Data<Arc<Mutex<SpotifmConfig>>>,
) -> HttpResponse {
    match path.0.as_str() {
        "bumper" => return HttpResponse::Ok().json(config.lock().unwrap().clone().announce.bumper),
        "song" => return HttpResponse::Ok().json(config.lock().unwrap().clone().announce.song),
        _ => return HttpResponse::NotFound().finish(),
    }
}

#[get("/search/{type}/{num}")]
pub async fn search(
    req: HttpRequest,
    path: Path<(String, u32)>,
    session: Data<Arc<Mutex<Session>>>,
) -> HttpResponse {
    let query = web::Query::<HashMap<String, String>>::from_query(req.query_string()).unwrap();
    let search_type = match path.0 .0.to_string().to_lowercase().as_str() {
        "track" => SearchType::Track,
        "artist" => SearchType::Artist,
        "album" => SearchType::Album,
        "playlist" => SearchType::Playlist,
        _ => SearchType::Track,
    };

    return match api(session).await {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err)])),
        Ok(spotify) => {
            return match spotify.search(
                query.get("q").unwrap(),
                search_type,
                None,
                None,
                Some(path.1),
                None,
            ) {
                Ok(result) => match result {
                    SearchResult::Tracks(track) => return HttpResponse::Ok().json(track.items),
                    SearchResult::Artists(artist) => return HttpResponse::Ok().json(artist.items),
                    SearchResult::Albums(album) => return HttpResponse::Ok().json(album.items),
                    SearchResult::Playlists(playlist) => {
                        return HttpResponse::Ok().json(playlist.items)
                    }
                    _ => {
                        return HttpResponse::NotFound()
                            .json(HashMap::from([("error", "no results")]))
                    }
                },
                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
            };
        }
    };
}

#[get("/np")]
pub async fn np(db: Data<SpotifyDatabase>) -> HttpResponse {
    return match db.current_track() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.unwrap().to_string())])),
        Ok(track) => HttpResponse::Ok().json(track),
    };
}

#[get("/prev")]
pub async fn prev_track(db: Data<SpotifyDatabase>) -> HttpResponse {
    return match db.prev_track() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(track) => HttpResponse::Ok().json(track),
    };
}

#[get("/next")]
pub async fn next_track(db: Data<SpotifyDatabase>) -> HttpResponse {
    return match db.next_track() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(track) => HttpResponse::Ok().json(track),
    };
}

#[get("/skip")]
pub async fn skip(data: Data<SyncSender<PlayerEvent>>, db: Data<SpotifyDatabase>) -> HttpResponse {
    let next_playing = db.next_track().unwrap();
    return match data.send(PlayerEvent::Stopped {
        play_request_id: 0,
        track_id: SpotifyId::from_base62("0").unwrap(),
    }) {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(_) => {
            return HttpResponse::Ok().json(next_playing);
        }
    };
}

#[get("/shuffle")]
pub async fn shuffle(db: Data<SpotifyDatabase>) -> HttpResponse {
    return match db.shuffle() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(state) => HttpResponse::Ok().json(state.queue),
    };
}

#[get("/queue/{id}")]
pub async fn queue(
    path: Path<String>,
    data: Data<SyncSender<PlayerEvent>>,
    session: Data<Arc<Mutex<Session>>>,
    db: Data<SpotifyDatabase>,
) -> HttpResponse {
    let now_playing = db.current_track().unwrap();
    let next_playing = db.next_track().unwrap();

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
                    let spotify_track = SpotifyTrack::new(
                        track
                            .id
                            .unwrap()
                            .to_string()
                            .split(":")
                            .collect::<Vec<&str>>()
                            .get(2)
                            .unwrap()
                            .to_string(),
                        track.name,
                        track.artists.iter().map(|x| x.clone().name).collect(),
                    );
                    return match db.queue_track(spotify_track.clone()) {
                        Err(err) => {
                            HttpResponse::Ok().json(HashMap::from([("error", err.to_string())]))
                        }
                        Ok(_) => {
                            return match data.send(PlayerEvent::Changed {
                                old_track_id: now_playing.spotify_id(),
                                new_track_id: spotify_track.spotify_id(),
                            }) {
                                Err(err) => HttpResponse::Ok()
                                    .json(HashMap::from([("error", err.to_string())])),
                                Ok(_) => {
                                    return HttpResponse::Ok().json(spotify_track);
                                }
                            }
                        }
                    };
                }
            }
        }
    };
}

#[get("/play/{id}")]
pub async fn play(
    path: Path<String>,
    data: Data<SyncSender<PlayerEvent>>,
    session: Data<Arc<Mutex<Session>>>,
    db: Data<SpotifyDatabase>,
) -> HttpResponse {
    let now_playing = db.current_track().unwrap();
    let next_playing = db.next_track().unwrap();

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
                    let spotify_track = SpotifyTrack::new(
                        track
                            .id
                            .unwrap()
                            .to_string()
                            .split(":")
                            .collect::<Vec<&str>>()
                            .get(2)
                            .unwrap()
                            .to_string(),
                        track.name,
                        track.artists.iter().map(|x| x.clone().name).collect(),
                    );
                    return match db.queue_track(spotify_track.clone()) {
                        Err(err) => {
                            HttpResponse::Ok().json(HashMap::from([("error", err.to_string())]))
                        }
                        Ok(_) => {
                            return match data.send(PlayerEvent::Changed {
                                old_track_id: now_playing.spotify_id(),
                                new_track_id: spotify_track.spotify_id(),
                            }) {
                                Err(err) => HttpResponse::Ok()
                                    .json(HashMap::from([("error", err.to_string())])),
                                Ok(_) => {
                                    return match data.send(PlayerEvent::Stopped {
                                        play_request_id: 0,
                                        track_id: spotify_track.spotify_id(),
                                    }) {
                                        Err(err) => HttpResponse::Ok()
                                            .json(HashMap::from([("error", err.to_string())])),
                                        Ok(_) => HttpResponse::Ok().json(spotify_track),
                                    }
                                }
                            }
                        }
                    };
                }
            }
        }
    };
}

#[get("/playlist")]
pub async fn show_playlist(db: Data<SpotifyDatabase>) -> HttpResponse {
    return match db.read() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(state) => HttpResponse::Ok().json(state.queue),
    };
}

async fn api(session: Data<Arc<Mutex<Session>>>) -> Result<AuthCodeSpotify, Option<String>> {
    return match keymaster::get_token(&session.lock().unwrap(), CLIENT_ID, SCOPES).await {
        Err(_) => Err(Some("could not get token".to_string())),
        Ok(search_token) => {
            let token = rspotify::Token {
                access_token: search_token.access_token.clone(),
                expires_in: ChronoDuration::seconds(search_token.expires_in.into()),
                expires_at: Some(
                    Utc::now() + ChronoDuration::seconds(search_token.expires_in.into()),
                ),
                refresh_token: None,
                scopes: HashSet::from_iter(SCOPES.split(",").into_iter().map(|x| x.to_string())),
            };

            let mut spotify = rspotify::AuthCodeSpotify::from_token(token.clone());

            spotify.creds.id = CLIENT_ID.to_string();

            Ok(spotify)
        }
    };
}

#[actix_rt::main]
pub async fn start(tx: SyncSender<PlayerEvent>, config: Arc<Mutex<SpotifmConfig>>, session: Arc<Mutex<Session>>, db: SpotifyDatabase) {
    thread::spawn(move || {
        //let session = session.lock().unwrap().clone();
        match rt::System::new("rest-api").block_on(
            HttpServer::new(move || {
                let tx = web::Data::new(tx.clone());
                let config = web::Data::new(config.clone());
                let session = web::Data::new(session.clone());
                let db = web::Data::new(db.clone());
                App::new()
                    .wrap(middleware::Logger::default())
                    .app_data(tx)
                    .app_data(config)
                    .app_data(session)
                    .app_data(db)
                    .service(np)
                    .service(prev_track)
                    .service(next_track)
                    .service(skip)
                    .service(queue)
                    .service(play)
                    .service(search)
                    .service(show_playlist)
                    .service(shuffle)
                    .service(get_announce)
                    .service(do_espeak_say)
                    .service(do_elevenlabs_say)
                    .service(edit_announce_song)
                    .service(edit_announce_bumper)
                    .service(delete_announce_bumper_tags)
            })
            .bind("0.0.0.0:9090")
            .unwrap()
            .run(),
        ) {
            Ok(_) => {}
            Err(err) => panic!("{}", err.to_string()),
        };
    });
}
