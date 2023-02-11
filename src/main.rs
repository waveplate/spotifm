#[macro_use]
extern crate actix_web;

use core::{time};
use std::{env, thread, sync::mpsc::{Receiver,SyncSender, sync_channel}, sync::Arc, sync::Mutex, process::exit, fmt::Debug};
use librespot::core::authentication::Credentials;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player,PlayerEvent};
use librespot::metadata::{Track,Artist,Album,Playlist,Metadata};
use serde::{Serialize,Deserialize};
use rustbreak::{
    deser::Ron,
    FileDatabase,
};

use std::collections::HashMap;

use actix_web::{web::{self}, rt, middleware, App, HttpServer};

mod backend;

#[derive(Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
struct SpotifyTrack {
    track: String,
    artists: Vec<String>,
}

#[tokio::main]
async fn main() {

    let (tx, rx): (SyncSender<PlayerEvent>, Receiver<PlayerEvent>)  = sync_channel(100);

    start_rest(tx.clone());

    let np_db = FileDatabase::<HashMap<String, SpotifyTrack>, Ron>::load_from_path_or_default("/tmp/now-playing").map_err(|err| { ini_error(err.to_string().as_str()) }).unwrap();

    let session_config = SessionConfig::default();
    let player_config = PlayerConfig::default();
    let audio_format = AudioFormat::default();

    let args: Vec<_> = env::args().collect();

    if args.len() < 5 {
        eprintln!("Usage: {} USERNAME PASSWORD TRACK PIPE", args[0]);
        return;
    }

    let credentials = Credentials::with_password(&args[1], &args[2]);

    let spotify_id = SpotifyId::from_uri(&args[3])
        .map_err(|_| { ini_error("Invalid Spotify URI") })
        .unwrap();

    let spotify_uri: Vec<&str> = args[3].split(":").collect();

    let backend = audio_backend::find(Some(args[4].to_string())).unwrap();

    let (session, _) = Session::connect(session_config, credentials, None, false).await
        .map_err(|err| { ini_error(err.to_string().as_str())} )
        .unwrap();
    
    let mut tracks: Vec<SpotifyId> = Vec::new();

    match spotify_uri[1] {
        "track" => {
            tracks.push(spotify_id);
        },
        "album" => {
            let alist = Album::get(&session, spotify_id).await.unwrap();
            alist.tracks.iter().for_each(|track| { tracks.push(*track)});
        },
        "playlist" => {
            let plist = Playlist::get(&session, spotify_id).await.unwrap();
            plist.tracks.iter().for_each(|track| { tracks.push(*track)});
        },
        _ => {
            ini_error("Malformed Spotify URI")
        },
    };

    for i in 0..tracks.len() {
        let track_id = tracks[i].clone();
        let session = session.clone();
        
        let track = Track::get(&session, track_id).await
            .map_err(|_| { ini_error("Could not retrieve metadata"); })
            .unwrap();

        let mut artists: Vec<String> = Vec::new();

        for id in track.artists.iter() {
            artists.push(Artist::get(&session, *id).await.unwrap().name);
        };

        np_db.write(|db| {
            // let mut db = db.clone();
            db.clear();
            db.insert(
                track.id.to_base62().unwrap(),
                SpotifyTrack {
                    track: track.name,
                    artists: artists,
                }
            );
        }).expect("error");

        np_db.save().expect("db save error");
        np_db.load().expect("db load error");

        np_db.read(|db| {
            eprintln!("Results:");
            eprintln!("track={}", db.values().next().unwrap().track);
            eprintln!("artists={}", db.values().next().unwrap().artists.join(", "));            
        }).expect("could not read db");

        let (player, _) = Player::new(player_config.clone(), session, Box::new(NoOpVolume), move || {
            backend(None, audio_format)
        });
        
        let player_mutex = Arc::new(Mutex::new(player));

        player_mutex.lock().unwrap().load(track_id, true, 0);

        let mut channel = player_mutex.lock().unwrap().get_player_event_channel();
        
        'event_listener: while let event = channel.try_recv() {

            std::thread::sleep(time::Duration::from_millis(500));

            let rest_event = rx.try_recv();

            let mut player_events: Vec<PlayerEvent> = Vec::new();

            if !event.is_err() {
                player_events.push(event.unwrap());
            }

            if !rest_event.is_err() {
                player_events.push(rest_event.unwrap());
            }
            
            for player_event in player_events {
                match player_event {
                    PlayerEvent::EndOfTrack { .. } => { break 'event_listener },
                    PlayerEvent::Stopped { .. } => { player_mutex.lock().unwrap().stop(); break 'event_listener },   
                    PlayerEvent::Changed { old_track_id: _, new_track_id } => {
                        eprintln!("up next: {}", new_track_id.to_base62().unwrap().as_str());
                        tracks.insert(tracks.iter().position(|&x| x == track_id).unwrap()+1, new_track_id);
                    },
                    _ => {}    
                }
            };

        }

    }

}

fn ini_hash(key: &str, val: &str) {
    eprintln!("{}={}", key.to_ascii_uppercase(), val);
}

fn ini_error(msg: &str) {
    ini_hash("error", msg);
    exit(1);
}

#[actix_rt::main]
async fn start_rest(tx: SyncSender<PlayerEvent>) {
    thread::spawn(move || {
        env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
        env_logger::init();
    
        let tx = web::Data::new(tx.clone());

        rt::System::new("rest-api").block_on(
            HttpServer::new(move || {
                let tx = tx.clone();
                App::new()
                    // enable logger - always register actix-web Logger middleware last
                    .wrap(middleware::Logger::default())
                    .app_data(tx)
                    // register HTTP requests handlers
                    .service(backend::skip)
                    .service(backend::queue)
                    .service(backend::play)
            })
            .bind("0.0.0.0:9090")
            .unwrap()
            .run()
        ).expect("could not start rest api");

    });
}