use std::process::Command;
use std::sync::{Arc, Mutex};
use crate::db::SpotifyTrack;
use crate::config::{SpotifmConfig, SpotifmEspeakCfg, SpotifmElevenLabsCfg};

pub fn announcements(_config: Arc<Mutex<SpotifmConfig>>, track: &SpotifyTrack, tracks_played: usize) {
    let mut config = _config.lock().unwrap();

    if config.announce.bumper.enable {
        if config.announce.bumper.freq > 0 && tracks_played % config.announce.bumper.freq == 0 {
            espeak(config.announce.bumper.next(), config.announce.clone().bumper.espeak);
        }
    }

    if config.announce.song.enable {
        let announce_text = format!(
            "{} by {}",
            track.track,
            track.artists[0]
        );
        espeak(announce_text, config.announce.clone().song.espeak);
    }
}

pub fn espeak(text: String, config: SpotifmEspeakCfg){
    Command::new("espeak")
        .arg("-s")
        .arg(config.speed.to_string())
        .arg("-a")
        .arg(config.amplitude.to_string())
        .arg("-p")
        .arg(config.pitch.to_string())
        .arg("-g")
        .arg(config.gap.to_string())
        .arg("-v")
        .arg(config.voice.to_string())
        .arg(text)
        .output()
        .unwrap();
}

pub fn get_elevenlabs_tts(text: &str, config: SpotifmElevenLabsCfg) {
    Command::new("/elevenlabs.sh")
        .arg(config.key)
        .arg(config.voice)
        .arg(text)
        .output()
        .unwrap();
}

pub fn play_elevenlabs(){
    Command::new("mplayer")
        .arg("/tmp/output.mp3")
        .output()
        .unwrap();
}