use core::{default, time};
use std::{env, thread, sync::mpsc, sync::Arc, sync::Mutex, process::exit, fs, fmt::Debug};
use rustbreak::Database;
use signal_hook::{consts::SIGINT, iterator::Signals};
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

#[derive(Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
struct SpotifyTrack {
    track: String,
    artists: Vec<String>,
}

#[tokio::main]
async fn main() {
    
    
    // let (tx, rx) = mpsc::channel();

    // let mut signals = Signals::new(&[SIGINT])?;

    // thread::spawn(move || {
    //     for sig in signals.forever() {
    //         println!("Received signal {:?}", sig);
    //     }
    // });

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

    for track_id in tracks {
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
        let player_clone = Arc::clone(&player_mutex);

        let mut signals = Signals::new(&[signal_hook::consts::SIGHUP]).expect("error creating signal handler");

        thread::spawn(move || {
            for sig in signals.forever() {
                eprintln!("Received signal {:?}", sig);
                player_clone.lock().unwrap().stop();
            }
        });
   
        player_mutex.lock().unwrap().load(track_id, true, 0);

        let mut i = 1;

        let mut channel = player_mutex.lock().unwrap().get_player_event_channel();
        while let Some(event) = channel.recv().await {
            eprintln!("i={}", i);
            i=i+1;

            if matches!(
                event,
                PlayerEvent::EndOfTrack { .. } | PlayerEvent::Stopped { .. }
            ) {
                break;
            }
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

// fn get_playing(db: FileDatabase::<HashMap<String, SpotifyTrack>, Ron>) -> &'static SpotifyTrack {
//     let ok: &'static SpotifyTrack = db.read(|db| {
//         return db.values().next().clone();
//     }).unwrap().unwrap();
//     return ok;
// }