use std::process::{Command};
use std::thread;
use std::time::{Duration, Instant};

pub fn volume_fade(start: u32, end: u32, duration: u64) {
    let start_time = Instant::now();
    let total_duration = Duration::from_millis(duration);

    while Instant::now() - start_time < total_duration {
        let elapsed = Instant::now() - start_time;
        let progress = elapsed.as_millis() as f64 / duration as f64;
        let current_value = (start as f64 + (end as f64 - start as f64) * progress) as u32;

        Command::new("pactl")
            .arg("set-sink-volume")
            .arg("spotifm_music")
            .arg(format!("{}%", current_value))
            .output()
            .unwrap();

        let sleep_duration = total_duration / 100;
        thread::sleep(sleep_duration);
    }
}

pub fn announce_volume(volume: String){
    Command::new("pactl")
        .arg("set-sink-volume")
        .arg("spotifm_announce")
        .arg(volume)
        .output()
        .unwrap();
}