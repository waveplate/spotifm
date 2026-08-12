self.addEventListener('install', (event) => {
    event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', (event) => {
    event.waitUntil(self.clients.claim());
});

const GRANULE_CHANNEL_NAME = 'spotifm-audio-granules';

function concatBytes(a, b) {
    const out = new Uint8Array(a.length + b.length);
    out.set(a, 0);
    out.set(b, a.length);
    return out;
}

function readGranulePosition(page, offset) {
    let value = 0n;
    for (let i = 0; i < 8; i += 1) {
        value |= BigInt(page[offset + 6 + i]) << BigInt(i * 8);
    }
    return value;
}

async function parseOggGranules(stream, sid, granuleRate) {
    const channel = new BroadcastChannel(GRANULE_CHANNEL_NAME);
    const reader = stream.getReader();
    let buffer = new Uint8Array(0);

    try {
        while (true) {
            const { value, done } = await reader.read();
            if (done) {
                break;
            }
            if (!value || value.length === 0) {
                continue;
            }

            buffer = concatBytes(buffer, value);
            let offset = 0;

            while (offset + 27 <= buffer.length) {
                if (
                    buffer[offset] !== 0x4f ||
                    buffer[offset + 1] !== 0x67 ||
                    buffer[offset + 2] !== 0x67 ||
                    buffer[offset + 3] !== 0x53
                ) {
                    offset += 1;
                    continue;
                }

                const segmentCount = buffer[offset + 26];
                if (offset + 27 + segmentCount > buffer.length) {
                    break;
                }

                let bodySize = 0;
                for (let i = 0; i < segmentCount; i += 1) {
                    bodySize += buffer[offset + 27 + i];
                }

                const pageSize = 27 + segmentCount + bodySize;
                if (offset + pageSize > buffer.length) {
                    break;
                }

                const granulePosition = readGranulePosition(buffer, offset);
                if (granulePosition !== 0xffffffffffffffffn) {
                    channel.postMessage({
                        type: 'granule',
                        sid,
                        granuleSec: Number(granulePosition) / granuleRate,
                    });
                }

                offset += pageSize;
            }

            if (offset > 0) {
                buffer = buffer.slice(offset);
            }
        }
    } catch (error) {
        channel.postMessage({
            type: 'error',
            sid,
            message: error && error.message ? error.message : String(error),
        });
    } finally {
        channel.close();
    }
}

self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    if (url.origin !== self.location.origin || url.searchParams.get('spotifm_tap') !== '1') {
        return;
    }

    event.respondWith((async () => {
        const response = await fetch(event.request);
        const contentType = response.headers.get('content-type') || '';
        if (!response.body || !contentType.includes('ogg')) {
            return response;
        }

        const advertisedGranuleRate = Number(
            response.headers.get('x-spotifm-ogg-granule-rate')
        );
        const granuleRate = Number.isFinite(advertisedGranuleRate) && advertisedGranuleRate > 0
            ? advertisedGranuleRate
            : 48000.0;
        const sid = url.searchParams.get('sid') || '';
        const [audioBody, parseBody] = response.body.tee();
        parseOggGranules(parseBody, sid, granuleRate);

        return new Response(audioBody, {
            status: response.status,
            statusText: response.statusText,
            headers: response.headers,
        });
    })());
});
