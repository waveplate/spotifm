(() => {
    'use strict';
    const catalog = window.SpotifmDemoCatalog;
    const seed = JSON.parse(document.getElementById('demo-builder-seed').textContent);
    const clone = value => JSON.parse(JSON.stringify(value));
    const draftKey = 'spotifm_demo_builder_draft:v1';
    const element = id => document.getElementById(id);
    const audio = element('preview-audio');
    const status = element('builder-status');
    const recordSelect = element('lyric-record');
    const lineContainer = element('lyric-lines');
    let tracks = clone(seed);
    let selected = -1;
    let generation = 0;
    let lyricRequest = null;
    let searchGeneration = 0;
    let records = [];
    let lines = [];
    let activeLine = -1;
    let exportingLyrics = false;
    try {
        const draft = JSON.parse(window.localStorage.getItem(draftKey));
        if (Array.isArray(draft?.tracks)) {
            if (draft.tracks.length) catalog.validateTracks(draft.tracks);
            tracks = draft.tracks;
        }
    } catch (_) { /* Start with the bundled selection when storage is unavailable or invalid. */ }

    const stringFields = ['title', 'artist', 'album', 'spotifyId', 'previewUrl'];
    const numberFields = ['durationSec', 'appleSongId', 'lrclibId'];
    const listFields = ['artists', 'lyricsTitles'];
    const current = () => tracks[selected];

    function message(text, error = false) {
        status.textContent = text;
        status.classList.toggle('error', error);
    }

    function downloadFile(text, filename, type) {
        const url = URL.createObjectURL(new Blob([text], { type }));
        const link = document.createElement('a');
        link.href = url;
        link.download = filename;
        document.body.appendChild(link);
        link.click();
        link.remove();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    }

    function canonical(value) {
        if (Array.isArray(value)) return value.map(canonical);
        if (value !== null && typeof value === 'object') {
            return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
        }
        return value;
    }

    function saveDraft() {
        element('json-preview').value = JSON.stringify(tracks, null, 2) + '\n';
        try { window.localStorage.setItem(draftKey, JSON.stringify({ tracks })); }
        catch (_) { /* Export remains available without browser storage. */ }
        renderSelection();
    }

    function renderSelection() {
        const list = element('song-list');
        list.replaceChildren();
        element('song-count').textContent = `(${tracks.length})`;
        element('export-json').disabled = !tracks.length;
        element('export-lyrics').disabled = !tracks.length || exportingLyrics;
        tracks.forEach((track, index) => {
            const item = document.createElement('li');
            const button = document.createElement('button');
            button.type = 'button';
            button.setAttribute('aria-current', String(index === selected));
            const title = document.createElement('span');
            title.textContent = track.title || 'Untitled song';
            const metadata = document.createElement('span');
            metadata.className = 'song-meta';
            const offset = Number.isFinite(track.previewStartMs) ? `${track.previewStartMs / 1000}s` : 'Offset not set';
            metadata.textContent = `${track.artist} · ${offset}`;
            button.append(title, metadata);
            button.addEventListener('click', () => selectTrack(index));
            item.appendChild(button);
            list.appendChild(item);
        });
        element('move-up').disabled = selected <= 0;
        element('move-down').disabled = selected < 0 || selected >= tracks.length - 1;
    }

    function fillFields() {
        const track = current();
        if (!track) return;
        element('selected-title').textContent = `${track.title} — ${track.artist}`;
        for (const field of [...stringFields, ...numberFields]) element(`edit-${field}`).value = track[field] ?? '';
        for (const field of listFields) element(`edit-${field}`).value = (track[field] || []).join('\n');
        element('edit-appleSearch').checked = Boolean(track.appleSearch);
        element('edit-previewStartMs').value = Number.isFinite(track.previewStartMs) ? track.previewStartMs / 1000 : '';
    }

    function cancelRequests() {
        generation += 1;
        lyricRequest?.abort();
        lyricRequest = null;
    }

    function selectTrack(index) {
        cancelRequests();
        selected = index;
        audio.pause();
        audio.removeAttribute('src');
        audio.load();
        records = [];
        lines = [];
        activeLine = -1;
        lineContainer.replaceChildren();
        recordSelect.replaceChildren(new Option('No record loaded', ''));
        recordSelect.disabled = true;
        element('lyrics-provider').hidden = true;
        const track = current();
        element('song-editor').hidden = !track;
        element('empty-selection').hidden = Boolean(track);
        renderSelection();
        if (!track) return;
        fillFields();
        element('preview-status').textContent = 'Loading preview…';
        element('lyrics-status').textContent = 'Looking for timed lyrics…';
        void loadPreview(track, generation);
        void loadLyrics(track, generation);
        updateHighlight();
    }

    async function loadPreview(track, requestGeneration) {
        try {
            const playbackTrack = { ...track };
            if (!playbackTrack.previewUrl) {
                const song = await catalog.lookupAppleTrack(playbackTrack);
                if (requestGeneration !== generation || current() !== track) return;
                catalog.applyAppleMetadata(playbackTrack, song);
                // Export stable song IDs; the provider's CDN URL is resolved
                // at playback time. Explicit direct URLs remain in the JSON.
                track.appleSongId = playbackTrack.appleSongId;
                track.durationSec = playbackTrack.durationSec;
                track.album = playbackTrack.album;
                track.coverUrl = playbackTrack.coverUrl;
                delete track.appleSearch;
                const record = records.find(result => result.id === track.lrclibId);
                if (record) catalog.cacheLyrics(track, record);
                fillFields();
                saveDraft();
            }
            if (requestGeneration !== generation || current() !== track) return;
            audio.src = playbackTrack.previewUrl;
            element('preview-status').textContent = 'Play the clip, then align a lyric line or enter its start time.';
        } catch (error) {
            if (requestGeneration === generation && current() === track) {
                element('preview-status').textContent = `${error.message}. You can still choose lyrics or supply another preview URL.`;
            }
        }
    }

    function showRecord(result) {
        const track = current();
        if (!track || !catalog.hasTimedLyrics(result)) return;
        track.lrclibId = Number(result.id);
        element('edit-lrclibId').value = track.lrclibId;
        catalog.cacheLyrics(track, result);
        if (!records.some(record => record.id === result.id)) records.unshift(result);
        renderRecords();
        recordSelect.value = String(result.id);
        lines = catalog.parseLrc(result.syncedLyrics);
        activeLine = -1;
        lineContainer.replaceChildren();
        for (const line of lines) {
            const button = document.createElement('button');
            button.type = 'button';
            button.title = 'Align this lyric with the current preview time';
            const time = document.createElement('time');
            time.textContent = catalog.formatTime(line.time_ms / 1000);
            const words = document.createElement('span');
            words.textContent = line.text || 'Instrumental';
            button.append(time, words);
            button.addEventListener('click', () => {
                track.previewStartMs = Math.round(line.time_ms - audio.currentTime * 1000);
                element('edit-previewStartMs').value = track.previewStartMs / 1000;
                saveDraft();
                updateHighlight();
                message(`Offset saved: ${track.previewStartMs / 1000}s.`);
            });
            lineContainer.appendChild(button);
        }
        element('lyrics-provider').hidden = false;
        element('lyrics-status').textContent = `LRCLIB record ${result.id} pinned for export. Click a line to align it with the audio.`;
        saveDraft();
        updateHighlight();
    }

    function renderRecords() {
        recordSelect.replaceChildren(new Option('Choose a timed lyric record', ''));
        for (const result of records) {
            recordSelect.appendChild(new Option(`${result.trackName} — ${result.artistName} · ${result.albumName || 'No album'} · ${catalog.formatTime(result.duration)} · #${result.id}`, String(result.id)));
        }
        recordSelect.disabled = !records.length;
    }

    async function loadLyrics(track, requestGeneration, search = false) {
        lyricRequest?.abort();
        const request = new AbortController();
        lyricRequest = request;
        const timeout = window.setTimeout(() => request.abort(), 45000);
        element('lyrics-status').textContent = search ? 'Searching timed lyric records…' : 'Loading timed lyrics…';
        try {
            const metadata = { ...track };
            if (search) {
                const found = await catalog.searchLyrics(metadata, request.signal);
                if (requestGeneration !== generation || current() !== track || lyricRequest !== request) return;
                const selectedRecord = records.find(record => record.id === track.lrclibId);
                records = found;
                if (selectedRecord && !records.some(record => record.id === selectedRecord.id)) records.unshift(selectedRecord);
                renderRecords();
                recordSelect.value = track.lrclibId ? String(track.lrclibId) : '';
                element('lyrics-status').textContent = records.length
                    ? `${records.length} timed records found. Choose the recording that matches your clip.`
                    : 'No timed records found. Try another title or artist spelling in the metadata.';
            } else {
                const result = await catalog.getLyrics(metadata, request.signal);
                if (requestGeneration !== generation || current() !== track || lyricRequest !== request) return;
                if (result) showRecord(result);
                else element('lyrics-status').textContent = 'No matching timed lyrics found. Search other records or enter a LRCLIB record ID below.';
            }
        } catch (error) {
            if (requestGeneration === generation && current() === track && lyricRequest === request) {
                element('lyrics-status').textContent = error.name === 'AbortError' ? 'Lyric lookup timed out. Try again.' : error.message;
            }
        } finally {
            window.clearTimeout(timeout);
            if (lyricRequest === request) lyricRequest = null;
        }
    }

    function updateHighlight() {
        const track = current();
        if (!track) return;
        const position = audio.currentTime * 1000 + (track.previewStartMs || 0);
        const absolute = Math.abs(position);
        element('song-clock').textContent = `Song time ${position < 0 ? '−' : ''}${catalog.formatTime(absolute / 1000)}.${Math.floor(absolute % 1000 / 100)}`;
        let next = -1;
        for (let index = 0; index < lines.length && lines[index].time_ms <= position; index++) next = index;
        if (next === activeLine) return;
        lineContainer.children[activeLine]?.classList.remove('active');
        activeLine = next;
        const active = lineContainer.children[activeLine];
        active?.classList.add('active');
        if (active) active.scrollIntoView({ block: 'nearest' });
    }

    function metadataChanged() {
        cancelRequests();
        saveDraft();
        element('selected-title').textContent = `${current().title} — ${current().artist}`;
        message('Draft saved. Reload preview and lyrics to apply the edited metadata.');
    }

    for (const field of stringFields) {
        element(`edit-${field}`).addEventListener('input', event => {
            if (!current()) return;
            const value = event.target.value.trim();
            if (value || ['title', 'artist'].includes(field)) current()[field] = value;
            else delete current()[field];
            metadataChanged();
        });
    }
    for (const field of numberFields) {
        element(`edit-${field}`).addEventListener('input', event => {
            if (!current()) return;
            if (event.target.value === '') delete current()[field];
            else if (Number.isFinite(event.target.valueAsNumber)) current()[field] = event.target.valueAsNumber;
            else return;
            metadataChanged();
        });
    }
    for (const field of listFields) {
        element(`edit-${field}`).addEventListener('input', event => {
            if (!current()) return;
            const values = event.target.value.split(/\r?\n/).map(value => value.trim()).filter(Boolean);
            if (values.length) current()[field] = values;
            else delete current()[field];
            metadataChanged();
        });
    }
    element('edit-appleSearch').addEventListener('change', event => {
        if (!current()) return;
        current().appleSearch = event.target.checked;
        metadataChanged();
    });
    element('edit-previewStartMs').addEventListener('input', event => {
        if (!current()) return;
        if (event.target.value === '') current().previewStartMs = null;
        else if (Number.isFinite(event.target.valueAsNumber)) current().previewStartMs = Math.round(event.target.valueAsNumber * 1000);
        else return;
        saveDraft();
        updateHighlight();
    });

    audio.addEventListener('timeupdate', updateHighlight);
    audio.addEventListener('seeked', updateHighlight);
    audio.addEventListener('error', () => {
        if (audio.error) element('preview-status').textContent = 'The preview could not be played. Choose another recording or supply a working HTTPS preview URL.';
    });
    element('reload-song').addEventListener('click', () => {
        try { catalog.validateTracks([current()]); selectTrack(selected); }
        catch (error) { message(error.message, true); }
    });
    element('search-lyrics').addEventListener('click', () => {
        if (current()?.title && current()?.artist) void loadLyrics(current(), generation, true);
    });
    recordSelect.addEventListener('change', () => {
        const record = records.find(result => String(result.id) === recordSelect.value);
        if (record) {
            lyricRequest?.abort();
            lyricRequest = null;
            showRecord(record);
        }
    });
    element('remove-song').addEventListener('click', () => {
        if (!current()) return;
        tracks.splice(selected, 1);
        selectTrack(tracks.length ? Math.min(selected, tracks.length - 1) : -1);
        saveDraft();
        message('Song removed from the demo selection.');
    });
    function moveSong(direction) {
        const target = selected + direction;
        if (target < 0 || target >= tracks.length || selected < 0) return;
        [tracks[selected], tracks[target]] = [tracks[target], tracks[selected]];
        selected = target;
        saveDraft();
    }
    element('move-up').addEventListener('click', () => moveSong(-1));
    element('move-down').addEventListener('click', () => moveSong(1));
    element('reset-draft').addEventListener('click', () => {
        tracks = clone(seed);
        selectTrack(tracks.length ? 0 : -1);
        saveDraft();
        message('Restored the selection bundled with this builder.');
    });

    function addTrack(track) {
        catalog.validateTracks([track]);
        tracks.push(track);
        selectTrack(tracks.length - 1);
        saveDraft();
        message(`Added ${track.title}.`);
    }
    element('manual-add').addEventListener('submit', event => {
        event.preventDefault();
        const data = new FormData(event.target);
        const track = { title: data.get('title').trim(), artist: data.get('artist').trim(), previewStartMs: null };
        if (data.get('appleSongId')) track.appleSongId = Number(data.get('appleSongId'));
        if (data.get('previewUrl')) track.previewUrl = data.get('previewUrl').trim();
        if (!track.appleSongId && !track.previewUrl) track.appleSearch = true;
        try { addTrack(track); event.target.reset(); }
        catch (error) { message(error.message, true); }
    });
    element('catalog-search').addEventListener('submit', async event => {
        event.preventDefault();
        const query = element('catalog-query').value.trim();
        if (!query) return;
        const searchId = ++searchGeneration;
        element('catalog-status').textContent = 'Searching Apple recordings…';
        element('catalog-results').replaceChildren();
        element('catalog-submit').disabled = true;
        try {
            const results = await catalog.searchAppleSongs(query);
            if (searchId !== searchGeneration) return;
            element('catalog-status').textContent = results.length ? `${results.length} recordings found. Select the version with the preview you want.` : 'No recordings found.';
            for (const song of results) {
                const button = document.createElement('button');
                button.type = 'button';
                button.className = 'result';
                button.disabled = !song.previewUrl;
                const details = document.createElement('span');
                const title = document.createElement('span');
                title.textContent = `${song.trackName} — ${song.artistName}`;
                const album = document.createElement('small');
                album.textContent = `${song.collectionName} · ${catalog.formatTime(song.trackTimeMillis / 1000)}`;
                details.append(title, album);
                const action = document.createElement('small');
                action.textContent = song.previewUrl ? 'Add preview' : 'No preview';
                button.append(details, action);
                button.addEventListener('click', () => {
                    const track = { appleSongId: Number(song.trackId), title: song.trackName, artist: song.artistName,
                        album: song.collectionName, durationSec: Math.round(song.trackTimeMillis / 1000),
                        coverUrl: song.artworkUrl100, previewStartMs: null };
                    const artists = song.artistName.split(/\s+(?:&|feat\.?|ft\.?|and|x)\s+/i);
                    if (artists.length > 1) track.artists = artists;
                    try { addTrack(track); } catch (error) { message(error.message, true); }
                });
                element('catalog-results').appendChild(button);
            }
        } catch (error) {
            if (searchId === searchGeneration) element('catalog-status').textContent = error.message;
        } finally {
            if (searchId === searchGeneration) element('catalog-submit').disabled = false;
        }
    });

    element('import-json').addEventListener('click', () => element('import-file').click());
    element('import-file').addEventListener('change', async event => {
        const file = event.target.files[0];
        if (!file) return;
        try {
            const imported = JSON.parse(await file.text());
            catalog.validateTracks(imported);
            tracks = imported;
            selectTrack(0);
            saveDraft();
            message(`Imported ${tracks.length} demo songs.`);
        } catch (error) { message(`Import failed: ${error.message}`, true); }
        finally { event.target.value = ''; }
    });
    element('export-json').addEventListener('click', () => {
        try {
            if (current()) {
                for (const input of element('song-editor').querySelectorAll('input')) {
                    if (!input.reportValidity()) return;
                }
            }
            catalog.validateTracks(tracks);
            downloadFile(JSON.stringify(tracks, null, 2) + '\n', 'demo-tracks.json', 'application/json');
            message(`Exported ${tracks.length} songs. Use this JSON with player/build.sh --demo-only --demo-tracks <file>.`);
        } catch (error) { message(error.message, true); }
    });

    element('export-lyrics').addEventListener('click', async () => {
        if (exportingLyrics) return;
        const snapshot = clone(tracks);
        const request = new AbortController();
        exportingLyrics = true;
        renderSelection();
        try {
            catalog.validateTracks(snapshot);
            if (!window.crypto?.subtle) throw new Error('Open the builder on localhost or HTTPS to export a lyric bundle.');
            const digest = await window.crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(canonical(snapshot))));
            const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
            const output = [{ type: 'manifest', version: 1, tracks_sha256: hash, track_count: snapshot.length }];
            for (let index = 0; index < snapshot.length; index++) {
                message(`Prefetching lyrics ${index + 1}/${snapshot.length}: ${snapshot[index].title}…`);
                const timeout = window.setTimeout(() => request.abort(), 90000);
                let result;
                try { result = await catalog.getLyrics(snapshot[index], request.signal); }
                finally { window.clearTimeout(timeout); }
                if (!catalog.hasTimedLyrics(result)) throw new Error(`${snapshot[index].title}: choose a timed lyric record before exporting.`);
                const bytes = new TextEncoder().encode(result.syncedLyrics);
                const encoded = btoa(Array.from(bytes, byte => String.fromCharCode(byte)).join(''));
                output.push({ track_index: index, source: 'lrclib', lrclib_id: result.id, lyrics_base64: encoded });
            }
            // Editing while downloads run would give this bundle a different
            // fingerprint from the current manifest. Keep that mismatch visible.
            if (JSON.stringify(canonical(tracks)) !== JSON.stringify(canonical(snapshot))) {
                throw new Error('The selection changed while lyrics were loading. Export the JSON and lyric bundle again. Cached lyrics will be reused.');
            }
            downloadFile(output.map(record => JSON.stringify(record)).join('\n') + '\n', 'demo-lyrics.jsonl', 'application/x-ndjson');
            message(`Exported ${snapshot.length} bundled lyric records. Build with the matching song JSON and --lyrics-bundle <file>.`);
        } catch (error) {
            message(error.name === 'AbortError' ? 'Lyric prefetch timed out. Retry to reuse the songs already cached.' : error.message, true);
        } finally {
            exportingLyrics = false;
            renderSelection();
        }
    });

    selectTrack(tracks.length ? 0 : -1);
    saveDraft();
})();
