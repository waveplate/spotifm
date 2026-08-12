pub mod helpers;
pub mod lyrics;
pub mod oauth;
pub mod playback;
pub mod player;
pub mod playlist;
pub mod queue;
pub mod search;
pub mod stream;

pub use lyrics::{handle_lyrics_rest, handle_lyrics_ws};
pub use oauth::{authorization_transition_page, handle_oauth_page, handle_oauth_status};
pub use playback::{
    handle_next, handle_np, handle_play_category, handle_play_category_search, handle_skip,
    handle_skip_category, handle_skip_n,
};
pub use player::{
    handle_minimal_player, handle_player, handle_player_asset_name, handle_player_audio_worklet,
    handle_player_service_worker, handle_player_vendor_ogg_vorbis_decoder,
};
pub use playlist::{
    handle_playlist, handle_playlist_add, handle_playlist_add_json, handle_playlist_delete,
    handle_playlist_delete_where, handle_playlist_delete_where_json, handle_playlist_get,
    handle_playlist_play_track, handle_playlist_remove_track, handle_playlist_shuffle,
    handle_playlist_shuffle_named, handle_playlist_sort, handle_playlist_sort_named,
    handle_playlist_switch, handle_playlists,
};
pub use queue::{
    handle_queue_category, handle_queue_category_search, handle_queue_list,
    handle_remove_queue_track,
};
pub use search::handle_search_category;
pub use stream::handle_stream;
