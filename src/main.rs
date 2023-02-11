#[macro_use]
extern crate actix_web;

use core::{time};
use std::{env, thread, sync::mpsc::{Receiver,SyncSender, sync_channel}, process::exit};
use librespot::core::authentication::Credentials;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player,PlayerEvent};

mod db;
mod rest;
mod signals;

const BACKEND: &str = "pulseaudio";

#[tokio::main]
async fn main() {

    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        eprintln!("Usage: {} USERNAME PASSWORD SPOTIFY_URI", args[0]);
        exit(0);
    }

    let db = db::SpotifyDatabase::new();
    let session = create_session(&args[1], &args[2]).await;
    let (rest_tx, rest_rx): (SyncSender<PlayerEvent>, Receiver<PlayerEvent>) = sync_channel(100);

    // worker threads    
    signals::start();
    rest::start(rest_tx.clone(), session.clone(), db.clone());
    db::populate(args[3].clone(), session.clone(), db.clone());
    
    // wait until at least one track in playlist
    while db.len() == 0 {
        thread::sleep(time::Duration::from_millis(10));
    }

    'track_list: loop {
        
        db.advance_track();

        match db.current_track() {
            Err(err) => panic!("{}", err.unwrap()),
            Ok(track) => {
                eprintln!("Playing: {} - {}", track.track, track.artists.join(", "));
        
                let (mut player, mut player_rx) = Player::new(PlayerConfig::default(), session.clone(), Box::new(NoOpVolume), move || {
                    audio_backend::find(Some(BACKEND.to_string())).unwrap()(None, AudioFormat::default())
                });

                player.load(track.spotify_id(), true, 0);
        
                match db.next_track() {
                    Err(err) => eprintln!("preload error: {}", err),
                    Ok(track) => player.preload(track.spotify_id()),
                }
                
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

async fn create_session(username: &String, password: &String) -> Session {
    let session_config = SessionConfig::default();
    let credentials = Credentials::with_password(username, password);

    let (session, _) = Session::connect(session_config, credentials, None, false).await
        .map_err(|err| { panic!("{}", err.to_string())} )
        .unwrap();

    return session;
}


