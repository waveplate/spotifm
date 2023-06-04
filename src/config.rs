use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmConfig {
    pub user: String,
    pub pass: String,
    pub uris: Vec<String>,
    pub announce:  SpotifmAnnounceConfig,
    pub elevenlabs: SpotifmElevenLabsCfg,
}

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmAnnounceConfig {
    pub song: SpotifmSongConfig,
    pub bumper: SpotifmBumperConfig,
}

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmSongConfig {
    pub enable: bool,
    pub espeak: SpotifmEspeakCfg,
}

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmBumperConfig {
    idx: Option<usize>,
    pub enable: bool,
    pub tags: Vec<String>,
    pub freq: usize,
    pub espeak: SpotifmEspeakCfg,
}

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmElevenLabsCfg {
    pub key: String,
    pub voice: String,
}

#[derive(Serialize, Clone, Deserialize)]
pub struct SpotifmEspeakCfg {
    pub speed: u32,
    pub amplitude: u32,
    pub pitch: u32,
    pub gap: u32,
    pub voice: String,
}

impl SpotifmConfig {
    pub fn load(path: String) -> SpotifmConfig {
        let str = std::fs::read_to_string(path)
        .expect("unable to read config file");

        let mut config: SpotifmConfig = serde_json::from_str(&str)
            .expect("unable to parse config file");

        config.announce.bumper.idx = Some(0);

        return config;
    }
}

impl SpotifmBumperConfig {
    pub fn next(&mut self) -> String {
        let tag = self.tags[self.idx.unwrap()].clone();
        self.idx = Some((self.idx.unwrap() + 1) % self.tags.len());
        return tag;
    }

    pub fn clear_tags(&mut self) {
        self.idx = Some(0);
        self.tags = Vec::new();
    }

    pub fn add_tag(&mut self, tag: String) {
        self.tags.push(tag);
    }

    pub fn update_tags(&mut self, tags: Vec<String>) {
        self.idx = Some(0);
        self.tags = tags;
    }
}