use actix_web::{web::{self, Path, Data}, rt, middleware, App, HttpServer, HttpRequest, HttpResponse};
use std::{env, time, thread, sync::mpsc::{SyncSender}};
use std::sync::{Arc,Mutex};
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, NaiveDateTime, Utc};
use librespot::core::{keymaster, session::Session, spotify_id::SpotifyId};
use librespot::playback::player::{PlayerEvent};
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::FromIterator;
use rspotify::{
    model::{AdditionalType, Country, Market},
    prelude::*,
    scopes, AuthCodeSpotify, Credentials, OAuth,
};

use crate::db;

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const SCOPES: &str =
    "streaming,user-read-playback-state,user-modify-playback-state,user-read-currently-playing";

#[get("/search/{query}")]
pub async fn search(req: HttpRequest, session: Data<Arc<Mutex<Session>>>) -> HttpResponse {
    let query = web::Query::<HashMap<String,String>>::from_query(req.query_string()).unwrap();
    eprintln!("query: {:?}", query);
    let ok: HashSet<String> = HashSet::from_iter(SCOPES.split(",").into_iter().map(|x| x.to_string()));

    return match keymaster::get_token(&session.clone().lock().unwrap(), CLIENT_ID, SCOPES).await {
        Err(_) => HttpResponse::Ok().json(HashMap::from([("error", "token error")])),
        Ok(search_token) => {
            let token = rspotify::Token {
                access_token: search_token.access_token.clone(),
                expires_in: ChronoDuration::seconds(search_token.expires_in.into()),
                expires_at: Some(Utc::now() + ChronoDuration::seconds(search_token.expires_in.into())),
                refresh_token: None,
                scopes: HashSet::new(),
            };
            let mut spotify = rspotify::AuthCodeSpotify::from_token(token);
            

            let market = Market::Country(Country::UnitedStates);
            let additional_types = [AdditionalType::Episode];

            let artists = spotify.current_playing(Some(market), Some(&additional_types)).unwrap().unwrap();

            eprintln!("{:?}", artists);

            return HttpResponse::Ok().json(HashMap::from([("token", artists)]));
        },
    }
}

#[get("/np")]
pub async fn np() -> HttpResponse {
    return match db::now_playing() {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(track) => HttpResponse::Ok().json(track),
    }
}

#[get("/skip")]
pub async fn skip(data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    return match data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: SpotifyId::from_base62("0").unwrap() }) {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(_) => HttpResponse::Ok().json(HashMap::from([("skip", true)])),
    };
}

#[get("/queue/{id}")]
pub async fn queue(path: Path<String>, data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    return match data.send(PlayerEvent::Changed { old_track_id: SpotifyId::from_base62("0").unwrap(), new_track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }) {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(_) => HttpResponse::Ok().json(HashMap::from([("queue", true)])),
    };
}

#[get("/play/{id}")]
pub async fn play(path: Path<String>, data: Data<SyncSender<PlayerEvent>>) -> HttpResponse {
    return match data.send(PlayerEvent::Changed { old_track_id: SpotifyId::from_base62("0").unwrap(), new_track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }) {
        Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
        Ok(_) => {
            return match data.send(PlayerEvent::Stopped { play_request_id: 0, track_id: SpotifyId::from_base62(path.0.as_str()).unwrap() }) {
                Err(err) => HttpResponse::Ok().json(HashMap::from([("error", err.to_string())])),
                Ok(_) => HttpResponse::Ok().json(HashMap::from([("play", true)])),
            }
        }
    }
}

#[actix_rt::main]
pub async fn start(tx: SyncSender<PlayerEvent>, session: Arc<Mutex<Session>>) {
    thread::spawn(move || {
        env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
        env_logger::init();

        match rt::System::new("rest-api").block_on(
            HttpServer::new(move || {
                let tx = web::Data::new(tx.clone());
                let session = web::Data::new(session.clone());
                App::new()
                    .wrap(middleware::Logger::default())
                    .app_data(tx)
                    .app_data(session)
                    .service(np)
                    .service(skip)
                    .service(queue)
                    .service(play)
                    .service(search)
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