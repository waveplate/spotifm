use rustbreak::{
    deser::Ron,
    FileDatabase,
    RustbreakError,
    error
};
use serde::{Serialize,Deserialize};
use std::collections::HashMap;
use librespot::metadata::{Track,Artist};

#[derive(Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
pub struct SpotifyTrack {
    pub track: String,
    pub artists: Vec<String>,
}

pub fn open() -> Result<FileDatabase::<HashMap<String, SpotifyTrack>, Ron>, RustbreakError> {
    return FileDatabase::<HashMap<String, SpotifyTrack>, Ron>::load_from_path_or_default("./now-playing");
}

pub fn write(track: Track, artists: Vec<Artist>) -> error::Result<()> {
    return match open() {
        Err(err) => Err(err),
        Ok(npdb) => {
            return match npdb.write(|db| {
                db.clear();
                db.insert(
                    track.id.to_base62().unwrap(),
                    SpotifyTrack {
                        track: track.name,
                        artists: artists.iter().map(|a| a.name.clone()).clone().collect(),
                    }
                );
                
                match npdb.save() {
                    Err(err) => Err(err),
                    Ok(_) => {
                        match npdb.load() {
                            Err(err) => Err(err),
                            Ok(_) => Ok(()),
                        }
                    }
                }
            }) {
                Err(err) => Err(err),
                Ok(_) => Ok(()),
            };
        },
    }

}

pub fn now_playing() -> Result<SpotifyTrack, RustbreakError> {
    return match open() {
        Err(err) => Err(err),
        Ok(db) => {
            match db.read(|db| {
                return db.values().next().unwrap().clone()
            })
            {
                Err(err) => Err(err),
                Ok(track) => Ok(track),
            }
        },
    }

}