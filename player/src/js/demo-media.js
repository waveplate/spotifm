// Preview audio and LRC parsing shared by the demo and song builder.
(() => {
    'use strict';

    function normalized(value) {
        return String(value || '').normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase()
            .replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
    }

    function artistKey(value) {
        // $uicideboy$ and Suicideboys identify the same performer. Providers
        // also vary collaborator order and use &, feat., or x as separators.
        return normalized(String(value || '').replace(/\$/g, 's'))
            .split(' ').filter(word => word && !['and', 'feat', 'featuring', 'ft', 'with', 'x'].includes(word))
            .sort().join(' ');
    }

    function matchesArtist(value, track) {
        const artists = [track.artist, ...(track.artists || [])].filter(Boolean);
        const keys = artists.map(artistKey);
        keys.push(artistKey((track.artists || [track.artist]).join(' ')));
        return keys.includes(artistKey(value));
    }

    function parseLrc(lrc) {
        const offset = Number(/\[offset:([+-]?\d+)\]/i.exec(lrc)?.[1] || 0);
        const lines = [];
        for (const row of lrc.split(/\r?\n/)) {
            const timestamps = [...row.matchAll(/\[(\d+):(\d{2})(?:\.(\d{1,3}))?\]/g)];
            const text = row.replace(/\[[^\]]*\]/g, '').trim();
            for (const match of timestamps) {
                if (Number(match[2]) >= 60) continue;
                const fraction = Number((match[3] || '').padEnd(3, '0'));
                lines.push({ time_ms: Math.max(0, Number(match[1]) * 60000 + Number(match[2]) * 1000 + fraction - offset), text });
            }
        }
        return lines.sort((a, b) => a.time_ms - b.time_ms);
    }

    function hasTimedLyrics(result) {
        return typeof result?.syncedLyrics === 'string' && parseLrc(result.syncedLyrics).some(line => line.text);
    }

    function formatTime(seconds) {
        const value = Math.max(0, Math.floor(seconds || 0));
        return `${Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`;
    }

    let appleCallbackIndex = 0;
    const appleTrackCache = new Map();
    function appleRequest(endpoint, parameters) {
        return new Promise((resolve, reject) => {
            const callback = `spotifmApplePreview${++appleCallbackIndex}`;
            const script = document.createElement('script');
            let timeout;
            const cleanup = (lateResponse = false) => {
                window.clearTimeout(timeout);
                script.remove();
                if (lateResponse) {
                    window[callback] = () => {};
                    window.setTimeout(() => { delete window[callback]; }, 30000);
                } else delete window[callback];
            };
            window[callback] = data => {
                cleanup();
                resolve(Array.isArray(data?.results) ? data.results : []);
            };
            script.onerror = () => { cleanup(true); reject(new Error('Could not load Apple preview metadata')); };
            timeout = window.setTimeout(() => { cleanup(true); reject(new Error('Apple preview request timed out')); }, 15000);
            script.src = `https://itunes.apple.com/${endpoint}?${new URLSearchParams({ ...parameters, country: 'us', callback })}`;
            script.async = true;
            document.head.appendChild(script);
        });
    }

    async function searchAppleSongs(query) {
        const results = await appleRequest('search', { term: query, entity: 'song', media: 'music', limit: '50' });
        return results.filter(result => result.kind === 'song');
    }

    function lookupAppleTrack(track) {
        const id = track.appleSongId;
        const key = id || JSON.stringify([track.title, track.artist, track.artists, track.album, track.durationSec]);
        if (appleTrackCache.has(key)) return appleTrackCache.get(key);
        const lookup = (async () => {
            const results = id ? await appleRequest('lookup', { id: String(id) })
                : await searchAppleSongs(`${track.title} ${(track.artists || [track.artist]).join(' ')}`);
            const matches = results.filter(result => result.kind === 'song' && (id ? Number(result.trackId) === id
                : normalized(result.trackName) === normalized(track.title) && matchesArtist(result.artistName, track) &&
                    (!track.durationSec || Math.abs(result.trackTimeMillis / 1000 - track.durationSec) <= 2)));
            const song = matches.filter(result => result.previewUrl).sort((a, b) =>
                Number(normalized(b.collectionName) === normalized(track.album)) -
                Number(normalized(a.collectionName) === normalized(track.album)))[0];
            if (!song?.previewUrl || new URL(song.previewUrl).protocol !== 'https:') {
                throw new Error('No public preview is available for this recording');
            }
            return song;
        })();
        appleTrackCache.set(key, lookup);
        void lookup.catch(() => { appleTrackCache.delete(key); });
        return lookup;
    }

    function applyAppleMetadata(track, song) {
        track.appleSongId = Number(song.trackId);
        track.previewUrl = song.previewUrl;
        if (Number.isFinite(song.trackTimeMillis) && song.trackTimeMillis > 0) track.durationSec = Math.round(song.trackTimeMillis / 1000);
        if (song.collectionName) track.album = song.collectionName;
        if (song.artworkUrl100) track.coverUrl = song.artworkUrl100;
    }

    window.SpotifmDemoMedia = { normalized, matchesArtist, parseLrc, formatTime, hasTimedLyrics,
        searchAppleSongs, lookupAppleTrack, applyAppleMetadata };
})();
