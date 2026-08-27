use clap::Parser;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn validate_player_path(path: PathBuf) -> Result<PathBuf, String> {
    if path.to_string_lossy().contains("://") {
        Err("player must be a local directory or index.html path".to_string())
    } else {
        Ok(path)
    }
}

fn parse_player_path(value: &str) -> Result<PathBuf, String> {
    validate_player_path(PathBuf::from(value))
}

pub fn normalize_endpoint_path(value: &str, default_value: &str) -> String {
    let mut endpoint = value.trim();
    if endpoint.is_empty() {
        endpoint = default_value;
    }

    let mut normalized = if endpoint.starts_with('/') {
        endpoint.to_string()
    } else {
        format!("/{endpoint}")
    };

    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }

    normalized
}

pub fn join_endpoint_path(base: &str, child: &str) -> String {
    let base = normalize_endpoint_path(base, "/");
    let child = child.trim_matches('/');
    if child.is_empty() {
        return base;
    }
    if base == "/" {
        format!("/{child}")
    } else {
        format!("{base}/{child}")
    }
}

fn default_player_path() -> PathBuf {
    PathBuf::from("player")
}

/// Command-line arguments parsed by Clap
#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
pub struct CliArgs {
    /// Path to the Spotifm config.toml file [default: ./config.toml, fallback to XDG_CONFIG_HOME]
    #[arg(
        short = 'c',
        long = "conf",
        value_name = "FILE",
        help_heading = "Main options"
    )]
    pub conf: Option<PathBuf>,

    /// Spotify API client ID [default: d420a117a32841c2b3474932e49fb54b]
    #[arg(short = 'd', long = "client-id", help_heading = "Main options")]
    pub client_id: Option<String>,

    /// Override the default OAuth redirect URI [default: http://127.0.0.1:8898/login]
    #[arg(short = 'r', long = "redirect-uri", help_heading = "Main options")]
    pub redirect_uri: Option<String>,

    /// HTTP and HTTPS server IP [default: 0.0.0.0]
    #[arg(short = 's', long = "api-ip", help_heading = "Main options")]
    pub api_ip: Option<String>,

    /// HTTP API and player port [default: 3333]
    #[arg(short = 'p', long = "api-port", help_heading = "Main options")]
    pub api_port: Option<u16>,

    /// HTTPS API and player port [default: 3443]
    #[arg(long = "tls-port", help_heading = "TLS options")]
    pub tls_port: Option<u16>,

    /// PEM certificate chain for the HTTPS server [default: generated automatically]
    #[arg(long = "tls-cert", value_name = "FILE", help_heading = "TLS options")]
    pub tls_cert: Option<PathBuf>,

    /// PEM private key for the HTTPS server [default: generated automatically]
    #[arg(long = "tls-key", value_name = "FILE", help_heading = "TLS options")]
    pub tls_key: Option<PathBuf>,

    /// Path to web player directory or index.html [default: ./player, fallback to XDG data directory]
    #[arg(
        short = 'w',
        long = "player",
        value_name = "PATH",
        value_parser = parse_player_path,
        help_heading = "Main options"
    )]
    pub player: Option<PathBuf>,

    /// OAuth listener IP used when --redirect-uri is not specified [default: 0.0.0.0]
    #[arg(short = 'S', long = "oauth-ip", help_heading = "OAuth options")]
    pub oauth_ip: Option<String>,

    /// OAuth listener port [default: redirect-uri port]
    #[arg(short = 'P', long = "oauth-port", help_heading = "OAuth options")]
    pub oauth_port: Option<u16>,

    /// Do not open the OAuth URL in a web browser [default: false]
    #[arg(
        short = 'x',
        long = "no-browser",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
        help_heading = "OAuth options"
    )]
    pub no_browser: Option<bool>,

    /// Skip importing existing credentials/token from ncspot [default: false]
    #[arg(
        short = 'X',
        long = "skip-import",
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
        help_heading = "OAuth options"
    )]
    pub skip_import: Option<bool>,

    /// Write the OAuth authorization URL to a file [default: none]
    #[arg(short = 'u', long = "url-file", help_heading = "OAuth options")]
    pub url_file: Option<PathBuf>,

    /// Spotify source bitrate in kbps (96, 160, or 320) [default: 320]
    #[arg(short = 'b', long = "bitrate", help_heading = "Streaming options")]
    pub bitrate: Option<u32>,

    /// GStreamer broadcast queue size capacity (lower = lower latency) [default: 32]
    #[arg(short = 'Q', long = "queue-size", help_heading = "Streaming options")]
    pub queue_size: Option<u32>,

    /// GStreamer appsink maximum buffer count [default: 10]
    #[arg(short = 'M', long = "max-buffers", help_heading = "Streaming options")]
    pub max_buffers: Option<u32>,

    /// Stream silence chunk interval in milliseconds [default: 40]
    #[arg(
        short = 'I',
        long = "silence-interval",
        help_heading = "Streaming options"
    )]
    pub silence_interval: Option<u64>,

    /// Audio pipeline to use, or passthrough for native Ogg/Vorbis [default: passthrough]
    #[arg(short = 'g', long = "pipeline", help_heading = "Streaming options")]
    pub pipeline: Option<String>,

    /// URL path at which the live audio stream is served [default: /listen]
    #[arg(
        short = 'E',
        long = "stream-endpoint",
        value_name = "PATH",
        help_heading = "Streaming options"
    )]
    pub stream_endpoint: Option<String>,

    /// URL path at which the web player is served [default: /]
    #[arg(
        short = 'e',
        long = "player-endpoint",
        value_name = "PATH",
        help_heading = "Main options"
    )]
    pub player_endpoint: Option<String>,

    /// Milliseconds between lyrics WebSocket ticks [default: 250]
    #[arg(short = 'L', long = "lyrics-interval", help_heading = "Lyrics options")]
    pub lyrics_interval: Option<u64>,

    /// Capacity for lyrics WebSocket broadcast channel [default: 1024]
    #[arg(short = 'C', long = "capacity", help_heading = "Lyrics options")]
    pub capacity: Option<usize>,
}

/// Configuration structure deserialized from config.toml
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub oauth_ip: Option<String>,
    pub oauth_port: Option<u16>,
    pub redirect_uri: Option<String>,
    pub no_browser: Option<bool>,
    pub skip_import: Option<bool>,
    pub url_file: Option<PathBuf>,
    pub client_id: Option<String>,
    pub api_ip: Option<String>,
    pub api_port: Option<u16>,
    pub tls_port: Option<u16>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub bitrate: Option<u32>,
    pub queue_size: Option<u32>,
    pub max_buffers: Option<u32>,
    pub silence_interval: Option<u64>,
    pub lyrics_interval: Option<u64>,
    pub capacity: Option<usize>,
    pub pipeline: Option<String>,
    pub gst_pipelines: Option<BTreeMap<String, String>>,
    pub stream_endpoint: Option<String>,
    pub player_endpoint: Option<String>,
    pub player: Option<PathBuf>,
    pub api_key: Option<Vec<ApiKeyConfig>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ApiKeyConfig {
    pub key: String,
    pub scope: Vec<String>,
}

/// Finalized configuration resolved by merging CLI args and TOML file values
#[derive(Debug, Clone)]
pub struct Cli {
    pub oauth_ip: Option<String>,
    pub oauth_port: Option<u16>,
    pub redirect_uri: String,
    pub no_browser: bool,
    pub skip_import: bool,
    pub url_file: Option<PathBuf>,
    pub client_id: String,
    pub api_ip: String,
    pub api_port: u16,
    pub tls_port: u16,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub playlist_path: Option<PathBuf>,
    pub bitrate: u32,
    pub queue_size: u32,
    pub max_buffers: u32,
    pub silence_interval: u64,
    pub lyrics_interval: u64,
    pub capacity: usize,
    pub pipeline: String,
    pub passthrough: bool,
    pub gst_pipeline: String,
    pub stream_endpoint: String,
    pub player_endpoint: String,
    pub player: PathBuf,
    pub api_keys: Option<std::collections::HashMap<String, Vec<String>>>,
}

fn default_gst_pipelines() -> BTreeMap<String, String> {
    let mut pipelines = BTreeMap::new();
    pipelines.insert(
        "mp3".to_string(),
        "appsrc name=src format=time is-live=true block=false max-time=10000000000 leaky-type=downstream caps=audio/x-raw,format=S16LE,rate=44100,channels=2,layout=interleaved ! audioconvert ! audioresample ! lamemp3enc bitrate={bitrate} ! appsink name=sink max-buffers={max_buffers} drop=true".to_string(),
    );
    pipelines.insert(
        "opus".to_string(),
        "appsrc name=src format=time is-live=true block=false max-time=10000000000 leaky-type=downstream caps=audio/x-raw,format=S16LE,rate=44100,channels=2,layout=interleaved ! audioconvert ! audioresample ! opusenc ! oggmux max-delay=20000000 max-page-delay=20000000 ! appsink name=sink max-buffers={max_buffers} drop=true".to_string(),
    );
    pipelines.insert(
        "vorbis".to_string(),
        "appsrc name=src format=time is-live=true block=false max-time=10000000000 leaky-type=downstream caps=audio/x-raw,format=S16LE,rate=44100,channels=2,layout=interleaved ! audioconvert ! audioresample ! vorbisenc ! oggmux max-delay=20000000 max-page-delay=20000000 ! appsink name=sink max-buffers={max_buffers} drop=true".to_string(),
    );
    pipelines
}

fn expand_pipeline_template(template: &str, bitrate: u32, max_buffers: u32) -> String {
    template
        .replace("{bitrate}", &bitrate.to_string())
        .replace("{max_buffers}", &max_buffers.to_string())
}

pub fn xdg_data_home() -> Option<PathBuf> {
    if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data_home.is_empty() {
            return Some(PathBuf::from(xdg_data_home));
        }
    }

    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".local").join("share"))
}

pub fn xdg_data_dirs() -> Vec<PathBuf> {
    if let Ok(xdg_data_dirs) = std::env::var("XDG_DATA_DIRS") {
        if !xdg_data_dirs.is_empty() {
            return xdg_data_dirs
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();
        }
    }

    vec![
        PathBuf::from("/usr/local/share"),
        PathBuf::from("/usr/share"),
    ]
}

pub fn player_path_for_data_home(data_home: &Path) -> PathBuf {
    data_home.join("spotifm").join("player")
}

pub fn player_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("player"));

    if let Some(data_home) = xdg_data_home() {
        candidates.push(player_path_for_data_home(&data_home));
    }

    for data_dir in xdg_data_dirs() {
        candidates.push(player_path_for_data_home(&data_dir));
    }

    candidates
}

pub fn resolve_player_path_from(
    cli_player: Option<PathBuf>,
    toml_player: Option<PathBuf>,
    candidates: &[PathBuf],
) -> PathBuf {
    if let Some(path) = cli_player {
        return path;
    }

    if let Some(path) = toml_player {
        let is_default_relative = path == Path::new("player") || path == Path::new("./player");
        if !is_default_relative || path.exists() {
            return path;
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    default_player_path()
}

pub fn resolve_player_path(
    cli_player: Option<PathBuf>,
    toml_player: Option<PathBuf>,
) -> PathBuf {
    resolve_player_path_from(cli_player, toml_player, &player_candidates())
}

fn playlist_path_for_data_home(data_home: &Path) -> PathBuf {
    data_home
        .join("spotifm")
        .join("playlists")
        .join("default.json")
}

fn xdg_playlist_path() -> PathBuf {
    let data_home = xdg_data_home().unwrap_or_else(|| PathBuf::from("."));
    playlist_path_for_data_home(&data_home)
}

fn resolve_config_and_playlist_paths(conf: Option<&Path>) -> (PathBuf, PathBuf) {
    let playlist_path = xdg_playlist_path();

    if let Some(config_path) = conf {
        return (config_path.to_path_buf(), playlist_path);
    }

    if Path::new("config.toml").is_file() {
        let config_path = PathBuf::from("./config.toml");
        return (config_path, playlist_path);
    }

    let config_home = if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.is_empty() {
            PathBuf::from(xdg_config_home)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            PathBuf::from(".")
        }
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config")
    } else {
        PathBuf::from(".")
    };
    let config_path = config_home.join("spotifm").join("config.toml");

    (config_path, playlist_path)
}

impl Cli {
    /// Parse command-line args, overlay TOML file if present, and resolve with defaults.
    pub fn resolve() -> Self {
        let args = CliArgs::parse();

        let (config_path, playlist_path) = resolve_config_and_playlist_paths(args.conf.as_deref());

        // 1. Try to load TOML config file from the resolved config directory.
        let mut toml_config = ConfigFile::default();
        if !config_path.is_file() {
            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let default_toml_content = r#"api_ip = "0.0.0.0"
api_port = 3333
tls_port = 3443
# tls_cert = "/path/to/fullchain.pem"
# tls_key = "/path/to/privkey.pem"
client_id = "d420a117a32841c2b3474932e49fb54b"
redirect_uri = "http://127.0.0.1:3333/login"
oauth_ip = "0.0.0.0"
no_browser = false
bitrate = 320
queue_size = 32
max_buffers = 10
silence_interval = 40
pipeline = "passthrough"
stream_endpoint = "/listen"
player_endpoint = "/"
lyrics_interval = 250
capacity = 1024
player = "player"
"#;
            if let Err(e) = std::fs::write(&config_path, default_toml_content) {
                eprintln!(
                    "[Config] ⚠️ Failed to create default config file {}: {}",
                    config_path.display(),
                    e
                );
            } else {
                println!(
                    "[Config] Created default configuration file at {}",
                    config_path.display()
                );
            }
        }

        if config_path.is_file() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str::<ConfigFile>(&content) {
                    Ok(parsed) => {
                        toml_config = parsed;
                        println!(
                            "[Config] Loaded configuration from {}",
                            config_path.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("[Config] ⚠️ Failed to parse config file: {}", e);
                    }
                },
                Err(e) => {
                    eprintln!(
                        "[Config] ⚠️ Failed to read config file {}: {}",
                        config_path.display(),
                        e
                    );
                }
            }
        }

        // 2. Merge values: CLI args take precedence over TOML values, which take precedence over defaults
        let redirect_uri = args
            .redirect_uri
            .clone()
            .or(toml_config.redirect_uri.clone())
            .unwrap_or_else(|| "http://127.0.0.1:3333/login".to_string());

        let api_ip = args
            .api_ip
            .clone()
            .or(toml_config.api_ip.clone())
            .unwrap_or_else(|| "0.0.0.0".to_string());

        let bitrate = args.bitrate.or(toml_config.bitrate).unwrap_or(320);
        let max_buffers = args.max_buffers.or(toml_config.max_buffers).unwrap_or(10);

        let mut gst_pipelines = default_gst_pipelines();
        if let Some(custom_pipelines) = toml_config.gst_pipelines.clone() {
            gst_pipelines.extend(custom_pipelines);
        }

        let pipeline = args
            .pipeline
            .or(toml_config.pipeline.clone())
            .unwrap_or_else(|| "passthrough".to_string());
        let passthrough = pipeline.eq_ignore_ascii_case("passthrough");
        let gst_pipeline = if passthrough {
            String::new()
        } else {
            let gst_pipeline_template = gst_pipelines.get(&pipeline).unwrap_or_else(|| {
                let available = std::iter::once("passthrough".to_string())
                    .chain(gst_pipelines.keys().cloned())
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!(
                    "[Config] Unknown pipeline '{}'. Available pipelines: {}",
                    pipeline, available
                );
                std::process::exit(2);
            });
            expand_pipeline_template(gst_pipeline_template, bitrate, max_buffers)
        };

        Self {
            oauth_ip: args.oauth_ip.or(toml_config.oauth_ip),
            oauth_port: args.oauth_port.or(toml_config.oauth_port),
            redirect_uri,
            no_browser: args.no_browser.or(toml_config.no_browser).unwrap_or(false),
            skip_import: args
                .skip_import
                .or(toml_config.skip_import)
                .unwrap_or(false),
            url_file: args.url_file.or(toml_config.url_file),
            client_id: args
                .client_id
                .or(toml_config.client_id)
                .unwrap_or_else(|| "d420a117a32841c2b3474932e49fb54b".to_string()),
            api_ip,
            api_port: args.api_port.or(toml_config.api_port).unwrap_or(3333),
            tls_port: args.tls_port.or(toml_config.tls_port).unwrap_or(3443),
            tls_cert: args.tls_cert.or(toml_config.tls_cert),
            tls_key: args.tls_key.or(toml_config.tls_key),
            playlist_path: Some(playlist_path),
            bitrate,
            queue_size: args.queue_size.or(toml_config.queue_size).unwrap_or(32),
            max_buffers,
            silence_interval: args
                .silence_interval
                .or(toml_config.silence_interval)
                .unwrap_or(40),
            lyrics_interval: args
                .lyrics_interval
                .or(toml_config.lyrics_interval)
                .unwrap_or(250),
            capacity: args.capacity.or(toml_config.capacity).unwrap_or(1024),
            pipeline,
            passthrough,
            gst_pipeline,
            stream_endpoint: normalize_endpoint_path(
                &args
                    .stream_endpoint
                    .or(toml_config.stream_endpoint)
                    .unwrap_or_else(|| "/listen".to_string()),
                "/listen",
            ),
            player_endpoint: normalize_endpoint_path(
                &args
                    .player_endpoint
                    .or(toml_config.player_endpoint)
                    .unwrap_or_else(|| "/".to_string()),
                "/",
            ),
            player: validate_player_path(resolve_player_path(
                args.player,
                toml_config.player,
            ))
            .unwrap_or_else(|error| {
                eprintln!("[Config] {error}");
                std::process::exit(2);
            }),
            api_keys: {
                let mut resolved = std::collections::HashMap::new();
                if let Some(ref keys) = toml_config.api_key {
                    for k in keys {
                        resolved.insert(k.key.clone(), k.scope.clone());
                    }
                }
                if resolved.is_empty() {
                    None
                } else {
                    Some(resolved)
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_gst_pipelines, default_player_path, playlist_path_for_data_home,
        resolve_config_and_playlist_paths, xdg_playlist_path, CliArgs, ConfigFile,
    };
    use clap::Parser;
    use std::path::{Path, PathBuf};

    #[test]
    fn conf_is_a_config_file_path() {
        let args =
            CliArgs::try_parse_from(["spotifm", "--conf", "/srv/spotifm/custom.toml"]).unwrap();
        let (config_path, playlist_path) = resolve_config_and_playlist_paths(args.conf.as_deref());

        assert_eq!(config_path, PathBuf::from("/srv/spotifm/custom.toml"));
        assert_eq!(playlist_path, xdg_playlist_path());
    }

    #[test]
    fn boolean_flags_accept_bare_and_explicit_values() {
        let bare = CliArgs::try_parse_from(["spotifm", "--no-browser", "--skip-import"]).unwrap();
        assert_eq!(bare.no_browser, Some(true));
        assert_eq!(bare.skip_import, Some(true));

        let explicit =
            CliArgs::try_parse_from(["spotifm", "--no-browser", "false", "--skip-import=false"])
                .unwrap();
        assert_eq!(explicit.no_browser, Some(false));
        assert_eq!(explicit.skip_import, Some(false));
    }

    #[test]
    fn passthrough_is_selected_as_a_pipeline() {
        let args = CliArgs::try_parse_from(["spotifm", "--pipeline", "passthrough"]).unwrap();
        assert_eq!(args.pipeline.as_deref(), Some("passthrough"));
        assert!(CliArgs::try_parse_from(["spotifm", "--passthrough"]).is_err());
    }

    #[test]
    fn built_in_ogg_pipelines_use_low_latency_pages() {
        let pipelines = default_gst_pipelines();
        for name in ["opus", "vorbis"] {
            let pipeline = pipelines.get(name).unwrap();
            assert!(pipeline.contains("oggmux max-delay=20000000 max-page-delay=20000000"));
        }
    }

    #[test]
    fn undefined_config_parameters_are_rejected() {
        for config in [
            "oauth_redirect_uri = 'http://127.0.0.1/login'",
            "oauth_no_browser = true",
            "oauth_url_file = '/tmp/oauth'",
            "stream_bitrate = 320",
            "stream_queue_size = 32",
            "stream_max_buffers = 10",
            "stream_silence_interval = 40",
            "lyrics_update_interval_ms = 250",
            "lyrics_ws_capacity = 1024",
            "listen_endpoint = '/listen'",
            "passthrough = true",
        ] {
            assert!(toml::from_str::<ConfigFile>(config).is_err(), "{config}");
        }
    }

    #[test]
    fn tls_certificate_and_key_flags_accept_paths() {
        let args = CliArgs::try_parse_from([
            "spotifm",
            "--tls-port",
            "4443",
            "--tls-cert",
            "/etc/letsencrypt/live/radio.example/fullchain.pem",
            "--tls-key",
            "/etc/letsencrypt/live/radio.example/privkey.pem",
        ])
        .unwrap();

        assert_eq!(args.tls_port, Some(4443));
        assert_eq!(
            args.tls_cert,
            Some(PathBuf::from(
                "/etc/letsencrypt/live/radio.example/fullchain.pem"
            ))
        );
        assert_eq!(
            args.tls_key,
            Some(PathBuf::from(
                "/etc/letsencrypt/live/radio.example/privkey.pem"
            ))
        );
    }

    #[test]
    fn explicit_conf_does_not_relocate_xdg_playlists() {
        let (config_path, playlist_path) =
            resolve_config_and_playlist_paths(Some(Path::new("custom.toml")));
        assert_eq!(config_path, PathBuf::from("custom.toml"));
        assert_eq!(playlist_path, xdg_playlist_path());
    }

    #[test]
    fn playlists_live_under_the_xdg_data_home() {
        assert_eq!(
            playlist_path_for_data_home(Path::new("/srv/data")),
            PathBuf::from("/srv/data/spotifm/playlists/default.json")
        );
    }

    #[test]
    fn player_flag_accepts_file_or_directory_paths() {
        let args =
            CliArgs::try_parse_from(["spotifm", "--player", "/srv/player/index.html"]).unwrap();
        assert_eq!(args.player, Some(PathBuf::from("/srv/player/index.html")));
        assert_eq!(default_player_path(), PathBuf::from("player"));

        assert!(
            CliArgs::try_parse_from(["spotifm", "--player", "https://example.com/index.html"])
                .is_err()
        );
    }

    #[test]
    fn player_path_under_data_home() {
        assert_eq!(
            super::player_path_for_data_home(Path::new("/srv/data")),
            PathBuf::from("/srv/data/spotifm/player")
        );
    }

    #[test]
    fn resolve_player_path_prefers_cli_arg() {
        let candidates = vec![PathBuf::from("/tmp/nonexistent-player")];
        let resolved = super::resolve_player_path_from(
            Some(PathBuf::from("/opt/custom-player")),
            Some(PathBuf::from("/etc/spotifm/player")),
            &candidates,
        );
        assert_eq!(resolved, PathBuf::from("/opt/custom-player"));
    }

    #[test]
    fn resolve_player_path_uses_custom_toml_path() {
        let candidates = vec![PathBuf::from("/tmp/nonexistent-player")];
        let resolved = super::resolve_player_path_from(
            None,
            Some(PathBuf::from("/etc/spotifm/player")),
            &candidates,
        );
        assert_eq!(resolved, PathBuf::from("/etc/spotifm/player"));
    }

    #[test]
    fn resolve_player_path_falls_back_to_xdg_when_local_does_not_exist() {
        let xdg_candidate = std::env::temp_dir().join(format!("spotifm-test-xdg-player-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&xdg_candidate);

        let non_existent_local = PathBuf::from("/tmp/definitely-nonexistent-local-player-dir-12345");
        let candidates = vec![non_existent_local, xdg_candidate.clone()];

        let resolved = super::resolve_player_path_from(
            None,
            None,
            &candidates,
        );
        assert_eq!(resolved, xdg_candidate);

        let _ = std::fs::remove_dir_all(&xdg_candidate);
    }

    #[test]
    fn resolve_player_path_returns_default_when_no_candidates_exist() {
        let non_existent_1 = PathBuf::from("/tmp/definitely-nonexistent-1");
        let non_existent_2 = PathBuf::from("/tmp/definitely-nonexistent-2");
        let candidates = vec![non_existent_1, non_existent_2];

        let resolved = super::resolve_player_path_from(
            None,
            None,
            &candidates,
        );
        assert_eq!(resolved, PathBuf::from("player"));
    }
}
