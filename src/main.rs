#[macro_use]
extern crate actix_web;

use core::time;
use std::sync::{Arc, Mutex};
use std::{thread, sync::mpsc::{Receiver,SyncSender, sync_channel}};
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
mod config;
mod announce;

use config::SpotifmConfig;

const BACKEND: &str = "pulseaudio";

#[tokio::main]
async fn main() {

    let mut tracks_played = 0;

    // get the first argument
    let args: Vec<String> = std::env::args().collect();

    let db = db::SpotifyDatabase::new();
    let config = Arc::new(Mutex::new(SpotifmConfig::load(args.get(1).unwrap().clone())));
    let session = Arc::new(Mutex::new(create_session(&config).await));
    let (rest_tx, rest_rx): (SyncSender<PlayerEvent>, Receiver<PlayerEvent>) = sync_channel(100);
    let (signal_tx, signal_rx): (SyncSender<signals::SignalMessage>, Receiver<signals::SignalMessage>) = sync_channel(100);

    // worker threads    
    signals::start(signal_tx.clone());
    rest::start(rest_tx.clone(), config.clone(), session.clone(), db.clone());
    db::populate(config.lock().unwrap().uris.clone(), session.clone(), db.clone());
    
    eprintln!("waiting for playlist...");

    // wait until at least one track in playlist
    while db.len() == 0 {
        thread::sleep(time::Duration::from_millis(10));
    }

    eprintln!("playlist ready, starting playback...");

    'track_list: loop {

        tracks_played += 1;

        db.advance_track();

        match db.current_track() {
            Err(err) => panic!("{}", err.unwrap()),
            Ok(track) => {
                tracks_played += 1;

                eprintln!("Playing: {} - {}", track.track, track.artists.join(", "));

                let (mut player, mut player_rx) = Player::new(PlayerConfig::default(), session.lock().unwrap().clone(), Box::new(NoOpVolume), move || {
                    audio_backend::find(Some(BACKEND.to_string())).unwrap()(None, AudioFormat::default())
                });

                announce::announcements(config.clone(), &track, tracks_played);

                player.load(track.spotify_id(), true, 0);
        
                match db.next_track() {
                    Err(err) => eprintln!("preload error: {}", err),
                    Ok(track) => player.preload(track.spotify_id()),
                }
                
                loop {
                    thread::sleep(time::Duration::from_millis(100));
                    
                    let rest_event = rest_rx.try_recv();
                    let player_event = player_rx.try_recv();
                    let signal_event = signal_rx.try_recv();

                    if !signal_event.is_err() {
                        match signal_event.unwrap() {
                            signals::SignalMessage::SessionExpired => {
                                eprintln!("Session expired, creating new session...");
                                *session.lock().unwrap() = create_session(&config).await;
                                continue 'track_list;
                            }
                        }
                    }

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

pub async fn create_session(config: &Arc<Mutex<SpotifmConfig>>) -> Session {
    let config = config.lock().unwrap();
    let session_config = SessionConfig::default();
    let credentials = Credentials::with_password(config.user.clone(), config.pass.clone());

    let (session, _) = Session::connect(session_config, credentials, None, false).await
        .map_err(|err| { panic!("{}", err.to_string())} )
        .unwrap();

    return session;
}


