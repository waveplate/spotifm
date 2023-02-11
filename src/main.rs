#[macro_use]
extern crate actix_web;

use core::{time};
use std::sync::{Arc,Mutex};
use std::{env, thread, sync::mpsc::{Receiver,SyncSender, sync_channel}, process::exit};
use librespot::core::authentication::Credentials;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player,PlayerEvent};
use librespot::metadata::{Track,Artist,Album,Playlist,Metadata};

use tokio::runtime::Runtime;

mod db;
mod rest;
mod signals;

#[tokio::main]
async fn main() {

    let args: Vec<_> = env::args().collect();

    if args.len() < 5 {
        eprintln!("Usage: {} USERNAME PASSWORD TRACK PIPE", args[0]);
        exit(0);
    }

    signals::start();

    let db = db::SpotifyDatabase::new();
    let session_config = SessionConfig::default();
    let player_config = PlayerConfig::default();
    let audio_format = AudioFormat::default();

    let credentials = Credentials::with_password(&args[1], &args[2]);

    let backend = audio_backend::find(Some(args[4].to_string())).unwrap();

    let (session, _) = Session::connect(session_config, credentials, None, false).await
        .map_err(|err| { panic!("{}", err.to_string())} )
        .unwrap();

    let rest_session = Arc::new(Mutex::new(session.clone()));
    let metadata_session = Arc::clone(&rest_session);

    let rest_db = Arc::new(Mutex::new(db.clone()));
    let player_db = Arc::clone(&rest_db);
    let metadata_db = Arc::clone(&player_db);

    let (rest_tx, rest_rx): (SyncSender<PlayerEvent>, Receiver<PlayerEvent>) = sync_channel(100);

    rest::start(rest_tx.clone(), rest_session.clone(), rest_db.clone());

    get_track_metadata(args[3].clone(), metadata_session, metadata_db.clone());

    while player_db.lock().unwrap().len() == 0 {
        thread::sleep(time::Duration::from_millis(10));
    }
    
    'track_list: loop {

        player_db.lock().unwrap().advance_track().expect("error advancing track");

        let current_track = player_db.lock().unwrap().current_track();

        match current_track {
            Err(err) => panic!("{}", err.unwrap()),
            Ok(track) => {
                eprintln!("Playing: {} - {}", track.track, track.artists.join(", "));

                let (mut player, _) = Player::new(player_config.clone(), session.clone(), Box::new(NoOpVolume), move|| {
                    backend(None, audio_format)
                });
        
                player.load(track.spotify_id(), true, 0);
        
                match player_db.lock().unwrap().next_track() {
                    Err(err) => eprintln!("preload error: {}", err),
                    Ok(track) => player.preload(track.spotify_id()),
                }
        
                let mut player_rx = player.get_player_event_channel();
                
                loop {
        
                    std::thread::sleep(time::Duration::from_millis(100));
                    
                    let rest_event = rest_rx.try_recv();
                    let player_event = player_rx.try_recv();
        
                    let mut events: Vec<PlayerEvent> = Vec::new();
        
                    if !rest_event.is_err() {
                        events.push(rest_event.unwrap());
                    }
        
                    if !player_event.is_err() {
                        events.push(player_event.unwrap());
                    }
                    
                    for event in events {
                        match event {
                            PlayerEvent::EndOfTrack { .. } => {
                                continue 'track_list;
                            },
                            PlayerEvent::Stopped { .. } => {
                                player.stop();
                                continue 'track_list;
                            },   
                            PlayerEvent::Changed { new_track_id, .. } => {
                                player.preload(new_track_id);
                            },
                            _ => {}    
                        }
                    };
        
                }
            },
        }

    }

}

fn get_track_metadata(uri: String, session: Arc<Mutex<Session>>, db: Arc<Mutex<db::SpotifyDatabase>>) {
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async move {
    
            let spotify_uri = uri.split(":").collect::<Vec<&str>>();
            let spotify_id = SpotifyId::from_uri(uri.as_str()).unwrap();

            let mut tracks: Vec<SpotifyId> = Vec::new();

            match *spotify_uri.get(1).unwrap() {
                "track" => {
                    tracks.push(spotify_id);
                },
                "album" => {
                    let alist = Album::get(&session.lock().unwrap().clone(), spotify_id).await.unwrap();
                    alist.tracks.iter().for_each(|track| { tracks.push(*track)});
                },
                "playlist" => {
                    let plist = Playlist::get(&session.lock().unwrap().clone(), spotify_id).await.unwrap();
                    plist.tracks.iter().for_each(|track| { tracks.push(*track)});
                },
                _ => {
                    panic!("Malformed Spotify URI")
                },
            };

            for track_id in tracks {
                let session = session.clone();
                let mut track = db::SpotifyTrack::new(track_id.to_base62().unwrap(), "".to_string(), Vec::new());

                let mut artist_ids: Vec<SpotifyId> = Vec::new();

                match Track::get(&session.lock().unwrap().clone(), track_id).await {
                    Err(_) => {},
                    Ok(track_info) => { 
                        artist_ids = track_info.artists;
                        track.track = track_info.name;
                    },
                };

                for id in artist_ids {
                    match Artist::get(&session.clone().lock().unwrap(), id).await {
                        Err(_) => {  },
                        Ok(artist) => { track.artists.push(artist.name) },
                    }
                };
  
                if track.artists.len() > 0 {
                    db.lock().unwrap().add_track(track).expect("error adding track to in-memory database");
                }
            }
    
            //bonus, you could spawn tasks too
            //tokio::spawn(async { do_thing(tracks, session.clone(), db).await });
            // tokio::spawn(async { async_function("task2").await });
        });
    });
}
