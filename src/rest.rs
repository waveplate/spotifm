use actix_web::{web::{self, Path, Data}, rt, middleware, App, HttpServer, HttpResponse};
use std::{env, thread, sync::mpsc::{SyncSender}};
use librespot::core::spotify_id::SpotifyId;
use librespot::playback::player::{PlayerEvent};
use std::collections::HashMap;

use crate::db;

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
pub async fn start(tx: SyncSender<PlayerEvent>) {
    thread::spawn(move || {
        env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
        env_logger::init();
    
        let tx = web::Data::new(tx.clone());

        match rt::System::new("rest-api").block_on(
            HttpServer::new(move || {
                let tx = tx.clone();
                App::new()
                    .wrap(middleware::Logger::default())
                    .app_data(tx)
                    .service(np)
                    .service(skip)
                    .service(queue)
                    .service(play)
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