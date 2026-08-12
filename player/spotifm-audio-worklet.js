class SpotifmAudioOutputProcessor extends AudioWorkletProcessor {
    constructor() {
        super();
        this.queue = [];
        this.current = null;
        this.currentOffset = 0;
        this.lastPositionSec = 0;
        this.framesUntilReport = 0;
        this.reportEveryFrames = Math.max(128, Math.floor(sampleRate / 30));
        this.volume = 1;

        this.port.onmessage = (event) => {
            const message = event.data || {};

            if (message.type === 'enqueue') {
                const left = new Float32Array(message.left);
                const right = new Float32Array(message.right);
                this.queue.push({
                    kind: 'audio',
                    left,
                    right,
                    frames: Math.min(left.length, right.length),
                    startSec: Number.isFinite(message.startSec) ? message.startSec : 0,
                });
            } else if (message.type === 'track-boundary') {
                this.queue.push({ kind: 'boundary' });
            } else if (message.type === 'reset-now') {
                this.queue = [];
                this.current = null;
                this.currentOffset = 0;
                this.lastPositionSec = Number.isFinite(message.positionSec) ? message.positionSec : 0;
                this.postPosition(true, true);
            } else if (message.type === 'volume') {
                this.volume = Number.isFinite(message.volume)
                    ? Math.max(0, Math.min(1, message.volume))
                    : this.volume;
            }
        };
    }

    nextAudioChunk() {
        while (!this.current && this.queue.length > 0) {
            const next = this.queue.shift();
            if (next.kind === 'boundary') {
                this.lastPositionSec = 0;
                this.postPosition(true, true);
                this.port.postMessage({ type: 'track-boundary' });
                continue;
            }

            if (next.frames > 0) {
                this.current = next;
                this.currentOffset = 0;
            }
        }

        return this.current;
    }

    postPosition(force = false, boundary = false) {
        if (!force && this.framesUntilReport > 0) {
            return;
        }

        let bufferedFrames = 0;
        if (this.current) {
            bufferedFrames += Math.max(0, this.current.frames - this.currentOffset);
        }
        for (const item of this.queue) {
            if (item.kind === 'audio') {
                bufferedFrames += item.frames;
            }
        }

        this.framesUntilReport = this.reportEveryFrames;
        this.port.postMessage({
            type: 'position',
            seconds: this.lastPositionSec,
            bufferedSeconds: bufferedFrames / sampleRate,
            boundary,
        });
    }

    process(_inputs, outputs) {
        const output = outputs[0];
        const leftOut = output[0];
        const rightOut = output[1] || output[0];
        let emittedFrames = 0;

        for (let i = 0; i < leftOut.length; i += 1) {
            const chunk = this.nextAudioChunk();

            if (!chunk) {
                leftOut[i] = 0;
                rightOut[i] = 0;
                continue;
            }

            leftOut[i] = chunk.left[this.currentOffset] * this.volume;
            rightOut[i] = chunk.right[this.currentOffset] * this.volume;
            this.lastPositionSec = chunk.startSec + this.currentOffset / sampleRate;
            this.currentOffset += 1;
            emittedFrames += 1;

            if (this.currentOffset >= chunk.frames) {
                this.current = null;
                this.currentOffset = 0;
            }
        }

        if (emittedFrames > 0) {
            this.framesUntilReport -= emittedFrames;
            this.postPosition(false, false);
        }
        return true;
    }
}

registerProcessor('spotifm-audio-output', SpotifmAudioOutputProcessor);
