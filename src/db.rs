
use std::sync::{Arc,Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize,Deserialize};
use librespot::core::spotify_id::SpotifyId;

use rustbreak::{
    deser::Ron,
    MemoryDatabase,
    RustbreakError,
};

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

    pub fn add_track(&self, track: SpotifyTrack) -> Result<SpotifyState, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {
                let new_pos: usize = if state.queue.len() > 0 { state.queue.len() } else { 0 };
                state.queue.insert(new_pos, track);
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
                    state.queue_position = state.queue_position + 1;
                } else {
                    state.queue_position = 0;
                }

                return Ok(state.queue.get(state.queue_position).unwrap().clone());
            },
        }     
    }

    pub fn advance_track(&self) -> Result<SpotifyState, String> {
        return match self.read() {
            Err(err) => Err(err.to_string()),
            Ok(mut state) => {

                if state.queue_position < state.queue.len()-1 {
                    state.queue_position = state.queue_position + 1;
                } else {
                    state.queue_position = 0;
                }

                self.write(state.clone());
                return Ok(state);
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