# Anonymous Spotify search fallback

Spotifm uses anonymous web-player search by default when `rspotify_client_id`
is unset. If Web API access is enabled, it also uses anonymous search when the Web API returns
`429`, rejects authorization, encounters a temporary server/network failure, or
returns an incompatible response. It supports tracks, albums, artists, and
playlists. The fallback runs directly in Rust; Python is not required by Spotifm.

After a Web API rate limit, catalogue requests use the backup for Spotify's
`Retry-After` interval plus one second. Other qualifying failures select the
backup for 60 seconds. After that interval, the next request tries the Web API
again. A rate limit does not cause an OAuth refresh or a visitor-token refresh.

The anonymous visitor token and cookies live only in memory. Token renewal is
serialized, uses Spotify's server clock, happens before expiry, and is retried
once after a real anonymous-search `401`. Repeated searches share a 60-second
cache. The fallback reports its own HTTP errors and `Retry-After`, without
retrying anonymous `429` responses.

Track metadata returned by anonymous search is cached for playback. Additional
track, album, artist, and playlist lookups use the existing librespot playback
session when the Web API is unavailable. Play-by-query, queueing, and local
playlist operations use the same fallback. Track metadata is cached in memory
for 15 minutes; simultaneous track metadata requests are bounded. Incomplete playlist
responses produce an error rather than silently importing only part of a list.

Successful `/search/...` responses keep their existing JSON array format and
include `X-Spotify-Source: spotify_web_api` or `spotify_anonymous`. Search errors
from the fallback have `source: "spotify_anonymous"`. Logs use
`[Catalogue/Anonymous]`, `[Catalogue/Playback session]`, and `[Catalogue]` prefixes.
Unavailable popularity and preview URLs are returned as `null`.

Startup requires playback authentication. Web API authorization and its token
cache are used only when `rspotify_client_id` is configured. No additional
credentials or developer keys are needed for this fallback.
Anonymous endpoints are undocumented and may change or rate-limit requests.

## Standalone probe

This standalone experiment searches Spotify through the web player's internal
GraphQL API without a Spotify account login, developer app, or supplied token.
It requires Python 3.10 or newer and uses only the standard library.

From the repository root:

```sh
python3 scripts/spotify-anonymous-search.py "Dr. Worm"
python3 scripts/spotify-anonymous-search.py "They Might Be Giants" --type artist
python3 scripts/spotify-anonymous-search.py "Gucci Gucci" --limit 3
```

Supported result types are `track`, `album`, `artist`, and `playlist`. Output is
JSON with Spotify IDs and available public metadata, including artwork and
track duration. Search text is passed directly to the web player's search.

The probe creates a visitor session in memory, reads the current web-player
bundle, obtains an anonymous token using Spotify's server time, and performs
the search. It renews tokens before expiry or retries once with a new visitor
token after a `401`. A `429` reports Spotify's wait interval and stops.
It does not read, modify, or persist account credential files or visitor tokens.

The protocol reference is
[idonthavespotify's Spotify adapter](https://github.com/sjdonado/idonthavespotify/blob/main/src/adapters/spotify.ts).
The search operation's persisted hash is currently pinned in the script;
Spotify can retire it or change the visitor-token bootstrap. The probe remains
an independent diagnostic tool; the running application uses its Rust adapter.

## Live trial

On October 19, 2026, searches from this workspace returned Doctor Worm by
They Might Be Giants (`6OmApaLQPqHZL3iI78FOUR`), Gucci Gucci by Kreayshawn,
the They Might Be Giants artist, the Severe Tire Damage album, and public
Daft Punk playlists. Repeating the visitor-token bootstrap after forcing the
cached token to expire also returned the expected Doctor Worm result.
