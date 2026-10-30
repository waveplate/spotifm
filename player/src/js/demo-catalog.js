// Live LRCLIB lookups belong to the authoring tool, not the public demo.
(() => {
    'use strict';
    const { normalized, matchesArtist, parseLrc, hasTimedLyrics } = window.SpotifmDemoMedia;

    async function lrclibRequest(path, signal) {
        for (let attempt = 0; attempt < 5; attempt++) {
            const response = await fetch(`https://lrclib.net/api/${path}`, { signal, credentials: 'omit' });
            if (response.status === 404) return null;
            if (response.ok) return response.json();
            if (attempt === 4 || ![429, 500, 502, 503, 504].includes(response.status)) {
                throw new Error(`LRCLIB returned HTTP ${response.status}`);
            }
            const retry = Number(response.headers.get('Retry-After'));
            const delay = Number.isFinite(retry) && retry > 0 ? Math.min(30, retry) : 2 ** (attempt + 1);
            await new Promise((resolve, reject) => {
                const abort = () => { window.clearTimeout(timer); reject(new DOMException('Request cancelled', 'AbortError')); };
                const timer = window.setTimeout(() => { signal?.removeEventListener('abort', abort); resolve(); }, delay * 1000);
                if (signal?.aborted) abort();
                else signal?.addEventListener('abort', abort, { once: true });
            });
        }
    }

    function rankedLyrics(candidates, track) {
        const titles = [track.title, ...(track.lyricsTitles || [])].map(normalized);
        return candidates.filter(result => hasTimedLyrics(result) && titles.includes(normalized(result.trackName)) &&
            matchesArtist(result.artistName, track)).sort((a, b) => {
                // Duration ranks recordings rather than silently discarding
                // lyrics from a differently trimmed release of the same song.
                const delta = result => track.durationSec && Number.isFinite(result.duration)
                    ? Math.abs(result.duration - track.durationSec) : 0;
                return delta(a) - delta(b) || Number(normalized(b.albumName) === normalized(track.album)) -
                    Number(normalized(a.albumName) === normalized(track.album));
            });
    }

    async function searchLyrics(track, signal) {
        const titles = [...new Set([track.title, ...(track.lyricsTitles || [])])];
        const found = new Map();
        const add = results => {
            for (const result of Array.isArray(results) ? results : []) if (hasTimedLyrics(result)) found.set(result.id, result);
        };
        // Try the provider spelling and a spelling without dollar-sign aliases.
        const artists = [...new Set([track.artist, track.artist.replace(/\$/g, 's')])];
        for (const artist of artists) {
            add(await lrclibRequest(`search?${new URLSearchParams({ track_name: track.title, artist_name: artist })}`, signal));
        }
        // Title-only search catches differing collaborator separators/names.
        // The automatic picker still checks title and performer identity.
        for (const title of titles) {
            add(await lrclibRequest(`search?${new URLSearchParams({ track_name: title })}`, signal));
        }
        const candidates = [...found.values()];
        const ranked = rankedLyrics(candidates, track);
        return [...ranked, ...candidates.filter(result => !ranked.includes(result))];
    }

    const lyricsMemoryCache = new Map();
    function lyricsCacheKey(track) {
        return JSON.stringify([track.appleSongId || track.spotifyId || track.previewUrl,
            track.title, track.artist, track.artists || [], track.album || '', track.durationSec || null,
            track.lrclibId || null, track.lyricsTitles || []]);
    }

    function readLyricsCache(key) {
        if (lyricsMemoryCache.has(key)) return lyricsMemoryCache.get(key);
        try {
            const entry = JSON.parse(window.localStorage.getItem(`spotifm_demo_lrclib:${key}`));
            if (entry?.version === 2 && hasTimedLyrics(entry.lyrics)) {
                lyricsMemoryCache.set(key, entry.lyrics);
                return entry.lyrics;
            }
        } catch (_) { /* Storage is optional. */ }
        return null;
    }

    function cacheLyrics(track, lyrics) {
        if (!hasTimedLyrics(lyrics)) return;
        const key = lyricsCacheKey(track);
        lyricsMemoryCache.set(key, lyrics);
        try {
            window.localStorage.setItem(`spotifm_demo_lrclib:${key}`, JSON.stringify({ version: 2, lyrics }));
        } catch (_) { /* Full or blocked storage does not prevent playback. */ }
    }

    async function getLyrics(track, signal, refresh = false) {
        const cached = !refresh && readLyricsCache(lyricsCacheKey(track));
        if (cached) return cached;
        let result;
        if (track.lrclibId) {
            result = await lrclibRequest(`get/${track.lrclibId}`, signal);
            if (!hasTimedLyrics(result)) return null;
        } else {
            const params = new URLSearchParams({ track_name: track.title, artist_name: track.artist });
            if (track.album) params.set('album_name', track.album);
            if (track.durationSec) params.set('duration', String(track.durationSec));
            const exact = track.album && track.durationSec ? await lrclibRequest(`get?${params}`, signal) : null;
            result = rankedLyrics(exact ? [exact] : [], track)[0];
            if (!result) result = rankedLyrics(await searchLyrics(track, signal), track)[0];
        }
        if (result) cacheLyrics(track, result);
        return result || null;
    }

    function validateTracks(tracks) {
        if (!Array.isArray(tracks) || !tracks.length) throw new Error('Choose at least one demo song.');
        for (const track of tracks) {
            if (!track || typeof track.title !== 'string' || !track.title.trim() || typeof track.artist !== 'string' || !track.artist.trim()) {
                throw new Error('Every song needs a title and artist.');
            }
            if (track.spotifyId !== undefined && !/^[A-Za-z0-9]{22}$/.test(track.spotifyId)) throw new Error(`${track.title}: invalid Spotify ID.`);
            if (track.appleSongId !== undefined && (!Number.isSafeInteger(track.appleSongId) || track.appleSongId <= 0)) throw new Error(`${track.title}: invalid Apple song ID.`);
            if (track.appleSearch !== undefined && typeof track.appleSearch !== 'boolean') throw new Error(`${track.title}: appleSearch must be a boolean.`);
            if (track.previewUrl && (typeof track.previewUrl !== 'string' || new URL(track.previewUrl).protocol !== 'https:')) throw new Error(`${track.title}: use an HTTPS preview URL.`);
            if (!track.previewUrl && !track.appleSongId && !track.appleSearch) throw new Error(`${track.title}: choose an Apple song or supply a preview URL.`);
            if (track.previewStartMs != null && !Number.isFinite(track.previewStartMs)) throw new Error(`${track.title}: offset must be a finite number or null.`);
            if (track.durationSec !== undefined && (!Number.isFinite(track.durationSec) || track.durationSec <= 0)) throw new Error(`${track.title}: duration must be positive.`);
            if (track.lrclibId !== undefined && (!Number.isSafeInteger(track.lrclibId) || track.lrclibId <= 0)) throw new Error(`${track.title}: invalid LRCLIB ID.`);
            for (const field of ['artists', 'lyricsTitles']) {
                if (track[field] !== undefined && (!Array.isArray(track[field]) || track[field].some(value => typeof value !== 'string' || !value.trim()))) {
                    throw new Error(`${track.title}: ${field} must be a list of names.`);
                }
            }
        }
        return tracks;
    }

    window.SpotifmDemoCatalog = { ...window.SpotifmDemoMedia,
        searchLyrics, getLyrics, cacheLyrics, validateTracks };
})();
