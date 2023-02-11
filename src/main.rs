#[macro_use]
extern crate actix_web;

use core::{time};
use std::{env, sync::mpsc::{Receiver,SyncSender, sync_channel}, process::exit};
use librespot::core::authentication::Credentials;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, PlayerConfig};
use librespot::playback::mixer::NoOpVolume;
use librespot::playback::player::{Player,PlayerEvent};
use librespot::metadata::{Track,Artist,Album,Playlist,Metadata};

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

    let session_config = SessionConfig::default();
    let player_config = PlayerConfig::default();
    let audio_format = AudioFormat::default();

    let credentials = Credentials::with_password(&args[1], &args[2]);

    let spotify_id = SpotifyId::from_uri(&args[3])
        .map_err(|_| { panic!("Invalid Spotify URI") })
        .unwrap();

    let backend = audio_backend::find(Some(args[4].to_string())).unwrap();

    let (session, _) = Session::connect(session_config, credentials, None, false).await
        .map_err(|err| { panic!("{}", err.to_string())} )
        .unwrap();

    let (rest_tx, rest_rx): (SyncSender<PlayerEvent>, Receiver<PlayerEvent>) = sync_channel(100);

    rest::start(rest_tx.clone());
    
    let mut tracks: Vec<SpotifyId> = Vec::new();

    match *args[3].split(":").collect::<Vec<&str>>().get(1).unwrap() {
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
            panic!("Malformed Spotify URI")
        },
    };

    'track_list: for i in 0..tracks.len() {
        let session = session.clone();

        let track = match Track::get(&session, tracks[i]).await {
            Err(_) => continue 'track_list,
            Ok(t) => t,
        };

        let mut artists: Vec<Artist> = Vec::new();

        for id in track.artists.iter() {
            match Artist::get(&session, *id).await {
                Err(_) => continue 'track_list,
                Ok(artist) => artists.push(artist),
            }
        };

        match db::write(track, artists) {
            Err(err) => panic!("{}", err.to_string()),
            Ok(_) => {},
        };

        let (mut player, _) = Player::new(player_config.clone(), session, Box::new(NoOpVolume), move || {
            backend(None, audio_format)
        });

        player.load(tracks[i], true, 0);

        let mut player_rx = player.get_player_event_channel();
        
        'event_listener: loop {

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
                    PlayerEvent::EndOfTrack { .. } => { break 'event_listener },
                    PlayerEvent::Stopped { .. } => { player.stop(); break 'event_listener },   
                    PlayerEvent::Changed { old_track_id: _, new_track_id } => {
                        match tracks.iter().position(|x| *x == new_track_id) {
                            Some(pos) => {
                                if pos == i+1 {
                                    continue;
                                }
                            },
                            None => {},
                        }
                        tracks.insert(i+1, new_track_id);
                    },
                    _ => {}    
                }
            };

        }

    }

}