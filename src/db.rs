use std::thread;
use std::sync::{Arc,Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize,Deserialize};
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::metadata::{Track,Artist,Album,Playlist,Metadata};
use tokio::runtime::Runtime;

use rustbreak::{
    deser::Ron,
    MemoryDatabase,
    RustbreakError,
};

use rand::thread_rng;
use rand::seq::SliceRandom;

#[derive(Clone)]
pub struct SpotifyDatabase {
    pub handle: Arc<Mutex<MemoryDatabase::<SpotifyState, Ron>>>
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
pub struct SpotifyState {
    pub queue: Vec<SpotifyTrack>,
    pub queue_position: usize,
}

#[derive(Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
pub struct SpotifyTrack {
    pub id: String,
    pub rid: u128,
    pub track: String,
    pub artists: Vec<String>,
}

impl SpotifyState {
    pub fn new() -> SpotifyState {
        return SpotifyState {
            queue: Vec::new(),
            queue_position: 0,
        }
    }
}

impl SpotifyTrack {
    pub fn new(id: String, track: String, artists: Vec<String>) -> SpotifyTrack {
        return SpotifyTrack {
            id: id,
            rid: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
            track: track,
            artists: artists,
        }
    }

    pub fn spotify_id(&self) -> SpotifyId {
        return SpotifyId::from_base62(self.id.as_str()).unwrap();
    }
}

impl SpotifyDatabase {
    pub fn new() -> SpotifyDatabase {
        let handle = MemoryDatabase::<SpotifyState, Ron>::memory(SpotifyState::new());
        if handle.is_err() {
            panic!("could not open in-memory database");
        }
        return SpotifyDatabase { handle: Arc::new(Mutex::new(handle.unwrap())) };
    }

    pub fn len(&self) -> usize {
        return match self.read() {
            Err(_) => 0,
            Ok(state) => state.queue.len(),
        }
    }

    pub fn shuffle(&self) -> Result<SpotifyState, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {
                let current_id = state.queue.get(state.queue_position).unwrap().id.clone();
                state.queue.shuffle(&mut thread_rng());
                state.queue_position = state.queue.iter().position(|x| x.id == current_id).unwrap();
                self.write(state.clone());
                return Ok(state);
            }
        }
    }

    pub fn add_track(&self, track: SpotifyTrack) -> Result<SpotifyState, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {
                state.queue.insert(state.queue.len(), track);
                self.write(state.clone());
                return Ok(state);
            },
        }
    }

    pub fn queue_track(&self, track: SpotifyTrack) -> Result<SpotifyState, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {
                let current_id = state.queue.get(state.queue_position).unwrap().id.clone();
                state.queue = state.queue.iter().filter(|x| x.id != track.id).map(|x| x.clone()).collect();
                state.queue_position = state.queue.iter().position(|x| x.id == current_id).unwrap();
                let new_pos: usize = if state.queue.len() > 0 { state.queue_position+1 } else { 0 };
                state.queue.insert(new_pos, track);
                self.write(state.clone());
                return Ok(state);
            },
        }
    }

    pub fn current_track(&self) -> Result<SpotifyTrack, Option<String>> {
        return match self.read() {
            Err(err) => Err(Some(err.to_string())),
            Ok(state) => {
                if state.queue.len() == 0 {
                    return Err(Some("no tracks in database".to_string()));
                }
                return Ok(state.queue.get(state.queue_position).unwrap().clone());
            },
        }
    }

    pub fn next_track(&self) -> Result<SpotifyTrack, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {
                if state.queue_position < state.queue.len()-1 {
                    state.queue_position += 1;
                } else {
                    state.queue_position = 0;
                }
                return Ok(state.queue.get(state.queue_position).unwrap().clone());
            },
        }     
    }

    pub fn advance_track(&self) {
        return match self.read() {
            Err(_) => {},
            Ok(mut state) => {
                if state.queue_position < state.queue.len()-1 {
                    state.queue_position += 1;
                } else {
                    state.queue_position = 0;
                }
                self.write(state.clone());
            },
        }
    }

    pub fn write(&self, state: SpotifyState) -> () {
        self.handle.lock().unwrap().write(|db| {
            db.queue = state.queue;
            db.queue_position = state.queue_position;
        }).expect("error writing to in-memory database")
    }

    pub fn read(&self) -> Result<SpotifyState, RustbreakError> {
        return match self.handle.clone().lock().unwrap().read(|x| x.clone()) {
            Err(err) => Err(err),
            Ok(tracks) => {
                let ok = tracks.clone();
                return Ok(ok);
            }
        };
    }

}

pub fn populate(uri: String, session: Session, db: SpotifyDatabase) {
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
                "artist" => {
                    let list = Artist::get(&session.clone(), spotify_id).await.unwrap();
                    tracks = list.top_tracks.iter().map(|x| x.clone()).collect::<Vec<SpotifyId>>();
                },
                "album" => {
                    let list = Album::get(&session.clone(), spotify_id).await.unwrap();
                    tracks = list.tracks.iter().map(|x| x.clone()).collect::<Vec<SpotifyId>>();
                },
                "playlist" => {
                    let list = Playlist::get(&session.clone(), spotify_id).await.unwrap();
                    tracks = list.tracks.iter().map(|x| x.clone()).collect::<Vec<SpotifyId>>();
                },
                _ => {
                    panic!("Malformed Spotify URI")
                },
            };

            for track_id in tracks {
                let session = session.clone();
                let mut track = SpotifyTrack::new(track_id.to_base62().unwrap(), "".to_string(), Vec::new());

                match Track::get(&session.clone(), track_id).await {
                    Err(_) => {},
                    Ok(track_info) => { 
                        track.track = track_info.name;
                        for id in track_info.artists {
                            match Artist::get(&session.clone(), id).await {
                                Err(_) => {  },
                                Ok(artist) => { track.artists.push(artist.name) },
                            }
                        };
          
                        db.add_track(track).expect("error adding track to in-memory database");
                    },
                };
            }
        });
    });
}