
        // ==========================================
        // Configuration Parameters (Hardcoded at top of script)
        // ==========================================
        const runtimeConfigElement = document.getElementById('player-runtime-config');
        const runtimeConfig = runtimeConfigElement
            ? JSON.parse(runtimeConfigElement.textContent || '{}')
            : {};
        const IS_SINGLE_FILE_BUILD_SOURCE = window.location.protocol === 'file:';
        const LYRICS_WS_STALL_MS = 6000;
        const LYRICS_WS_RECOVERY_COOLDOWN_MS = 5000;
        const NOW_PLAYING_REFRESH_MS = 1500;
        const NOW_PLAYING_HTTP_REFRESH_MS = 3000;
        const LYRICS_SWAP_SCROLL_FREEZE_MS = 1200;
        const DEFAULT_AUTO_SYNC_LYRICS_OFFSET_SEC = 0;
        const AUDIO_STALL_TIMEOUT_MS = 10000;
        const AUDIO_RECONNECT_BASE_DELAY_MS = 1000;
        const AUDIO_RECONNECT_MAX_DELAY_MS = 10000;
        const SPOTIFM_PLAYER_VERSION = 57;

        // Check if saved localStorage version matches/exceeds current version, otherwise clear it.
        (function checkLocalStorageVersion() {
            try {
                const savedVersionStr = localStorage.getItem('spotifm_player_version');
                const savedVersion = savedVersionStr ? parseInt(savedVersionStr, 10) : null;

                if (savedVersion === null || isNaN(savedVersion) || savedVersion < SPOTIFM_PLAYER_VERSION) {
                    console.log(`Local storage version mismatch (saved: ${savedVersion}, current: ${SPOTIFM_PLAYER_VERSION}). Clearing local storage.`);
                    
                    let customFontsBackup = null;
                    try {
                        const savedCustom = localStorage.getItem('spotifm_custom_fonts');
                        if (savedCustom) {
                            customFontsBackup = JSON.parse(savedCustom);
                        } else {
                            const savedSettingsStr = localStorage.getItem('spotifm_player_settings');
                            if (savedSettingsStr) {
                                const parsed = JSON.parse(savedSettingsStr);
                                if (parsed && Array.isArray(parsed.customFonts)) {
                                    customFontsBackup = parsed.customFonts;
                                }
                            }
                        }
                    } catch (err) {
                        console.error('Failed to backup custom fonts on localStorage version mismatch:', err);
                    }

                    localStorage.clear();
                    localStorage.setItem('spotifm_player_version', String(SPOTIFM_PLAYER_VERSION));

                    if (Array.isArray(customFontsBackup)) {
                        localStorage.setItem('spotifm_custom_fonts', JSON.stringify(customFontsBackup));
                    }
                }
            } catch (e) {
                console.error('Failed to access or clear localStorage:', e);
            }
        })();

        const PROJECT_DEFAULT_SETTINGS =
            runtimeConfig.projectDefaults && typeof runtimeConfig.projectDefaults === 'object'
                ? runtimeConfig.projectDefaults
                : {};
        const wasmDecoderNamespace = window["ogg-vorbis-decoder"];
        const wasmPlaybackRequired = Boolean(
            !IS_SINGLE_FILE_BUILD_SOURCE &&
            runtimeConfig.streamIsPassthrough === true
        );
        const wasmPlaybackEnabled = Boolean(
            wasmPlaybackRequired &&
            runtimeConfig.streamIsOgg === true &&
            wasmDecoderNamespace &&
            (wasmDecoderNamespace.OggVorbisDecoderWebWorker || wasmDecoderNamespace.OggVorbisDecoder) &&
            window.AudioWorkletNode &&
            (window.AudioContext || window.webkitAudioContext)
        );

        const audio = document.getElementById('audio-stream');
        const listenEndpoint =
            audio.dataset.listenEndpoint || runtimeConfig.listenEndpoint || '/listen';
        const playerServiceWorkerEndpoint =
            runtimeConfig.playerServiceWorkerEndpoint || '/spotifm-player-sw.js';
        let streamSessionId = generateStreamSessionId();
        const playBtn = document.getElementById('play-button');
        const playIcon = document.getElementById('overlay-play-icon');
        const pauseIcon = document.getElementById('overlay-pause-icon');
        const loadingIcon = document.getElementById('overlay-loading-icon');
        const idlePlayPrompt = document.getElementById('idle-play-prompt');
        const albumCover = document.getElementById('album-cover');
        const albumPlaceholder = document.getElementById('album-placeholder');
        const volumeFlyout = document.getElementById('volume-flyout');
        const volumeToggleBtn = document.getElementById('volume-toggle-btn');
        const volumeControl = document.getElementById('volume-control');
        const volumeWaveSmall = document.getElementById('volume-wave-small');
        const volumeWaveLarge = document.getElementById('volume-wave-large');
        const volumeMutedLine = document.getElementById('volume-muted-line');

        const songTitle = document.getElementById('song-title');
        const songArtists = document.getElementById('song-artists');
        const syncStatus = document.getElementById('sync-status');
        const listenerCount = document.getElementById('listener-count');
        const trackProgressFill = document.getElementById('track-progress-fill');
        const trackInfo = document.querySelector('.track-info');

        const syncStatusBadge = document.getElementById('sync-status-badge');
        
        function updateSyncStatusIcon() {
            const svgLyrics = document.getElementById('sync-svg-lyrics');
            const svgDisabled = document.getElementById('sync-svg-disabled');
            const svgConnecting = document.getElementById('sync-svg-connecting');
            if (!svgLyrics || !svgDisabled || !svgConnecting || !syncStatus || !syncStatusBadge) return;

            if (!hasMusicBeenPlayed || !isAutosync) {
                syncStatusBadge.style.display = 'none';
            } else {
                syncStatusBadge.style.display = '';
                syncStatus.style.display = '';
            }

            const text = syncStatus.textContent;
            const isConnecting = (text === 'Connecting' || text === 'Syncing' || text === 'Resyncing');
            const noLyrics = (currentLyricsStateType === 'NoLyrics');

            if (isConnecting) {
                svgLyrics.style.display = 'none';
                svgDisabled.style.display = 'none';
                svgConnecting.style.display = 'block';
            } else if (noLyrics) {
                svgLyrics.style.display = 'none';
                svgDisabled.style.display = 'block';
                svgConnecting.style.display = 'none';
            } else {
                svgLyrics.style.display = 'block';
                svgDisabled.style.display = 'none';
                svgConnecting.style.display = 'none';
            }
        }

        const syncObserver = new MutationObserver(() => {
            if (!syncStatus) return;
            const text = syncStatus.textContent;
            updateSyncStatusIcon();
            if (syncStatusBadge) {
                syncStatusBadge.setAttribute('title', text);
                if (text === 'Lyrics Synced' || text === 'Manual Delay') {
                    syncStatusBadge.style.color = ''; // Green
                } else {
                    syncStatusBadge.style.color = '#f59e0b'; // Syncing / Orange
                }
            }
        });
        if (syncStatus) {
            syncObserver.observe(syncStatus, { childList: true, characterData: true, subtree: true });
        }

        const scrollPanel = document.getElementById('scroll-panel');
        const lyricsBody = document.getElementById('lyrics-body');

        let currentTrackId = null;
        let currentTrackDurationMs = null;
        let currentTrackPositionMs = 0;
        let lastTitleTooltipClientX = 0;
        let lastTitleTooltipClientY = 0;
        const titleContainer = document.querySelector('.title-container');
        const titleTooltip = document.getElementById('title-tooltip');

        if (titleTooltip && titleTooltip.parentElement !== document.body) {
            document.body.appendChild(titleTooltip);
        }

        let isPlaying = false;
        let playState = 'paused';
        let playbackRequested = false;
        let mutedAutoplayPriming = false;
        let playbackGestureRequired = false;
        let audioContextActivationPending = false;
        let audioReconnectTimer = null;
        let audioReconnectAttempts = 0;
        let audioReconnectInProgress = false;
        let audioLastProgressAt = performance.now();
        let audioLastCurrentTime = 0;
        let audioRecoveryLastCheckedAt = 0;
        let wasmLastChunkAt = 0;
        let activePlaylistHasTracks = null;
        let emptyPlaylistSearchOpened = false;
        let isVolumeMuted = false;
        let lastAudibleVolume = 1.0;
        let syncedLines = [];
        let activeLineIndex = -1;
        let currentLyricsTrackId = null;
        let currentLyricsStateType = null;
        let displayedLyricsHoldStartMs = null;
        let displayedLyricsHoldAudioTimeSec = 0;
        let currentPlaybackTrackId = null;
        let confirmedPlaybackTrackId = null;
        let pendingLyricsState = null;
        let lyricsScrollFreezeUntilMs = 0;
        let lyricsScrollRestoreTimer = null;
        let awaitingPlaybackTrackConfirmation = false;
        let lastPositionTrackMs = null;
        let lastServerTime = 0;
        let lastSyncInstant = 0;
        let streamFirstGranuleSec = null;
        let currentTrackStartGranuleSec = null;
        let localGranuleClockMode = null;
        let serviceWorkerSyncEnabled = false;
        let serviceWorkerSyncReady = false;
        let wasmAudioNode = null;
        let wasmAudioWorkletReady = false;
        let wasmDecoder = null;
        let wasmDecoderReady = false;
        let wasmFetchAbortController = null;
        let wasmFetchPromise = null;
        let wasmStreamGeneration = 0;
        let wasmOggParserBuffer = new Uint8Array(0);
        let wasmSeenBos = false;
        let wasmClockReady = false;
        let wasmPlaybackPositionSec = 0;
        let wasmBufferedSeconds = 0;
        let wasmLastTrackBoundaryAt = 0;
        let lyricsVisible = true;
        let lyricTextScale = 1.25;
        let lyricFadeCurve = 5.0;
        let lyricFont = 'vcrOsdMono';
        let playerFont = 'vcrOsdMono';
        let customFonts = [];
        let autoSyncLyricsOffsetSec = DEFAULT_AUTO_SYNC_LYRICS_OFFSET_SEC;
        let playerOpacity = 0.35;
        let playerBorderOpacity = 1.0;
        let playerBlur = 5;
        let playerScale = 0.9;
        let playerModalWidth = 1120;
        let playerEdgeGap = 20;
        let playerTitleColor = '#ffffff';
        let playerArtistColor = '#9ca3af';
        let playerTitleFontSize = 24;
        let playerArtistFontSize = 20;
        let playerTextGap = -1;
        let playerApplyEffects = false;
        let playerTextHighlight = false;
        let playerTextBlendMode = 'normal';
        let playerTextLeft = 20;
        let playerTextBottom = 20;
        let liquidGlassEnabled = true;
        let playerAlign = 'center';
        let showTrackInfo = true;
        let showAlbumArt = true;
        let showProgressBar = true;
        let showSyncStatus = true;
        let showListenerNumber = true;
        let playerMinimized = false;
        let milkdropCycleOnSongChange = false;
        let albumArtSize = 92;
        let milkdropFrameLimit = 0;
        let milkdropCanvasSize = 'native';
        let milkdropMeshSize = '32x24';
        let milkdropLastFrameTime = 0;

        let isAutosync = true;
        let manualDelaySec = 2.0;
        let hasMusicBeenPlayed = false;
        let lastTrackData = null;

        let apiKeyEnabled = false;
        let apiKey = '';

        // UI Settings Elements
        const apikeyToggle = document.getElementById('apikey-toggle');
        const apikeyInput = document.getElementById('apikey-input');
        const apikeySettingsGroup = document.getElementById('apikey-settings-group');

        const autosyncToggle = document.getElementById('autosync-toggle');
        const delaySlider = document.getElementById('delay-slider');
        const delayVal = document.getElementById('delay-val');
        const delayControlGroup = document.getElementById('delay-control-group');
        const autoSyncOffsetSlider = document.getElementById('autosync-offset-slider');
        const autoSyncOffsetVal = document.getElementById('autosync-offset-val');
        const autoSyncOffsetControlGroup = document.getElementById('autosync-offset-control-group');
        const toggleSettingsBtn = document.getElementById('toggle-settings-btn');
        const settingsContent = document.getElementById('settings-content');
        const settingsArrow = document.getElementById('settings-arrow');
        const settingsActionRail = document.getElementById('settings-action-rail');
        const settingsSectionButtons = document.querySelectorAll('.settings-section-button');
        const settingsSections = document.querySelectorAll('.settings-section');
        const showLyricsToggle = document.getElementById('show-lyrics-toggle');
        const lyricOptionsGroup = document.getElementById('lyric-options-group');
        const lyricsSyncModeButtons = document.querySelectorAll('.lyric-sync-mode-button');
        const lyricsSizeSlider = document.getElementById('lyrics-size-slider');
        const lyricsSizeVal = document.getElementById('lyrics-size-val');
        const lyricsFadeSlider = document.getElementById('lyrics-fade-slider');
        const lyricsFadeVal = document.getElementById('lyrics-fade-val');
        const lyricsFontSelect = document.getElementById('lyrics-font-select');
        const lyricsFontCustomInput = document.getElementById('lyrics-font-custom-input');
        const playerFontSelect = document.getElementById('player-font-select');
        const playerFontCustomInput = document.getElementById('player-font-custom-input');
        const container = document.querySelector('.container');
        const playerPanel = document.querySelector('.player-panel');
        const settingsDrawer = document.getElementById('settings-panel');
        const visualizerToggle = document.getElementById('visualizer-toggle');
        const milkdropPresetList = document.getElementById('milkdrop-preset-list');
        const milkdropRandomBtn = document.getElementById('milkdrop-random-btn');
        const milkdropSelectAllBtn = document.getElementById('milkdrop-select-all-btn');
        const milkdropSelectNoneBtn = document.getElementById('milkdrop-select-none-btn');
        const milkdropCycleSongToggle = document.getElementById('milkdrop-cycle-song-toggle');
        const milkdropCycleTimeGroup = document.getElementById('milkdrop-cycle-time-group');
        const milkdropCycleSlider = document.getElementById('milkdrop-cycle-slider');
        const milkdropCycleVal = document.getElementById('milkdrop-cycle-val');
        const gammaFuncR = document.getElementById('gamma-func-r');
        const gammaFuncG = document.getElementById('gamma-func-g');
        const gammaFuncB = document.getElementById('gamma-func-b');
        const milkdropBlendSlider = document.getElementById('milkdrop-blend-slider');
        const milkdropBlendVal = document.getElementById('milkdrop-blend-val');
        const vizOpacitySlider = document.getElementById('viz-opacity-slider');
        const vizOpacityVal = document.getElementById('viz-opacity-val');
        const milkdropStatus = document.getElementById('milkdrop-status');
        const playerOpacitySlider = document.getElementById('player-opacity-slider');
        const playerOpacityVal = document.getElementById('player-opacity-val');
        const playerBorderSlider = document.getElementById('player-border-slider');
        const playerBorderVal = document.getElementById('player-border-val');
        const playerBlurSlider = document.getElementById('player-blur-slider');
        const playerBlurVal = document.getElementById('player-blur-val');
        const playerScaleSlider = document.getElementById('player-scale-slider');
        const playerScaleVal = document.getElementById('player-scale-val');
        const playerModalWidthSlider = document.getElementById('player-modal-width-slider');
        const playerModalWidthVal = document.getElementById('player-modal-width-val');
        const playerEdgeGapSlider = document.getElementById('player-edge-gap-slider');
        const playerEdgeGapVal = document.getElementById('player-edge-gap-val');
        const playerTitleColorPicker = document.getElementById('player-title-color-picker');
        const playerTitleColorVal = document.getElementById('player-title-color-val');
        const playerArtistColorPicker = document.getElementById('player-artist-color-picker');
        const playerArtistColorVal = document.getElementById('player-artist-color-val');
        const playerTitleFontSizeSlider = document.getElementById('player-title-font-size-slider');
        const playerTitleFontSizeVal = document.getElementById('player-title-font-size-val');
        const playerArtistFontSizeSlider = document.getElementById('player-artist-font-size-slider');
        const playerArtistFontSizeVal = document.getElementById('player-artist-font-size-val');
        const playerTextGapSlider = document.getElementById('player-text-gap-slider');
        const playerTextGapVal = document.getElementById('player-text-gap-val');
        const playerApplyEffectsToggle = document.getElementById('player-apply-effects-toggle');
        const playerTextHighlightToggle = document.getElementById('player-text-highlight-toggle');
        const playerTextHighlightRow = document.getElementById('player-text-highlight-row');
        const playerTextBlendSelect = document.getElementById('player-text-blend-select');
        const playerTextBlendField = document.getElementById('player-text-blend-field');
        const playerAlignField = document.getElementById('player-align-field');
        const playerModalWidthField = document.getElementById('player-modal-width-field');
        const showTrackInfoRow = document.getElementById('show-track-info-row');
        const showAlbumArtRow = document.getElementById('show-album-art-row');
        const showProgressBarRow = document.getElementById('show-progress-bar-row');
        const showSyncStatusRow = document.getElementById('show-sync-status-row');
        const showListenerNumberRow = document.getElementById('show-listener-number-row');
        const playerTextLeftSlider = document.getElementById('player-text-left-slider');
        const playerTextLeftVal = document.getElementById('player-text-left-val');
        const playerTextBottomSlider = document.getElementById('player-text-bottom-slider');
        const playerTextBottomVal = document.getElementById('player-text-bottom-val');
        const liquidGlassToggle = document.getElementById('liquid-glass-toggle');
        const liquidGlassRow = document.getElementById('liquid-glass-row');
        const liquidGlassSettingsGroup = document.getElementById('liquid-glass-settings-group');
        const playerAlignButtons = document.querySelectorAll('.player-align-button');
        const showTrackInfoToggle = document.getElementById('show-track-info-toggle');
        const showAlbumArtToggle = document.getElementById('show-album-art-toggle');
        const showProgressBarToggle = document.getElementById('show-progress-bar-toggle');
        const showSyncStatusToggle = document.getElementById('show-sync-status-toggle');
        const showListenerNumberToggle = document.getElementById('show-listener-number-toggle');
        const minimizePlayerBtn = document.getElementById('minimize-player-btn');
        const restorePlayerBtn = document.getElementById('restore-player-btn');
        const lyricsWrapSlider = document.getElementById('lyrics-wrap-slider');
        const lyricsWrapVal = document.getElementById('lyrics-wrap-val');
        const lyricsDynamicStyle = document.getElementById('lyrics-dynamic-style');
        const lyricsBorderSlider = document.getElementById('lyrics-border-slider');
        const lyricsBorderVal = document.getElementById('lyrics-border-val');
        const lyricsWeightSlider = document.getElementById('lyrics-weight-slider');
        const lyricsWeightVal = document.getElementById('lyrics-weight-val');
        const lyricsLineHeightSlider = document.getElementById('lyrics-line-height-slider');
        const lyricsLineHeightVal = document.getElementById('lyrics-line-height-val');
        const lyricsWordSpacingSlider = document.getElementById('lyrics-word-spacing-slider');
        const lyricsWordSpacingVal = document.getElementById('lyrics-word-spacing-val');
        const lyricsLetterSpacingSlider = document.getElementById('lyrics-letter-spacing-slider');
        const lyricsLetterSpacingVal = document.getElementById('lyrics-letter-spacing-val');

        const ntscSettingsGroup = document.getElementById('ntsc-settings-group');
        const ntscAmountSlider = document.getElementById('ntsc-amount-slider');
        const ntscAmountVal = document.getElementById('ntsc-amount-val');
        const ntscSmearSlider = document.getElementById('ntsc-smear-slider');
        const ntscSmearVal = document.getElementById('ntsc-smear-val');
        const ntscWiggleSlider = document.getElementById('ntsc-wiggle-slider');
        const ntscWiggleVal = document.getElementById('ntsc-wiggle-val');
        const ntscWiggleSpeedSlider = document.getElementById('ntsc-wiggle-speed-slider');
        const ntscWiggleSpeedVal = document.getElementById('ntsc-wiggle-speed-val');
        const ntscChromaShiftSlider = document.getElementById('ntsc-chroma-shift-slider');
        const ntscChromaShiftVal = document.getElementById('ntsc-chroma-shift-val');
        const effectOrderGroup = document.getElementById('effect-order-group');
        const effectOrderButtons = effectOrderGroup ? effectOrderGroup.querySelectorAll('.effect-order-button') : [];

        const vhsSettingsGroup = document.getElementById('vhs-settings-group');
        const vhsStrengthSlider = document.getElementById('vhs-strength-slider');
        const vhsStrengthVal = document.getElementById('vhs-strength-val');
        const vhsNoiseSlider = document.getElementById('vhs-noise-slider');
        const vhsNoiseVal = document.getElementById('vhs-noise-val');
        const vhsGrainSizeSlider = document.getElementById('vhs-grain-size-slider');
        const vhsGrainSizeVal = document.getElementById('vhs-grain-size-val');
        const vhsVignetteSlider = document.getElementById('vhs-vignette-slider');
        const vhsVignetteVal = document.getElementById('vhs-vignette-val');
        const vhsBlurSlider = document.getElementById('vhs-blur-slider');
        const vhsBlurVal = document.getElementById('vhs-blur-val');
        const vhsAmountSlider = document.getElementById('vhs-amount-slider');
        const vhsAmountVal = document.getElementById('vhs-amount-val');
        const vhsLumaResolutionSlider = document.getElementById('vhs-luma-resolution-slider');
        const vhsLumaResolutionVal = document.getElementById('vhs-luma-resolution-val');
        const vhsChromaResolutionSlider = document.getElementById('vhs-chroma-resolution-slider');
        const albumArtSizeSlider = document.getElementById('album-art-size-slider');
        const albumArtSizeVal = document.getElementById('album-art-size-val');
        const milkdropFrameLimitSelect = document.getElementById('milkdrop-frame-limit-select');
        const milkdropCanvasSizeSelect = document.getElementById('milkdrop-canvas-size-select');
        const milkdropMeshSizeSelect = document.getElementById('milkdrop-mesh-size-select');
        const vhsChromaResolutionVal = document.getElementById('vhs-chroma-resolution-val');
        const vhsLineHeightSlider = document.getElementById('vhs-line-height-slider');
        const vhsLineHeightVal = document.getElementById('vhs-line-height-val');
        const vhsSharpenSlider = document.getElementById('vhs-sharpen-slider');
        const vhsSharpenVal = document.getElementById('vhs-sharpen-val');
        const vhsSharpenRadiusSlider = document.getElementById('vhs-sharpen-radius-slider');
        const vhsSharpenRadiusVal = document.getElementById('vhs-sharpen-radius-val');
        const vhsBlackLevelSlider = document.getElementById('vhs-black-level-slider');
        const vhsBlackLevelVal = document.getElementById('vhs-black-level-val');
        const vhsWhiteLevelSlider = document.getElementById('vhs-white-level-slider');
        const vhsWhiteLevelVal = document.getElementById('vhs-white-level-val');
        const vhsSaturationSlider = document.getElementById('vhs-saturation-slider');
        const vhsSaturationVal = document.getElementById('vhs-saturation-val');
        const vhsShadowTintPicker = document.getElementById('vhs-shadow-tint-picker');
        const vhsShadowTintVal = document.getElementById('vhs-shadow-tint-val');
        const vhsTrackingSpeedSlider = document.getElementById('vhs-tracking-speed-slider');
        const vhsTrackingSpeedVal = document.getElementById('vhs-tracking-speed-val');
        const vhsTrackingOffsetSlider = document.getElementById('vhs-tracking-offset-slider');
        const vhsTrackingOffsetVal = document.getElementById('vhs-tracking-offset-val');
        const vhsTrackingJitterSlider = document.getElementById('vhs-tracking-jitter-slider');
        const vhsTrackingJitterVal = document.getElementById('vhs-tracking-jitter-val');
        const vhsWaveFrequencySlider = document.getElementById('vhs-wave-frequency-slider');
        const vhsWaveFrequencyVal = document.getElementById('vhs-wave-frequency-val');
        const vhsWaveAmountSlider = document.getElementById('vhs-wave-amount-slider');
        const vhsWaveAmountVal = document.getElementById('vhs-wave-amount-val');
        const vhsBottomWarpHeightSlider = document.getElementById('vhs-bottom-warp-height-slider');
        const vhsBottomWarpHeightVal = document.getElementById('vhs-bottom-warp-height-val');
        const vhsBottomWarpOffsetSlider = document.getElementById('vhs-bottom-warp-offset-slider');
        const vhsBottomWarpOffsetVal = document.getElementById('vhs-bottom-warp-offset-val');
        const vhsBottomWarpJitterSlider = document.getElementById('vhs-bottom-warp-jitter-slider');
        const vhsBottomWarpJitterVal = document.getElementById('vhs-bottom-warp-jitter-val');
        const vhsStaticLineHeightSlider = document.getElementById('vhs-static-line-height-slider');
        const vhsStaticLineHeightVal = document.getElementById('vhs-static-line-height-val');
        const vhsStaticLineOpacitySlider = document.getElementById('vhs-static-line-opacity-slider');
        const vhsStaticLineOpacityVal = document.getElementById('vhs-static-line-opacity-val');
        const vhsVignettePowerSlider = document.getElementById('vhs-vignette-power-slider');
        const vhsVignettePowerVal = document.getElementById('vhs-vignette-power-val');
        const vhsVignetteBoostSlider = document.getElementById('vhs-vignette-boost-slider');
        const vhsVignetteBoostVal = document.getElementById('vhs-vignette-boost-val');

        const LYRICS_SYNC_MODE_MANUAL = 'manual';
        const LYRICS_SYNC_MODE_AUTO = 'auto';
        const LYRICS_SYNC_MODE_DISABLED = 'disabled';
        const PLAYER_ALIGN_LEFT = 'left';
        const PLAYER_ALIGN_CENTER = 'center';
        const PLAYER_TEXT_BLEND_DEFAULT = 'normal';
        const PLAYER_TEXT_BLEND_MODE_IDS = {
            normal: 0,
            multiply: 1,
            screen: 2,
            overlay: 3,
            darken: 4,
            lighten: 5,
            'color-dodge': 6,
            'color-burn': 7,
            'hard-light': 8,
            'soft-light': 9,
            difference: 10,
            exclusion: 11,
            hue: 12,
            saturation: 13,
            color: 14,
            luminosity: 15,
            'plus-darker': 16,
            'plus-lighter': 17,
        };
        const PLAYER_TEXT_BLEND_MODES = new Set([
            'normal',
            'multiply',
            'screen',
            'overlay',
            'darken',
            'lighten',
            'color-dodge',
            'color-burn',
            'hard-light',
            'soft-light',
            'difference',
            'exclusion',
            'hue',
            'saturation',
            'color',
            'luminosity',
            'plus-darker',
            'plus-lighter',
        ]);
        const EFFECT_ORDER_VHS_THEN_NTSC = 'vhs-then-ntsc';
        const EFFECT_ORDER_NTSC_THEN_VHS = 'ntsc-then-vhs';

        let visualizerEnabled = true;
        let visualizerOpacity = 0.8;
        let ntscEnabled = true;
        let ntscAmount = 1.0;
        let ntscSmear = 2.0;
        let ntscWiggle = 2.0;
        let ntscWiggleSpeed = 25.0;
        let ntscChromaShift = 1.0;
        let vhsEnabled = true;
        let vhsAmount = 1.0;
        let videoEffectOrder = EFFECT_ORDER_VHS_THEN_NTSC;
        let vhsStrength = 0.3;
        let vhsNoise = 0.4;
        let vhsGrainSize = 2.0;
        let vhsVignette = 1.0;
        let vhsBlur = 0.01;
        let vhsLumaResolution = 0.5;
        let vhsChromaResolution = 1.0 / 32.0;
        let vhsLineHeight = 0.0;
        let vhsSharpen = 2.0;
        let vhsSharpenRadius = 4.0;
        let vhsBlackLevel = 0.1;
        let vhsWhiteLevel = 0.9;
        let vhsSaturation = 0.75;
        let vhsShadowTint = '#b300e6';
        let vhsTrackingSpeed = 8.0;
        let vhsTrackingOffset = 8.0;
        let vhsTrackingJitter = 20.0;
        let vhsWaveFrequency = 70.0;
        let vhsWaveAmount = 1.0;
        let vhsBottomWarpHeight = 0.0;
        let vhsBottomWarpOffset = 100.0;
        let vhsBottomWarpJitter = 50.0;
        let vhsStaticLineHeight = 1.0;
        let vhsStaticLineOpacity = 0.3;
        let vhsVignettePower = 0.25;
        let vhsVignetteBoost = 2.2;
        let lyricsBorderWidth = 10;
        let lyricsWeight = 100;
        let lyricsLineHeight = 1.45;
        let lyricsWordSpacing = 0;
        let lyricsLetterSpacing = 0;
        
        let vhsProgA = null, vhsProgB = null, vhsProgC = null, vhsProgD = null, vhsProgImage = null;
        let fboA = null, texA = null;
        let fboB = null, texB = null;
        let fboC1 = null, texC1 = null, fboC2 = null, texC2 = null;
        let fboD = null, texD = null;
        let fboBase = null, texBase = null;
        let dummyTextTex = null;
        let vhsFrameCount = 0;
        let vhsInitialized = false;
        let isWebGL2 = false;
        let lyricsWrapWidth = 100;
        const DEFAULT_CUSTOM_CSS = 
`.lyric-line {
  color: #ffffff !important;
  font-size: calc(32px * var(--lyrics-size-scale)) !important;
  font-weight: var(--lyrics-font-weight, 700) !important;
  opacity: 0.12;
}

.lyric-line.active {
  color: #ffffff !important;
  font-size: calc(46px * var(--lyrics-size-scale)) !important;
  font-weight: var(--lyrics-font-weight, 700) !important;
  opacity: 1.0;
}`;
        let milkdropPresetName = '';
        let milkdropCycleSeconds = 15;
        let milkdropBlendSeconds = 3.0;
        let milkdropDisabledPresetNames = new Set();
        let milkdropFailedPresetNames = new Set();
        let milkdropTitleShownForTrackKey = '';

        const SLIDERS_MAP = {
            delay: { id: 'delay-slider', key: 'delay', defaultMin: 0, defaultMax: 15, defaultStep: 0.1 },
            autoSyncLyricsOffset: { id: 'autosync-offset-slider', key: 'autoSyncLyricsOffset', defaultMin: -2, defaultMax: 2, defaultStep: 0.05 },
            lyricTextScale: { id: 'lyrics-size-slider', key: 'lyricTextScale', defaultMin: 0.7, defaultMax: 1.8, defaultStep: 0.05 },
            lyricFadeCurve: { id: 'lyrics-fade-slider', key: 'lyricFadeCurve', defaultMin: 0.0, defaultMax: 5.0, defaultStep: 0.01 },
            albumArtSize: { id: 'album-art-size-slider', key: 'albumArtSize', defaultMin: 32, defaultMax: 256, defaultStep: 1 },
        };

        const sliderRanges = {
            delay: { type: 'simple', minKey: 'delayMin', maxKey: 'delayMax', stepKey: 'delayStep' },
            autoSyncLyricsOffset: { type: 'simple', minKey: 'autoSyncLyricsOffsetMin', maxKey: 'autoSyncLyricsOffsetMax', stepKey: 'autoSyncLyricsOffsetStep' },
            lyricTextScale: { type: 'simple', minKey: 'lyricTextScaleMin', maxKey: 'lyricTextScaleMax', stepKey: 'lyricTextScaleStep' },
            lyricFadeCurve: { type: 'simple', minKey: 'lyricFadeCurveMin', maxKey: 'lyricFadeCurveMax', stepKey: 'lyricFadeCurveStep' },
            albumArtSize: { type: 'simple', minKey: 'albumArtSizeMin', maxKey: 'albumArtSizeMax', stepKey: 'albumArtSizeStep' },
        };

        function clamp01(value) {
            return value;
        }

        function formatPercent(value) {
            return Math.round(value * 100) + '%';
        }

        function formatPercent1(value) {
            return (value * 100).toFixed(1) + '%';
        }

        function formatFixed(value, digits) {
            return Number(value).toFixed(digits);
        }

        function clampNumber(value, min, max, fallback) {
            const parsed = parseFloat(value);
            if (!Number.isFinite(parsed)) {
                return fallback;
            }
            return parsed;
        }

        function normalizeHexColor(value, fallback = '#ffffff') {
            if (typeof value !== 'string') {
                return fallback;
            }
            const trimmed = value.trim();
            if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) {
                return trimmed.toLowerCase();
            }
            if (/^#[0-9a-fA-F]{3}$/.test(trimmed)) {
                return ('#' + trimmed.slice(1).split('').map((char) => char + char).join('')).toLowerCase();
            }
            return fallback;
        }

        function hexToRgb01(value) {
            const hex = normalizeHexColor(value, '#b300e6').slice(1);
            return [
                parseInt(hex.slice(0, 2), 16) / 255,
                parseInt(hex.slice(2, 4), 16) / 255,
                parseInt(hex.slice(4, 6), 16) / 255,
            ];
        }

        function getLyricsSyncMode() {
            if (!lyricsVisible) {
                return LYRICS_SYNC_MODE_DISABLED;
            }
            return isAutosync ? LYRICS_SYNC_MODE_AUTO : LYRICS_SYNC_MODE_MANUAL;
        }

        function syncLyricsModeControls() {
            const mode = getLyricsSyncMode();
            lyricsSyncModeButtons.forEach((button) => {
                const isActive = button.dataset.lyricsSyncMode === mode;
                button.classList.toggle('active', isActive);
                button.setAttribute('aria-pressed', isActive ? 'true' : 'false');
            });

            if (lyricOptionsGroup) {
                lyricOptionsGroup.style.display = lyricsVisible ? 'flex' : 'none';
            }
            if (delayControlGroup) {
                delayControlGroup.style.display = lyricsVisible && !isAutosync ? 'flex' : 'none';
                delayControlGroup.style.opacity = lyricsVisible && !isAutosync ? '1.0' : '0.5';
            }
            if (delaySlider) {
                delaySlider.disabled = !lyricsVisible || isAutosync;
            }
            if (autoSyncOffsetControlGroup) {
                autoSyncOffsetControlGroup.style.display = lyricsVisible && isAutosync ? 'flex' : 'none';
                autoSyncOffsetControlGroup.style.opacity = lyricsVisible && isAutosync ? '1.0' : '0.5';
            }
            if (autoSyncOffsetSlider) {
                autoSyncOffsetSlider.disabled = !lyricsVisible || !isAutosync;
            }
            if (autosyncToggle) {
                autosyncToggle.checked = isAutosync;
            }
            if (showLyricsToggle) {
                showLyricsToggle.checked = lyricsVisible;
            }
        }

        function setLyricsSyncMode(mode) {
            const nextMode = mode === LYRICS_SYNC_MODE_AUTO || mode === LYRICS_SYNC_MODE_DISABLED
                ? mode
                : LYRICS_SYNC_MODE_MANUAL;
            lyricsVisible = nextMode !== LYRICS_SYNC_MODE_DISABLED;
            isAutosync = nextMode === LYRICS_SYNC_MODE_AUTO;
            if (!isAutosync) {
                clearDisplayedLyricsHold();
                clearPendingLyricsTransition();
            }
            applyLyricsVisibility();
            updateAutoSyncStatus();
            updateLyricsHighlight(true);
            saveSettings();
        }

        function isNtscActive() {
            return ntscAmount > 0.0001;
        }

        function isVhsActive() {
            return vhsAmount > 0.0001;
        }

        function isVideoEffectActive() {
            return isNtscActive() || isVhsActive();
        }

        function syncEffectEnabledFlags() {
            ntscEnabled = isNtscActive();
            vhsEnabled = isVhsActive();
        }

        function formatSignedSeconds(value) {
            const sign = value > 0 ? '+' : '';
            return sign + value.toFixed(2) + 's';
        }

        function getLyricsBorderShadow(width) {
            if (!Number.isFinite(width) || width <= 0) {
                return 'none';
            }
            const outline = Math.max(0.1, width * 0.15).toFixed(2) + 'px';
            const glow = Math.max(0.1, width * 0.30).toFixed(2) + 'px';
            return [
                '-' + outline + ' -' + outline + ' 0 #000',
                outline + ' -' + outline + ' 0 #000',
                '-' + outline + ' ' + outline + ' 0 #000',
                outline + ' ' + outline + ' 0 #000',
                '0 0 ' + glow + ' #000',
                '0 0 ' + glow + ' #000',
                '0 0 ' + glow + ' #000',
            ].join(', ');
        }

        function getCurrentSettingsSnapshot() {
            const snapshot = {
                autosync: isAutosync,
                lyricsVisible: lyricsVisible,
                lyricTextScale: lyricTextScale,
                lyricFadeCurve: lyricFadeCurve,
                lyricFont: lyricFont,
                playerFont: playerFont,
                customFonts: customFonts.slice(),
                autoSyncLyricsOffset: autoSyncLyricsOffsetSec,
                delay: manualDelaySec,
                visualizerEnabled: visualizerEnabled,
                visualizerOpacity: visualizerOpacity,
                ntscEnabled: isNtscActive(),
                ntscAmount: ntscAmount,
                ntscSmear: ntscSmear,
                ntscWiggle: ntscWiggle,
                ntscWiggleSpeed: ntscWiggleSpeed,
                ntscChromaShift: ntscChromaShift,
                vhsEnabled: isVhsActive(),
                vhsAmount: vhsAmount,
                videoEffectOrder: videoEffectOrder,
                vhsStrength: vhsStrength,
                vhsNoise: vhsNoise,
                vhsGrainSize: vhsGrainSize,
                vhsVignette: vhsVignette,
                vhsBlur: vhsBlur,
                vhsLumaResolution: vhsLumaResolution,
                vhsChromaResolution: vhsChromaResolution,
                vhsLineHeight: vhsLineHeight,
                vhsSharpen: vhsSharpen,
                vhsSharpenRadius: vhsSharpenRadius,
                vhsBlackLevel: vhsBlackLevel,
                vhsWhiteLevel: vhsWhiteLevel,
                vhsSaturation: vhsSaturation,
                vhsShadowTint: vhsShadowTint,
                vhsTrackingSpeed: vhsTrackingSpeed,
                vhsTrackingOffset: vhsTrackingOffset,
                vhsTrackingJitter: vhsTrackingJitter,
                vhsWaveFrequency: vhsWaveFrequency,
                vhsWaveAmount: vhsWaveAmount,
                vhsBottomWarpHeight: vhsBottomWarpHeight,
                vhsBottomWarpOffset: vhsBottomWarpOffset,
                vhsBottomWarpJitter: vhsBottomWarpJitter,
                vhsStaticLineHeight: vhsStaticLineHeight,
                vhsStaticLineOpacity: vhsStaticLineOpacity,
                vhsVignettePower: vhsVignettePower,
                vhsVignetteBoost: vhsVignetteBoost,
                lyricsBorderWidth: lyricsBorderWidth,
                lyricsWeight: lyricsWeight,
                lyricsLineHeight: lyricsLineHeight,
                lyricsWordSpacing: lyricsWordSpacing,
                lyricsLetterSpacing: lyricsLetterSpacing,
                lyricsWrapWidth: lyricsWrapWidth,
                milkdropPreset: milkdropPresetName,
                milkdropDisabledPresets: Array.from(milkdropDisabledPresetNames),
                milkdropCycleSeconds: milkdropCycleSeconds,
                milkdropBlendSeconds: milkdropBlendSeconds,
                milkdropCycleOnSongChange: milkdropCycleOnSongChange,
                playerOpacity: playerOpacity,
                playerBorderOpacity: playerBorderOpacity,
                playerBlur: playerBlur,
                playerScale: playerScale,
                playerModalWidth: playerModalWidth,
                playerEdgeGap: playerEdgeGap,
                playerTitleColor: playerTitleColor,
                playerArtistColor: playerArtistColor,
                playerTitleFontSize: playerTitleFontSize,
                playerArtistFontSize: playerArtistFontSize,
                playerTextGap: playerTextGap,
                playerApplyEffects: playerApplyEffects,
                playerTextHighlight: playerTextHighlight,
                playerTextBlendMode: playerTextBlendMode,
                playerTextLeft: playerTextLeft,
                playerTextBottom: playerTextBottom,
                liquidGlassEnabled: liquidGlassEnabled,
                playerAlign: playerAlign,
                showTrackInfo: showTrackInfo,
                showAlbumArt: showAlbumArt,
                showProgressBar: showProgressBar,
                showSyncStatus: showSyncStatus,
                showListenerNumber: showListenerNumber,
                playerMinimized: playerMinimized,
                apiKeyEnabled: apiKeyEnabled,
                apiKey: apiKey,
                albumArtSize: albumArtSize,
                milkdropFrameLimit: milkdropFrameLimit,
                milkdropCanvasSize: milkdropCanvasSize,
                milkdropMeshSize: milkdropMeshSize,
            };

            Object.keys(sliderRanges).forEach((key) => {
                const info = sliderRanges[key];
                const sliderEl = document.getElementById(SLIDERS_MAP[key].id);
                if (!sliderEl) {
                    return;
                }
                snapshot[info.minKey] = parseFloat(sliderEl.min);
                snapshot[info.maxKey] = parseFloat(sliderEl.max);
                snapshot[info.stepKey] = parseFloat(sliderEl.step);
            });

            return snapshot;
        }

        function generateStreamSessionId() {
            return (window.crypto && crypto.randomUUID)
                ? crypto.randomUUID()
                : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
        }

        function buildStreamUrl() {
            const url = new URL(listenEndpoint, window.location.origin);
            url.searchParams.set('sid', streamSessionId);
            return url.toString();
        }

        function buildWebSocketUrl() {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            const url = new URL('/ws', `${protocol}//${window.location.host}`);
            url.searchParams.set('sid', streamSessionId);
            return url.toString();
        }

        function wantsAudioStreamConnection() {
            return playbackRequested || mutedAutoplayPriming;
        }

        function serverExpectsAudio() {
            return !lastTrackData || lastTrackData.status === 'playing';
        }

        function clearAudioReconnectTimer() {
            if (audioReconnectTimer) {
                window.clearTimeout(audioReconnectTimer);
                audioReconnectTimer = null;
            }
        }

        function markAudioStreamHealthy() {
            clearAudioReconnectTimer();
            audioReconnectAttempts = 0;
            audioLastProgressAt = performance.now();
        }

        function stopAudioRecovery() {
            clearAudioReconnectTimer();
            audioReconnectAttempts = 0;
            audioReconnectInProgress = false;
        }

        function scheduleAudioReconnect(reason) {
            if (
                IS_SINGLE_FILE_BUILD_SOURCE ||
                !wantsAudioStreamConnection() ||
                audioReconnectTimer ||
                audioReconnectInProgress
            ) {
                return;
            }

            const delay = Math.min(
                AUDIO_RECONNECT_MAX_DELAY_MS,
                AUDIO_RECONNECT_BASE_DELAY_MS * (2 ** Math.min(audioReconnectAttempts, 4))
            );
            audioReconnectAttempts += 1;

            if (!mutedAutoplayPriming) {
                setPlayState('loading');
            }
            console.warn(`[Audio] ${reason}. Reconnecting in ${delay}ms.`);

            audioReconnectTimer = window.setTimeout(() => {
                audioReconnectTimer = null;
                void reconnectAudioStream(reason);
            }, delay);
        }

        async function reconnectAudioStream(reason) {
            if (!wantsAudioStreamConnection() || audioReconnectInProgress) {
                return;
            }

            audioReconnectInProgress = true;
            resetLocalGranuleSync();
            console.info(`[Audio] Reconnecting stream after: ${reason}`);

            if (wasmPlaybackEnabled) {
                let retryError = null;
                stopWasmStream(true);
                wasmLastChunkAt = performance.now();
                try {
                    await ensureWasmAudioOutput();
                    if (!await resumeAudioContextWithTimeout(1200)) {
                        throw new DOMException('AudioContext requires a user gesture', 'NotAllowedError');
                    }
                    startWasmStream();
                } catch (error) {
                    retryError = error;
                } finally {
                    audioReconnectInProgress = false;
                }

                if (retryError) {
                    console.warn('[WASM Audio] Reconnect attempt failed:', retryError);
                    scheduleAudioReconnect('WASM reconnect attempt failed');
                }
                return;
            }

            let retryError = null;
            try {
                audio.src = buildStreamUrl();
                audio.load();
                applyEffectiveVolume();
                await audio.play();
            } catch (error) {
                retryError = error;
            } finally {
                audioReconnectInProgress = false;
            }

            if (!retryError) {
                if (!wantsAudioStreamConnection()) {
                    audio.pause();
                    setPlayState('paused');
                }
                return;
            }

            if (retryError.name === 'NotAllowedError') {
                playbackRequested = false;
                mutedAutoplayPriming = false;
                applyEffectiveVolume();
                setPlaybackGestureRequired(true);
                setPlayState('paused');
                console.warn('[Audio] Reconnect needs a user gesture:', retryError);
                return;
            }

            console.warn('[Audio] Reconnect attempt failed:', retryError);
            scheduleAudioReconnect('Reconnect attempt failed');
        }

        function maybeRecoverAudioStream() {
            const now = performance.now();
            if (
                (now - audioRecoveryLastCheckedAt) < 1000 ||
                !wantsAudioStreamConnection() ||
                !serverExpectsAudio()
            ) {
                return;
            }
            audioRecoveryLastCheckedAt = now;

            if (wasmPlaybackEnabled) {
                if (!wasmFetchPromise) {
                    scheduleAudioReconnect('WASM audio stream is disconnected');
                    return;
                }
                if (wasmLastChunkAt > 0 && (now - wasmLastChunkAt) > AUDIO_STALL_TIMEOUT_MS) {
                    stopWasmStream(true);
                    scheduleAudioReconnect('WASM audio stream stopped delivering data');
                }
                return;
            }

            const currentTime = Number(audio.currentTime);
            if (
                Number.isFinite(currentTime) &&
                Math.abs(currentTime - audioLastCurrentTime) >= 0.05
            ) {
                audioLastCurrentTime = currentTime;
                markAudioStreamHealthy();
                return;
            }

            if (audio.error) {
                scheduleAudioReconnect(`Audio element failed with media error ${audio.error.code}`);
            } else if (audio.paused) {
                scheduleAudioReconnect('Audio element stopped unexpectedly');
            } else if ((now - audioLastProgressAt) > AUDIO_STALL_TIMEOUT_MS) {
                scheduleAudioReconnect('Audio stream stopped making progress');
            }
        }

        function appendUint8Array(a, b) {
            if (!a || a.length === 0) {
                return b;
            }
            const merged = new Uint8Array(a.length + b.length);
            merged.set(a, 0);
            merged.set(b, a.length);
            return merged;
        }

        function readOggGranule(view, offset) {
            const lo = view.getUint32(offset + 6, true);
            const hi = view.getUint32(offset + 10, true);
            if (lo === 0xffffffff && hi === 0xffffffff) {
                return null;
            }
            return lo + hi * 4294967296;
        }

        function parseWasmOggPages(chunk) {
            wasmOggParserBuffer = appendUint8Array(wasmOggParserBuffer, chunk);
            const pages = [];
            let offset = 0;

            while (offset + 27 <= wasmOggParserBuffer.length) {
                if (
                    wasmOggParserBuffer[offset] !== 0x4f ||
                    wasmOggParserBuffer[offset + 1] !== 0x67 ||
                    wasmOggParserBuffer[offset + 2] !== 0x67 ||
                    wasmOggParserBuffer[offset + 3] !== 0x53
                ) {
                    offset += 1;
                    continue;
                }

                const segmentCount = wasmOggParserBuffer[offset + 26];
                if (offset + 27 + segmentCount > wasmOggParserBuffer.length) {
                    break;
                }

                let bodySize = 0;
                for (let i = 0; i < segmentCount; i += 1) {
                    bodySize += wasmOggParserBuffer[offset + 27 + i];
                }

                const totalSize = 27 + segmentCount + bodySize;
                if (offset + totalSize > wasmOggParserBuffer.length) {
                    break;
                }

                const view = new DataView(
                    wasmOggParserBuffer.buffer,
                    wasmOggParserBuffer.byteOffset,
                    wasmOggParserBuffer.byteLength
                );
                pages.push({
                    bytes: wasmOggParserBuffer.slice(offset, offset + totalSize),
                    headerType: wasmOggParserBuffer[offset + 5],
                    granule: readOggGranule(view, offset),
                    serial: view.getUint32(offset + 14, true),
                });
                offset += totalSize;
            }

            if (offset > 0) {
                wasmOggParserBuffer = wasmOggParserBuffer.slice(offset);
            }

            return pages;
        }

        async function ensureWasmDecoder() {
            if (!wasmPlaybackEnabled) {
                throw new Error('WASM playback is not available for this stream.');
            }

            if (wasmDecoder) {
                if (!wasmDecoderReady) {
                    await wasmDecoder.ready;
                    wasmDecoderReady = true;
                }
                return wasmDecoder;
            }

            const workerDecoderClass = wasmDecoderNamespace.OggVorbisDecoderWebWorker;
            const mainDecoderClass = wasmDecoderNamespace.OggVorbisDecoder;

            if (workerDecoderClass) {
                try {
                    wasmDecoder = new workerDecoderClass();
                    await wasmDecoder.ready;
                    wasmDecoderReady = true;
                    return wasmDecoder;
                } catch (error) {
                    console.warn('[WASM Audio] Worker decoder failed; falling back to main-thread decoder.', error);
                    if (wasmDecoder && typeof wasmDecoder.free === 'function') {
                        try {
                            wasmDecoder.free();
                        } catch (_) {}
                    }
                    wasmDecoder = null;
                    wasmDecoderReady = false;
                }
            }

            if (!mainDecoderClass) {
                throw new Error('No Ogg Vorbis WASM decoder class was loaded.');
            }

            wasmDecoder = new mainDecoderClass();
            await wasmDecoder.ready;
            wasmDecoderReady = true;
            return wasmDecoder;
        }

        async function resetWasmDecoderStream() {
            const decoder = await ensureWasmDecoder();
            await decoder.reset();
            wasmOggParserBuffer = new Uint8Array(0);
            wasmSeenBos = false;
        }

        function resampleChannel(input, inputRate, outputRate) {
            if (!input || input.length === 0) {
                return new Float32Array(0);
            }

            if (inputRate === outputRate) {
                return new Float32Array(input);
            }

            const ratio = outputRate / inputRate;
            const outputLength = Math.max(1, Math.round(input.length * ratio));
            const output = new Float32Array(outputLength);

            for (let i = 0; i < outputLength; i += 1) {
                const sourcePos = i / ratio;
                const sourceIndex = Math.floor(sourcePos);
                const frac = sourcePos - sourceIndex;
                const a = input[sourceIndex] || 0;
                const b = input[Math.min(sourceIndex + 1, input.length - 1)] || a;
                output[i] = a + (b - a) * frac;
            }

            return output;
        }

        function enqueueDecodedWasmAudio(decoded, page) {
            if (!wasmAudioNode || !decoded || !decoded.samplesDecoded || page.granule === null) {
                return;
            }

            const decodedSampleRate = decoded.sampleRate || 44100;
            const outputSampleRate = audioCtx ? audioCtx.sampleRate : decodedSampleRate;
            const channelData = Array.isArray(decoded.channelData) ? decoded.channelData : [];
            const sourceLeft = channelData[0];
            const sourceRight = channelData[1] || sourceLeft;

            if (!sourceLeft || sourceLeft.length === 0) {
                return;
            }

            const decodedStartSec = Math.max(
                0,
                (page.granule - decoded.samplesDecoded) / decodedSampleRate
            );
            const left = resampleChannel(sourceLeft, decodedSampleRate, outputSampleRate);
            const right = sourceRight === sourceLeft
                ? new Float32Array(left)
                : resampleChannel(sourceRight, decodedSampleRate, outputSampleRate);
            const frames = Math.min(left.length, right.length);

            if (frames === 0) {
                return;
            }

            const leftBuffer = frames === left.length ? left : left.slice(0, frames);
            const rightBuffer = frames === right.length ? right : right.slice(0, frames);
            wasmAudioNode.port.postMessage(
                {
                    type: 'enqueue',
                    left: leftBuffer.buffer,
                    right: rightBuffer.buffer,
                    startSec: decodedStartSec,
                },
                [leftBuffer.buffer, rightBuffer.buffer]
            );
        }

        async function processWasmOggPage(page, generation) {
            if (generation !== wasmStreamGeneration) {
                return;
            }

            const decoder = await ensureWasmDecoder();
            const isBos = (page.headerType & 0x02) !== 0;

            if (isBos) {
                if (wasmSeenBos) {
                    await decoder.reset();
                    if (wasmAudioNode) {
                        wasmAudioNode.port.postMessage({ type: 'track-boundary' });
                    }
                } else {
                    wasmSeenBos = true;
                    if (wasmAudioNode) {
                        wasmAudioNode.port.postMessage({ type: 'reset-now', positionSec: 0 });
                    }
                }
            }

            const decoded = await decoder.decode(page.bytes);
            if (generation !== wasmStreamGeneration) {
                return;
            }

            if (decoded && decoded.errors && decoded.errors.length > 0) {
                console.warn('[WASM Audio] Ogg/Vorbis decode warnings:', decoded.errors);
            }
            enqueueDecodedWasmAudio(decoded, page);
        }

        async function startWasmStream() {
            if (!wasmPlaybackEnabled || wasmFetchPromise) {
                return wasmFetchPromise;
            }

            const controller = new AbortController();
            const generation = wasmStreamGeneration + 1;
            wasmStreamGeneration = generation;
            wasmFetchAbortController = controller;
            wasmLastChunkAt = performance.now();
            const streamPromise = (async () => {
                await resetWasmDecoderStream();
                const response = await fetch(buildStreamUrl(), {
                    cache: 'no-store',
                    signal: controller.signal,
                });

                if (!response.ok || !response.body) {
                    throw new Error(`Audio stream request failed with HTTP ${response.status}`);
                }

                const reader = response.body.getReader();
                while (true) {
                    const { value, done } = await reader.read();
                    if (done) {
                        break;
                    }

                    wasmLastChunkAt = performance.now();
                    markAudioStreamHealthy();
                    const pages = parseWasmOggPages(value);
                    for (const page of pages) {
                        await processWasmOggPage(page, generation);
                    }
                }

                if (!controller.signal.aborted) {
                    throw new Error('Audio stream ended unexpectedly');
                }
            })()
                .catch((error) => {
                    if (error && error.name === 'AbortError') {
                        return;
                    }
                    console.error('[WASM Audio] Stream failed:', error);
                    scheduleAudioReconnect('WASM audio stream failed');
                })
                .finally(() => {
                    if (wasmFetchPromise === streamPromise) {
                        wasmFetchAbortController = null;
                        wasmFetchPromise = null;
                    }
                });

            wasmFetchPromise = streamPromise;
            return wasmFetchPromise;
        }

        function stopWasmStream(clearQueue = true) {
            if (wasmFetchAbortController) {
                wasmFetchAbortController.abort();
            }
            wasmFetchAbortController = null;
            wasmFetchPromise = null;
            wasmStreamGeneration += 1;
            wasmOggParserBuffer = new Uint8Array(0);
            wasmSeenBos = false;

            if (clearQueue && wasmAudioNode) {
                wasmClockReady = false;
                wasmPlaybackPositionSec = 0;
                wasmBufferedSeconds = 0;
                wasmAudioNode.port.postMessage({ type: 'reset-now', positionSec: 0 });
            }
        }

        function resetLocalGranuleSync() {
            streamFirstGranuleSec = null;
            localGranuleClockMode = null;
            serviceWorkerSyncReady = false;
        }

        function handleLocalGranuleMessage(event) {
            const msg = event.data || {};
            if (msg.sid !== streamSessionId) {
                return;
            }

            if (msg.type === 'granule' && Number.isFinite(msg.granuleSec)) {
                if (streamFirstGranuleSec === null) {
                    streamFirstGranuleSec = msg.granuleSec;
                    serviceWorkerSyncReady = true;
                    updateAutoSyncStatus();
                }
            } else if (msg.type === 'error') {
                console.warn('[LyricsSync] Service Worker granule parser error:', msg.message);
            }
        }

        async function registerAudioTapServiceWorker() {
            if (IS_SINGLE_FILE_BUILD_SOURCE || !('serviceWorker' in navigator) || !('BroadcastChannel' in window)) {
                return false;
            }

            const channel = new BroadcastChannel('spotifm-audio-granules');
            channel.onmessage = handleLocalGranuleMessage;

            try {
                const registration = await navigator.serviceWorker.register(playerServiceWorkerEndpoint, {
                    scope: '/',
                });
                await navigator.serviceWorker.ready;
                serviceWorkerSyncEnabled = true;

                if (!navigator.serviceWorker.controller) {
                    await Promise.race([
                        new Promise((resolve) => {
                            navigator.serviceWorker.addEventListener('controllerchange', resolve, { once: true });
                        }),
                        new Promise((resolve) => window.setTimeout(resolve, 1500)),
                    ]);
                }

                console.info('[LyricsSync] Audio stream Service Worker tap active.', registration.scope);
                return true;
            } catch (error) {
                console.warn('[LyricsSync] Could not register audio stream Service Worker tap:', error);
                channel.close();
                return false;
            }
        }

        async function initializeAudioStream() {
            if (IS_SINGLE_FILE_BUILD_SOURCE) {
                return;
            }

            resetLocalGranuleSync();
            if (wasmPlaybackEnabled) {
                audio.removeAttribute('src');
                audio.load();
                setPlayState('paused');
                console.info('[WASM Audio] Ogg/Vorbis passthrough playback enabled.');
                return;
            }

            if (wasmPlaybackRequired) {
                audio.removeAttribute('src');
                audio.load();
                setPlayState('paused');
                syncStatus.textContent = 'WASM Required';
                console.error('[WASM Audio] Passthrough stream requires the Ogg/Vorbis WASM decoder and AudioWorklet support.');
                return;
            }

            audio.src = buildStreamUrl();
            attemptAutoplay();
        }

        function getVolumeSliderValue() {
            if (!volumeControl) {
                return 1.0;
            }
            const parsed = parseFloat(volumeControl.value);
            return Number.isFinite(parsed) ? Math.max(0, Math.min(1, parsed)) : 1.0;
        }

        function postWasmVolume(volumeValue) {
            if (wasmAudioNode) {
                wasmAudioNode.port.postMessage({
                    type: 'volume',
                    volume: volumeValue,
                });
            }
        }

        function updateVolumeButtonState() {
            const sliderValue = getVolumeSliderValue();
            const isEffectivelyMuted = isVolumeMuted || sliderValue <= 0;
            if (volumeFlyout) {
                volumeFlyout.classList.toggle('muted', isEffectivelyMuted);
            }
            if (volumeToggleBtn) {
                volumeToggleBtn.setAttribute('aria-label', isEffectivelyMuted ? 'Unmute audio' : 'Mute audio');
                volumeToggleBtn.setAttribute('aria-pressed', isEffectivelyMuted ? 'true' : 'false');
                volumeToggleBtn.setAttribute('title', isEffectivelyMuted ? 'Unmute' : 'Mute');
            }
            if (volumeMutedLine) {
                volumeMutedLine.style.display = isEffectivelyMuted ? '' : 'none';
            }
            if (volumeWaveSmall) {
                volumeWaveSmall.style.display = isEffectivelyMuted ? 'none' : '';
            }
            if (volumeWaveLarge) {
                volumeWaveLarge.style.display = (!isEffectivelyMuted && sliderValue >= 0.55) ? '' : 'none';
            }
        }

        function applyEffectiveVolume() {
            const sliderValue = getVolumeSliderValue();
            const effectiveVolume = isVolumeMuted ? 0 : sliderValue;
            audio.volume = sliderValue;
            audio.muted = mutedAutoplayPriming || isVolumeMuted || sliderValue <= 0;
            postWasmVolume(effectiveVolume);
            updateVolumeButtonState();
        }

        function setVolumeMuted(nextMuted) {
            isVolumeMuted = !!nextMuted;
            if (!isVolumeMuted && getVolumeSliderValue() <= 0 && volumeControl) {
                volumeControl.value = String(lastAudibleVolume || 1);
            }
            applyEffectiveVolume();
        }

        function attemptAutoplay() {
            playbackRequested = true;
            mutedAutoplayPriming = false;
            setPlaybackGestureRequired(false);
            applyEffectiveVolume();
            setPlayState('loading');

            const playPromise = audio.play();
            if (playPromise && typeof playPromise.then === 'function') {
                playPromise
                    .then(() => {
                        console.info('[Audio] Unmuted autoplay succeeded!');
                    })
                    .catch((err) => {
                        if (err && err.name === 'NotAllowedError') {
                            console.warn('[Audio] Unmuted autoplay blocked, falling back to muted autoplay...', err);
                            attemptMutedAutoplay();
                        } else {
                            console.warn('[Audio] Unmuted autoplay failed:', err);
                            scheduleAudioReconnect('Initial audio stream failed');
                        }
                    });
            } else {
                attemptMutedAutoplay();
            }
        }

        function attemptMutedAutoplay() {
            playbackRequested = false;
            mutedAutoplayPriming = true;
            applyEffectiveVolume();
            setPlayState('loading');

            const playPromise = audio.play();
            if (!playPromise || typeof playPromise.then !== 'function') {
                return;
            }

            playPromise
                .then(() => {
                    if (mutedAutoplayPriming) {
                        setPlaybackGestureRequired(true);
                        setPlayState('paused');
                    }
                })
                .catch((error) => {
                    if (error && error.name !== 'NotAllowedError') {
                        console.warn('[Audio] Muted audio stream failed:', error);
                        setPlayState('paused');
                        scheduleAudioReconnect('Muted audio stream failed');
                        return;
                    }
                    mutedAutoplayPriming = false;
                    applyEffectiveVolume();
                    setPlaybackGestureRequired(true);
                    setPlayState('paused');
                    console.warn('[Audio] Muted autoplay was blocked:', error);
                });
        }

        function positionTitleTooltip(clientX, clientY) {
            if (!titleTooltip) {
                return;
            }

            const tooltipWidth = titleTooltip.offsetWidth || 140;
            const left = Math.min(
                window.innerWidth - tooltipWidth - 12,
                Math.max(12, clientX - tooltipWidth / 2)
            );
            const top = Math.max(12, clientY - 42);
            titleTooltip.style.left = `${left}px`;
            titleTooltip.style.top = `${top}px`;
        }

        function showTitleTooltip(clientX = lastTitleTooltipClientX, clientY = lastTitleTooltipClientY) {
            if (!titleTooltip || !currentTrackId) {
                return;
            }

            positionTitleTooltip(clientX, clientY);
            titleTooltip.classList.add('visible');
        }

        function hideTitleTooltip() {
            if (!titleTooltip) {
                return;
            }
            titleTooltip.classList.remove('visible');
        }

        function applyLyricsVisibility() {
            container.classList.toggle('lyrics-hidden', !lyricsVisible);
            syncLyricsModeControls();
            resizeLyricsPadding();
        }

        function applyBottomDockLayout() {
            container.classList.add('layout-bottom-dock');
            document.body.classList.add('layout-bottom-dock');
            applyPlayerStyleSettings();
            resizeLyricsPadding();
            updateContainerHeight();
        }

        function normalizePlayerAlign(value) {
            return value === PLAYER_ALIGN_LEFT ? PLAYER_ALIGN_LEFT : PLAYER_ALIGN_CENTER;
        }

        function normalizePlayerTextBlendMode(value) {
            return PLAYER_TEXT_BLEND_MODES.has(value) ? value : PLAYER_TEXT_BLEND_DEFAULT;
        }

        function formatPlayerModalWidth(value) {
            return value <= 0 ? 'Auto' : Math.round(value) + 'px';
        }

        function syncPlayerTextPositionControls() {
            const maxSafeGap = Math.max(0, Math.floor(Math.min(window.innerWidth, window.innerHeight) * 0.2));
            const safeGap = Math.max(0, Math.min(maxSafeGap, Number.isFinite(playerEdgeGap) ? playerEdgeGap : 20));
            playerTextBottom = safeGap;
            playerTextLeft = safeGap;

            if (playerTextLeftVal) {
                playerTextLeftVal.textContent = playerTextLeft + 'px';
            }
            if (playerTextBottomVal) {
                playerTextBottomVal.textContent = playerTextBottom + 'px';
            }
        }

        function getPlayerTextBlendModeId() {
            return PLAYER_TEXT_BLEND_MODE_IDS[playerTextBlendMode] || PLAYER_TEXT_BLEND_MODE_IDS.normal;
        }

        function isPlayerTextBlendActive() {
            return playerTextBlendMode !== PLAYER_TEXT_BLEND_DEFAULT && showTrackInfo;
        }

        function truncateTextWithEllipsis(ctx, text, font, maxWidth) {
            ctx.save();
            ctx.font = font;
            
            let metrics = ctx.measureText(text);
            let left = Number.isFinite(metrics.actualBoundingBoxLeft) ? metrics.actualBoundingBoxLeft : 0;
            let right = Number.isFinite(metrics.actualBoundingBoxRight) ? metrics.actualBoundingBoxRight : metrics.width;
            let textWidth = left + right;
            if (textWidth <= maxWidth) {
                ctx.restore();
                return text;
            }

            let ellipsis = '...';
            let truncated = text;
            while (truncated.length > 0) {
                truncated = truncated.slice(0, -1);
                let testText = truncated + ellipsis;
                let testMetrics = ctx.measureText(testText);
                let testLeft = Number.isFinite(testMetrics.actualBoundingBoxLeft) ? testMetrics.actualBoundingBoxLeft : 0;
                let testRight = Number.isFinite(testMetrics.actualBoundingBoxRight) ? testMetrics.actualBoundingBoxRight : testMetrics.width;
                let testWidth = testLeft + testRight;
                if (testWidth <= maxWidth) {
                    ctx.restore();
                    return testText;
                }
            }
            ctx.restore();
            return ellipsis;
        }

        function getCanvasPlayerTextLine(source, color) {
            if (!playerTextCtx || !source) return null;
            const computed = window.getComputedStyle(source);
            const isHidden = computed.display === 'none' ||
                computed.visibility === 'hidden';

            if (isHidden) {
                return null;
            }

            const rect = source.getBoundingClientRect();
            const scale = source.offsetHeight > 0 ? rect.height / source.offsetHeight : 1;
            const fontSize = parseFloat(computed.fontSize);
            const effectiveFontSize = Number.isFinite(fontSize) ? fontSize * scale : rect.height;
            const text = source.textContent || '';
            if (!text) {
                return null;
            }

            const font = `${computed.fontStyle} ${computed.fontVariant} ${computed.fontWeight} ${effectiveFontSize}px ${computed.fontFamily}`;
            playerTextCtx.font = font;

            const paddingX = playerTextHighlight ? 8 : 0;
            const maxWidth = Math.max(20, playerTextCanvas.width - 2 * Math.max(0, playerTextLeft) - 2 * paddingX);
            const truncatedText = truncateTextWithEllipsis(playerTextCtx, text, font, maxWidth);

            const metrics = playerTextCtx.measureText(truncatedText);
            const fallbackAscent = effectiveFontSize * 0.8;
            const fallbackDescent = effectiveFontSize * 0.2;

            return {
                text: truncatedText,
                font,
                color,
                left: Number.isFinite(metrics.actualBoundingBoxLeft) ? metrics.actualBoundingBoxLeft : 0,
                right: Number.isFinite(metrics.actualBoundingBoxRight) ? metrics.actualBoundingBoxRight : metrics.width,
                ascent: Number.isFinite(metrics.actualBoundingBoxAscent) ? metrics.actualBoundingBoxAscent : fallbackAscent,
                descent: Number.isFinite(metrics.actualBoundingBoxDescent) ? metrics.actualBoundingBoxDescent : fallbackDescent,
            };
        }

        function drawCanvasPlayerText() {
            if (!playerTextCtx || !playerTextCanvas) {
                return;
            }
            playerTextCtx.clearRect(0, 0, playerTextCanvas.width, playerTextCanvas.height);
            if (!isPlayerTextBlendActive()) {
                return;
            }

            const titleLine = getCanvasPlayerTextLine(songTitle, playerTitleColor);
            const artistLine = getCanvasPlayerTextLine(songArtists, playerArtistColor);
            const lines = [titleLine, artistLine].filter(Boolean);
            if (lines.length === 0) {
                return;
            }

            const paddingX = playerTextHighlight ? 8 : 0;
            const paddingY = playerTextHighlight ? 4 : 0;

            const visualGap = playerTextGap;

            const anchorBottomY = playerTextCanvas.height - Math.max(0, playerTextBottom) - paddingY;
            let baselineY = anchorBottomY - lines[lines.length - 1].descent;

            playerTextCtx.save();
            playerTextCtx.textAlign = 'left';
            playerTextCtx.textBaseline = 'alphabetic';

            const linePositions = [];
            let currentBaselineY = baselineY;
            for (let i = lines.length - 1; i >= 0; i--) {
                const line = lines[i];
                linePositions[i] = currentBaselineY;
                if (i > 0) {
                    currentBaselineY -= line.ascent + visualGap + lines[i - 1].descent;
                }
            }

            if (playerTextHighlight) {
                for (let i = 0; i < lines.length; i++) {
                    const line = lines[i];
                    const y = linePositions[i];
                    const paddingX = 8;
                    const paddingY = 4;
                    const rectX = Math.max(0, playerTextLeft);
                    const rectY = y - line.ascent - paddingY;
                    const rectW = line.left + line.right + 2 * paddingX;
                    const rectH = line.ascent + line.descent + 2 * paddingY;
                    playerTextCtx.fillStyle = line.color;
                    playerTextCtx.fillRect(rectX, rectY, rectW, rectH);
                }
            }

            for (let i = lines.length - 1; i >= 0; i--) {
                const line = lines[i];
                const y = linePositions[i];
                const lineAnchorX = Math.max(0, playerTextLeft) + line.left + paddingX;
                playerTextCtx.font = line.font;
                playerTextCtx.fillStyle = playerTextHighlight ? '#000000' : line.color;
                playerTextCtx.fillText(line.text, lineAnchorX, y);
            }

            playerTextCtx.restore();
        }

        function updatePlayerTextBlendLayer() {
            if (isPlayerTextBlendActive() && canvas && !ntscGl) {
                initNtscWebGL();
            }
            drawCanvasPlayerText();
            if (ntscGl && !isPlaying) {
                renderNtscFrame(performance.now() / 1000.0);
            }
        }

        function updatePlayerViewportConstraints() {
            const maxSafeGap = Math.max(0, Math.floor(Math.min(window.innerWidth, window.innerHeight) * 0.2));
            const safeGap = Math.max(0, Math.min(maxSafeGap, Number.isFinite(playerEdgeGap) ? playerEdgeGap : 20));
            const safeScale = Math.max(0.1, Number.isFinite(playerScale) ? playerScale : 1);
            const maxWidth = Math.max(160, Math.floor((window.innerWidth - safeGap * 2) / safeScale));
            const widthValue = playerModalWidth <= 0
                ? 'auto'
                : Math.min(Math.max(160, playerModalWidth), maxWidth) + 'px';
            document.documentElement.style.setProperty('--player-safe-gap', safeGap + 'px');
            document.documentElement.style.setProperty('--player-max-width', maxWidth + 'px');
            document.documentElement.style.setProperty('--player-width', widthValue);

            playerTextBottom = safeGap;
            playerTextLeft = safeGap;
        }

        let albumArtSizeRaf = null;

        function updateAlbumArtSize() {
            if (!document.documentElement) {
                return;
            }
            const size = Number.isFinite(albumArtSize) ? albumArtSize : 92;
            document.documentElement.style.setProperty('--album-art-size', size + 'px');
        }

        function scheduleAlbumArtSizeUpdate() {
            if (albumArtSizeRaf !== null) {
                window.cancelAnimationFrame(albumArtSizeRaf);
            }
            albumArtSizeRaf = window.requestAnimationFrame(() => {
                albumArtSizeRaf = null;
                updateAlbumArtSize();
            });
        }

        function syncPlayerStyleControls() {
            playerAlignButtons.forEach((button) => {
                const isActive = button.dataset.playerAlign === playerAlign;
                button.classList.toggle('active', isActive);
                button.setAttribute('aria-pressed', isActive ? 'true' : 'false');
            });

            if (liquidGlassToggle) liquidGlassToggle.checked = liquidGlassEnabled;
            if (liquidGlassSettingsGroup) {
                liquidGlassSettingsGroup.style.display = (liquidGlassEnabled && !playerApplyEffects) ? 'flex' : 'none';
            }
            if (playerModalWidthSlider) playerModalWidthSlider.value = String(playerModalWidth);
            if (playerModalWidthVal) playerModalWidthVal.textContent = formatPlayerModalWidth(playerModalWidth);
            if (playerEdgeGapSlider) playerEdgeGapSlider.value = String(playerEdgeGap);
            if (playerEdgeGapVal) playerEdgeGapVal.textContent = Math.round(playerEdgeGap) + 'px';
            if (playerTitleColorPicker) playerTitleColorPicker.value = playerTitleColor;
            if (playerTitleColorVal) playerTitleColorVal.textContent = playerTitleColor.toUpperCase();
            if (playerArtistColorPicker) playerArtistColorPicker.value = playerArtistColor;
            if (playerArtistColorVal) playerArtistColorVal.textContent = playerArtistColor.toUpperCase();
            if (playerTextBlendSelect) playerTextBlendSelect.value = playerTextBlendMode;
            if (playerTitleFontSizeSlider) playerTitleFontSizeSlider.value = String(playerTitleFontSize);
            if (playerTitleFontSizeVal) playerTitleFontSizeVal.textContent = playerTitleFontSize + 'px';
            if (playerArtistFontSizeSlider) playerArtistFontSizeSlider.value = String(playerArtistFontSize);
            if (playerArtistFontSizeVal) playerArtistFontSizeVal.textContent = playerArtistFontSize + 'px';
            if (playerTextGapSlider) playerTextGapSlider.value = String(playerTextGap);
            if (playerTextGapVal) playerTextGapVal.textContent = playerTextGap + 'px';
            if (playerApplyEffectsToggle) playerApplyEffectsToggle.checked = playerApplyEffects;
            if (playerTextHighlightToggle) playerTextHighlightToggle.checked = playerTextHighlight;
            syncPlayerTextPositionControls();
            if (showTrackInfoToggle) showTrackInfoToggle.checked = showTrackInfo;
            if (showAlbumArtToggle) showAlbumArtToggle.checked = showAlbumArt;
            if (showProgressBarToggle) showProgressBarToggle.checked = showProgressBar;
            if (showSyncStatusToggle) showSyncStatusToggle.checked = showSyncStatus;
            if (showListenerNumberToggle) showListenerNumberToggle.checked = showListenerNumber;
            if (albumArtSizeSlider) albumArtSizeSlider.value = String(albumArtSize);
            if (albumArtSizeVal) albumArtSizeVal.textContent = albumArtSize + 'px';
        }

        function applyPlayerStyleSettings() {
            playerAlign = normalizePlayerAlign(playerAlign);
            playerTitleColor = normalizeHexColor(playerTitleColor, '#ffffff');
            playerArtistColor = normalizeHexColor(playerArtistColor, '#9ca3af');
            playerTextBlendMode = normalizePlayerTextBlendMode(playerTextBlendMode);

            if (playerTextBlendSelect) {
                const normalOpt = playerTextBlendSelect.querySelector('option[value="normal"]');
                if (playerApplyEffects) {
                    if (normalOpt) {
                        normalOpt.remove();
                    }
                    if (playerTextBlendMode === 'normal') {
                        playerTextBlendMode = 'overlay';
                    }
                } else {
                    if (!normalOpt) {
                        const opt = document.createElement('option');
                        opt.value = 'normal';
                        opt.textContent = 'Normal';
                        playerTextBlendSelect.insertBefore(opt, playerTextBlendSelect.firstChild);
                    }
                    playerTextBlendMode = 'normal';
                }
            }

            document.documentElement.style.setProperty('--player-title-color', playerTitleColor);
            document.documentElement.style.setProperty('--player-artist-color', playerArtistColor);
            document.documentElement.style.setProperty('--player-text-blend-mode', playerTextBlendMode);
            document.documentElement.style.setProperty('--player-title-font-size', playerTitleFontSize + 'px');
            document.documentElement.style.setProperty('--player-artist-font-size', playerArtistFontSize + 'px');
            document.documentElement.style.setProperty('--player-text-gap', playerTextGap + 'px');
            updatePlayerViewportConstraints();
            if (container) {
                container.classList.toggle('player-align-left', playerAlign === PLAYER_ALIGN_LEFT);
                container.classList.toggle('player-align-center', playerAlign === PLAYER_ALIGN_CENTER);
                container.classList.toggle('player-text-blend-active', playerApplyEffects || playerTextBlendMode !== PLAYER_TEXT_BLEND_DEFAULT);
                container.classList.toggle('player-apply-effects-active', playerApplyEffects);
                container.classList.toggle('player-text-highlight-active', playerApplyEffects && playerTextHighlight);
                container.classList.toggle('hide-track-info', !showTrackInfo && !playerApplyEffects);
                container.classList.toggle('hide-album-art', !showAlbumArt || playerApplyEffects);
                container.classList.toggle('hide-progress-bar', !showProgressBar || playerApplyEffects);
                container.classList.toggle('hide-sync-status', !showSyncStatus || playerApplyEffects);
                container.classList.toggle('hide-listener-number', !showListenerNumber || playerApplyEffects);
            }
            if (!showTrackInfo && !playerApplyEffects) {
                hideTitleTooltip();
            }

            const visibilityStyle = playerApplyEffects ? 'none' : 'flex';
            if (showTrackInfoRow) showTrackInfoRow.style.display = visibilityStyle;
            if (showAlbumArtRow) showAlbumArtRow.style.display = visibilityStyle;
            if (showProgressBarRow) showProgressBarRow.style.display = visibilityStyle;
            if (showSyncStatusRow) showSyncStatusRow.style.display = visibilityStyle;
            if (showListenerNumberRow) showListenerNumberRow.style.display = visibilityStyle;
            if (playerModalWidthField) playerModalWidthField.style.display = visibilityStyle;
            if (playerAlignField) playerAlignField.style.display = visibilityStyle;
            if (liquidGlassRow) liquidGlassRow.style.display = visibilityStyle;
            if (playerTextBlendField) playerTextBlendField.style.display = playerApplyEffects ? 'flex' : 'none';
            if (playerTextHighlightRow) playerTextHighlightRow.style.display = playerApplyEffects ? 'flex' : 'none';
            const albumArtSizeField = document.getElementById('album-art-size-field');
            if (albumArtSizeField) {
                albumArtSizeField.style.display = (showAlbumArt && !playerApplyEffects) ? 'flex' : 'none';
            }

            syncPlayerStyleControls();
            scheduleAlbumArtSizeUpdate();
            updateVisualizerCanvasOpacity();
            updatePlayerTextBlendLayer();
        }

        let playerMinimizeHideTimer = null;

        function setPlayerMinimizeVisible(isVisible) {
            if (!container || !minimizePlayerBtn) {
                return;
            }
            if (playerMinimizeHideTimer !== null) {
                window.clearTimeout(playerMinimizeHideTimer);
                playerMinimizeHideTimer = null;
            }
            const shouldShow = isVisible && !playerMinimized;
            container.classList.toggle('player-minimize-visible', shouldShow);
            if (shouldShow) {
                minimizePlayerBtn.removeAttribute('tabindex');
                minimizePlayerBtn.removeAttribute('aria-hidden');
            } else {
                minimizePlayerBtn.setAttribute('tabindex', '-1');
                minimizePlayerBtn.setAttribute('aria-hidden', 'true');
            }
        }

        function schedulePlayerMinimizeHidden() {
            if (playerMinimizeHideTimer !== null) {
                window.clearTimeout(playerMinimizeHideTimer);
            }
            playerMinimizeHideTimer = window.setTimeout(() => {
                setPlayerMinimizeVisible(false);
            }, 220);
        }

        const LYRIC_FONT_MAP = {
            vcrOsdMono: "'VCR OSD Mono', monospace",
            pirataOne: "'Pirata One', serif",
            workbench: "'Workbench', sans-serif",
            rubikBubbles: "'Rubik Bubbles', cursive",
            jollyLodger: "'Jolly Lodger', cursive",
            creepster: "'Creepster', cursive",
            jersey25Charted: "'Jersey 25 Charted', monospace",
            sixtyfour: "'Sixtyfour', monospace",
            yuyuShort: "'Yuyu Short', sans-serif",
            linefont: "'Linefont', monospace",
            nabla: "'Nabla', sans-serif",
            outfit: "'Outfit', sans-serif",
            inter: "'Inter', sans-serif",
        };

        const loadedGoogleFonts = new Set();
        const validGoogleFonts = new Set();
        const googleFontValidationPromises = new Map();

        function normalizeLyricFontName(fontName) {
            if (typeof fontName !== 'string') {
                return '';
            }
            return fontName.trim().replace(/\s+/g, ' ');
        }

        function isCustomLyricFontName(fontName) {
            const normalized = normalizeLyricFontName(fontName);
            if (!normalized || normalized.toLowerCase() === 'custom') {
                return false;
            }

            const cleanInput = normalized.replace(/[^a-zA-Z0-9]/g, '').toLowerCase();

            // 1. Check against keys of LYRIC_FONT_MAP (e.g. 'vcrOsdMono')
            const matchKey = Object.keys(LYRIC_FONT_MAP).some((key) => {
                return key.replace(/[^a-zA-Z0-9]/g, '').toLowerCase() === cleanInput;
            });
            if (matchKey) {
                return false;
            }

            // 2. Check against the display family names inside the map values (e.g. 'VCR OSD Mono')
            const matchValue = Object.values(LYRIC_FONT_MAP).some((val) => {
                const parts = val.split(',');
                if (parts.length > 0) {
                    const family = parts[0].replace(/['"]/g, '').trim();
                    return family.replace(/[^a-zA-Z0-9]/g, '').toLowerCase() === cleanInput;
                }
                return false;
            });
            if (matchValue) {
                return false;
            }

            return true;
        }

        function getGoogleFontCssUrl(fontName, withVariableWeight = true) {
            const apiName = normalizeLyricFontName(fontName).replace(/\s+/g, '+');
            const weightPart = withVariableWeight ? ':wght@100..900' : '';
            return `https://fonts.googleapis.com/css2?family=${apiName}${weightPart}&display=swap`;
        }

        async function googleFontCssExists(fontName, withVariableWeight = true) {
            try {
                const response = await fetch(getGoogleFontCssUrl(fontName, withVariableWeight), {
                    cache: 'no-store',
                });
                if (!response.ok) {
                    return false;
                }
                const cssText = await response.text();
                return cssText.includes('@font-face') && cssText.toLowerCase().includes('font-family');
            } catch (error) {
                return false;
            }
        }

        async function validateGoogleFont(fontName) {
            const normalized = normalizeLyricFontName(fontName);
            if (!isCustomLyricFontName(normalized)) {
                return !!normalized;
            }
            if (validGoogleFonts.has(normalized)) {
                return true;
            }
            if (googleFontValidationPromises.has(normalized)) {
                return googleFontValidationPromises.get(normalized);
            }

            const validationPromise = (async () => {
                try {
                    const hasVariableCss = await googleFontCssExists(normalized, true);
                    const hasStandardCss = hasVariableCss || await googleFontCssExists(normalized, false);
                    if (hasStandardCss) {
                        validGoogleFonts.add(normalized);
                        return true;
                    }
                } catch (error) {
                    console.warn('[Fonts] Google Fonts validation failed:', error);
                } finally {
                    googleFontValidationPromises.delete(normalized);
                }
                return false;
            })();

            googleFontValidationPromises.set(normalized, validationPromise);
            return validationPromise;
        }

        function loadGoogleFont(fontName) {
            if (!fontName) return;
            const normalized = normalizeLyricFontName(fontName);
            if (!normalized) return;
            if (loadedGoogleFonts.has(normalized)) return;
            loadedGoogleFonts.add(normalized);
            
            const linkId = 'google-font-' + normalized.toLowerCase().replace(/[^a-z0-9]/g, '-');
            if (document.getElementById(linkId)) return;
            
            const linkVar = document.createElement('link');
            linkVar.id = linkId + '-var';
            linkVar.rel = 'stylesheet';
            linkVar.href = getGoogleFontCssUrl(normalized, true);
            document.head.appendChild(linkVar);
            
            const linkStd = document.createElement('link');
            linkStd.id = linkId + '-std';
            linkStd.rel = 'stylesheet';
            linkStd.href = getGoogleFontCssUrl(normalized, false);
            document.head.appendChild(linkStd);
        }

        function ensureCustomFontOption(selectEl, fontName) {
            if (!selectEl) {
                return normalizeLyricFontName(fontName);
            }

            const normalized = normalizeLyricFontName(fontName);
            if (!normalized) {
                return '';
            }

            const existing = Array.from(selectEl.options).find((opt) => (
                opt.value.toLowerCase() === normalized.toLowerCase()
            ));
            if (existing) {
                return existing.value;
            }

            const opt = document.createElement('option');
            opt.value = normalized;
            opt.textContent = normalized;
            selectEl.appendChild(opt);
            return normalized;
        }

        function ensureCustomLyricFontOption(fontName) {
            return ensureCustomFontOption(lyricsFontSelect, fontName);
        }

        function ensureCustomPlayerFontOption(fontName) {
            return ensureCustomFontOption(playerFontSelect, fontName);
        }

        function addCustomFont(fontName) {
            const normalized = normalizeLyricFontName(fontName);
            if (!isCustomLyricFontName(normalized)) {
                return normalized;
            }

            const existing = customFonts.find((entry) => (
                entry.toLowerCase() === normalized.toLowerCase()
            ));
            const finalName = existing || normalized;
            if (!existing) {
                customFonts.push(finalName);
            }

            loadGoogleFont(finalName);
            ensureCustomLyricFontOption(finalName);
            ensureCustomPlayerFontOption(finalName);
            return finalName;
        }

        function addCustomLyricFont(fontName) {
            return addCustomFont(fontName);
        }

        function addCustomPlayerFont(fontName) {
            return addCustomFont(fontName);
        }

        async function addValidatedCustomFont(fontName) {
            const normalized = normalizeLyricFontName(fontName);
            if (!normalized) {
                return '';
            }
            if (!isCustomLyricFontName(normalized)) {
                return normalized;
            }

            const isValid = await validateGoogleFont(normalized);
            if (!isValid) {
                return '';
            }

            return addCustomFont(normalized);
        }

        function setCustomFonts(fonts, shouldReset = true) {
            if (shouldReset) {
                customFonts = [];
            }
            if (!Array.isArray(fonts)) {
                return;
            }

            fonts.forEach((fontName) => {
                addCustomFont(fontName);
            });
        }

        function setCustomLyricFonts(fonts) {
            setCustomFonts(fonts, false);
        }

        function setCustomPlayerFonts(fonts) {
            setCustomFonts(fonts, false);
        }

        function syncCustomFontOptions() {
            customFonts.forEach((fontName) => {
                addCustomFont(fontName);
            });
        }

        function getFontFamilyValue(fontName) {
            return LYRIC_FONT_MAP[fontName] || `'${fontName}', sans-serif`;
        }

        function applyLyricsTypography() {
            if (isCustomLyricFontName(lyricFont)) {
                lyricFont = addCustomLyricFont(lyricFont);
            }
            syncCustomFontOptions();

            document.documentElement.style.setProperty(
                '--lyrics-font-family',
                getFontFamilyValue(lyricFont)
            );
            document.documentElement.style.setProperty(
                '--lyrics-font-weight',
                String(lyricsWeight)
            );
            document.documentElement.style.setProperty(
                '--lyrics-size-scale',
                String(lyricTextScale)
            );
            document.documentElement.style.setProperty(
                '--lyrics-wrap-width',
                lyricsWrapWidth + '%'
            );
            document.documentElement.style.setProperty(
                '--lyrics-border-shadow',
                getLyricsBorderShadow(lyricsBorderWidth)
            );
            document.documentElement.style.setProperty(
                '--lyrics-border-width',
                lyricsBorderWidth + 'px'
            );
            document.documentElement.style.setProperty(
                '--lyrics-line-height',
                String(lyricsLineHeight)
            );
            document.documentElement.style.setProperty(
                '--lyrics-word-spacing',
                lyricsWordSpacing + 'px'
            );
            document.documentElement.style.setProperty(
                '--lyrics-letter-spacing',
                lyricsLetterSpacing + 'px'
            );
            lyricsSizeVal.textContent = `${Math.round(lyricTextScale * 100)}%`;
            lyricsFadeVal.textContent = lyricFadeCurve.toFixed(2);

            if (lyricsFontSelect) {
                lyricsFontSelect.value = lyricFont;
            }
            if (lyricsFontCustomInput) {
                lyricsFontCustomInput.style.display = 'block';
            }
            
            if (lyricsWrapSlider) {
                lyricsWrapSlider.value = String(lyricsWrapWidth);
            }
            if (lyricsWrapVal) {
                lyricsWrapVal.textContent = lyricsWrapWidth + '%';
            }
            if (lyricsDynamicStyle) {
                lyricsDynamicStyle.textContent = DEFAULT_CUSTOM_CSS;
            }
        }

        function applyPlayerTypography() {
            if (isCustomLyricFontName(playerFont)) {
                playerFont = addCustomPlayerFont(playerFont);
            }
            syncCustomFontOptions();

            document.documentElement.style.setProperty(
                '--player-font-family',
                getFontFamilyValue(playerFont)
            );

            if (playerFontSelect) {
                playerFontSelect.value = playerFont;
            }
            if (playerFontCustomInput) {
                playerFontCustomInput.style.display = 'block';
            }
        }

        function resetLyricsScrollPosition() {
            if (!scrollPanel) {
                return;
            }

            if (lyricsScrollRestoreTimer) {
                clearTimeout(lyricsScrollRestoreTimer);
                lyricsScrollRestoreTimer = null;
            }

            scrollPanel.style.scrollBehavior = 'auto';
            scrollPanel.scrollTop = 0;

            lyricsScrollRestoreTimer = window.setTimeout(() => {
                scrollPanel.style.scrollBehavior = '';
                lyricsScrollRestoreTimer = null;
            }, LYRICS_SWAP_SCROLL_FREEZE_MS);
        }

        function clearDisplayedLyricsHold() {
            displayedLyricsHoldStartMs = null;
            displayedLyricsHoldAudioTimeSec = 0;
        }

        function clearPendingLyricsTransition() {
            pendingLyricsState = null;
        }

        function beginDisplayedLyricsHold() {
            if (displayedLyricsHoldStartMs !== null) {
                return;
            }

            displayedLyricsHoldStartMs = getInterpolatedPositionMs();
            displayedLyricsHoldAudioTimeSec = getAudioContentPositionSec();
        }

        function getHeldLyricsPositionMs() {
            if (displayedLyricsHoldStartMs === null) {
                return null;
            }

            const heldDeltaMs = Math.max(0, (getAudioContentPositionSec() - displayedLyricsHoldAudioTimeSec) * 1000.0);
            return displayedLyricsHoldStartMs + heldDeltaMs;
        }

        function applyLyricsState(state) {
            clearDisplayedLyricsHold();
            clearPendingLyricsTransition();
            lyricsScrollFreezeUntilMs = performance.now() + LYRICS_SWAP_SCROLL_FREEZE_MS;
            activeLineIndex = -1;
            resetLyricsScrollPosition();

            if (state.type === 'Lyrics') {
                currentLyricsStateType = 'Lyrics';
                currentLyricsTrackId = state.track_id || null;
                renderLyrics(state.lines);
            } else if (state.type === 'NoLyrics') {
                currentLyricsStateType = 'NoLyrics';
                currentLyricsTrackId = null;
                renderPlaceholder("Not Available", "No lyrics available for this track");
            }
            updateSyncStatusIcon();
        }

        function pendingLyricsMatchesCurrentPlaybackTrack() {
            if (!pendingLyricsState || !currentPlaybackTrackId) {
                return false;
            }

            if (pendingLyricsState.type === 'Lyrics') {
                return !!pendingLyricsState.track_id && pendingLyricsState.track_id === currentPlaybackTrackId;
            }

            return currentLyricsTrackId !== null && currentLyricsTrackId !== currentPlaybackTrackId;
        }

        function maybeCommitPendingLyricsTransition() {
            if (!pendingLyricsState) {
                return false;
            }

            if (awaitingPlaybackTrackConfirmation || !pendingLyricsMatchesCurrentPlaybackTrack()) {
                return false;
            }

            applyLyricsState(pendingLyricsState);
            updateLyricsHighlight(true);
            return true;
        }

        function stageLyricsTransition(state) {
            beginDisplayedLyricsHold();
            pendingLyricsState = state;
        }

        function handleIncomingLyricsState(state) {
            if (
                state.type === 'Lyrics' &&
                state.track_id &&
                currentPlaybackTrackId &&
                state.track_id !== currentPlaybackTrackId
            ) {
                stageLyricsTransition(state);
                return;
            }

            if (currentLyricsStateType === null) {
                applyLyricsState(state);
                updateLyricsHighlight(true);
                return;
            }

            const isDifferentTrackLyrics =
                state.type === 'Lyrics' &&
                state.track_id &&
                state.track_id !== currentLyricsTrackId;
            const isDifferentLyricsStateType =
                state.type === 'NoLyrics' &&
                currentLyricsStateType !== 'NoLyrics';
            const shouldStageTransition =
                isAutosync &&
                currentLyricsStateType !== null &&
                (isDifferentTrackLyrics || isDifferentLyricsStateType);

            if (shouldStageTransition) {
                stageLyricsTransition(state);
                maybeCommitPendingLyricsTransition();
                return;
            }

            if (
                state.type === 'NoLyrics' &&
                currentLyricsTrackId &&
                currentPlaybackTrackId &&
                currentLyricsTrackId !== currentPlaybackTrackId
            ) {
                stageLyricsTransition(state);
                return;
            }

            applyLyricsState(state);
            updateLyricsHighlight(true);
        }

        function applyNowPlayingState(track, authoritativeTrackChange = true) {
            const previousPlaybackStatus = lastTrackData ? lastTrackData.status : null;
            const wasIdle = previousPlaybackStatus === 'idle';
            lastTrackData = track;
            if (track.track_name) {
                songTitle.textContent = track.track_name;
                songArtists.textContent = Array.isArray(track.artists) ? track.artists.join(', ') : '';
            } else if (track.status === 'idle') {
                songTitle.textContent = 'Idle';
                songArtists.textContent = 'No song playing';
            }

            listenerCount.textContent = track.listeners || 0;
            currentTrackDurationMs = Number.isFinite(track.track_duration_ms) ? track.track_duration_ms : null;
            currentTrackPositionMs = Number.isFinite(track.position_ms) ? track.position_ms : 0;
            updateTrackProgressDisplay();
            scheduleAlbumArtSizeUpdate();

            if (track.cover_url && hasMusicBeenPlayed) {
                albumCover.src = track.cover_url;
                albumCover.style.display = 'block';
                albumPlaceholder.style.display = 'none';
            } else {
                albumCover.src = '';
                albumCover.style.display = 'none';
                albumPlaceholder.style.display = 'flex';
            }

            if (track.track_id) {
                currentTrackId = track.track_id;
                titleContainer.classList.remove('no-link');
            } else {
                currentTrackId = null;
                titleContainer.classList.add('no-link');
                hideTitleTooltip();
            }

            if ('mediaSession' in navigator && window.MediaMetadata) {
                const metadata = {
                    title: track.track_name || 'Spotifm Radio',
                    artist: Array.isArray(track.artists) ? track.artists.join(', ') : '',
                    album: track.album_name || '',
                };
                if (track.cover_url) {
                    metadata.artwork = [{ src: track.cover_url }];
                }
                navigator.mediaSession.metadata = new MediaMetadata(metadata);
            }

            if (track.status === 'playing') {
                if (previousPlaybackStatus !== 'playing') {
                    audioLastProgressAt = performance.now();
                    wasmLastChunkAt = performance.now();
                }
                setActivePlaylistHasTracks(true);
            } else if (track.status === 'idle' && !wasIdle) {
                setPlayState('paused');
                void refreshActivePlaylistState();
            }

            if (authoritativeTrackChange) {
                const nextTrackId = track.track_id || null;
                const previousTrackId = currentPlaybackTrackId;
                currentPlaybackTrackId = nextTrackId;

                if (nextTrackId && nextTrackId !== previousTrackId) {
                    if (isAutosync && currentLyricsStateType !== null) {
                        beginDisplayedLyricsHold();
                    }
                    currentTrackStartGranuleSec = null;
                    localGranuleClockMode = null;
                    if (previousTrackId === null && confirmedPlaybackTrackId === null) {
                        confirmedPlaybackTrackId = nextTrackId;
                        awaitingPlaybackTrackConfirmation = false;
                    } else if (wasmPlaybackEnabled && performance.now() - wasmLastTrackBoundaryAt < 1500) {
                        confirmedPlaybackTrackId = nextTrackId;
                        awaitingPlaybackTrackConfirmation = false;
                    } else {
                        awaitingPlaybackTrackConfirmation = true;
                    }
                    maybeCommitPendingLyricsTransition();
                    if (visualizerEnabled && milkdropCycleOnSongChange) {
                        selectRandomMilkdropPreset(milkdropBlendSeconds, true);
                    }
                } else if (!nextTrackId) {
                    awaitingPlaybackTrackConfirmation = false;
                }
            }

            if (isPlaying) {
                //showMilkdropTrackTitleOnce();
            }
            updateActivePlaylistCurrentTrack();
            updateSyncStatusIcon();
        }

        function updateTrackProgressDisplay() {
            if (!trackProgressFill) {
                return;
            }

            if (!Number.isFinite(currentTrackDurationMs) || currentTrackDurationMs === null || currentTrackDurationMs <= 0) {
                trackProgressFill.style.width = '0%';
                return;
            }

            const heardPositionMs = isPlaying
                ? getInterpolatedPositionMs()
                : currentTrackPositionMs;
            const clampedPositionMs = Math.max(0, Math.min(currentTrackDurationMs, heardPositionMs));
            const progressRatio = currentTrackDurationMs > 0
                ? clampedPositionMs / currentTrackDurationMs
                : 0;

            trackProgressFill.style.width = `${Math.max(0, Math.min(1, progressRatio)) * 100}%`;
        }

        function updateAutoSyncStatus() {
            if (!isAutosync) {
                syncStatus.textContent = 'Manual Delay';
                return;
            }

            if (wasmPlaybackEnabled) {
                syncStatus.textContent = wasmClockReady ? 'Lyrics Synced' : 'Syncing';
                return;
            }

            if (runtimeConfig.streamIsOgg && streamFirstGranuleSec === null) {
                syncStatus.textContent = 'Syncing';
                return;
            }

            syncStatus.textContent = lastSyncInstant === 0 ? 'Syncing' : 'Lyrics Synced';
        }

        function getWebSocketPositionSec() {
            if (lastSyncInstant === 0) {
                return null;
            }

            const elapsedSinceSync = (performance.now() - lastSyncInstant) / 1000.0;
            return lastServerTime + elapsedSinceSync;
        }

        function getLocalGranulePositionSec() {
            if (
                wasmPlaybackEnabled ||
                !isAutosync ||
                currentTrackStartGranuleSec === null ||
                streamFirstGranuleSec === null ||
                !Number.isFinite(audio.currentTime) ||
                audio.currentTime <= 0
            ) {
                return null;
            }

            const durationSec = Number.isFinite(currentTrackDurationMs)
                ? currentTrackDurationMs / 1000.0
                : null;
            const isPlausiblePosition = (positionSec) => (
                Number.isFinite(positionSec) &&
                positionSec >= -0.25 &&
                (durationSec === null || positionSec <= durationSec + 30.0)
            );
            // Chromium can expose the absolute Ogg granule timeline while other playback paths
            // normalize currentTime to the start of this HTTP response. Compare both mappings to
            // the listener-specific server position instead of assuming one browser convention.
            const candidates = [
                {
                    mode: 'absolute-granule',
                    positionSec: audio.currentTime - currentTrackStartGranuleSec,
                },
                {
                    mode: 'server-origin',
                    positionSec: streamFirstGranuleSec + audio.currentTime - currentTrackStartGranuleSec,
                },
            ].filter((candidate) => isPlausiblePosition(candidate.positionSec));
            if (candidates.length === 0) {
                return null;
            }

            const wsPositionSec = getWebSocketPositionSec();
            const selected = Number.isFinite(wsPositionSec)
                ? candidates.reduce((best, candidate) => (
                    Math.abs(candidate.positionSec - wsPositionSec) <
                    Math.abs(best.positionSec - wsPositionSec)
                        ? candidate
                        : best
                ))
                : candidates.find((candidate) => candidate.mode === 'server-origin') || candidates[0];

            localGranuleClockMode = selected.mode;
            return Math.max(0, selected.positionSec);
        }

        function getAudioContentPositionSec() {
            if (wasmPlaybackEnabled) {
                return wasmPlaybackPositionSec;
            }
            return audio.currentTime || 0;
        }

        function handleWasmTrackBoundary() {
            wasmLastTrackBoundaryAt = performance.now();
            wasmPlaybackPositionSec = 0;
            currentTrackPositionMs = 0;
            lastPositionTrackMs = null;
            currentTrackStartGranuleSec = 0;
            localGranuleClockMode = 'wasm-worklet';

            if (currentPlaybackTrackId) {
                confirmedPlaybackTrackId = currentPlaybackTrackId;
                awaitingPlaybackTrackConfirmation = false;
            }

            clearDisplayedLyricsHold();
            maybeCommitPendingLyricsTransition();
            updateLyricsHighlight(true);
            updateTrackProgressDisplay();
        }

        function handleWasmAudioMessage(event) {
            const message = event.data || {};

            if (message.type === 'position') {
                if (Number.isFinite(message.seconds)) {
                    wasmPlaybackPositionSec = Math.max(0, message.seconds);
                    wasmClockReady = true;
                    currentTrackPositionMs = Math.round(wasmPlaybackPositionSec * 1000);
                    lastSyncInstant = performance.now();
                    lastWsPositionAt = lastWsPositionAt || lastSyncInstant;
                }

                if (Number.isFinite(message.bufferedSeconds)) {
                    wasmBufferedSeconds = Math.max(0, message.bufferedSeconds);
                }

                if (message.boundary) {
                    handleWasmTrackBoundary();
                } else {
                    updateAutoSyncStatus();
                    updateLyricsHighlight();
                    updateTrackProgressDisplay();
                }
            } else if (message.type === 'track-boundary') {
                handleWasmTrackBoundary();
            }
        }

        async function ensureWasmAudioOutput() {
            if (!wasmPlaybackEnabled) {
                throw new Error('WASM playback is not enabled for this stream.');
            }

            initAudioContext(false);
            if (!audioCtx || !audioCtx.audioWorklet) {
                throw new Error('AudioWorklet is not available in this browser.');
            }

            if (!wasmAudioWorkletReady) {
                const workletBase64 = "__SPOTIFM_AUDIO_WORKLET_BASE64__";
                const binary = atob(workletBase64);
                const bytes = new Uint8Array(binary.length);
                for (let i = 0; i < binary.length; i++) {
                    bytes[i] = binary.charCodeAt(i);
                }
                const workletCode = new TextDecoder('utf-8').decode(bytes);
                const blob = new Blob([workletCode], { type: 'application/javascript' });
                const workletUrl = URL.createObjectURL(blob);
                await audioCtx.audioWorklet.addModule(workletUrl);
                URL.revokeObjectURL(workletUrl);
                wasmAudioWorkletReady = true;
            }

            if (!wasmAudioNode) {
                wasmAudioNode = new AudioWorkletNode(audioCtx, 'spotifm-audio-output', {
                    numberOfInputs: 0,
                    numberOfOutputs: 1,
                    outputChannelCount: [2],
                });
                wasmAudioNode.port.onmessage = handleWasmAudioMessage;
                wasmAudioNode.onprocessorerror = (event) => {
                    console.error('[WASM Audio] AudioWorklet processor failed:', event);
                    stopWasmStream(true);
                    detachAnalysisSource();
                    try {
                        wasmAudioNode.disconnect();
                    } catch (error) {
                        console.warn('[WASM Audio] Failed to disconnect crashed AudioWorklet:', error);
                    }
                    wasmAudioNode = null;
                    scheduleAudioReconnect('WASM audio output failed');
                };
                postWasmVolume(isVolumeMuted ? 0 : getVolumeSliderValue());
            }

            bindAnalysisSource(true);
        }

        async function startWasmPlayback() {
            playbackRequested = true;
            setPlaybackGestureRequired(false);
            setPlayState('loading');
            await ensureWasmAudioOutput();
            await ensureWasmDecoder();

            if (!await resumeAudioContextWithTimeout()) {
                playbackRequested = false;
                setPlaybackGestureRequired(true);
                throw new DOMException('AudioContext requires a user gesture', 'NotAllowedError');
            }

            startWasmStream();
            setPlayState('playing');
            updateAutoSyncStatus();
            requestNowPlayingSnapshotSoon(250, 'wasm playback started');
        }

        async function pauseWasmPlayback() {
            playbackRequested = false;
            setPlaybackGestureRequired(false);
            stopAudioRecovery();
            stopWasmStream(true);
            if (audioCtx && audioCtx.state === 'running') {
                await audioCtx.suspend();
            }
            setPlayState('paused');
            updateAutoSyncStatus();
        }

        // ==========================================
        // 1. Play / Pause Control Logic
        // ==========================================

        async function autoStartWasmPlayback() {
            if (!wasmPlaybackEnabled || isPlaying) {
                return;
            }

            try {
                await startWasmPlayback();
            } catch (error) {
                console.warn('[WASM Audio] Automatic playback failed:', error);
                playbackRequested = false;
                stopAudioRecovery();
                setPlaybackGestureRequired(true);
                setPlayState('paused');
            }
        }

        window.addEventListener('load', () => {
            void autoStartWasmPlayback();
        }, { once: true });

        function refreshVisualizerAudioBinding() {
            if (!playbackRequested) {
                return;
            }
            initAudioContext(true, true);
            void resumeAudioContextWithTimeout().then((ready) => {
                if (ready) {
                    initializeAudioAnalysisGraph(true);
                    syncMilkdropAnimationState();
                }
            });
        }

        async function requestAudiblePlayback(markPlaying = true) {
            if (wasmPlaybackEnabled) {
                try {
                    if (!playbackRequested || !isPlaying) {
                        await startWasmPlayback();
                    } else {
                        await resumeAudioContext();
                    }
                    return true;
                } catch (error) {
                    console.error('[WASM Audio] Could not start audible playback:', error);
                    playbackRequested = false;
                    if (error && error.name === 'NotAllowedError') {
                        setPlaybackGestureRequired(true);
                    }
                    setPlayState('paused');
                    return false;
                }
            }

            if (wasmPlaybackRequired) {
                return false;
            }

            playbackRequested = true;
            mutedAutoplayPriming = false;
            setPlaybackGestureRequired(false);
            clearAudioReconnectTimer();
            audioReconnectAttempts = 0;
            applyEffectiveVolume();
            setPlayState('loading');
            audioContextActivationPending = true;

            // This must happen synchronously in the click handler so browsers
            // grant the AudioContext and media element their user activation.
            initAudioContext(true, true);
            const resumePromise = resumeAudioContextWithTimeout(1200);

            try {
                if (audio.paused) {
                    await audio.play();
                }
                const contextReady = await resumePromise;
                if (!contextReady) {
                    throw new DOMException('AudioContext requires a user gesture', 'NotAllowedError');
                }
                initializeAudioAnalysisGraph(true);
                mutedAutoplayPriming = false;
                applyEffectiveVolume();
                if (markPlaying && !audio.paused) {
                    setPlayState('playing');
                }
                return !audio.paused;
            } catch (error) {
                console.error('[Audio] Could not start audible playback:', error);
                if (error && error.name === 'NotAllowedError') {
                    playbackRequested = false;
                    mutedAutoplayPriming = !audio.paused;
                    applyEffectiveVolume();
                    setPlaybackGestureRequired(true);
                    setPlayState('paused');
                } else {
                    scheduleAudioReconnect('Audible playback failed');
                }
                return false;
            } finally {
                audioContextActivationPending = false;
            }
        }

        function finalizeRemotePlaybackActivation(activationPromise) {
            void Promise.resolve(activationPromise).then((activated) => {
                if (!activated || !playbackRequested) {
                    return;
                }
                refreshVisualizerAudioBinding();
                if (!wasmPlaybackEnabled && !audio.paused) {
                    setPlayState('playing');
                }
                [250, 1000].forEach((delay) => {
                    window.setTimeout(refreshVisualizerAudioBinding, delay);
                });
            });
        }

        async function playPlayerPlayback() {
            if (wasmPlaybackEnabled) {
                if (!playbackRequested || !isPlaying) {
                    await startWasmPlayback();
                } else {
                    await resumeAudioContext();
                }
                return;
            }

            if (wasmPlaybackRequired) {
                syncStatus.textContent = 'WASM Required';
                showPlayerNotice("This passthrough stream requires the Ogg/Vorbis WASM decoder and AudioWorklet support.", 'error');
                return;
            }

            if (mutedAutoplayPriming && !audio.paused) {
                playbackRequested = true;
                setPlaybackGestureRequired(false);
                initAudioContext(true, true);
                const contextReady = await resumeAudioContextWithTimeout(1200);
                if (!contextReady) {
                    playbackRequested = false;
                    setPlaybackGestureRequired(true);
                    setPlayState('paused');
                    return;
                }
                initializeAudioAnalysisGraph(true);
                mutedAutoplayPriming = false;
                applyEffectiveVolume();
                setPlayState('playing');
                return;
            }

            await requestAudiblePlayback(true);
        }

        async function pausePlayerPlayback() {
            if (wasmPlaybackEnabled) {
                if (playbackRequested || isPlaying) {
                    await pauseWasmPlayback();
                }
                return;
            }

            playbackRequested = false;
            mutedAutoplayPriming = false;
            setPlaybackGestureRequired(false);
            stopAudioRecovery();
            audio.pause();
            setPlayState('paused');
        }

        async function togglePlayerPlayback() {
            if (playbackRequested && (isPlaying || playState === 'loading')) {
                await pausePlayerPlayback();
            } else {
                await playPlayerPlayback();
            }
        }

        function runPlayerPlaybackAction(action) {
            void action().catch((error) => {
                console.error('[MediaControls] Playback action failed:', error);
            });
        }

        let remoteTrackChangePromise = null;

        function requestRemoteTrackChange(step) {
            if (remoteTrackChangePromise) {
                return remoteTrackChangePromise;
            }

            const path = step < 0 ? `/skip/${step}` : '/skip';
            remoteTrackChangePromise = (async () => {
                const response = await fetch(buildAuthenticatedUrl(path), {
                    cache: 'no-store',
                });
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}`);
                }
                const nowPlaying = await response.json();
                applyNowPlayingState(nowPlaying, true);
                return nowPlaying;
            })()
                .catch((error) => {
                    console.error(`[MediaControls] Could not change track by ${step}:`, error);
                    return null;
                })
                .finally(() => {
                    remoteTrackChangePromise = null;
                });

            return remoteTrackChangePromise;
        }

        function installMediaSessionControls() {
            if (!('mediaSession' in navigator)) {
                return;
            }

            const handlers = {
                previoustrack: () => { void requestRemoteTrackChange(-1); },
                play: () => { runPlayerPlaybackAction(playPlayerPlayback); },
                pause: () => { runPlayerPlaybackAction(pausePlayerPlayback); },
                nexttrack: () => { void requestRemoteTrackChange(1); },
            };

            Object.entries(handlers).forEach(([action, handler]) => {
                try {
                    navigator.mediaSession.setActionHandler(action, handler);
                } catch (error) {
                    console.warn(`[MediaControls] ${action} is not supported:`, error);
                }
            });
        }

        window.addEventListener('keydown', (event) => {
            if (event.repeat) {
                return;
            }

            if (event.key === 'F5' || event.key === 'MediaTrackPrevious') {
                event.preventDefault();
                void requestRemoteTrackChange(-1);
            } else if (event.key === 'F6' || event.key === 'MediaPlayPause') {
                event.preventDefault();
                runPlayerPlaybackAction(togglePlayerPlayback);
            } else if (event.key === 'F7' || event.key === 'MediaTrackNext') {
                event.preventDefault();
                void requestRemoteTrackChange(1);
            }
        });

        installMediaSessionControls();

        playBtn.addEventListener('click', () => {
            runPlayerPlaybackAction(togglePlayerPlayback);
        });

        if (idlePlayPrompt) {
            idlePlayPrompt.addEventListener('click', () => {
                runPlayerPlaybackAction(playPlayerPlayback);
            });
        }

        function updateIdlePlayerUi() {
            if (idlePlayPrompt) {
                idlePlayPrompt.hidden = !(
                    playbackGestureRequired &&
                    activePlaylistHasTracks === true
                );
            }

            if (
                !isPlaying &&
                activePlaylistHasTracks === false &&
                !emptyPlaylistSearchOpened
            ) {
                emptyPlaylistSearchOpened = true;
                openSearchModal();
            }
        }

        function setPlaybackGestureRequired(required) {
            playbackGestureRequired = !!required;
            updateIdlePlayerUi();
        }

        function setActivePlaylistHasTracks(hasTracks) {
            activePlaylistHasTracks = hasTracks;
            if (hasTracks) {
                emptyPlaylistSearchOpened = false;
            }
            updateIdlePlayerUi();
        }

        function setPlayState(stateVal) {
            // stateVal can be 'loading', 'playing', 'paused'
            playState = stateVal;
            if (stateVal === 'playing') {
                if (!hasMusicBeenPlayed) {
                    hasMusicBeenPlayed = true;
                    if (lastTrackData) {
                        applyNowPlayingState(lastTrackData, false);
                    }
                }
            }

            if (stateVal === 'playing') {
                isPlaying = true;
                playIcon.style.display = 'none';
                loadingIcon.style.display = 'none';
                pauseIcon.style.display = 'block';
                playBtn.classList.add('playing');
                syncMilkdropAnimationState();
                //showMilkdropTrackTitleOnce();
            } else if (stateVal === 'loading') {
                isPlaying = false;
                playIcon.style.display = 'none';
                loadingIcon.style.display = 'block';
                pauseIcon.style.display = 'none';
                playBtn.classList.remove('playing');
                syncMilkdropAnimationState();
            } else { // paused
                isPlaying = false;
                playIcon.style.display = 'block';
                loadingIcon.style.display = 'none';
                pauseIcon.style.display = 'none';
                playBtn.classList.remove('playing');
                syncMilkdropAnimationState();
            }
            if ('mediaSession' in navigator) {
                navigator.mediaSession.playbackState = stateVal === 'paused' ? 'paused' : 'playing';
            }
            updateIdlePlayerUi();
        }

        // Standard HTML5 Audio Media Events for play button buffering/loading synchronization
        audio.addEventListener('loadstart', () => {
            if (!audio.paused && !mutedAutoplayPriming) {
                setPlayState('loading');
            }
        });

        audio.addEventListener('waiting', () => {
            if (!audio.paused && !mutedAutoplayPriming) {
                setPlayState('loading');
            }
        });

        async function synchronizePlayingAudioPipeline() {
            if (audioContextActivationPending || mutedAutoplayPriming) {
                return;
            }

            audioContextActivationPending = true;
            try {
                // Do not attach MediaElementSource while the context is
                // suspended: doing so reroutes otherwise audible media through
                // a silent graph on origins where autoplay is restricted.
                initAudioContext(false, true);
                if (audioCtx && audioCtx.state !== 'running') {
                    mutedAutoplayPriming = true;
                    applyEffectiveVolume();
                }

                const contextReady = await resumeAudioContextWithTimeout();
                if (!contextReady) {
                    playbackRequested = false;
                    mutedAutoplayPriming = true;
                    applyEffectiveVolume();
                    setPlaybackGestureRequired(true);
                    setPlayState('paused');
                    console.info('[Audio] Waiting for a user gesture to start audio and visualizations.');
                    return;
                }

                playbackRequested = true;
                mutedAutoplayPriming = false;
                setPlaybackGestureRequired(false);
                applyEffectiveVolume();
                setPlayState('playing');
            } finally {
                audioContextActivationPending = false;
            }
        }

        audio.addEventListener('playing', () => {
            markAudioStreamHealthy();
            if (!wantsAudioStreamConnection()) {
                audio.pause();
                setPlayState('paused');
                return;
            }
            if (mutedAutoplayPriming) {
                setPlayState('paused');
                return;
            }
            void synchronizePlayingAudioPipeline();
        });

        audio.addEventListener('pause', () => {
            if (audioReconnectInProgress) {
                return;
            }
            mutedAutoplayPriming = false;
            if (playbackRequested) {
                setPlayState('loading');
                scheduleAudioReconnect('Audio element paused unexpectedly');
            } else {
                setPlayState('paused');
            }
        });

        audio.addEventListener('timeupdate', () => {
            const currentTime = Number(audio.currentTime);
            if (Number.isFinite(currentTime)) {
                if (Math.abs(currentTime - audioLastCurrentTime) >= 0.05) {
                    markAudioStreamHealthy();
                }
                audioLastCurrentTime = currentTime;
            } else {
                markAudioStreamHealthy();
            }

            // Some browsers do not emit a second `playing` event after a short
            // buffering period. If media time is advancing, audio is playing
            // and the visualizer/render loop should be running too.
            if (
                playbackRequested &&
                !mutedAutoplayPriming &&
                !playbackGestureRequired &&
                !audioContextActivationPending &&
                audioCtx &&
                audioCtx.state === 'running' &&
                !audio.paused &&
                playState !== 'playing'
            ) {
                setPlayState('playing');
            }
        });

        audio.addEventListener('error', () => {
            const errorCode = audio.error ? audio.error.code : 'unknown';
            scheduleAudioReconnect(`Audio element reported media error ${errorCode}`);
        });

        audio.addEventListener('ended', () => {
            scheduleAudioReconnect('Audio stream ended');
        });

        audio.addEventListener('stalled', () => {
            if (wantsAudioStreamConnection() && !mutedAutoplayPriming) {
                setPlayState('loading');
            }
        });

        audio.addEventListener('abort', () => {
            if (!audioReconnectInProgress) {
                scheduleAudioReconnect('Audio stream request was aborted');
            }
        });

        if (volumeToggleBtn) {
            volumeToggleBtn.addEventListener('click', () => {
                if (!isVolumeMuted && getVolumeSliderValue() > 0) {
                    lastAudibleVolume = getVolumeSliderValue();
                }
                setVolumeMuted(!isVolumeMuted);
            });
        }

        if (volumeControl) {
            volumeControl.addEventListener('input', (e) => {
                const nextVolume = Math.max(0, Math.min(1, parseFloat(e.target.value)));
                if (Number.isFinite(nextVolume) && nextVolume > 0) {
                    lastAudibleVolume = nextVolume;
                    isVolumeMuted = false;
                } else {
                    isVolumeMuted = true;
                }
                applyEffectiveVolume();
            });
            if (getVolumeSliderValue() > 0) {
                lastAudibleVolume = getVolumeSliderValue();
            }
        }


        titleContainer.addEventListener('mouseenter', (e) => {
            lastTitleTooltipClientX = e.clientX;
            lastTitleTooltipClientY = e.clientY;
            showTitleTooltip(e.clientX, e.clientY);
        });

        titleContainer.addEventListener('mousemove', (e) => {
            lastTitleTooltipClientX = e.clientX;
            lastTitleTooltipClientY = e.clientY;
            showTitleTooltip(e.clientX, e.clientY);
        });

        titleContainer.addEventListener('mouseleave', () => {
            hideTitleTooltip();
        });

        titleContainer.addEventListener('click', () => {
            if (!currentTrackId) return;
            const spotifyUrl = `https://open.spotify.com/track/${currentTrackId}`;
            navigator.clipboard.writeText(spotifyUrl)
                .then(() => {
                    titleTooltip.textContent = 'Copied!';
                    titleTooltip.style.borderColor = '#22c55e'; // Green feedback
                    titleTooltip.style.color = '#22c55e';
                    showTitleTooltip();
                    setTimeout(() => {
                        titleTooltip.textContent = 'Click to copy link';
                        titleTooltip.style.borderColor = 'rgba(255, 255, 255, 0.15)';
                        titleTooltip.style.color = '#fff';
                    }, 2000);
                })
                .catch(err => {
                    console.error('Failed to copy link to clipboard:', err);
                });
        });

        // UI Settings Control Listeners
        lyricsSyncModeButtons.forEach((button) => {
            button.addEventListener('click', () => {
                setLyricsSyncMode(button.dataset.lyricsSyncMode);
            });
        });

        if (autosyncToggle) {
            autosyncToggle.addEventListener('change', (e) => {
                setLyricsSyncMode(e.target.checked ? LYRICS_SYNC_MODE_AUTO : LYRICS_SYNC_MODE_MANUAL);
            });
        }

        delaySlider.addEventListener('input', (e) => {
            manualDelaySec = parseFloat(e.target.value);
            delayVal.textContent = manualDelaySec.toFixed(1) + 's';
            updateLyricsHighlight(true);
            saveSettings();
        });

        autoSyncOffsetSlider.addEventListener('input', (e) => {
            autoSyncLyricsOffsetSec = parseFloat(e.target.value);
            autoSyncOffsetVal.textContent = formatSignedSeconds(autoSyncLyricsOffsetSec);
            updateLyricsHighlight(true);
            saveSettings();
        });

        if (showLyricsToggle) {
            showLyricsToggle.addEventListener('change', (e) => {
                setLyricsSyncMode(e.target.checked
                    ? (isAutosync ? LYRICS_SYNC_MODE_AUTO : LYRICS_SYNC_MODE_MANUAL)
                    : LYRICS_SYNC_MODE_DISABLED);
            });
        }

        lyricsSizeSlider.addEventListener('input', (e) => {
            lyricTextScale = parseFloat(e.target.value);
            applyLyricsTypography();
            updateLyricsHighlight(true);
            saveSettings();
        });

        lyricsFadeSlider.addEventListener('input', (e) => {
            lyricFadeCurve = parseFloat(e.target.value);
            lyricsFadeVal.textContent = lyricFadeCurve.toFixed(2);
            updateLyricsHighlight(true);
            saveSettings();
        });

        playerAlignButtons.forEach((button) => {
            button.addEventListener('click', () => {
                playerAlign = normalizePlayerAlign(button.dataset.playerAlign);
                applyPlayerStyleSettings();
                saveSettings();
            });
        });

        if (showTrackInfoToggle) {
            showTrackInfoToggle.addEventListener('change', (e) => {
                showTrackInfo = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerTitleColorPicker) {
            playerTitleColorPicker.addEventListener('input', (e) => {
                playerTitleColor = normalizeHexColor(e.target.value, playerTitleColor);
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerArtistColorPicker) {
            playerArtistColorPicker.addEventListener('input', (e) => {
                playerArtistColor = normalizeHexColor(e.target.value, playerArtistColor);
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerTitleFontSizeSlider) {
            playerTitleFontSizeSlider.addEventListener('input', (e) => {
                const parsed = parseInt(e.target.value, 10);
                playerTitleFontSize = Math.round(clampNumber(parsed, 8, 72, playerTitleFontSize));
                if (playerTitleFontSizeVal) {
                    playerTitleFontSizeVal.textContent = playerTitleFontSize + 'px';
                }
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerArtistFontSizeSlider) {
            playerArtistFontSizeSlider.addEventListener('input', (e) => {
                const parsed = parseInt(e.target.value, 10);
                playerArtistFontSize = Math.round(clampNumber(parsed, 8, 72, playerArtistFontSize));
                if (playerArtistFontSizeVal) {
                    playerArtistFontSizeVal.textContent = playerArtistFontSize + 'px';
                }
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerTextGapSlider) {
            playerTextGapSlider.addEventListener('input', (e) => {
                const parsed = parseInt(e.target.value, 10);
                playerTextGap = Math.round(clampNumber(parsed, -16, 32, playerTextGap));
                if (playerTextGapVal) {
                    playerTextGapVal.textContent = playerTextGap + 'px';
                }
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerApplyEffectsToggle) {
            playerApplyEffectsToggle.addEventListener('change', (e) => {
                const nextVal = e.target.checked;
                if (nextVal && !playerApplyEffects) {
                    playerTextBlendMode = 'exclusion';
                }
                playerApplyEffects = nextVal;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerTextHighlightToggle) {
            playerTextHighlightToggle.addEventListener('change', (e) => {
                playerTextHighlight = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerTextBlendSelect) {
            playerTextBlendSelect.addEventListener('change', (e) => {
                playerTextBlendMode = normalizePlayerTextBlendMode(e.target.value);
                applyPlayerStyleSettings();
                saveSettings();
            });
        }


        if (showAlbumArtToggle) {
            showAlbumArtToggle.addEventListener('change', (e) => {
                showAlbumArt = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (showProgressBarToggle) {
            showProgressBarToggle.addEventListener('change', (e) => {
                showProgressBar = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (showSyncStatusToggle) {
            showSyncStatusToggle.addEventListener('change', (e) => {
                showSyncStatus = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (showListenerNumberToggle) {
            showListenerNumberToggle.addEventListener('change', (e) => {
                showListenerNumber = e.target.checked;
                applyPlayerStyleSettings();
                saveSettings();
            });
        }

        if (playerModalWidthSlider) {
            playerModalWidthSlider.addEventListener('input', (e) => {
                const parsed = parseInt(e.target.value, 10);
                playerModalWidth = Number.isFinite(parsed)
                    ? Math.max(0, Math.min(1120, parsed))
                    : 1120;
                if (playerModalWidthVal) {
                    playerModalWidthVal.textContent = formatPlayerModalWidth(playerModalWidth);
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerEdgeGapSlider) {
            playerEdgeGapSlider.addEventListener('input', (e) => {
                const parsed = parseInt(e.target.value, 10);
                playerEdgeGap = Number.isFinite(parsed)
                    ? Math.max(0, Math.min(80, parsed))
                    : 20;
                if (playerEdgeGapVal) {
                    playerEdgeGapVal.textContent = Math.round(playerEdgeGap) + 'px';
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerFontSelect) {
            playerFontSelect.addEventListener('change', (e) => {
                playerFont = e.target.value;
                applyPlayerTypography();
                saveSettings();
            });
        }

        if (playerFontCustomInput) {
            const handleCustomPlayerFontSubmit = async () => {
                const customVal = normalizeLyricFontName(playerFontCustomInput.value);
                playerFontCustomInput.value = '';
                if (customVal) {
                    const validatedFont = await addValidatedCustomFont(customVal);
                    if (validatedFont) {
                        playerFont = validatedFont;
                        applyPlayerTypography();
                        saveSettings();
                        playerFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                    } else {
                        console.warn(`[Fonts] "${customVal}" was not found on Google Fonts.`);
                        playerFontCustomInput.placeholder = `"${customVal}" not found`;
                    }
                } else {
                    playerFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                }
            };

            playerFontCustomInput.addEventListener('keydown', (e) => {
                if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleCustomPlayerFontSubmit();
                } else if (e.key === 'Escape') {
                    playerFontCustomInput.value = '';
                    playerFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                    playerFontCustomInput.blur();
                } else if (e.key === 'Tab') {
                    const val = playerFontCustomInput.value.trim().toLowerCase();
                    if (val) {
                        const datalist = document.getElementById('google-fonts-list');
                        if (datalist) {
                            const options = Array.from(datalist.options).map(opt => opt.value);
                            let match = options.find(opt => opt.toLowerCase().startsWith(val));
                            if (!match) {
                                match = options.find(opt => opt.toLowerCase().includes(val));
                            }
                            if (match && match.toLowerCase() !== playerFontCustomInput.value.toLowerCase()) {
                                e.preventDefault();
                                playerFontCustomInput.value = match;
                            }
                        }
                    }
                }
            });

            playerFontCustomInput.addEventListener('blur', () => {
                setTimeout(() => {
                    void handleCustomPlayerFontSubmit();
                }, 150);
            });
        }

        if (liquidGlassToggle) {
            liquidGlassToggle.addEventListener('change', (e) => {
                liquidGlassEnabled = e.target.checked;
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerOpacitySlider) {
            playerOpacitySlider.addEventListener('input', (e) => {
                playerOpacity = parseFloat(e.target.value);
                if (playerOpacityVal) {
                    playerOpacityVal.textContent = Math.round(playerOpacity * 100) + '%';
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerBorderSlider) {
            playerBorderSlider.addEventListener('input', (e) => {
                playerBorderOpacity = parseFloat(e.target.value);
                if (playerBorderVal) {
                    playerBorderVal.textContent = Math.round(playerBorderOpacity * 100) + '%';
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerBlurSlider) {
            playerBlurSlider.addEventListener('input', (e) => {
                playerBlur = parseInt(e.target.value, 10);
                if (playerBlurVal) {
                    playerBlurVal.textContent = playerBlur + 'px';
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (playerScaleSlider) {
            playerScaleSlider.addEventListener('input', (e) => {
                playerScale = parseFloat(e.target.value);
                if (playerScaleVal) {
                    playerScaleVal.textContent = Math.round(playerScale * 100) + '%';
                }
                updateGlassmorphism();
                saveSettings();
            });
        }

        if (minimizePlayerBtn) {
            minimizePlayerBtn.addEventListener('click', () => {
                playerMinimized = true;
                playerPanel.classList.add('minimized');
                setPlayerMinimizeVisible(false);
                if (restorePlayerBtn) {
                    restorePlayerBtn.style.display = 'flex';
                }
                markChromeActivity();
                saveSettings();
            });
        }

        if (playerPanel && minimizePlayerBtn) {
            playerPanel.addEventListener('pointerenter', () => setPlayerMinimizeVisible(true));
            playerPanel.addEventListener('pointerleave', schedulePlayerMinimizeHidden);
            minimizePlayerBtn.addEventListener('pointerenter', () => setPlayerMinimizeVisible(true));
            minimizePlayerBtn.addEventListener('pointerleave', schedulePlayerMinimizeHidden);
            minimizePlayerBtn.addEventListener('focus', () => setPlayerMinimizeVisible(true));
            minimizePlayerBtn.addEventListener('blur', schedulePlayerMinimizeHidden);
            setPlayerMinimizeVisible(false);
        }

        if (restorePlayerBtn) {
            restorePlayerBtn.addEventListener('click', () => {
                playerMinimized = false;
                playerPanel.classList.remove('minimized');
                restorePlayerBtn.style.display = 'none';
                setPlayerMinimizeVisible(false);
                markChromeActivity();
                saveSettings();
            });
        }

        let chromeIdleTimer = null;

        function isDisplayedElementOpen(id) {
            const element = document.getElementById(id);
            if (!element) {
                return false;
            }
            return window.getComputedStyle(element).display !== 'none';
        }

        function isChromeFadeBlocked() {
            return container.classList.contains('settings-open') ||
                isDisplayedElementOpen('slider-config-modal') ||
                isDisplayedElementOpen('search-modal') ||
                isDisplayedElementOpen('playlist-modal') ||
                isDisplayedElementOpen('active-playlist-modal') ||
                (volumeFlyout && volumeFlyout.matches(':hover, :focus-within'));
        }

        function setChromeIdleState(isIdle) {
            document.body.classList.toggle('chrome-idle', isIdle && !isChromeFadeBlocked());
        }

        function markChromeActivity() {
            setChromeIdleState(false);
            if (chromeIdleTimer !== null) {
                window.clearTimeout(chromeIdleTimer);
            }
            chromeIdleTimer = window.setTimeout(() => {
                setChromeIdleState(true);
            }, 5000);
        }

        ['mousemove', 'pointermove', 'mousedown', 'touchstart', 'keydown'].forEach((eventName) => {
            window.addEventListener(eventName, markChromeActivity, { passive: true });
        });
        markChromeActivity();

        if (lyricsWrapSlider) {
            lyricsWrapSlider.addEventListener('input', (e) => {
                lyricsWrapWidth = parseInt(e.target.value, 10);
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        lyricsFontSelect.addEventListener('change', (e) => {
            lyricFont = e.target.value;
            applyLyricsTypography();
            updateLyricsHighlight(true);
            saveSettings();
        });

        if (lyricsFontCustomInput) {
            const handleCustomFontSubmit = async () => {
                const customVal = normalizeLyricFontName(lyricsFontCustomInput.value);
                lyricsFontCustomInput.value = '';
                if (customVal) {
                    const validatedFont = await addValidatedCustomFont(customVal);
                    if (validatedFont) {
                        lyricFont = validatedFont;
                        applyLyricsTypography();
                        updateLyricsHighlight(true);
                        saveSettings();
                        lyricsFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                    } else {
                        console.warn(`[Fonts] "${customVal}" was not found on Google Fonts.`);
                        lyricsFontCustomInput.placeholder = `"${customVal}" not found`;
                    }
                } else {
                    lyricsFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                }
            };

            lyricsFontCustomInput.addEventListener('keydown', (e) => {
                if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleCustomFontSubmit();
                } else if (e.key === 'Escape') {
                    lyricsFontCustomInput.value = '';
                    lyricsFontCustomInput.placeholder = 'e.g. Pacifico or Roboto';
                    lyricsFontCustomInput.blur();
                } else if (e.key === 'Tab') {
                    const val = lyricsFontCustomInput.value.trim().toLowerCase();
                    if (val) {
                        const datalist = document.getElementById('google-fonts-list');
                        if (datalist) {
                            const options = Array.from(datalist.options).map(opt => opt.value);
                            let match = options.find(opt => opt.toLowerCase().startsWith(val));
                            if (!match) {
                                match = options.find(opt => opt.toLowerCase().includes(val));
                            }
                            if (match && match.toLowerCase() !== lyricsFontCustomInput.value.toLowerCase()) {
                                e.preventDefault();
                                lyricsFontCustomInput.value = match;
                            }
                        }
                    }
                }
            });

            lyricsFontCustomInput.addEventListener('blur', () => {
                // Short timeout to allow clicking options or buttons without canceling immediately
                setTimeout(() => {
                    void handleCustomFontSubmit();
                }, 150);
            });
        }

        if (lyricsBorderSlider) {
            lyricsBorderSlider.addEventListener('input', (e) => {
                lyricsBorderWidth = parseInt(e.target.value, 10);
                if (lyricsBorderVal) {
                    lyricsBorderVal.textContent = lyricsBorderWidth + 'px';
                }
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        if (lyricsWeightSlider) {
            lyricsWeightSlider.addEventListener('input', (e) => {
                lyricsWeight = parseInt(e.target.value, 10);
                if (lyricsWeightVal) {
                    lyricsWeightVal.textContent = String(lyricsWeight);
                }
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        if (lyricsLineHeightSlider) {
            lyricsLineHeightSlider.addEventListener('input', (e) => {
                lyricsLineHeight = parseFloat(e.target.value);
                if (lyricsLineHeightVal) {
                    lyricsLineHeightVal.textContent = lyricsLineHeight.toFixed(2);
                }
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        if (lyricsWordSpacingSlider) {
            lyricsWordSpacingSlider.addEventListener('input', (e) => {
                lyricsWordSpacing = parseFloat(e.target.value);
                if (lyricsWordSpacingVal) {
                    lyricsWordSpacingVal.textContent = lyricsWordSpacing + 'px';
                }
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        if (lyricsLetterSpacingSlider) {
            lyricsLetterSpacingSlider.addEventListener('input', (e) => {
                lyricsLetterSpacing = parseFloat(e.target.value);
                if (lyricsLetterSpacingVal) {
                    lyricsLetterSpacingVal.textContent = lyricsLetterSpacing + 'px';
                }
                applyLyricsTypography();
                updateLyricsHighlight(true);
                saveSettings();
            });
        }

        function getLastSettingsSection() {
            return localStorage.getItem('spotifm_last_settings_section') || 'lyrics';
        }

        function setActiveSettingsSection(sectionName) {
            const targetSection = Array.from(settingsSections).find((section) => section.dataset.settingsSection === sectionName);
            const activeSectionName = targetSection ? sectionName : getLastSettingsSection();

            settingsSections.forEach((section) => {
                section.classList.toggle('active', section.dataset.settingsSection === activeSectionName);
            });

            settingsSectionButtons.forEach((button) => {
                const isActive = button.dataset.settingsSectionTarget === activeSectionName;
                button.classList.toggle('active', isActive);
                button.setAttribute('aria-pressed', isActive ? 'true' : 'false');
            });

            localStorage.setItem('spotifm_last_settings_section', activeSectionName);
        }

        function setTransientButtonAccess(element, isAccessible) {
            if (!element) {
                return;
            }
            if (isAccessible) {
                element.removeAttribute('tabindex');
                element.removeAttribute('aria-hidden');
            } else {
                element.setAttribute('tabindex', '-1');
                element.setAttribute('aria-hidden', 'true');
            }
        }

        function syncSettingsChromeAccess(isOpen) {
            if (settingsActionRail) {
                settingsActionRail.setAttribute('aria-hidden', isOpen ? 'false' : 'true');
            }
            settingsSectionButtons.forEach((button) => setTransientButtonAccess(button, isOpen));

            setTransientButtonAccess(document.getElementById('toggle-search-btn'), !isOpen);
            setTransientButtonAccess(document.getElementById('toggle-playlist-btn'), !isOpen);
            setTransientButtonAccess(document.getElementById('toggle-active-playlist-btn'), !isOpen);
            setTransientButtonAccess(volumeToggleBtn, !isOpen);
        }

        let settingsVisible = false;

        settingsSectionButtons.forEach((button) => {
            button.addEventListener('click', (e) => {
                e.preventDefault();
                e.stopPropagation();
                markChromeActivity();
                setActiveSettingsSection(button.dataset.settingsSectionTarget || getLastSettingsSection());
                if (settingsDrawer) {
                    settingsDrawer.scrollTop = 0;
                }
                updateContainerHeight();
                setTimeout(updateContainerHeight, 150);
                setTimeout(updateContainerHeight, 300);
            });
        });

        setActiveSettingsSection(getLastSettingsSection());
        syncSettingsChromeAccess(false);

        toggleSettingsBtn.addEventListener('click', () => {
            markChromeActivity();
            settingsVisible = !settingsVisible;
            if (settingsVisible) {
                setActiveSettingsSection(getLastSettingsSection());
                if (settingsDrawer) {
                    settingsDrawer.scrollTop = 0;
                }
                container.classList.add('settings-open');
                syncSettingsChromeAccess(true);
                updateContainerHeight();
                setTimeout(updateContainerHeight, 150);
                setTimeout(updateContainerHeight, 300);
                setTimeout(updateContainerHeight, 500);
            } else {
                container.classList.remove('settings-open');
                syncSettingsChromeAccess(false);
                updateContainerHeight();
            }
            // Smoothly adjust lyrics center padding as height transitions
            setTimeout(resizeLyricsPadding, 150);
            setTimeout(resizeLyricsPadding, 300);
            setTimeout(resizeLyricsPadding, 500);
        });

        function updateGlassmorphism() {
            settingsDrawer.style.background = 'rgba(22, 18, 36, 0.9)';
            settingsDrawer.style.backdropFilter = 'blur(26px) saturate(120%)';
            settingsDrawer.style.webkitBackdropFilter = 'blur(26px) saturate(120%)';
            applyPlayerStyleSettings();
            const effectivePlayerOpacity = liquidGlassEnabled ? playerOpacity : 0;
            const effectivePlayerBorderOpacity = liquidGlassEnabled ? playerBorderOpacity : 0;
            const effectivePlayerBlur = liquidGlassEnabled ? playerBlur : 0;
            const effectivePlayerBackdropFilter = liquidGlassEnabled
                ? `blur(${effectivePlayerBlur}px)`
                : 'none';
            document.documentElement.style.setProperty('--player-scale', playerScale);
            document.documentElement.style.setProperty('--player-bg-opacity', effectivePlayerOpacity);
            document.documentElement.style.setProperty('--player-border-opacity', String(effectivePlayerBorderOpacity * 0.08));
            document.documentElement.style.setProperty('--player-shadow-opacity', String(effectivePlayerBorderOpacity));
            document.documentElement.style.setProperty('--player-blur', effectivePlayerBlur + 'px');
            document.documentElement.style.setProperty('--player-backdrop-filter', effectivePlayerBackdropFilter);
            applyPageGamma();
        }

        function applyPageGamma() {
            if (gammaFuncR && gammaFuncG && gammaFuncB) {
                gammaFuncR.setAttribute('exponent', '1');
                gammaFuncG.setAttribute('exponent', '1');
                gammaFuncB.setAttribute('exponent', '1');
            }
        }

        // ==========================================
        // 2. Real-Time Lyrics WebSocket Integration
        // ==========================================
        let ws;
        let wsReconnectTimer = null;
        let wsOpenedAt = 0;
        let lastWsPositionAt = 0;
        let lastLyricsWsRecoveryMs = 0;
        let lastNowPlayingRequestMs = 0;
        let lastNowPlayingHttpRequestMs = 0;
        let nowPlayingHttpRequestInFlight = false;

        function scheduleWebSocketReconnect() {
            if (wsReconnectTimer) {
                return;
            }
            syncStatus.textContent = "Connecting";
            wsReconnectTimer = window.setTimeout(() => {
                wsReconnectTimer = null;
                connectWebSocket();
            }, 3000);
        }

        function recoverLyricsSocket(reason) {
            if (!ws || ws.readyState !== WebSocket.OPEN) {
                return;
            }

            const now = performance.now();
            if ((now - lastLyricsWsRecoveryMs) < LYRICS_WS_RECOVERY_COOLDOWN_MS) {
                return;
            }

            lastLyricsWsRecoveryMs = now;
            console.warn(`[LyricsSync] ${reason}. Reconnecting lyrics WebSocket.`);
            syncStatus.textContent = "Resyncing";

            try {
                ws.close();
            } catch (error) {
                console.warn('[LyricsSync] Failed to close stale lyrics WebSocket:', error);
            }
        }

        function maybeRecoverLyricsSocket() {
            if (!isPlaying || !ws || ws.readyState !== WebSocket.OPEN) {
                return;
            }

            const referenceTime = lastWsPositionAt || wsOpenedAt;
            if (referenceTime === 0) {
                return;
            }

            const stallMs = performance.now() - referenceTime;
            if (stallMs > LYRICS_WS_STALL_MS) {
                recoverLyricsSocket(`No position updates for ${(stallMs / 1000).toFixed(1)}s`);
            }
        }

        function isWaitingForNowPlayingSnapshot() {
            const title = (songTitle && songTitle.textContent || '').trim();
            const lastSnapshotWasIdle = !!lastTrackData && lastTrackData.status === 'idle';
            return (
                !lastTrackData ||
                !title ||
                title === 'Connecting...' ||
                title === 'Connecting' ||
                (isPlaying && lastSnapshotWasIdle)
            );
        }

        function requestNowPlayingSnapshot(reason = 'refresh') {
            if (!ws || ws.readyState !== WebSocket.OPEN) {
                return false;
            }

            try {
                ws.send(JSON.stringify({ action: 'get_now_playing' }));
                lastNowPlayingRequestMs = performance.now();
                return true;
            } catch (error) {
                console.warn(`[WebSocket] Failed to request NowPlaying snapshot (${reason}):`, error);
                recoverLyricsSocket('NowPlaying snapshot request failed');
                return false;
            }
        }

        function requestNowPlayingSnapshotSoon(delayMs = 0, reason = 'refresh') {
            window.setTimeout(() => {
                if (!requestNowPlayingSnapshot(reason)) {
                    void fetchNowPlayingSnapshot(reason);
                }
            }, delayMs);
        }

        async function fetchNowPlayingSnapshot(reason = 'refresh') {
            if (nowPlayingHttpRequestInFlight) {
                return false;
            }

            const now = performance.now();
            if ((now - lastNowPlayingHttpRequestMs) < NOW_PLAYING_HTTP_REFRESH_MS) {
                return false;
            }

            nowPlayingHttpRequestInFlight = true;
            lastNowPlayingHttpRequestMs = now;

            try {
                const response = await fetch('/np', { cache: 'no-store' });
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}`);
                }
                const snapshot = await response.json();
                applyNowPlayingState(snapshot, true);
                return true;
            } catch (error) {
                console.warn(`[NowPlaying] HTTP snapshot request failed (${reason}):`, error);
                return false;
            } finally {
                nowPlayingHttpRequestInFlight = false;
            }
        }

        function maybeRefreshNowPlayingSnapshot() {
            if (!isPlaying || !isWaitingForNowPlayingSnapshot()) {
                return;
            }

            const now = performance.now();
            if ((now - lastNowPlayingRequestMs) < NOW_PLAYING_REFRESH_MS) {
                return;
            }

            if (!requestNowPlayingSnapshot('player is still waiting for metadata')) {
                void fetchNowPlayingSnapshot('websocket unavailable');
                if (!ws || ws.readyState === WebSocket.CLOSED || ws.readyState === WebSocket.CLOSING) {
                    scheduleWebSocketReconnect();
                }
            }
        }

        function connectWebSocket() {
            if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
                return;
            }

            const socket = new WebSocket(buildWebSocketUrl());
            ws = socket;

            socket.onopen = () => {
                if (ws !== socket) {
                    return;
                }
                console.log("[WebSocket] Synced Lyrics WebSocket connected.");
                wsOpenedAt = performance.now();
                lastWsPositionAt = 0;
                updateAutoSyncStatus();
                requestNowPlayingSnapshot('socket opened');
                requestNowPlayingSnapshotSoon(500, 'socket opened delayed');
            };

            socket.onmessage = (event) => {
                if (ws !== socket) {
                    return;
                }
                try {
                    const msg = JSON.parse(event.data);

                    if (msg.type === "Lyrics") {
                        handleIncomingLyricsState(msg);
                    } else if (msg.type === "NowPlaying") {
                        applyNowPlayingState(msg, true);
                    } else if (msg.type === "Position") {
                        const previousTrackMs = lastPositionTrackMs;
                        const preUpdateDisplayedLyricsPositionMs = getInterpolatedPositionMs();
                        lastServerTime = msg.position_ms / 1000.0;
                        currentTrackPositionMs = msg.position_ms;
                        lastSyncInstant = performance.now();
                        lastWsPositionAt = lastSyncInstant;
                        if (msg.start_granule_sec !== undefined && msg.start_granule_sec !== null) {
                            const nextTrackStartGranuleSec = Number(msg.start_granule_sec);
                            if (
                                Number.isFinite(nextTrackStartGranuleSec) &&
                                currentTrackStartGranuleSec !== nextTrackStartGranuleSec
                            ) {
                                currentTrackStartGranuleSec = nextTrackStartGranuleSec;
                                localGranuleClockMode = null;
                            }
                        }
                        if (msg.stream_origin_sec !== undefined && msg.stream_origin_sec !== null) {
                            const nextStreamOriginSec = Number(msg.stream_origin_sec);
                            if (
                                Number.isFinite(nextStreamOriginSec) &&
                                streamFirstGranuleSec !== nextStreamOriginSec
                            ) {
                                streamFirstGranuleSec = nextStreamOriginSec;
                                serviceWorkerSyncReady = true;
                                localGranuleClockMode = null;
                            }
                        }
                        const positionReset =
                            previousTrackMs !== null &&
                            msg.position_ms + 2500 < previousTrackMs;
                        if (
                            isAutosync &&
                            currentLyricsStateType !== null &&
                            displayedLyricsHoldStartMs === null &&
                            positionReset
                        ) {
                            displayedLyricsHoldStartMs = preUpdateDisplayedLyricsPositionMs;
                            displayedLyricsHoldAudioTimeSec = getAudioContentPositionSec();
                        }

                        if (msg.track_id && msg.track_id !== currentPlaybackTrackId) {
                            currentPlaybackTrackId = msg.track_id;
                            awaitingPlaybackTrackConfirmation = true;
                        }

                        const initialTrackLock =
                            confirmedPlaybackTrackId === null &&
                            currentPlaybackTrackId !== null;
                        const earlyTrackWindow =
                            awaitingPlaybackTrackConfirmation &&
                            msg.position_ms < 5000;

                        if (
                            currentPlaybackTrackId &&
                            (initialTrackLock || (awaitingPlaybackTrackConfirmation && (positionReset || earlyTrackWindow || msg.track_id === currentPlaybackTrackId)))
                        ) {
                            confirmedPlaybackTrackId = currentPlaybackTrackId;
                            awaitingPlaybackTrackConfirmation = false;
                        }

                        lastPositionTrackMs = msg.position_ms;

                        maybeCommitPendingLyricsTransition();
                        updateAutoSyncStatus();
                        updateLyricsHighlight();
                        updateTrackProgressDisplay();
                    } else if (msg.type === "NoLyrics") {
                        handleIncomingLyricsState({ type: 'NoLyrics' });
                    } else if (msg.type === "Idle") {
                        lastTrackData = { status: 'idle' };
                        clearDisplayedLyricsHold();
                        clearPendingLyricsTransition();
                        currentLyricsTrackId = null;
                        currentLyricsStateType = null;
                        currentPlaybackTrackId = null;
                        confirmedPlaybackTrackId = null;
                        currentTrackDurationMs = null;
                        currentTrackPositionMs = 0;
                        currentTrackStartGranuleSec = null;
                        awaitingPlaybackTrackConfirmation = false;
                        lastPositionTrackMs = null;
                        lastServerTime = 0;
                        lastSyncInstant = 0;
                        renderPlaceholder("Idle", "Player is currently waiting for songs");
                        songTitle.textContent = "Idle";
                        songArtists.textContent = "No song playing";
                        albumCover.src = '';
                        albumCover.style.display = 'none';
                        albumPlaceholder.style.display = 'flex';
                        setPlayState('paused');
                        currentTrackId = null;
                        titleContainer.classList.add('no-link');
                        updateTrackProgressDisplay();
                        scheduleAlbumArtSizeUpdate();
                        void refreshActivePlaylistState();
                    }
                } catch (e) {
                    console.error("Error processing WebSocket packet:", e);
                }
            };

            socket.onerror = (event) => {
                if (ws !== socket) {
                    return;
                }
                console.warn('[WebSocket] Lyrics WebSocket error:', event);
                syncStatus.textContent = "Connecting";
                try {
                    socket.close();
                } catch (error) {
                    console.warn('[WebSocket] Failed to close errored lyrics WebSocket:', error);
                    scheduleWebSocketReconnect();
                }
            };

            socket.onclose = () => {
                if (ws !== socket) {
                    return;
                }
                console.log("[WebSocket] Lyrics WebSocket disconnected. Retrying in 3 seconds...");
                ws = null;
                wsOpenedAt = 0;
                lastWsPositionAt = 0;
                scheduleWebSocketReconnect();
            };
        }

        if (!IS_SINGLE_FILE_BUILD_SOURCE) {
            connectWebSocket();
        }

        // ==========================================
        // 4. Synced Lyrics Rendering & Highlight
        // ==========================================
        function renderLyrics(lines) {
            syncedLines = lines || [];
            syncedLines.sort((a, b) => a.time_ms - b.time_ms);

            if (syncedLines.length === 0) {
                renderPlaceholder("Not Available", "No synced lyrics available for this track");
                return;
            }

            lyricsBody.innerHTML = '';
            syncedLines.forEach((line, idx) => {
                const el = document.createElement('div');
                el.className = 'lyric-line';
                el.id = `line-${idx}`;
                el.textContent = line.text;
                el.dataset.time = line.time_ms;
                el.style.opacity = '0.14';

                lyricsBody.appendChild(el);
            });
            activeLineIndex = -1;
            resetLyricsScrollPosition();
            resizeLyricsPadding();
        }

        function renderPlaceholder(title, text) {
            syncedLines = [];
            activeLineIndex = -1;
            resetLyricsScrollPosition();
            lyricsBody.innerHTML = `
                <div class="lyric-placeholder">
                    <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
                    <span style="font-weight: 600; color: #fff;">${title}</span>
                    <span>${text}</span>
                </div>
            `;
            resizeLyricsPadding();
        }

        function updateLyricsHighlight(forceScroll = false) {
            if (syncedLines.length === 0) {
                if (isNtscActive() || isVhsActive()) {
                    drawCanvasLyrics();
                    triggerNtscRedraw();
                }
                return;
            }

            const currentPositionMs = getInterpolatedPositionMs();

            // Find active line index
            let targetIdx = -1;
            for (let i = 0; i < syncedLines.length; i++) {
                if (syncedLines[i].time_ms <= currentPositionMs) {
                    targetIdx = i;
                } else {
                    break;
                }
            }

            if (targetIdx !== activeLineIndex || forceScroll) {
                // Remove highlight from old active line
                if (activeLineIndex !== -1) {
                    const oldEl = document.getElementById(`line-${activeLineIndex}`);
                    if (oldEl) oldEl.classList.remove('active');
                }

                activeLineIndex = targetIdx;

                for (let i = 0; i < syncedLines.length; i++) {
                    const lineEl = document.getElementById(`line-${i}`);
                    if (!lineEl) continue;

                    if (activeLineIndex === -1) {
                        lineEl.style.opacity = '0.14';
                        continue;
                    }

                    const distance = Math.abs(i - activeLineIndex);
                    const opacity = distance === 0
                        ? 1
                        : (() => {
                            if (lyricFadeCurve <= 0.0) return 0.0;
                            const maxVal = lyricsFadeSlider ? parseFloat(lyricsFadeSlider.max) : 5.0;
                            const factor = maxVal > 0 ? (lyricFadeCurve / maxVal) : 0.0;
                            return Math.pow(factor, distance);
                        })();
                    lineEl.style.opacity = opacity.toFixed(3);
                }

                if (activeLineIndex !== -1) {
                    const activeEl = document.getElementById(`line-${activeLineIndex}`);
                    if (activeEl) {
                        activeEl.classList.add('active');

                        if (performance.now() >= lyricsScrollFreezeUntilMs) {
                            // Center the active line exactly in the vertical center of the screen
                            const scrollPanelRect = scrollPanel.getBoundingClientRect();
                            const elOffset = activeEl.offsetTop;
                            const elHeight = activeEl.clientHeight;

                            scrollPanel.scrollTop = scrollPanelRect.top + elOffset + (elHeight / 2) - (window.innerHeight / 2);
                        }
                    }
                }

                if (isNtscActive() || isVhsActive()) {
                    drawCanvasLyrics();
                    triggerNtscRedraw();
                }
            }
        }

        function getInterpolatedPositionMs() {
            if (!isPlaying) {
                return 0;
            }

            const heldLyricsPositionMs = getHeldLyricsPositionMs();
            if (heldLyricsPositionMs !== null) {
                return heldLyricsPositionMs;
            }

            if (wasmPlaybackEnabled) {
                if (!wasmClockReady) {
                    return 0;
                }
                const delayOffset = isAutosync ? 0 : manualDelaySec;
                const offset = isAutosync ? autoSyncLyricsOffsetSec : 0;
                return Math.max(0, (wasmPlaybackPositionSec - delayOffset + offset) * 1000.0);
            }

            if (isAutosync) {
                const localPositionSec = getLocalGranulePositionSec();
                if (localPositionSec !== null) {
                    return (localPositionSec + autoSyncLyricsOffsetSec) * 1000.0;
                }
            }

            const wsPositionSec = getWebSocketPositionSec();
            if (wsPositionSec === null) return 0;
            const delayOffset = isAutosync ? 0 : manualDelaySec;
            const currentPositionSec = wsPositionSec - delayOffset + (isAutosync ? autoSyncLyricsOffsetSec : 0);
            return currentPositionSec * 1000.0;
        }

        // ==========================================
        // 5. Butterchurn Milkdrop Visualizer
        // ==========================================
        let BUTTERCHURN_MODULE_URL = 'https://unpkg.com/butterchurn@3.0.0-beta.5/dist/butterchurn.js';
        let BUTTERCHURN_PRESETS_URL = 'https://unpkg.com/butterchurn-presets@3.0.0-beta.4/dist/base.js';

        // Check if inlined code is available
        const inlineButterchurnBase64 = "__BUTTERCHURN_CODE_BASE64__";
        const inlinePresetsBase64 = "__BUTTERCHURN_PRESETS_CODE_BASE64__";

        if (inlineButterchurnBase64 && !inlineButterchurnBase64.startsWith("__")) {
            try {
                const binary = atob(inlineButterchurnBase64);
                const array = new Uint8Array(binary.length);
                for (let i = 0; i < binary.length; i++) {
                    array[i] = binary.charCodeAt(i);
                }
                const blob = new Blob([array], { type: 'application/javascript' });
                BUTTERCHURN_MODULE_URL = URL.createObjectURL(blob);
            } catch (e) {
                console.error("Failed to load inlined Butterchurn module:", e);
            }
        }

        if (inlinePresetsBase64 && !inlinePresetsBase64.startsWith("__")) {
            try {
                const binary = atob(inlinePresetsBase64);
                const array = new Uint8Array(binary.length);
                for (let i = 0; i < binary.length; i++) {
                    array[i] = binary.charCodeAt(i);
                }
                const blob = new Blob([array], { type: 'application/javascript' });
                BUTTERCHURN_PRESETS_URL = URL.createObjectURL(blob);
            } catch (e) {
                console.error("Failed to load inlined Butterchurn presets:", e);
            }
        }
        const MILKDROP_DEFAULT_BLEND_SECONDS = 2.7;
        const MILKDROP_USER_BLEND_SECONDS = 5.7;

        let audioCtx = null;
        let analyser = null;
        let source = null;
        let mediaElementSourceNode = null;
        let analysisSourceMode = null;
        let analyserOutputConnected = false;
        const canvas = document.getElementById('visualizer-canvas');
        let butterchurnModule = null;
        let butterchurnModulePromise = null;
        let milkdropVisualizer = null;
        let milkdropAudioConnected = false;
        let milkdropRenderFrameId = null;
        let milkdropCycleIntervalId = null;
        let milkdropPresetLoadPromise = null;
        let visualizerRecoveryInProgress = false;
        let visualizerRecoveryLastAttemptAt = 0;
        let milkdropPresets = [];
        let milkdropPresetMap = new Map();
        let loadedMilkdropPresetName = '';
        let resizeHandlerAttached = false;
        let ntscGl = null;
        let passProgram = null;
        let ntscProgram = null;
        let ntscTexture = null;
        let ntscTextTexture = null;
        let ntscPlayerTextTexture = null;
        let ntscBuffer = null;
        const offscreenCanvas = document.createElement('canvas');
        const blankVisualizerCanvas = document.createElement('canvas');
        const lyricsCanvas = document.createElement('canvas');
        const playerTextCanvas = document.createElement('canvas');
        let lyricsCtx = null;
        let playerTextCtx = null;

        function getVisualizerSourceCanvas() {
            return visualizerEnabled ? offscreenCanvas : blankVisualizerCanvas;
        }

        function setMilkdropStatus(message, isError = false) {
            if (!milkdropStatus) {
                return;
            }
            milkdropStatus.textContent = message || '';
            milkdropStatus.classList.toggle('error', isError);
        }

        function reportMilkdropError(statusMessage, logMessage, error) {
            setMilkdropStatus(statusMessage, true);
            console.error(logMessage, error);
        }

        function updateVisualizerCanvasOpacity() {
            if (!canvas) {
                return;
            }
            canvas.style.opacity = (visualizerEnabled || isVideoEffectActive() || isPlayerTextBlendActive())
                ? String(visualizerOpacity)
                : '0';
            syncMilkdropAnimationState();
        }

        function syncMilkdropControls() {
            syncEffectEnabledFlags();
            if (visualizerToggle) {
                visualizerToggle.checked = visualizerEnabled;
            }
            if (ntscAmountSlider) {
                ntscAmountSlider.value = String(ntscAmount);
            }
            if (ntscAmountVal) {
                ntscAmountVal.textContent = formatPercent(ntscAmount);
            }
            if (ntscSettingsGroup) {
                ntscSettingsGroup.style.display = isNtscActive() ? 'flex' : 'none';
            }
            if (ntscSmearSlider) {
                ntscSmearSlider.value = String(ntscSmear);
            }
            if (ntscSmearVal) {
                ntscSmearVal.textContent = Math.round(ntscSmear * 100) + '%';
            }
            if (ntscWiggleSlider) {
                ntscWiggleSlider.value = String(ntscWiggle);
            }
            if (ntscWiggleVal) {
                ntscWiggleVal.textContent = Math.round(ntscWiggle * 100) + '%';
            }
            if (ntscWiggleSpeedSlider) {
                ntscWiggleSpeedSlider.value = String(ntscWiggleSpeed);
            }
            if (ntscWiggleSpeedVal) {
                ntscWiggleSpeedVal.textContent = String(Math.round(ntscWiggleSpeed));
            }
            if (ntscChromaShiftSlider) {
                ntscChromaShiftSlider.value = String(ntscChromaShift);
            }
            if (ntscChromaShiftVal) {
                ntscChromaShiftVal.textContent = formatPercent(ntscChromaShift);
            }
            if (vhsAmountSlider) {
                vhsAmountSlider.value = String(vhsAmount);
            }
            if (vhsAmountVal) {
                vhsAmountVal.textContent = formatPercent(vhsAmount);
            }
            if (vhsSettingsGroup) {
                vhsSettingsGroup.style.display = isVhsActive() ? 'flex' : 'none';
            }
            if (effectOrderGroup) {
                effectOrderGroup.style.display = 'none';
            }
            effectOrderButtons.forEach((button) => {
                const isActive = button.dataset.effectOrder === videoEffectOrder;
                button.classList.toggle('active', isActive);
                button.setAttribute('aria-pressed', isActive ? 'true' : 'false');
            });
            if (vhsStrengthSlider) {
                vhsStrengthSlider.value = String(vhsStrength);
            }
            if (vhsStrengthVal) {
                vhsStrengthVal.textContent = Math.round(vhsStrength * 100) + '%';
            }
            if (vhsNoiseSlider) {
                vhsNoiseSlider.value = String(vhsNoise);
            }
            if (vhsNoiseVal) {
                vhsNoiseVal.textContent = Math.round(vhsNoise * 100) + '%';
            }
            if (vhsGrainSizeSlider) {
                vhsGrainSizeSlider.value = String(vhsGrainSize);
            }
            if (vhsGrainSizeVal) {
                vhsGrainSizeVal.textContent = vhsGrainSize.toFixed(1);
            }
            if (vhsVignetteSlider) {
                vhsVignetteSlider.value = String(vhsVignette);
            }
            if (vhsVignetteVal) {
                vhsVignetteVal.textContent = formatPercent(vhsVignette);
            }
            if (milkdropFrameLimitSelect) {
                milkdropFrameLimitSelect.value = String(milkdropFrameLimit);
            }
            if (milkdropCanvasSizeSelect) {
                milkdropCanvasSizeSelect.value = String(milkdropCanvasSize);
            }
            if (milkdropMeshSizeSelect) {
                milkdropMeshSizeSelect.value = String(milkdropMeshSize);
            }
            if (vhsBlurSlider) {
                vhsBlurSlider.value = String(vhsBlur);
            }
            if (vhsBlurVal) {
                vhsBlurVal.textContent = vhsBlur.toFixed(3);
            }
            if (vhsLumaResolutionSlider) {
                vhsLumaResolutionSlider.value = String(vhsLumaResolution);
            }
            if (vhsLumaResolutionVal) {
                vhsLumaResolutionVal.textContent = formatPercent(vhsLumaResolution);
            }
            if (vhsChromaResolutionSlider) {
                vhsChromaResolutionSlider.value = String(vhsChromaResolution);
            }
            if (vhsChromaResolutionVal) {
                vhsChromaResolutionVal.textContent = formatPercent1(vhsChromaResolution);
            }
            if (vhsLineHeightSlider) {
                vhsLineHeightSlider.value = String(vhsLineHeight);
            }
            if (vhsLineHeightVal) {
                vhsLineHeightVal.textContent = formatFixed(vhsLineHeight, 1);
            }
            if (vhsSharpenSlider) {
                vhsSharpenSlider.value = String(vhsSharpen);
            }
            if (vhsSharpenVal) {
                vhsSharpenVal.textContent = formatFixed(vhsSharpen, 1);
            }
            if (vhsSharpenRadiusSlider) {
                vhsSharpenRadiusSlider.value = String(vhsSharpenRadius);
            }
            if (vhsSharpenRadiusVal) {
                vhsSharpenRadiusVal.textContent = formatFixed(vhsSharpenRadius, 1);
            }
            if (vhsBlackLevelSlider) {
                vhsBlackLevelSlider.value = String(vhsBlackLevel);
            }
            if (vhsBlackLevelVal) {
                vhsBlackLevelVal.textContent = formatPercent(vhsBlackLevel);
            }
            if (vhsWhiteLevelSlider) {
                vhsWhiteLevelSlider.value = String(vhsWhiteLevel);
            }
            if (vhsWhiteLevelVal) {
                vhsWhiteLevelVal.textContent = formatPercent(vhsWhiteLevel);
            }
            if (vhsSaturationSlider) {
                vhsSaturationSlider.value = String(vhsSaturation);
            }
            if (vhsSaturationVal) {
                vhsSaturationVal.textContent = formatPercent(vhsSaturation);
            }
            if (vhsShadowTintPicker) {
                vhsShadowTintPicker.value = vhsShadowTint;
            }
            if (vhsShadowTintVal) {
                vhsShadowTintVal.textContent = vhsShadowTint.toUpperCase();
            }
            if (vhsTrackingSpeedSlider) {
                vhsTrackingSpeedSlider.value = String(vhsTrackingSpeed);
            }
            if (vhsTrackingSpeedVal) {
                vhsTrackingSpeedVal.textContent = formatFixed(vhsTrackingSpeed, 1);
            }
            if (vhsTrackingOffsetSlider) {
                vhsTrackingOffsetSlider.value = String(vhsTrackingOffset);
            }
            if (vhsTrackingOffsetVal) {
                vhsTrackingOffsetVal.textContent = formatFixed(vhsTrackingOffset, 0);
            }
            if (vhsTrackingJitterSlider) {
                vhsTrackingJitterSlider.value = String(vhsTrackingJitter);
            }
            if (vhsTrackingJitterVal) {
                vhsTrackingJitterVal.textContent = formatFixed(vhsTrackingJitter, 0);
            }
            if (vhsWaveFrequencySlider) {
                vhsWaveFrequencySlider.value = String(vhsWaveFrequency);
            }
            if (vhsWaveFrequencyVal) {
                vhsWaveFrequencyVal.textContent = formatFixed(vhsWaveFrequency, 0);
            }
            if (vhsWaveAmountSlider) {
                vhsWaveAmountSlider.value = String(vhsWaveAmount);
            }
            if (vhsWaveAmountVal) {
                vhsWaveAmountVal.textContent = formatFixed(vhsWaveAmount, 2);
            }
            if (vhsBottomWarpHeightSlider) {
                vhsBottomWarpHeightSlider.value = String(vhsBottomWarpHeight);
            }
            if (vhsBottomWarpHeightVal) {
                vhsBottomWarpHeightVal.textContent = formatFixed(vhsBottomWarpHeight, 0);
            }
            if (vhsBottomWarpOffsetSlider) {
                vhsBottomWarpOffsetSlider.value = String(vhsBottomWarpOffset);
            }
            if (vhsBottomWarpOffsetVal) {
                vhsBottomWarpOffsetVal.textContent = formatFixed(vhsBottomWarpOffset, 0);
            }
            if (vhsBottomWarpJitterSlider) {
                vhsBottomWarpJitterSlider.value = String(vhsBottomWarpJitter);
            }
            if (vhsBottomWarpJitterVal) {
                vhsBottomWarpJitterVal.textContent = formatFixed(vhsBottomWarpJitter, 0);
            }
            if (vhsStaticLineHeightSlider) {
                vhsStaticLineHeightSlider.value = String(vhsStaticLineHeight);
            }
            if (vhsStaticLineHeightVal) {
                vhsStaticLineHeightVal.textContent = formatFixed(vhsStaticLineHeight, 1);
            }
            if (vhsStaticLineOpacitySlider) {
                vhsStaticLineOpacitySlider.value = String(vhsStaticLineOpacity);
            }
            if (vhsStaticLineOpacityVal) {
                vhsStaticLineOpacityVal.textContent = formatPercent(vhsStaticLineOpacity);
            }
            if (vhsVignettePowerSlider) {
                vhsVignettePowerSlider.value = String(vhsVignettePower);
            }
            if (vhsVignettePowerVal) {
                vhsVignettePowerVal.textContent = formatFixed(vhsVignettePower, 2);
            }
            if (vhsVignetteBoostSlider) {
                vhsVignetteBoostSlider.value = String(vhsVignetteBoost);
            }
            if (vhsVignetteBoostVal) {
                vhsVignetteBoostVal.textContent = formatFixed(vhsVignetteBoost, 2);
            }
            if (lyricsBorderSlider) {
                lyricsBorderSlider.value = String(lyricsBorderWidth);
            }
            if (lyricsBorderVal) {
                lyricsBorderVal.textContent = lyricsBorderWidth + 'px';
            }
            if (lyricsWeightSlider) {
                lyricsWeightSlider.value = String(lyricsWeight);
            }
            if (lyricsWeightVal) {
                lyricsWeightVal.textContent = String(lyricsWeight);
            }
            if (lyricsLineHeightSlider) {
                lyricsLineHeightSlider.value = String(lyricsLineHeight);
            }
            if (lyricsLineHeightVal) {
                lyricsLineHeightVal.textContent = lyricsLineHeight.toFixed(2);
            }
            if (lyricsWordSpacingSlider) {
                lyricsWordSpacingSlider.value = String(lyricsWordSpacing);
            }
            if (lyricsWordSpacingVal) {
                lyricsWordSpacingVal.textContent = lyricsWordSpacing + 'px';
            }
            if (lyricsLetterSpacingSlider) {
                lyricsLetterSpacingSlider.value = String(lyricsLetterSpacing);
            }
            if (lyricsLetterSpacingVal) {
                lyricsLetterSpacingVal.textContent = lyricsLetterSpacing + 'px';
            }
            applyNtscState();
            if (vizOpacitySlider) {
                vizOpacitySlider.value = String(visualizerOpacity);
            }
            if (vizOpacityVal) {
                vizOpacityVal.textContent = formatPercent(visualizerOpacity);
            }
            if (milkdropCycleSongToggle) {
                milkdropCycleSongToggle.checked = milkdropCycleOnSongChange;
            }
            if (milkdropCycleTimeGroup) {
                if (milkdropCycleOnSongChange) {
                    milkdropCycleTimeGroup.style.opacity = '0.5';
                    milkdropCycleTimeGroup.style.pointerEvents = 'none';
                    if (milkdropCycleSlider) milkdropCycleSlider.disabled = true;
                } else {
                    milkdropCycleTimeGroup.style.opacity = '1';
                    milkdropCycleTimeGroup.style.pointerEvents = 'auto';
                    if (milkdropCycleSlider) milkdropCycleSlider.disabled = false;
                }
            }
            if (milkdropCycleSlider) {
                milkdropCycleSlider.value = String(milkdropCycleSeconds);
            }
            if (milkdropCycleVal) {
                milkdropCycleVal.textContent = milkdropCycleSeconds + 's';
            }
            if (milkdropBlendSlider) {
                milkdropBlendSlider.value = String(milkdropBlendSeconds);
            }
            if (milkdropBlendVal) {
                milkdropBlendVal.textContent = milkdropBlendSeconds.toFixed(1) + 's';
            }
            syncMilkdropPresetListState();
        }

        async function loadButterchurnModule() {
            if (butterchurnModule) {
                return butterchurnModule;
            }
            if (!butterchurnModulePromise) {
                butterchurnModulePromise = import(BUTTERCHURN_MODULE_URL)
                    .then((module) => {
                        butterchurnModule = module.default || module;
                        return butterchurnModule;
                    })
                    .catch((error) => {
                        butterchurnModulePromise = null;
                        reportMilkdropError('Butterchurn failed to load', '[Milkdrop] Failed to load Butterchurn:', error);
                        return null;
                    });
            }
            return butterchurnModulePromise;
        }

        function getEnabledMilkdropPresets() {
            return milkdropPresets.filter((entry) => (
                !milkdropDisabledPresetNames.has(entry.name) &&
                !milkdropFailedPresetNames.has(entry.name)
            ));
        }

        function syncMilkdropPresetListState() {
            if (!milkdropPresetList) {
                return;
            }
            milkdropPresetList.querySelectorAll('.preset-row').forEach((row) => {
                const name = row.dataset.presetName || '';
                const activeName = loadedMilkdropPresetName || milkdropPresetName;
                const isActive = !!name && name === activeName;
                row.classList.toggle('active', isActive);
                if (isActive) {
                    row.setAttribute('aria-current', 'true');
                } else {
                    row.removeAttribute('aria-current');
                }
                row.classList.toggle('failed', !!name && milkdropFailedPresetNames.has(name));
                const checkbox = row.querySelector('.preset-checkbox');
                if (checkbox) {
                    checkbox.checked = !milkdropDisabledPresetNames.has(name);
                    checkbox.disabled = milkdropFailedPresetNames.has(name);
                }
            });
        }

        function renderMilkdropPresetList() {
            if (!milkdropPresetList) {
                return;
            }
            milkdropPresetList.textContent = '';
            if (milkdropPresets.length === 0) {
                const empty = document.createElement('div');
                empty.className = 'settings-status';
                empty.textContent = 'No presets loaded';
                milkdropPresetList.appendChild(empty);
                return;
            }

            const fragment = document.createDocumentFragment();
            milkdropPresets.forEach((entry) => {
                const row = document.createElement('div');
                row.className = 'preset-row';
                row.dataset.presetName = entry.name;

                const checkbox = document.createElement('input');
                checkbox.type = 'checkbox';
                checkbox.className = 'preset-checkbox';
                checkbox.dataset.presetName = entry.name;
                checkbox.checked = !milkdropDisabledPresetNames.has(entry.name);
                checkbox.setAttribute('aria-label', 'Include ' + entry.name);

                const button = document.createElement('button');
                button.type = 'button';
                button.className = 'preset-load-button';
                button.dataset.presetName = entry.name;
                button.textContent = entry.name;
                button.title = entry.name;

                row.appendChild(checkbox);
                row.appendChild(button);
                fragment.appendChild(row);
            });
            milkdropPresetList.appendChild(fragment);
            syncMilkdropPresetListState();
        }

        async function loadMilkdropPresets() {
            if (milkdropPresets.length > 0) {
                return milkdropPresets;
            }
            if (!milkdropPresetLoadPromise) {
                setMilkdropStatus('Loading Webamp presets');
                milkdropPresetLoadPromise = import(BUTTERCHURN_PRESETS_URL)
                    .then(() => {
                        const presetExports = window.base && (window.base.default || window.base);
                        if (!presetExports || typeof presetExports !== 'object') {
                            throw new Error('butterchurn-presets did not expose window.base');
                        }
                        milkdropPresets = Object.entries(presetExports)
                            .map(([name, preset]) => ({ name, preset }))
                            .sort((a, b) => a.name.localeCompare(b.name));
                        milkdropPresetMap = new Map(milkdropPresets.map((entry) => [entry.name, entry]));

                        if (
                            !milkdropPresetName ||
                            !milkdropPresetMap.has(milkdropPresetName) ||
                            milkdropDisabledPresetNames.has(milkdropPresetName)
                        ) {
                            milkdropPresetName = pickRandomMilkdropPresetName();
                        }

                        renderMilkdropPresetList();
                        syncMilkdropControls();
                        setMilkdropStatus(milkdropPresets.length + ' Webamp presets loaded');
                        return milkdropPresets;
                    })
                    .catch((error) => {
                        milkdropPresetLoadPromise = null;
                        reportMilkdropError('Webamp presets failed to load', '[Milkdrop] Failed to load Webamp presets:', error);
                        return [];
                    });
            }
            return milkdropPresetLoadPromise;
        }

        function pickRandomMilkdropPresetName() {
            const enabledPresets = getEnabledMilkdropPresets();
            if (enabledPresets.length === 0) {
                return '';
            }
            if (enabledPresets.length === 1) {
                return enabledPresets[0].name;
            }

            let nextName = milkdropPresetName;
            for (let attempts = 0; attempts < 6 && nextName === milkdropPresetName; attempts += 1) {
                const index = Math.floor(Math.random() * enabledPresets.length);
                nextName = enabledPresets[index].name;
            }
            return nextName || enabledPresets[0].name;
        }

        function resizeCanvas() {
            if (!canvas) {
                return;
            }
            const width = Math.max(1, window.innerWidth);
            const height = Math.max(1, window.innerHeight);

            let visualizerWidth = width;
            let visualizerHeight = height;

            const sizeOption = milkdropCanvasSize || 'native';
            if (sizeOption === 'auto') {
                const scale = 0.5;
                visualizerWidth = Math.max(1, Math.floor(width * scale));
                visualizerHeight = Math.max(1, Math.floor(height * scale));
            } else if (sizeOption !== 'native') {
                const maxDim = parseFloat(sizeOption);
                if (Number.isFinite(maxDim)) {
                    if (width > height) {
                        visualizerWidth = maxDim;
                        visualizerHeight = Math.max(1, Math.floor(maxDim * (height / width)));
                    } else {
                        visualizerHeight = maxDim;
                        visualizerWidth = Math.max(1, Math.floor(maxDim * (width / height)));
                    }
                }
            }

            offscreenCanvas.width = visualizerWidth;
            offscreenCanvas.height = visualizerHeight;
            blankVisualizerCanvas.width = width;
            blankVisualizerCanvas.height = height;
            lyricsCanvas.width = width;
            lyricsCanvas.height = height;
            playerTextCanvas.width = width;
            playerTextCanvas.height = height;
            canvas.width = width;
            canvas.height = height;
            if (milkdropVisualizer) {
                milkdropVisualizer.setRendererSize(visualizerWidth, visualizerHeight);
            }
            if (vhsInitialized) {
                resizeVhsTextures(width, height);
            }
            drawCanvasLyrics();
            drawCanvasPlayerText();
            triggerNtscRedraw();
        }

        function connectAnalyserToDestination() {
            if (!analyser || !audioCtx || analyserOutputConnected) {
                return;
            }
            analyser.connect(audioCtx.destination);
            analyserOutputConnected = true;
        }

        function connectMilkdropAudio() {
            if (!milkdropVisualizer || !analyser || milkdropAudioConnected) {
                return;
            }
            try {
                milkdropVisualizer.connectAudio(analyser);
                milkdropAudioConnected = true;
            } catch (error) {
                console.warn('[Milkdrop] Failed to connect analyser:', error);
            }
        }

        async function ensureMilkdropVisualizer() {
            if (!visualizerEnabled || !audioCtx || !canvas) {
                return null;
            }
            if (milkdropVisualizer) {
                connectMilkdropAudio();
                return milkdropVisualizer;
            }

            try {
                const butterchurn = await loadButterchurnModule();
                if (!butterchurn || typeof butterchurn.createVisualizer !== 'function') {
                    return null;
                }
                resizeCanvas();
                const mesh = (milkdropMeshSize || '32x24').split('x');
                const meshWidth = parseInt(mesh[0], 10) || 32;
                const meshHeight = parseInt(mesh[1], 10) || 24;
                milkdropVisualizer = butterchurn.createVisualizer(audioCtx, offscreenCanvas, {
                    width: offscreenCanvas.width,
                    height: offscreenCanvas.height,
                    meshWidth: meshWidth,
                    meshHeight: meshHeight,
                    pixelRatio: window.devicePixelRatio || 1,
                    onlyUseWASM: true,
                });
                connectMilkdropAudio();
                initNtscWebGL();
                syncMilkdropAnimationState();
                //showMilkdropTrackTitleOnce();
                return milkdropVisualizer;
            } catch (error) {
                reportMilkdropError('Milkdrop renderer failed', '[Milkdrop] Failed to initialize renderer:', error);
                return null;
            }
        }

        async function recreateMilkdropVisualizer() {
            if (milkdropVisualizer) {
                try {
                    if (typeof milkdropVisualizer.destroy === 'function') {
                        milkdropVisualizer.destroy();
                    }
                } catch (err) {
                    console.warn('[Milkdrop] Failed to destroy visualizer:', err);
                }
                milkdropVisualizer = null;
            }
            loadedMilkdropPresetName = '';
            milkdropAudioConnected = false;

            if (visualizerEnabled) {
                const viz = await ensureMilkdropVisualizer();
                if (viz) {
                    void loadCurrentMilkdropPreset(0);
                }
            }
        }

        function getMilkdropTrackTitle() {
            const title = (songTitle && songTitle.textContent || '').trim();
            const artists = (songArtists && songArtists.textContent || '').trim();
            if (!title || title === 'Connecting...') {
                return '';
            }
            return artists ? title + ' - ' + artists : title;
        }

        function getMilkdropTrackKey() {
            return currentTrackId || currentPlaybackTrackId || getMilkdropTrackTitle();
        }

        function showMilkdropTrackTitleOnce() {
            if (!milkdropVisualizer || !isPlaying) {
                return;
            }
            const text = getMilkdropTrackTitle();
            const key = getMilkdropTrackKey();
            if (!text || !key || milkdropTitleShownForTrackKey === key) {
                return;
            }
            try {
                milkdropVisualizer.launchSongTitleAnim(text);
                milkdropTitleShownForTrackKey = key;
            } catch (error) {
                console.warn('[Milkdrop] Failed to launch title animation:', error);
            }
        }

        async function loadMilkdropPresetByName(name, blendTime = milkdropBlendSeconds, shouldSave = true, retryCount = 0) {
            await loadMilkdropPresets();
            const entry = milkdropPresetMap.get(name);
            if (!entry) {
                return;
            }

            milkdropPresetName = entry.name;
            syncMilkdropControls();
            const visualizer = await ensureMilkdropVisualizer();
            if (!visualizer) {
                setMilkdropStatus('Waiting for audio context');
                if (shouldSave) {
                    saveSettings();
                }
                return;
            }

            try {
                visualizer.loadPreset(entry.preset, Math.max(0, blendTime));
                loadedMilkdropPresetName = entry.name;
                setMilkdropStatus('');
                syncMilkdropPresetListState();
                if (visualizerEnabled) {
                    try {
                        visualizer.render();
                    } catch (renderError) {
                        console.warn('[Milkdrop] Initial render failed (possibly silent/uninitialized audio):', renderError);
                    }
                }
                //showMilkdropTrackTitleOnce();
                syncMilkdropAnimationState();
                if (shouldSave) {
                    saveSettings();
                }
            } catch (error) {
                milkdropFailedPresetNames.add(entry.name);
                loadedMilkdropPresetName = '';
                setMilkdropStatus('Preset failed, skipping', true);
                syncMilkdropPresetListState();
                console.error('[Milkdrop] Failed to load preset:', error);
                const nextName = pickRandomMilkdropPresetName();
                if (nextName && retryCount < milkdropPresets.length) {
                    await loadMilkdropPresetByName(nextName, blendTime, shouldSave, retryCount + 1);
                } else {
                    setMilkdropStatus('No working presets selected', true);
                }
            }
        }

        async function loadCurrentMilkdropPreset(blendTime = 0) {
            await loadMilkdropPresets();
            if (getEnabledMilkdropPresets().length === 0) {
                setMilkdropStatus('No presets selected');
                syncMilkdropAnimationState();
                return;
            }
            if (!milkdropPresetName || milkdropDisabledPresetNames.has(milkdropPresetName)) {
                milkdropPresetName = pickRandomMilkdropPresetName();
            }
            if (milkdropPresetName && loadedMilkdropPresetName !== milkdropPresetName) {
                await loadMilkdropPresetByName(milkdropPresetName, blendTime, false);
            }
            syncMilkdropAnimationState();
        }

        function selectRandomMilkdropPreset(blendTime = milkdropBlendSeconds, shouldSave = true) {
            const nextName = pickRandomMilkdropPresetName();
            if (nextName) {
                void loadMilkdropPresetByName(nextName, blendTime, shouldSave);
            } else {
                setMilkdropStatus('No presets selected');
            }
        }

        const vhsCommonShader = `
            #define pow2(a) (a * a)
            #define PI 3.1415926535897932384626433832795
            #define THIRD 1.0 / 3.0
            #define BLACK vec4(0.0, 0.0, 0.0, 1.0)
            #define WHITE vec4(1.0)
            #define W vec3(0.2126, 0.7152, 0.0722)
            #define PHI 1.61803398874989484820459
            #define SOURCE_FPS 30.0

            float GetLuminance(vec3 color) {
                return W.r * color.r + W.g * color.g + W.b * color.b;
            }
            float GetLuminance(vec4 color) {
                return W.r * color.r + W.g * color.g + W.b * color.b;
            }
            float GoldNoise(const in vec2 xy, const in float seed) {
                return fract(sin(dot(xy * seed, vec2(12.9898, 78.233))) * 43758.5453);
            }
            float BlendSoftLight(float base, float blend) {
                return (blend<0.5)?(2.0*base*blend+base*base*(1.0-2.0*blend)):(sqrt(base)*(2.0*blend-1.0)+2.0*base*(1.0-blend));
            }
            vec4 BlendSoftLight(vec4 base, vec4 blend) {
                return vec4(BlendSoftLight(base.r,blend.r),BlendSoftLight(base.g,blend.g),BlendSoftLight(base.b,blend.b), 1.0);
            }
            vec4 BlendSoftLight(vec4 base, vec4 blend, float opacity) {
                return (BlendSoftLight(base, blend) * opacity + base * (1.0 - opacity));
            }
            vec4 BlurHorizontal(sampler2D tex, vec2 uv, float blurAmount, vec2 resolution) {
                if (blurAmount <= 0.0) {
                    return texture2D(tex, uv);
                }
                vec4 col = vec4(0.0);
                float total = 0.0;
                float step = blurAmount / resolution.x;
                for (int i = -2; i <= 2; i++) {
                    float weight = 1.0;
                    if (i == -1 || i == 1) weight = 0.6;
                    if (i == -2 || i == 2) weight = 0.3;
                    col += texture2D(tex, uv + vec2(float(i) * step, 0.0)) * weight;
                    total += weight;
                }
                return col / total;
            }
            vec4 Noise(const in float grainSize, const in bool monochromatic, in vec2 fragCoord, float fps) {
                float seed = fps > 0.0 ? floor(fract(u_time) * fps) / fps : u_time;
                seed += 1.0;
                if (grainSize > 1.0) {
                    fragCoord.x = floor(fragCoord.x / grainSize);
                    fragCoord.y = floor(fragCoord.y / grainSize);
                }
                fragCoord.x += 1.0;
                float r = GoldNoise(fragCoord, seed);    
                float g = monochromatic ? r : GoldNoise(fragCoord, seed + 1.0);
                float b = monochromatic ? r : GoldNoise(fragCoord, seed + 2.0);
                return vec4(r, g, b, 1.0);
            }
        `;

        const fsVhsA = `
            precision mediump float;
            uniform vec2 u_resolution;
            uniform float u_time;
            uniform float u_vhs_blur;
            uniform float u_vhs_luma_resolution;
            uniform float u_vhs_chroma_resolution;
            uniform sampler2D u_channel0;

            #define DEFINE(a) (u_resolution.y / 450.0) * a
            ${vhsCommonShader}

            vec4 Shrink(in vec2 fragCoord, const in float shrinkRatio) {
                float scale = 1.0 / u_resolution.x;
                float numBands = u_resolution.x * shrinkRatio;
                float bandWidth = u_resolution.x / numBands;
                float t = mod(fragCoord.x, bandWidth) / bandWidth;
                fragCoord.x = floor(fragCoord.x * shrinkRatio) / shrinkRatio;
                vec2 uv = fragCoord / u_resolution.xy;
                vec4 colorA = BlurHorizontal(u_channel0, uv, u_vhs_blur * 19.2, u_resolution);
                uv.x += bandWidth * scale; 
                vec4 colorB = BlurHorizontal(u_channel0, uv, u_vhs_blur * 19.2, u_resolution);
                return mix(colorA, colorB, t);
            }

            vec3 ClipColor(in vec3 c) {
                float l = GetLuminance(c);
                float n = min(min(c.r, c.g), c.b);
                float x = max(max(c.r, c.g), c.b);
                if (n < 0.0) {
                    c.r = l + (((c.r - l) * l) / (l - n));
                    c.g = l + (((c.g - l) * l) / (l - n));
                    c.b = l + (((c.b - l) * l) / (l - n));
                }
                if (x > 1.0) {
                    c.r = l + (((c.r - l) * (1.0 - l)) / (x - l));
                    c.g = l + (((c.g - l) * (1.0 - l)) / (x - l));
                    c.b = l + (((c.b - l) * (1.0 - l)) / (x - l));
                }
                return c;
            }

            vec3 SetLum(in vec3 c, in float l) {
                float d = l - GetLuminance(c);
                c += d;
                return ClipColor(c);
            }

            vec4 BlendColor(const in vec4 base, const in vec4 blend) {
                vec3 c = SetLum(blend.rgb, GetLuminance(base));
                return vec4(c, blend.a);
            }

            vec4 BlendLuminosity(const in vec4 base, const in vec4 blend) {
                vec3 c = SetLum(base.rgb, GetLuminance(blend));
                return vec4(c, blend.a);
            }

            void main() {
                vec2 fragCoord = gl_FragCoord.xy;
                vec4 luma = Shrink(fragCoord, max(0.001, u_vhs_luma_resolution));
                luma = BlendLuminosity(vec4(0.5, 0.5, 0.5, 1.0), luma);
                vec4 chroma = Shrink(fragCoord, max(0.001, u_vhs_chroma_resolution));
                chroma = BlendColor(luma, chroma);

                gl_FragColor = chroma;
            }
        `;

        const fsVhsB = `
            precision mediump float;
            uniform vec2 u_resolution;
            uniform float u_time;
            uniform float u_vhs_sharpen;
            uniform float u_vhs_sharpen_radius;
            uniform float u_vhs_black_level;
            uniform float u_vhs_white_level;
            uniform float u_vhs_saturation;
            uniform vec3 u_vhs_shadow_tint;
            uniform sampler2D u_channel0;

            #define DEFINE(a) (u_resolution.y / 450.0) * a
            ${vhsCommonShader}

            vec4 UnsharpMask(const in float amount, const in float radius, const in float threshold, const in vec2 fragCoord) {
                vec2 uv = fragCoord / u_resolution.xy;
                vec4 pixel = texture2D(u_channel0, uv);
                vec4 blurPixel = BlurHorizontal(u_channel0, uv, radius, u_resolution);
                float lumDelta = abs(GetLuminance(pixel) - GetLuminance(blurPixel));
                if (lumDelta >= threshold)
                    pixel = pixel + (pixel - blurPixel) * amount;
                return pixel;
            }

            vec4 ClampLevels(in vec4 pixel, const in float blackLevel, const in float whiteLevel) {
                pixel = mix(pixel, BLACK, 1.0 - whiteLevel);
                pixel = mix(pixel, WHITE, blackLevel);
                return pixel;
            }

            vec4 Saturation(vec4 pixel, float adjustment) {
                vec3 intensity = vec3(dot(pixel.rgb, W));
                return vec4(mix(intensity, pixel.rgb, adjustment), 1.0);
            }

            vec4 TintShadows(vec4 pixel, vec3 color) {
                const float POWER = 1.5;
                if (color.r > 0.0)
                    pixel.r = mix(pixel.r, 1.0 - pow(abs(pixel.r - 1.0), POWER), color.r);
                if (color.g > 0.0)
                    pixel.g = mix(pixel.g, 1.0 - pow(abs(pixel.g - 1.0), POWER), color.g);
                if (color.b > 0.0)
                    pixel.b = mix(pixel.b, 1.0 - pow(abs(pixel.b - 1.0), POWER), color.b);
                return pixel;
            }

            const float UNSHARP_THRESHOLD = 0.0;

            void main() {
                vec2 fragCoord = gl_FragCoord.xy;
                float UNSHARP_RADIUS = DEFINE(u_vhs_sharpen_radius);
                vec4 pixel = UnsharpMask(u_vhs_sharpen, UNSHARP_RADIUS, UNSHARP_THRESHOLD, fragCoord);
                pixel = ClampLevels(pixel, u_vhs_black_level, u_vhs_white_level);
                pixel = TintShadows(pixel, u_vhs_shadow_tint);
                pixel = Saturation(pixel, u_vhs_saturation);
                gl_FragColor = pixel;
            }
        `;

        const fsVhsC = `
            precision mediump float;
            uniform vec2 u_resolution;
            uniform float u_time;
            uniform float u_frame;
            uniform float u_vhs_line_height;
            uniform sampler2D u_channel0;
            uniform sampler2D u_channel1;

            #define DEFINE(a) (u_resolution.y / 450.0) * a
            ${vhsCommonShader}

            void main() {
                vec2 fragCoord = gl_FragCoord.xy;
                float LINE_HEIGHT = DEFINE(max(0.001, u_vhs_line_height));
                vec2 uv = fragCoord / u_resolution.xy;
                
                bool updateOddLines = mod(u_frame, 2.0) == 0.0;
                bool isOddLine = mod(floor(fragCoord.y), 2.0 * LINE_HEIGHT) >= LINE_HEIGHT;
                
                vec4 col;
                if (isOddLine && updateOddLines || !isOddLine && !updateOddLines)
                    col = texture2D(u_channel1, uv);
                else
                    col = texture2D(u_channel0, uv);
                    
                gl_FragColor = col;
            }
        `;

        const fsVhsD = `
            precision mediump float;
            uniform vec2 u_resolution;
            uniform float u_time;
            uniform float u_vhs_strength;
            uniform float u_noise_grain;
            uniform float u_noise_grain_size;
            uniform float u_vhs_line_height;
            uniform float u_vhs_tracking_speed;
            uniform float u_vhs_tracking_offset;
            uniform float u_vhs_tracking_jitter;
            uniform float u_vhs_wave_frequency;
            uniform float u_vhs_wave_amount;
            uniform float u_vhs_bottom_warp_height;
            uniform float u_vhs_bottom_warp_offset;
            uniform float u_vhs_bottom_warp_jitter;
            uniform float u_vhs_static_line_height;
            uniform float u_vhs_static_line_opacity;
            uniform sampler2D u_channel0;

            #define DEFINE(a) (u_resolution.y / 450.0) * a
            ${vhsCommonShader}

            vec2 Tracking(const in float speed, const in float offset, const in float jitter, const in vec2 fragCoord) {
                float safeSpeed = max(0.001, speed);
                float t = 1.0 - mod(u_time, safeSpeed) / safeSpeed;
                float trackingStart = mod(t * u_resolution.y, u_resolution.y);
                float trackingJitter = GoldNoise(vec2(5000.0, 5000.0), 10.0 + fract(u_time)) * jitter;
                trackingStart += trackingJitter;
                vec2 uv;
                if (fragCoord.y > trackingStart)
                    uv = (fragCoord + vec2(offset, 0.0)) / u_resolution.xy;
                else
                    uv = fragCoord / u_resolution.xy;
                return uv;
            }

            vec2 Wave(const in float frequency, const in float offset, const in vec2 fragCoord, const in vec2 uv) {
                if (frequency <= 0.0 || offset <= 0.0) {
                    return uv;
                }
                float phaseNumber = floor(fragCoord.y / (u_resolution.y / frequency));
                float offsetNoiseModifier = GoldNoise(vec2(1.0 + phaseNumber, phaseNumber), 10.0);
                float offsetUV = sin((uv.y + fract(u_time * 0.05)) * PI * 2.0 * frequency) * ((offset * offsetNoiseModifier) / u_resolution.x);
                return uv + vec2(offsetUV, 0.0);
            }

            vec4 WarpBottom(const in float height, const in float offset, const in float jitterExtent, in vec2 uv) {
                if (height <= 0.0 || (offset <= 0.0 && jitterExtent <= 0.0)) {
                    return texture2D(u_channel0, uv);
                }
                float uvHeight = height / u_resolution.y;
                if (uv.y > uvHeight)
                    return texture2D(u_channel0, uv);
                float t = uv.y / uvHeight;
                float offsetUV = t * (offset / u_resolution.x);
                float jitterUV = (GoldNoise(vec2(500.0, 500.0), fract(u_time)) * jitterExtent) / u_resolution.x; 
                uv = vec2(uv.x - offsetUV - jitterUV, uv.y);
                vec4 pixel = texture2D(u_channel0, uv);
                pixel = pixel * t;
                return pixel;
            }

            vec4 WhiteNoise(const in float lineThickness, const in float opacity, const in vec4 pixel, const in vec2 fragCoord) {
                if (lineThickness <= 0.0 || opacity <= 0.0) {
                    return pixel;
                }
                if (GoldNoise(vec2(600.0, 500.0), fract(u_time) * 10.0) > 0.97) {
                    float lineStart = floor(GoldNoise(vec2(800.0, 50.0), fract(u_time)) * u_resolution.y);
                    float lineEnd = floor(lineStart + lineThickness);
                    if (floor(fragCoord.y) >= lineStart && floor(fragCoord.y) < lineEnd) {
                        float frequency = GoldNoise(vec2(850.0, 50.0), fract(u_time)) * 3.0 + 1.0;
                        float offset = GoldNoise(vec2(900.0, 51.0), fract(u_time));            
                        float x = floor(fragCoord.x) / floor(u_resolution.x) + offset;
                        float white = pow(cos(PI * fract(x * frequency) / 2.0), 10.0) * opacity;
                        float grit = GoldNoise(vec2(floor(fragCoord.x / 3.0), 800.0), fract(u_time));
                        white = max(white - grit * 0.3, 0.0);
                        return pixel + white;
                    }
                }
                return pixel;
            }

            void main() {
                vec2 fragCoord = gl_FragCoord.xy;
                float TRACKING_HORIZONTAL_OFFSET = DEFINE(u_vhs_tracking_offset) * u_vhs_strength;
                float WAVE_OFFSET = DEFINE(u_vhs_wave_amount) * u_vhs_strength;
                float BOTTOM_WARP_HEIGHT = DEFINE(u_vhs_bottom_warp_height) * u_vhs_strength;
                float BOTTOM_WARP_OFFSET = DEFINE(u_vhs_bottom_warp_offset) * u_vhs_strength;
                float BOTTOM_WARP_JITTER_EXTENT = DEFINE(u_vhs_bottom_warp_jitter) * u_vhs_strength;
                float NOISE_HEIGHT = DEFINE(u_vhs_static_line_height);

                vec2 uv = Tracking(u_vhs_tracking_speed, TRACKING_HORIZONTAL_OFFSET, u_vhs_tracking_jitter * u_vhs_strength, fragCoord);
                uv = Wave(u_vhs_wave_frequency, WAVE_OFFSET, fragCoord, uv);
                
                vec4 pixel = WarpBottom(BOTTOM_WARP_HEIGHT, BOTTOM_WARP_OFFSET, BOTTOM_WARP_JITTER_EXTENT, uv);
                
                // Apply analog grain noise to both background and lyrics text!
                float LINE_HEIGHT = DEFINE(u_vhs_line_height);
                float NOISE_GRAIN_SIZE = DEFINE(u_noise_grain_size);
                float grain = (Noise(NOISE_GRAIN_SIZE, true, fragCoord, SOURCE_FPS).r - 0.5) * 0.22 * u_noise_grain;
                pixel.rgb = clamp(pixel.rgb + vec3(grain), 0.0, 1.0);

                // Apply white static noise lines on top of both!
                pixel = WhiteNoise(NOISE_HEIGHT, u_vhs_static_line_opacity * u_noise_grain, pixel, fragCoord);
                
                gl_FragColor = pixel;
            }
        `;

        const fsVhsImage = `
            precision mediump float;
            uniform vec2 u_resolution;
            uniform float u_time;
            uniform float u_vhs_amount;
            uniform float u_vhs_vignette;
            uniform float u_vhs_vignette_power;
            uniform float u_vhs_vignette_boost;
            uniform sampler2D u_channel0;
            uniform sampler2D u_channel1;

            vec4 Televisionfy(in vec4 pixel, const in vec2 uv) {
                float vignette = pow(uv.x * (1.0 - uv.x) * uv.y * (1.0 - uv.y), u_vhs_vignette_power) * u_vhs_vignette_boost;
                return pixel * mix(1.0, vignette, u_vhs_vignette);
            }

            void main() {
                vec2 uv = gl_FragCoord.xy / u_resolution.xy;
                vec4 mixedCol = texture2D(u_channel0, uv);
                vec4 originalCol = texture2D(u_channel1, uv);
                gl_FragColor = mix(originalCol, Televisionfy(mixedCol, uv), u_vhs_amount);
            }
        `;

        function resizeVhsTextures(width, height) {
            if (!vhsInitialized || !ntscGl) return;
            const gl = ntscGl;
            
            function resizeTex(tex) {
                gl.bindTexture(gl.TEXTURE_2D, tex);
                gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
            }
            
            resizeTex(texA);
            resizeTex(texB);
            resizeTex(texC1);
            resizeTex(texC2);
            resizeTex(texD);
            resizeTex(texBase);
            
            gl.bindTexture(gl.TEXTURE_2D, null);
        }

        function initVhsWebGL() {
            if (vhsInitialized || !ntscGl) return;
            const gl = ntscGl;
            isWebGL2 = (gl.createVertexArray !== undefined);

            function compileShader(source, type) {
                const shader = gl.createShader(type);
                gl.shaderSource(shader, source);
                gl.compileShader(shader);
                if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
                    console.error('Shader compile error:', gl.getShaderInfoLog(shader));
                }
                return shader;
            }

            function compileVhsProgram(fsSource) {
                const vs = gl.createShader(gl.VERTEX_SHADER);
                gl.shaderSource(vs, `
                    attribute vec2 position;
                    void main() {
                        gl_Position = vec4(position, 0.0, 1.0);
                    }
                `);
                gl.compileShader(vs);
                const fs = compileShader(fsSource, gl.FRAGMENT_SHADER);
                const prog = gl.createProgram();
                gl.attachShader(prog, vs);
                gl.attachShader(prog, fs);
                gl.linkProgram(prog);
                return prog;
            }

            vhsProgA = compileVhsProgram(fsVhsA);
            vhsProgB = compileVhsProgram(fsVhsB);
            vhsProgC = compileVhsProgram(fsVhsC);
            vhsProgD = compileVhsProgram(fsVhsD);
            vhsProgImage = compileVhsProgram(fsVhsImage);

            function createFboTexture() {
                const tex = gl.createTexture();
                gl.bindTexture(gl.TEXTURE_2D, tex);
                gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
                gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
                gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
                gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
                return tex;
            }

            function createFramebuffer(tex) {
                const fbo = gl.createFramebuffer();
                gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
                gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
                return fbo;
            }

            texA = createFboTexture(); fboA = createFramebuffer(texA);
            texB = createFboTexture(); fboB = createFramebuffer(texB);
            texC1 = createFboTexture(); fboC1 = createFramebuffer(texC1);
            texC2 = createFboTexture(); fboC2 = createFramebuffer(texC2);
            texD = createFboTexture(); fboD = createFramebuffer(texD);
            texBase = createFboTexture(); fboBase = createFramebuffer(texBase);

            // Configure ntscTexture and ntscTextTexture using LINEAR minification
            gl.bindTexture(gl.TEXTURE_2D, ntscTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);

            gl.bindTexture(gl.TEXTURE_2D, ntscTextTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);

            gl.bindTexture(gl.TEXTURE_2D, ntscPlayerTextTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);

            // Create 1x1 transparent dummy texture for chained NTSC background smear
            dummyTextTex = gl.createTexture();
            gl.bindTexture(gl.TEXTURE_2D, dummyTextTex);
            gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 0, 0, 0]));
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

            gl.bindFramebuffer(gl.FRAMEBUFFER, null);
            gl.bindTexture(gl.TEXTURE_2D, null);

            vhsInitialized = true;
            resizeVhsTextures(canvas.width, canvas.height);
        }

        function initNtscWebGL() {
            if (ntscGl) return;
            ntscGl = canvas.getContext('webgl2', { alpha: false, depth: false, antialias: false }) ||
                     canvas.getContext('webgl', { alpha: false, depth: false, antialias: false });
            if (!ntscGl) {
                console.warn('[NTSC] WebGL not supported on main canvas');
                return;
            }
            const gl = ntscGl;
            lyricsCtx = lyricsCanvas.getContext('2d');
            playerTextCtx = playerTextCanvas.getContext('2d');
            gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
            
            const vsSource = `
                attribute vec2 position;
                varying vec2 uv;
                void main() {
                    uv = position * 0.5 + 0.5;
                    gl_Position = vec4(position, 0.0, 1.0);
                }
            `;

            const playerBlendShader = `
                uniform sampler2D PlayerTextSource;
                uniform int u_player_text_blend_mode;

                float ColorDodge(float base, float blend) {
                    return blend >= 1.0 ? 1.0 : min(base / max(0.0001, 1.0 - blend), 1.0);
                }

                float ColorBurn(float base, float blend) {
                    return blend <= 0.0 ? 0.0 : 1.0 - min((1.0 - base) / max(0.0001, blend), 1.0);
                }

                float SoftLight(float base, float blend) {
                    if (blend <= 0.5) {
                        return base - (1.0 - 2.0 * blend) * base * (1.0 - base);
                    }
                    float d = base <= 0.25
                        ? ((16.0 * base - 12.0) * base + 4.0) * base
                        : sqrt(base);
                    return base + (2.0 * blend - 1.0) * (d - base);
                }

                vec3 RgbToHsv(vec3 c) {
                    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
                    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
                    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
                    float d = q.x - min(q.w, q.y);
                    float e = 1.0e-10;
                    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
                }

                vec3 HsvToRgb(vec3 c) {
                    vec3 p = abs(fract(c.xxx + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
                    return c.z * mix(vec3(1.0), clamp(p - 1.0, 0.0, 1.0), c.y);
                }

                vec3 ApplyPlayerBlendMode(vec3 base, vec3 blend, int mode) {
                    if (mode == 1) return base * blend;
                    if (mode == 2) return 1.0 - (1.0 - base) * (1.0 - blend);
                    if (mode == 3) return mix(2.0 * base * blend, 1.0 - 2.0 * (1.0 - base) * (1.0 - blend), step(0.5, base));
                    if (mode == 4) return min(base, blend);
                    if (mode == 5) return max(base, blend);
                    if (mode == 6) return vec3(ColorDodge(base.r, blend.r), ColorDodge(base.g, blend.g), ColorDodge(base.b, blend.b));
                    if (mode == 7) return vec3(ColorBurn(base.r, blend.r), ColorBurn(base.g, blend.g), ColorBurn(base.b, blend.b));
                    if (mode == 8) return mix(2.0 * base * blend, 1.0 - 2.0 * (1.0 - base) * (1.0 - blend), step(0.5, blend));
                    if (mode == 9) return vec3(SoftLight(base.r, blend.r), SoftLight(base.g, blend.g), SoftLight(base.b, blend.b));
                    if (mode == 10) return abs(base - blend);
                    if (mode == 11) return base + blend - 2.0 * base * blend;
                    if (mode == 12) {
                        vec3 baseHsv = RgbToHsv(base);
                        vec3 blendHsv = RgbToHsv(blend);
                        return HsvToRgb(vec3(blendHsv.x, baseHsv.y, baseHsv.z));
                    }
                    if (mode == 13) {
                        vec3 baseHsv = RgbToHsv(base);
                        vec3 blendHsv = RgbToHsv(blend);
                        return HsvToRgb(vec3(baseHsv.x, blendHsv.y, baseHsv.z));
                    }
                    if (mode == 14) {
                        vec3 baseHsv = RgbToHsv(base);
                        vec3 blendHsv = RgbToHsv(blend);
                        return HsvToRgb(vec3(blendHsv.x, blendHsv.y, baseHsv.z));
                    }
                    if (mode == 15) {
                        vec3 baseHsv = RgbToHsv(base);
                        vec3 blendHsv = RgbToHsv(blend);
                        return HsvToRgb(vec3(baseHsv.x, baseHsv.y, blendHsv.z));
                    }
                    if (mode == 16) return max(base + blend - 1.0, 0.0);
                    if (mode == 17) return min(base + blend, 1.0);
                    return blend;
                }

                vec3 CompositePlayerText(vec3 base, vec2 uvCoord) {
                    vec4 player = texture2D(PlayerTextSource, uvCoord);
                    vec3 blended = ApplyPlayerBlendMode(base, player.rgb, u_player_text_blend_mode);
                    return mix(base, blended, player.a);
                }
            `;
            
            const fsPassSource = `
                precision mediump float;
                varying vec2 uv;
                uniform sampler2D Source;
                uniform sampler2D TextSource;
                ${playerBlendShader}
                void main() {
                    vec4 vis = texture2D(Source, uv);
                    vec4 txt = texture2D(TextSource, uv);
                    vec3 base = mix(vis.rgb, txt.rgb, txt.a);
                    gl_FragColor = vec4(CompositePlayerText(base, uv), 1.0);
                }
            `;
            
            const fsNtscSource = `
                precision mediump float;
                varying vec2 uv;
                uniform vec2 u_resolution;
                uniform sampler2D Source;
                uniform sampler2D TextSource;
                uniform float TIME;
                uniform float wiggle;
                uniform float wiggle_speed;
                uniform float smear;
                uniform float u_ntsc_amount;
                uniform float u_ntsc_chroma_shift;

                const int blur_samples = 15;
                const float NTSC_REF_WIDTH = 1920.0;
                ${playerBlendShader}

                float onOff(float a, float b, float c, float framecount) {
                    return step(c, sin((framecount * 0.001) + a * cos((framecount * 0.001) * b)));
                }

                vec2 jumpy(vec2 uvCoord, float framecount) {
                    vec2 look = uvCoord;
                    float window = 1.0 / (1.0 + 80.0 * (look.y - mod(framecount / 4.0, 1.0)) * (look.y - mod(framecount / 4.0, 1.0)));
                    look.x += 0.05 * sin(look.y * 10.0 + framecount) / 20.0 * onOff(4.0, 4.0, 0.3, framecount) * (0.5 + cos(framecount * 20.0)) * window;
                    float vShift = (0.1 * wiggle) * 0.4 * onOff(2.0, 3.0, 0.9, framecount) * (sin(framecount) * sin(framecount * 20.0) + (0.5 + 0.1 * sin(framecount * 200.0) * cos(framecount)));
                    look.y = mod(look.y - 0.01 * vShift, 1.0);
                    return look;
                }

                vec2 Circle(float Start, float Points, float Point) {
                    float Rad = (3.141592 * 2.0 * (1.0 / Points)) * (Point + Start);
                    return vec2(-(0.3 + Rad), cos(Rad));
                }

                vec3 rgb2yiq(vec3 c) {
                    return vec3(
                        (0.2989 * c.x + 0.5959 * c.y + 0.2115 * c.z),
                        (0.5870 * c.x - 0.2744 * c.y - 0.5229 * c.z),
                        (0.1140 * c.x - 0.3216 * c.y + 0.3114 * c.z)
                    );
                }

                vec3 yiq2rgb(vec3 c) {
                    return vec3(
                        (1.0 * c.x + 1.0 * c.y + 1.0 * c.z),
                        (0.956 * c.x - 0.2720 * c.y - 1.1060 * c.z),
                        (0.6210 * c.x - 0.6474 * c.y + 1.7046 * c.z)
                    );
                }

                vec4 SampleComposite(vec2 uvCoord) {
                    vec4 vis = texture2D(Source, uvCoord);
                    vec4 txt = texture2D(TextSource, uvCoord);
                    vec3 base = mix(vis.rgb, txt.rgb, txt.a);
                    return vec4(CompositePlayerText(base, uvCoord), 1.0);
                }

                vec4 BlurCompositeRGBA(vec2 uvCoord, float d) {
                    vec4 sum = vec4(0.0);
                    float W = 1.0 / float(blur_samples);
                    for (int i = 0; i < blur_samples; ++i) {
                        float t = 0.0;
                        vec2 PixelOffset = vec2(d + 0.0005 * t, 0.0);
                        
                        float Start = 2.0 / float(blur_samples);
                        vec2 Scale = 0.66 * 4.0 * 2.0 * PixelOffset.xy;
                        
                        vec4 N = SampleComposite(uvCoord + Circle(Start, float(blur_samples), float(i)) * Scale);
                        sum += N * W;
                    }
                    return sum;
                }

                void main() {
                    vec2 uvCoord = uv;
                    float horizontalScale = NTSC_REF_WIDTH / u_resolution.x;
                    float chromaShift = max(0.0, u_ntsc_chroma_shift);

                    float d = 0.1 - floor(mod(TIME/3.0, 1.0) + 0.5) * 0.1;
                    uvCoord = jumpy(uvCoord, mod(TIME * wiggle_speed, 7.0));

                    float s = 0.0001 * -d + 0.0001 * wiggle * (sin(TIME * wiggle_speed));
                    float e = min(0.30, pow(max(0.0, cos(uvCoord.y * 4.0 + 0.3) - 0.75) * (s + 0.5) * 1.0, 3.0)) * 25.0;
                    float r = (250.0 * (2.0 * s));
                    uvCoord.x += abs(r * pow(min(0.003, (-uvCoord.y + (0.01 * mod(TIME, 5.0)))) * 3.0, 2.0)) * wiggle;
                    
                    d = 0.051 + abs(sin(s / 4.0));
                    float c = max(0.0001, 0.002 * d) * smear;
                    
                    vec4 signalColor;
                    signalColor.rgb = BlurCompositeRGBA(uvCoord, (c + c * uvCoord.x) * horizontalScale).rgb;
                    float y_signal = rgb2yiq(signalColor.rgb).r;

                    vec2 uv_signal_i = uvCoord + vec2(0.01 * d * horizontalScale * chromaShift, 0.0);
                    signalColor.rgb = BlurCompositeRGBA(uv_signal_i, c * 6.0 * horizontalScale * chromaShift).rgb;
                    float i_signal = rgb2yiq(signalColor.rgb).g;

                    vec2 uv_signal_q = uvCoord + vec2(0.015 * d * horizontalScale * chromaShift, 0.0);
                    signalColor.rgb = BlurCompositeRGBA(uv_signal_q, c * 15.0 * horizontalScale * chromaShift).rgb;
                    float q_signal = rgb2yiq(signalColor.rgb).b;

                    vec3 finalSignal = yiq2rgb(vec3(y_signal, i_signal, q_signal)) - pow(s + e * 2.0, 3.0);

                    vec4 finalColor;
                    finalColor.rgb = finalSignal;
                    finalColor.a = 1.0;

                    vec4 originalColor = SampleComposite(uv);

                    gl_FragColor = mix(originalColor, finalColor, u_ntsc_amount);
                }
            `;
            
            function compileProgram(fsSource) {
                const vs = gl.createShader(gl.VERTEX_SHADER);
                gl.shaderSource(vs, vsSource);
                gl.compileShader(vs);
                if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) {
                    console.error('[NTSC] VS Compile Error:', gl.getShaderInfoLog(vs));
                }
                
                const fs = gl.createShader(gl.FRAGMENT_SHADER);
                gl.shaderSource(fs, fsSource);
                gl.compileShader(fs);
                if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
                    console.error('[NTSC] FS Compile Error:', gl.getShaderInfoLog(fs));
                }
                
                const prog = gl.createProgram();
                gl.attachShader(prog, vs);
                gl.attachShader(prog, fs);
                gl.linkProgram(prog);
                if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
                    console.error('[NTSC] Program link error:', gl.getProgramInfoLog(prog));
                }
                return prog;
            }
            
            passProgram = compileProgram(fsPassSource);
            ntscProgram = compileProgram(fsNtscSource);
            
            const vertices = new Float32Array([
                -1, -1,
                 1, -1,
                -1,  1,
                -1,  1,
                 1, -1,
                 1,  1
            ]);
            ntscBuffer = gl.createBuffer();
            gl.bindBuffer(gl.ARRAY_BUFFER, ntscBuffer);
            gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
            
            ntscTexture = gl.createTexture();
            gl.bindTexture(gl.TEXTURE_2D, ntscTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
            
            ntscTextTexture = gl.createTexture();
            gl.bindTexture(gl.TEXTURE_2D, ntscTextTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

            ntscPlayerTextTexture = gl.createTexture();
            gl.bindTexture(gl.TEXTURE_2D, ntscPlayerTextTexture);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
            gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        }

        function getWrappedLines(ctx, text, maxWidth) {
            const words = text.split(' ');
            const lines = [];
            let currentLine = '';
            
            for (let i = 0; i < words.length; i++) {
                const word = words[i];
                const testLine = currentLine ? currentLine + ' ' + word : word;
                const metrics = ctx.measureText(testLine);
                if (metrics.width > maxWidth && currentLine) {
                    lines.push(currentLine);
                    currentLine = word;
                } else {
                    currentLine = testLine;
                }
            }
            if (currentLine) {
                lines.push(currentLine);
            }
            return lines;
        }

        function getStyleNoTransition(className, isActive) {
            const dummy = document.createElement('div');
            dummy.className = className;
            if (isActive) {
                dummy.classList.add('active');
            }
            dummy.style.position = 'absolute';
            dummy.style.visibility = 'hidden';
            dummy.style.display = 'block';
            dummy.style.transition = 'none';
            dummy.style.setProperty('transition', 'none', 'important');
            dummy.style.setProperty('animation', 'none', 'important');
            
            const parent = document.querySelector('.lyrics-scroll-content') || document.body;
            parent.appendChild(dummy);
            
            const styles = getComputedStyle(dummy);
            const result = {
                fontSize: parseFloat(styles.fontSize),
                fontFamily: styles.fontFamily,
                color: styles.color,
                lineHeight: parseFloat(styles.lineHeight),
                opacity: parseFloat(styles.opacity)
            };
            
            parent.removeChild(dummy);
            return result;
        }

        function drawCanvasLyrics() {
            if (!lyricsCtx) return;
            lyricsCtx.clearRect(0, 0, lyricsCanvas.width, lyricsCanvas.height);
            if (!lyricsVisible || !isVideoEffectActive() || syncedLines.length === 0) return;

            const isSmallScreen = window.innerWidth <= 900;

            const inactiveStyles = getStyleNoTransition('lyric-line', false);
            const activeStyles = getStyleNoTransition('lyric-line', true);

            const fontFamily = activeStyles.fontFamily || 'Outfit, sans-serif';
            
            const defaultActiveFontSize = Math.round((isSmallScreen ? 34 : 46) * lyricTextScale);
            const defaultInactiveFontSize = Math.round((isSmallScreen ? 25 : 32) * lyricTextScale);

            const activeFontSize = (Number.isFinite(activeStyles.fontSize) ? activeStyles.fontSize : defaultActiveFontSize) * 1.025;
            const inactiveFontSize = Number.isFinite(inactiveStyles.fontSize) ? inactiveStyles.fontSize : defaultInactiveFontSize;
            const activeLineHeight = Number.isFinite(activeStyles.lineHeight) ? activeStyles.lineHeight : activeFontSize * lyricsLineHeight;
            const inactiveLineHeight = Number.isFinite(inactiveStyles.lineHeight) ? inactiveStyles.lineHeight : inactiveFontSize * lyricsLineHeight;

            const activeFillColor = activeStyles.color || '#ffffff';
            const inactiveFillColor = inactiveStyles.color || 'rgba(255,255,255,0.12)';

            const activeBaseOpacity = (activeStyles.opacity === inactiveStyles.opacity)
                ? 1.0
                : (Number.isFinite(activeStyles.opacity) ? activeStyles.opacity : 1.0);
            const inactiveBaseOpacity = Number.isFinite(inactiveStyles.opacity) ? inactiveStyles.opacity : 0.12;

            const containerWidth = isSmallScreen
                ? (lyricsCanvas.width - 48)
                : (lyricsCanvas.width * 0.76);
            const wrapLimit = containerWidth * (lyricsWrapWidth / 100);

            const lineData = [];

            for (let i = 0; i < syncedLines.length; i++) {
                const text = syncedLines[i].text.trim();
                const isActive = (i === activeLineIndex);
                const fontSize = isActive ? activeFontSize : inactiveFontSize;
                
                lyricsCtx.font = `${lyricsWeight} ${fontSize}px ${fontFamily}`;
                if (typeof lyricsCtx.wordSpacing !== 'undefined') {
                    lyricsCtx.wordSpacing = lyricsWordSpacing + 'px';
                }
                if (typeof lyricsCtx.letterSpacing !== 'undefined') {
                    lyricsCtx.letterSpacing = lyricsLetterSpacing + 'px';
                }
                const wrapped = getWrappedLines(lyricsCtx, text, wrapLimit);
                const lineHeight = isActive ? activeLineHeight : inactiveLineHeight;
                const height = wrapped.length * lineHeight;
                
                const distance = Math.abs(i - activeLineIndex);
                const opacity = isActive
                    ? activeBaseOpacity
                    : (() => {
                        if (lyricFadeCurve <= 0.0) return 0.0;
                        const maxVal = lyricsFadeSlider ? parseFloat(lyricsFadeSlider.max) : 5.0;
                        const factor = maxVal > 0 ? (lyricFadeCurve / maxVal) : 0.0;
                        return Math.pow(factor, distance);
                    })();

                const fillStyle = isActive ? activeFillColor : inactiveFillColor;

                lineData.push({
                    wrapped,
                    fontSize,
                    lineHeight,
                    height,
                    opacity,
                    fillStyle
                });
            }

            const centerIdx = activeLineIndex === -1 ? 0 : activeLineIndex;
            const centerData = lineData[centerIdx];
            if (!centerData) return;

            const centerY = lyricsCanvas.height / 2;
            const centerTopY = centerY - (centerData.height / 2);
            lineData[centerIdx].y = centerTopY;

            let currentY = centerTopY;
            for (let i = centerIdx - 1; i >= 0; i--) {
                currentY -= (lineData[i].height + 40);
                lineData[i].y = currentY;
            }

            currentY = centerTopY + centerData.height;
            for (let i = centerIdx + 1; i < syncedLines.length; i++) {
                currentY += 40;
                lineData[i].y = currentY;
                currentY += lineData[i].height;
            }

            const x = lyricsCanvas.width / 2;
            lyricsCtx.textAlign = 'center';
            lyricsCtx.textBaseline = 'middle';

            for (let i = 0; i < lineData.length; i++) {
                const item = lineData[i];

                if (item.y + item.height < 0 || item.y > lyricsCanvas.height) {
                    continue;
                }

                lyricsCtx.font = `${lyricsWeight} ${item.fontSize}px ${fontFamily}`;
                if (typeof lyricsCtx.wordSpacing !== 'undefined') {
                    lyricsCtx.wordSpacing = lyricsWordSpacing + 'px';
                }

                if (typeof lyricsCtx.letterSpacing !== 'undefined') {
                    lyricsCtx.letterSpacing = lyricsLetterSpacing + 'px';
                }
                
                lyricsCtx.globalAlpha = item.opacity;
                
                lyricsCtx.strokeStyle = '#000000';
                lyricsCtx.lineWidth = Math.max(4, Math.round(item.fontSize / 6));
                lyricsCtx.lineJoin = 'round';
                
                lyricsCtx.fillStyle = item.fillStyle;

                let drawY = item.y + (item.fontSize / 2);
                for (let j = 0; j < item.wrapped.length; j++) {
                    if (lyricsBorderWidth > 0) {
                        lyricsCtx.lineWidth = lyricsBorderWidth;
                        lyricsCtx.strokeText(item.wrapped[j], x, drawY);
                    }
                    lyricsCtx.fillText(item.wrapped[j], x, drawY);
                    drawY += item.lineHeight;
                }
            }
            
            lyricsCtx.globalAlpha = 1.0;
        }

        function triggerNtscRedraw() {
            if (isVideoEffectActive() && !isPlaying) {
                renderNtscFrame(performance.now() / 1000.0);
            }
        }

        function applyNtscState() {
            const effectsRendering = isVideoEffectActive();
            if (effectsRendering && canvas) {
                initNtscWebGL();
            }
            if (scrollPanel) {
                scrollPanel.style.visibility = effectsRendering ? 'hidden' : 'visible';
            }
            drawCanvasLyrics();
            updateVisualizerCanvasOpacity();
            triggerNtscRedraw();
        }

        function renderNtscFrame(timeValue) {
            if (!ntscGl || !ntscProgram || !passProgram) return;
            const gl = ntscGl;
            drawCanvasPlayerText();

            if (isVhsActive()) {
                initVhsWebGL();
                const visualizerSourceCanvas = getVisualizerSourceCanvas();
                
                // 1. Upload offscreen visualizer canvas
                gl.activeTexture(gl.TEXTURE0);
                gl.bindTexture(gl.TEXTURE_2D, ntscTexture);
                gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, visualizerSourceCanvas);

                // 2. Upload transparent lyrics canvas
                gl.activeTexture(gl.TEXTURE1);
                gl.bindTexture(gl.TEXTURE_2D, ntscTextTexture);
                gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, lyricsCanvas);

                // 3. Upload player text blend canvas
                gl.activeTexture(gl.TEXTURE2);
                gl.bindTexture(gl.TEXTURE_2D, ntscPlayerTextTexture);
                gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, playerTextCanvas);

                gl.bindBuffer(gl.ARRAY_BUFFER, ntscBuffer);
                
                // Helper to draw a pass
                const drawPass = (prog, fbo, channel0, channel1, hasFrame) => {
                    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
                    gl.viewport(0, 0, canvas.width, canvas.height);
                    gl.useProgram(prog);
                    
                    const posAttrib = gl.getAttribLocation(prog, 'position');
                    gl.enableVertexAttribArray(posAttrib);
                    gl.vertexAttribPointer(posAttrib, 2, gl.FLOAT, false, 0, 0);
                    
                    gl.uniform2f(gl.getUniformLocation(prog, 'u_resolution'), canvas.width, canvas.height);
                    gl.uniform1f(gl.getUniformLocation(prog, 'u_time'), timeValue);
                    if (hasFrame) {
                        gl.uniform1f(gl.getUniformLocation(prog, 'u_frame'), vhsFrameCount);
                    }
                    
                    const locNoise = gl.getUniformLocation(prog, 'u_noise_grain');
                    if (locNoise) gl.uniform1f(locNoise, vhsNoise);

                    const locGrainSize = gl.getUniformLocation(prog, 'u_noise_grain_size');
                    if (locGrainSize) gl.uniform1f(locGrainSize, vhsGrainSize);
                    
                    const locStrength = gl.getUniformLocation(prog, 'u_vhs_strength');
                    if (locStrength) gl.uniform1f(locStrength, vhsStrength);
                    
                    const locBlur = gl.getUniformLocation(prog, 'u_vhs_blur');
                    if (locBlur) gl.uniform1f(locBlur, vhsBlur);

                    const locVhsAmount = gl.getUniformLocation(prog, 'u_vhs_amount');
                    if (locVhsAmount) gl.uniform1f(locVhsAmount, vhsAmount);

                    const locVhsVignette = gl.getUniformLocation(prog, 'u_vhs_vignette');
                    if (locVhsVignette) gl.uniform1f(locVhsVignette, vhsVignette);

                    const locLumaResolution = gl.getUniformLocation(prog, 'u_vhs_luma_resolution');
                    if (locLumaResolution) gl.uniform1f(locLumaResolution, vhsLumaResolution);

                    const locChromaResolution = gl.getUniformLocation(prog, 'u_vhs_chroma_resolution');
                    if (locChromaResolution) gl.uniform1f(locChromaResolution, vhsChromaResolution);

                    const locLineHeight = gl.getUniformLocation(prog, 'u_vhs_line_height');
                    if (locLineHeight) gl.uniform1f(locLineHeight, vhsLineHeight);

                    const locSharpen = gl.getUniformLocation(prog, 'u_vhs_sharpen');
                    if (locSharpen) gl.uniform1f(locSharpen, vhsSharpen);

                    const locSharpenRadius = gl.getUniformLocation(prog, 'u_vhs_sharpen_radius');
                    if (locSharpenRadius) gl.uniform1f(locSharpenRadius, vhsSharpenRadius);

                    const locBlackLevel = gl.getUniformLocation(prog, 'u_vhs_black_level');
                    if (locBlackLevel) gl.uniform1f(locBlackLevel, vhsBlackLevel);

                    const locWhiteLevel = gl.getUniformLocation(prog, 'u_vhs_white_level');
                    if (locWhiteLevel) gl.uniform1f(locWhiteLevel, vhsWhiteLevel);

                    const locSaturation = gl.getUniformLocation(prog, 'u_vhs_saturation');
                    if (locSaturation) gl.uniform1f(locSaturation, vhsSaturation);

                    const locShadowTint = gl.getUniformLocation(prog, 'u_vhs_shadow_tint');
                    if (locShadowTint) {
                        const [r, g, b] = hexToRgb01(vhsShadowTint);
                        gl.uniform3f(locShadowTint, r, g, b);
                    }

                    const locTrackingSpeed = gl.getUniformLocation(prog, 'u_vhs_tracking_speed');
                    if (locTrackingSpeed) gl.uniform1f(locTrackingSpeed, vhsTrackingSpeed);

                    const locTrackingOffset = gl.getUniformLocation(prog, 'u_vhs_tracking_offset');
                    if (locTrackingOffset) gl.uniform1f(locTrackingOffset, vhsTrackingOffset);

                    const locTrackingJitter = gl.getUniformLocation(prog, 'u_vhs_tracking_jitter');
                    if (locTrackingJitter) gl.uniform1f(locTrackingJitter, vhsTrackingJitter);

                    const locWaveFrequency = gl.getUniformLocation(prog, 'u_vhs_wave_frequency');
                    if (locWaveFrequency) gl.uniform1f(locWaveFrequency, vhsWaveFrequency);

                    const locWaveAmount = gl.getUniformLocation(prog, 'u_vhs_wave_amount');
                    if (locWaveAmount) gl.uniform1f(locWaveAmount, vhsWaveAmount);

                    const locBottomWarpHeight = gl.getUniformLocation(prog, 'u_vhs_bottom_warp_height');
                    if (locBottomWarpHeight) gl.uniform1f(locBottomWarpHeight, vhsBottomWarpHeight);

                    const locBottomWarpOffset = gl.getUniformLocation(prog, 'u_vhs_bottom_warp_offset');
                    if (locBottomWarpOffset) gl.uniform1f(locBottomWarpOffset, vhsBottomWarpOffset);

                    const locBottomWarpJitter = gl.getUniformLocation(prog, 'u_vhs_bottom_warp_jitter');
                    if (locBottomWarpJitter) gl.uniform1f(locBottomWarpJitter, vhsBottomWarpJitter);

                    const locStaticLineHeight = gl.getUniformLocation(prog, 'u_vhs_static_line_height');
                    if (locStaticLineHeight) gl.uniform1f(locStaticLineHeight, vhsStaticLineHeight);

                    const locStaticLineOpacity = gl.getUniformLocation(prog, 'u_vhs_static_line_opacity');
                    if (locStaticLineOpacity) gl.uniform1f(locStaticLineOpacity, vhsStaticLineOpacity);

                    const locVignettePower = gl.getUniformLocation(prog, 'u_vhs_vignette_power');
                    if (locVignettePower) gl.uniform1f(locVignettePower, vhsVignettePower);

                    const locVignetteBoost = gl.getUniformLocation(prog, 'u_vhs_vignette_boost');
                    if (locVignetteBoost) gl.uniform1f(locVignetteBoost, vhsVignetteBoost);

                    gl.activeTexture(gl.TEXTURE0);
                    gl.bindTexture(gl.TEXTURE_2D, channel0);
                    gl.uniform1i(gl.getUniformLocation(prog, 'u_channel0'), 0);
                    
                    if (channel1) {
                        gl.activeTexture(gl.TEXTURE1);
                        gl.bindTexture(gl.TEXTURE_2D, channel1);
                        gl.uniform1i(gl.getUniformLocation(prog, 'u_channel1'), 1);
                    }
                    
                    gl.drawArrays(gl.TRIANGLES, 0, 6);
                };

                const drawSourceComposite = (fbo, sourceTexture, textTexture, playerTexture) => {
                    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
                    gl.viewport(0, 0, canvas.width, canvas.height);
                    gl.useProgram(passProgram);

                    const posAttrib = gl.getAttribLocation(passProgram, 'position');
                    gl.enableVertexAttribArray(posAttrib);
                    gl.vertexAttribPointer(posAttrib, 2, gl.FLOAT, false, 0, 0);

                    gl.activeTexture(gl.TEXTURE0);
                    gl.bindTexture(gl.TEXTURE_2D, sourceTexture);
                    gl.uniform1i(gl.getUniformLocation(passProgram, 'Source'), 0);

                    gl.activeTexture(gl.TEXTURE1);
                    gl.bindTexture(gl.TEXTURE_2D, textTexture);
                    gl.uniform1i(gl.getUniformLocation(passProgram, 'TextSource'), 1);

                    gl.activeTexture(gl.TEXTURE2);
                    gl.bindTexture(gl.TEXTURE_2D, playerTexture);
                    gl.uniform1i(gl.getUniformLocation(passProgram, 'PlayerTextSource'), 2);
                    gl.uniform1i(gl.getUniformLocation(passProgram, 'u_player_text_blend_mode'), getPlayerTextBlendModeId());

                    gl.drawArrays(gl.TRIANGLES, 0, 6);
                };

                const drawNtscPass = (fbo, sourceTexture, textTexture, playerTexture) => {
                    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
                    gl.viewport(0, 0, canvas.width, canvas.height);
                    gl.useProgram(ntscProgram);

                    const posAttrib = gl.getAttribLocation(ntscProgram, 'position');
                    gl.enableVertexAttribArray(posAttrib);
                    gl.vertexAttribPointer(posAttrib, 2, gl.FLOAT, false, 0, 0);

                    gl.activeTexture(gl.TEXTURE0);
                    gl.bindTexture(gl.TEXTURE_2D, sourceTexture);
                    gl.uniform1i(gl.getUniformLocation(ntscProgram, 'Source'), 0);

                    gl.activeTexture(gl.TEXTURE1);
                    gl.bindTexture(gl.TEXTURE_2D, textTexture);
                    gl.uniform1i(gl.getUniformLocation(ntscProgram, 'TextSource'), 1);

                    gl.activeTexture(gl.TEXTURE2);
                    gl.bindTexture(gl.TEXTURE_2D, playerTexture);
                    gl.uniform1i(gl.getUniformLocation(ntscProgram, 'PlayerTextSource'), 2);
                    gl.uniform1i(gl.getUniformLocation(ntscProgram, 'u_player_text_blend_mode'), getPlayerTextBlendModeId());

                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'TIME'), timeValue);
                    gl.uniform2f(gl.getUniformLocation(ntscProgram, 'u_resolution'), canvas.width, canvas.height);
                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'wiggle'), 0.03 * ntscWiggle);
                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'wiggle_speed'), ntscWiggleSpeed);
                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'smear'), 1.0 * ntscSmear);
                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'u_ntsc_amount'), ntscAmount);
                    gl.uniform1f(gl.getUniformLocation(ntscProgram, 'u_ntsc_chroma_shift'), ntscChromaShift);

                    gl.drawArrays(gl.TRIANGLES, 0, 6);
                };

                drawSourceComposite(fboBase, ntscTexture, ntscTextTexture, ntscPlayerTextTexture);
                let vhsBaseTexture = texBase;
                let vhsSourceTexture = texBase;
                if (isNtscActive() && videoEffectOrder === EFFECT_ORDER_NTSC_THEN_VHS) {
                    drawNtscPass(fboBase, ntscTexture, ntscTextTexture, ntscPlayerTextTexture);
                    vhsSourceTexture = texBase;
                    vhsBaseTexture = texBase;
                }

                // Pass A
                drawPass(vhsProgA, fboA, vhsSourceTexture, null, false);

                // Pass B
                drawPass(vhsProgB, fboB, texA, null, false);

                // Pass C
                const isEvenFrame = (vhsFrameCount % 2 === 0);
                const readC = isEvenFrame ? texC1 : texC2;
                const writeFbo = isEvenFrame ? fboC2 : fboC1;

                drawPass(vhsProgC, writeFbo, texB, readC, true);

                vhsFrameCount++;

                // Pass D
                drawPass(vhsProgD, fboD, isEvenFrame ? texC2 : texC1, null, false);

                if (isNtscActive()) {
                    if (videoEffectOrder === EFFECT_ORDER_VHS_THEN_NTSC) {
                        // Finish the VHS chain into texA, then apply NTSC to that composite.
                        drawPass(vhsProgImage, fboA, texD, vhsBaseTexture, false);
                        drawNtscPass(null, texA, dummyTextTex, dummyTextTex);
                    } else {
                        drawPass(vhsProgImage, null, texD, vhsBaseTexture, false);
                    }
                } else {
                    // Final Pass (Image) to screen
                    drawPass(vhsProgImage, null, texD, vhsBaseTexture, false);
                }
                return;
            }

            gl.bindFramebuffer(gl.FRAMEBUFFER, null);
            gl.viewport(0, 0, canvas.width, canvas.height);
            
            const prog = isNtscActive() ? ntscProgram : passProgram;
            gl.useProgram(prog);
            
            gl.bindBuffer(gl.ARRAY_BUFFER, ntscBuffer);
            const posAttrib = gl.getAttribLocation(prog, 'position');
            gl.enableVertexAttribArray(posAttrib);
            gl.vertexAttribPointer(posAttrib, 2, gl.FLOAT, false, 0, 0);
            
            // Source (Texture 0) - offscreenCanvas (just the visualizer)
            gl.activeTexture(gl.TEXTURE0);
            gl.bindTexture(gl.TEXTURE_2D, ntscTexture);
            gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, getVisualizerSourceCanvas());
            gl.uniform1i(gl.getUniformLocation(prog, 'Source'), 0);
            
            // TextSource (Texture 1) - lyricsCanvas (just the text)
            gl.activeTexture(gl.TEXTURE1);
            gl.bindTexture(gl.TEXTURE_2D, ntscTextTexture);
            gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, lyricsCanvas);
            gl.uniform1i(gl.getUniformLocation(prog, 'TextSource'), 1);

            // Player text blend layer (Texture 2)
            gl.activeTexture(gl.TEXTURE2);
            gl.bindTexture(gl.TEXTURE_2D, ntscPlayerTextTexture);
            gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, playerTextCanvas);
            gl.uniform1i(gl.getUniformLocation(prog, 'PlayerTextSource'), 2);
            gl.uniform1i(gl.getUniformLocation(prog, 'u_player_text_blend_mode'), getPlayerTextBlendModeId());
            
            if (isNtscActive()) {
                gl.uniform1f(gl.getUniformLocation(prog, 'TIME'), timeValue);
                gl.uniform2f(gl.getUniformLocation(prog, 'u_resolution'), canvas.width, canvas.height);
                gl.uniform1f(gl.getUniformLocation(prog, 'wiggle'), 0.03 * ntscWiggle);
                gl.uniform1f(gl.getUniformLocation(prog, 'wiggle_speed'), ntscWiggleSpeed);
                gl.uniform1f(gl.getUniformLocation(prog, 'smear'), 1.0 * ntscSmear);
                gl.uniform1f(gl.getUniformLocation(prog, 'u_ntsc_amount'), ntscAmount);
                gl.uniform1f(gl.getUniformLocation(prog, 'u_ntsc_chroma_shift'), ntscChromaShift);
            }
            
            gl.drawArrays(gl.TRIANGLES, 0, 6);
        }

        function stopMilkdropAnimation() {
            if (milkdropRenderFrameId !== null) {
                cancelAnimationFrame(milkdropRenderFrameId);
                milkdropRenderFrameId = null;
            }
        }

        function startMilkdropAnimation() {
            if (
                milkdropRenderFrameId !== null ||
                !isPlaying ||
                !(isVideoEffectActive() || (visualizerEnabled && milkdropVisualizer))
            ) {
                return;
            }
            const renderFrame = (timestamp) => {
                if (!isPlaying || !(isVideoEffectActive() || (visualizerEnabled && milkdropVisualizer))) {
                    milkdropRenderFrameId = null;
                    return;
                }
                const frameLimit = Number(milkdropFrameLimit) || 0;
                if (frameLimit > 0) {
                    const interval = 1000 / frameLimit;
                    const elapsed = timestamp - milkdropLastFrameTime;
                    if (elapsed < interval) {
                        milkdropRenderFrameId = requestAnimationFrame(renderFrame);
                        return;
                    }
                    milkdropLastFrameTime = timestamp - (elapsed % interval);
                }
                if (visualizerEnabled && milkdropVisualizer) {
                    try {
                        milkdropVisualizer.render();
                    } catch (renderError) {
                        console.warn('[Milkdrop] Render frame failed:', renderError);
                        if (milkdropPresetName) {
                            console.warn(`[Milkdrop] Preset "${milkdropPresetName}" crashed during render. Disabling and cycling...`);
                            milkdropFailedPresetNames.add(milkdropPresetName);
                            renderMilkdropPresetList();
                            selectRandomMilkdropPreset(2.0, false);
                        }
                    }
                }
                if (ntscGl) {
                    try {
                        renderNtscFrame(performance.now() / 1000.0);
                    } catch (renderError) {
                        console.warn('[Video Effects] Render frame failed:', renderError);
                    }
                }
                milkdropRenderFrameId = requestAnimationFrame(renderFrame);
            };
            milkdropRenderFrameId = requestAnimationFrame(renderFrame);
        }

        function stopMilkdropPresetCycling() {
            if (milkdropCycleIntervalId !== null) {
                console.log('[Milkdrop] stopMilkdropPresetCycling called. Clearing interval:', milkdropCycleIntervalId);
                clearInterval(milkdropCycleIntervalId);
                milkdropCycleIntervalId = null;
            }
        }

        function startMilkdropPresetCycling() {
            stopMilkdropPresetCycling();
            console.log('[Milkdrop] startMilkdropPresetCycling called. visualizerEnabled:', visualizerEnabled, 'isPlaying:', isPlaying, 'milkdropCycleOnSongChange:', milkdropCycleOnSongChange, 'presets count:', getEnabledMilkdropPresets().length);
            if (!visualizerEnabled || !isPlaying || milkdropCycleOnSongChange || getEnabledMilkdropPresets().length < 2) {
                return;
            }
            const cycleMs = Math.max(5, milkdropCycleSeconds) * 1000;
            console.log('[Milkdrop] Starting interval with', cycleMs, 'ms');
            milkdropCycleIntervalId = setInterval(() => {
                console.log('[Milkdrop] Interval fired. Cycling preset.');
                selectRandomMilkdropPreset(MILKDROP_DEFAULT_BLEND_SECONDS, true);
            }, cycleMs);
        }

        function syncMilkdropAnimationState() {
            if (isPlaying && (isVideoEffectActive() || (visualizerEnabled && milkdropVisualizer))) {
                startMilkdropAnimation();
            } else {
                stopMilkdropAnimation();
            }
            if (visualizerEnabled && isPlaying) {
                startMilkdropPresetCycling();
            } else {
                stopMilkdropPresetCycling();
            }
        }

        function detachAnalysisSource() {
            if (source) {
                try {
                    source.disconnect();
                } catch (error) {
                    console.warn('[Milkdrop] Failed to disconnect audio source:', error);
                }
            }
            if (analyser) {
                try {
                    analyser.disconnect();
                } catch (_) {}
            }
            source = null;
            analysisSourceMode = null;
            analyserOutputConnected = false;
            milkdropAudioConnected = false;
        }

        function bindAnalysisSource(forceCaptureStreamRefresh = false) {
            if (!audioCtx || !analyser) {
                return;
            }

            if (wasmPlaybackEnabled) {
                if (analysisSourceMode === 'wasm-worklet' && source === wasmAudioNode) {
                    return;
                }
                if (!wasmAudioNode) {
                    return;
                }
                detachAnalysisSource();
                source = wasmAudioNode;
                source.connect(analyser);
                connectAnalyserToDestination();
                analysisSourceMode = 'wasm-worklet';
                connectMilkdropAudio();
                void loadCurrentMilkdropPreset(0);
                console.log('[Milkdrop] Using WASM AudioWorklet output for analysis.');
                return;
            }

            if (analysisSourceMode === 'media-element' && source) {
                connectMilkdropAudio();
                return;
            }

            if (analysisSourceMode === 'capture-stream' && source && !forceCaptureStreamRefresh) {
                connectMilkdropAudio();
                return;
            }

            // Keep a single MediaElementSource attached to the audio element.
            // Unlike captureStream(), it remains live when the streaming response
            // starts producing audio later or the element reconnects to /listen.
            try {
                detachAnalysisSource();
                if (!mediaElementSourceNode) {
                    mediaElementSourceNode = audioCtx.createMediaElementSource(audio);
                }
                source = mediaElementSourceNode;
                source.connect(analyser);
                connectAnalyserToDestination();
                analysisSourceMode = 'media-element';
                connectMilkdropAudio();
                void loadCurrentMilkdropPreset(0);
                console.log('[Milkdrop] Using persistent MediaElementSource for analysis.');
                return;
            } catch (mediaElementErr) {
                source = null;
                analysisSourceMode = null;
                console.warn(
                    '[Milkdrop] MediaElementSource analysis failed, falling back to captureStream().',
                    mediaElementErr
                );
            }

            detachAnalysisSource();
            const captureStreamFn = audio.captureStream || audio.mozCaptureStream;
            if (!captureStreamFn) {
                throw new Error('No supported Web Audio source is available for the audio stream.');
            }

            const capturedStream = captureStreamFn.call(audio);
            source = audioCtx.createMediaStreamSource(capturedStream);
            source.connect(analyser);
            analysisSourceMode = 'capture-stream';
            connectMilkdropAudio();
            void loadCurrentMilkdropPreset(0);
            console.log('[Milkdrop] Using HTMLMediaElement.captureStream() fallback for analysis.');
        }

        function initializeAudioAnalysisGraph(forceCaptureStreamRefresh = false) {
            if (!audioCtx || audioCtx.state !== 'running') {
                return false;
            }

            bindAnalysisSource(forceCaptureStreamRefresh);
            void ensureMilkdropVisualizer()
                .then(() => loadCurrentMilkdropPreset(0))
                .catch((error) => {
                    reportMilkdropError('Milkdrop failed to start', '[Milkdrop] Failed to start:', error);
                });
            return true;
        }

        function initAudioContext(forceCaptureStreamRefresh = false, deferGraphBinding = false) {
            try {
                if (!audioCtx) {
                    const AudioContext = window.AudioContext || window.webkitAudioContext;
                    audioCtx = new AudioContext();
                    analyser = audioCtx.createAnalyser();
                    analyser.fftSize = 2048;
                    analyser.minDecibels = -100;
                    analyser.maxDecibels = -10;
                    analyser.smoothingTimeConstant = 0.0;
                    audioCtx.addEventListener('statechange', () => {
                        console.info(`[Audio] AudioContext state changed to ${audioCtx.state}.`);
                        if (audioCtx.state === 'running') {
                            initializeAudioAnalysisGraph(false);
                            if (playbackRequested && !mutedAutoplayPriming && !audio.paused) {
                                setPlaybackGestureRequired(false);
                                setPlayState('playing');
                            }
                        }
                        updateIdlePlayerUi();
                    });
                }

                if (!deferGraphBinding) {
                    initializeAudioAnalysisGraph(forceCaptureStreamRefresh);
                }
            } catch (e) {
                console.error('[Milkdrop] Failed to initialize Web Audio context:', e);
                setMilkdropStatus('Audio analysis failed', true);
            }
        }

        async function resumeAudioContext() {
            if (!audioCtx) {
                initAudioContext(false, true);
            }
            if (audioCtx && audioCtx.state !== 'running' && audioCtx.state !== 'closed') {
                try {
                    await audioCtx.resume();
                } catch (error) {
                    console.warn('[Audio] AudioContext resume failed:', error);
                    return false;
                }
            }
            if (!audioCtx || audioCtx.state !== 'running') {
                return false;
            }

            console.log('[Audio] AudioContext is running.');
            // Attach MediaElementSource only after the context is running. A
            // suspended MediaElementSource graph silences normal media output.
            initializeAudioAnalysisGraph(false);
            connectMilkdropAudio();
            void loadCurrentMilkdropPreset(0);
            syncMilkdropAnimationState();
            return true;
        }

        async function resumeAudioContextWithTimeout(timeoutMs = 400) {
            if (audioCtx && audioCtx.state === 'running') {
                initializeAudioAnalysisGraph(false);
                return true;
            }

            const resumed = await Promise.race([
                resumeAudioContext(),
                new Promise((resolve) => {
                    window.setTimeout(() => resolve(false), timeoutMs);
                }),
            ]);
            return resumed === true || !!(audioCtx && audioCtx.state === 'running');
        }

        function maybeRecoverVisualizer() {
            if (
                visualizerRecoveryInProgress ||
                !playbackRequested ||
                !isPlaying ||
                mutedAutoplayPriming ||
                !(visualizerEnabled || isVideoEffectActive())
            ) {
                return;
            }

            const now = performance.now();
            if (now - visualizerRecoveryLastAttemptAt < 2500) {
                return;
            }

            const needsRecovery =
                !audioCtx ||
                audioCtx.state !== 'running' ||
                !analysisSourceMode ||
                milkdropRenderFrameId === null ||
                (visualizerEnabled && !milkdropVisualizer);
            if (!needsRecovery) {
                return;
            }

            visualizerRecoveryLastAttemptAt = now;
            visualizerRecoveryInProgress = true;
            initAudioContext(false, true);

            void resumeAudioContextWithTimeout()
                .then(async (ready) => {
                    if (!ready || !audioCtx || audioCtx.state !== 'running') {
                        throw new Error('AudioContext is not running');
                    }
                    initializeAudioAnalysisGraph(false);
                    if (visualizerEnabled) {
                        await ensureMilkdropVisualizer();
                        await loadCurrentMilkdropPreset(0);
                    }
                    syncMilkdropAnimationState();
                })
                .catch((error) => {
                    console.warn('[Milkdrop] Visualizer recovery attempt failed:', error);
                })
                .finally(() => {
                    visualizerRecoveryInProgress = false;
                });
        }

        window.addEventListener('click', () => {
            if (playbackRequested && !playbackGestureRequired) {
                void resumeAudioContextWithTimeout();
            }
        });

        const sliderConfigModal = document.getElementById('slider-config-modal');
        const modalSliderTitle = document.getElementById('modal-slider-title');
        const modalMinInput = document.getElementById('modal-min-input');
        const modalMaxInput = document.getElementById('modal-max-input');
        const modalStepInput = document.getElementById('modal-step-input');
        const modalCancelBtn = document.getElementById('modal-cancel-btn');
        const modalSaveBtn = document.getElementById('modal-save-btn');
        let currentEditingSliderKey = null;

        document.addEventListener('click', (e) => {
            const btn = e.target.closest('.slider-edit-btn');
            if (btn) {
                openSliderConfigModal(btn.dataset.sliderKey);
            }
        });

        function openSliderConfigModal(key) {
            const info = sliderRanges[key];
            const sliderMap = SLIDERS_MAP[key];
            if (!info || !sliderMap || !sliderConfigModal) {
                return;
            }
            currentEditingSliderKey = key;
            const sliderEl = document.getElementById(sliderMap.id);
            const btnEl = document.querySelector('.slider-edit-btn[data-slider-key="' + key + '"]');
            const labelText = btnEl ? btnEl.parentElement.textContent.replace(/\s+/g, ' ').trim() : key;
            modalSliderTitle.textContent = 'Edit limits: ' + labelText;
            modalMinInput.value = sliderEl.min;
            modalMaxInput.value = sliderEl.max;
            modalStepInput.value = sliderEl.step || '1';
            sliderConfigModal.style.display = 'flex';
            markChromeActivity();
            setTimeout(() => sliderConfigModal.classList.add('show'), 10);
        }

        function closeSliderConfigModal() {
            if (!sliderConfigModal) {
                return;
            }
            sliderConfigModal.classList.remove('show');
            setTimeout(() => {
                sliderConfigModal.style.display = 'none';
                markChromeActivity();
            }, 250);
        }

        if (modalCancelBtn) {
            modalCancelBtn.addEventListener('click', closeSliderConfigModal);
        }

        if (modalSaveBtn) {
            modalSaveBtn.addEventListener('click', () => {
                if (!currentEditingSliderKey) return;
                const sliderMap = SLIDERS_MAP[currentEditingSliderKey];
                const sliderEl = document.getElementById(sliderMap.id);
                const newMin = parseFloat(modalMinInput.value);
                const newMax = parseFloat(modalMaxInput.value);
                const newStep = parseFloat(modalStepInput.value);

                if (Number.isFinite(newMin) && Number.isFinite(newMax) && Number.isFinite(newStep)) {
                    sliderEl.min = String(newMin);
                    sliderEl.max = String(newMax);
                    const val = parseFloat(sliderEl.value);
                    if (val < newMin) sliderEl.value = String(newMin);
                    if (val > newMax) sliderEl.value = String(newMax);
                    sliderEl.step = String(newStep);
                    sliderEl.dispatchEvent(new Event('input'));
                    saveSettings();
                    closeSliderConfigModal();
                } else {
                    showPlayerNotice('Please enter valid numbers for Min, Max, and Step.', 'error');
                }
            });
        }

        if (visualizerToggle) {
            visualizerToggle.addEventListener('change', (e) => {
                visualizerEnabled = e.target.checked;
                updateVisualizerCanvasOpacity();
                applyNtscState();
                if (visualizerEnabled) {
                    void ensureMilkdropVisualizer()
                        .then(() => loadCurrentMilkdropPreset(0))
                        .catch((error) => {
                            reportMilkdropError('Milkdrop failed to start', '[Milkdrop] Failed to start:', error);
                        });
                }
                saveSettings();
            });
        }

        if (ntscAmountSlider) {
            ntscAmountSlider.addEventListener('input', (e) => {
                ntscAmount = clamp01(parseFloat(e.target.value));
                syncMilkdropControls();
                saveSettings();
            });
        }

        if (vhsAmountSlider) {
            vhsAmountSlider.addEventListener('input', (e) => {
                vhsAmount = clamp01(parseFloat(e.target.value));
                syncMilkdropControls();
                saveSettings();
            });
        }

        if (albumArtSizeSlider) {
            albumArtSizeSlider.addEventListener('input', (e) => {
                albumArtSize = parseInt(e.target.value, 10);
                if (albumArtSizeVal) {
                    albumArtSizeVal.textContent = albumArtSize + 'px';
                }
                updateAlbumArtSize();
                saveSettings();
            });
        }

        if (milkdropFrameLimitSelect) {
            milkdropFrameLimitSelect.addEventListener('change', (e) => {
                milkdropFrameLimit = parseInt(e.target.value, 10);
                saveSettings();
            });
        }

        if (milkdropCanvasSizeSelect) {
            milkdropCanvasSizeSelect.addEventListener('change', (e) => {
                milkdropCanvasSize = e.target.value;
                resizeCanvas();
                void recreateMilkdropVisualizer();
                saveSettings();
            });
        }

        if (milkdropMeshSizeSelect) {
            milkdropMeshSizeSelect.addEventListener('change', (e) => {
                milkdropMeshSize = e.target.value;
                void recreateMilkdropVisualizer();
                saveSettings();
            });
        }

        if (effectOrderButtons) {
            effectOrderButtons.forEach((button) => {
                button.addEventListener('click', () => {
                    const nextOrder = button.dataset.effectOrder;
                    if (
                        nextOrder !== EFFECT_ORDER_VHS_THEN_NTSC &&
                        nextOrder !== EFFECT_ORDER_NTSC_THEN_VHS
                    ) {
                        return;
                    }
                    videoEffectOrder = nextOrder;
                    syncMilkdropControls();
                    triggerNtscRedraw();
                    saveSettings();
                });
            });
        }

        if (apikeyToggle) {
            apikeyToggle.addEventListener('change', (e) => {
                apiKeyEnabled = e.target.checked;
                if (apikeySettingsGroup) {
                    apikeySettingsGroup.style.display = apiKeyEnabled ? 'flex' : 'none';
                }
                updateSearchTabVisibility();
                saveSettings();
            });
        }

        if (apikeyInput) {
            apikeyInput.addEventListener('input', (e) => {
                apiKey = e.target.value;
                updateSearchTabVisibility();
                saveSettings();
            });
        }

        if (ntscSmearSlider) {
            ntscSmearSlider.addEventListener('input', (e) => {
                ntscSmear = parseFloat(e.target.value);
                if (ntscSmearVal) {
                    ntscSmearVal.textContent = Math.round(ntscSmear * 100) + '%';
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (ntscWiggleSlider) {
            ntscWiggleSlider.addEventListener('input', (e) => {
                ntscWiggle = parseFloat(e.target.value);
                if (ntscWiggleVal) {
                    ntscWiggleVal.textContent = Math.round(ntscWiggle * 100) + '%';
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (ntscWiggleSpeedSlider) {
            ntscWiggleSpeedSlider.addEventListener('input', (e) => {
                ntscWiggleSpeed = parseFloat(e.target.value);
                if (ntscWiggleSpeedVal) {
                    ntscWiggleSpeedVal.textContent = String(Math.round(ntscWiggleSpeed));
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (vhsStrengthSlider) {
            vhsStrengthSlider.addEventListener('input', (e) => {
                vhsStrength = parseFloat(e.target.value);
                if (vhsStrengthVal) {
                    vhsStrengthVal.textContent = Math.round(vhsStrength * 100) + '%';
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (vhsNoiseSlider) {
            vhsNoiseSlider.addEventListener('input', (e) => {
                vhsNoise = parseFloat(e.target.value);
                if (vhsNoiseVal) {
                    vhsNoiseVal.textContent = Math.round(vhsNoise * 100) + '%';
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (vhsGrainSizeSlider) {
            vhsGrainSizeSlider.addEventListener('input', (e) => {
                vhsGrainSize = parseFloat(e.target.value);
                if (vhsGrainSizeVal) {
                    vhsGrainSizeVal.textContent = vhsGrainSize.toFixed(1);
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (vhsVignetteSlider) {
            vhsVignetteSlider.addEventListener('input', (e) => {
                vhsVignette = clamp01(parseFloat(e.target.value));
                if (vhsVignetteVal) {
                    vhsVignetteVal.textContent = formatPercent(vhsVignette);
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        if (vhsBlurSlider) {
            vhsBlurSlider.addEventListener('input', (e) => {
                vhsBlur = parseFloat(e.target.value);
                if (vhsBlurVal) {
                    vhsBlurVal.textContent = vhsBlur.toFixed(3);
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        function bindEffectSlider(slider, valueEl, setValue, getValue, formatValue) {
            if (!slider) {
                return;
            }
            slider.addEventListener('input', (e) => {
                setValue(parseFloat(e.target.value));
                if (valueEl) {
                    valueEl.textContent = formatValue(getValue());
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }

        bindEffectSlider(ntscChromaShiftSlider, ntscChromaShiftVal,
            (value) => { ntscChromaShift = value; },
            () => ntscChromaShift,
            formatPercent
        );
        bindEffectSlider(vhsLumaResolutionSlider, vhsLumaResolutionVal,
            (value) => { vhsLumaResolution = value; },
            () => vhsLumaResolution,
            formatPercent
        );
        bindEffectSlider(vhsChromaResolutionSlider, vhsChromaResolutionVal,
            (value) => { vhsChromaResolution = value; },
            () => vhsChromaResolution,
            formatPercent1
        );
        bindEffectSlider(vhsLineHeightSlider, vhsLineHeightVal,
            (value) => { vhsLineHeight = value; },
            () => vhsLineHeight,
            (value) => formatFixed(value, 1)
        );
        bindEffectSlider(vhsSharpenSlider, vhsSharpenVal,
            (value) => { vhsSharpen = value; },
            () => vhsSharpen,
            (value) => formatFixed(value, 1)
        );
        bindEffectSlider(vhsSharpenRadiusSlider, vhsSharpenRadiusVal,
            (value) => { vhsSharpenRadius = value; },
            () => vhsSharpenRadius,
            (value) => formatFixed(value, 1)
        );
        bindEffectSlider(vhsBlackLevelSlider, vhsBlackLevelVal,
            (value) => { vhsBlackLevel = value; },
            () => vhsBlackLevel,
            formatPercent
        );
        bindEffectSlider(vhsWhiteLevelSlider, vhsWhiteLevelVal,
            (value) => { vhsWhiteLevel = value; },
            () => vhsWhiteLevel,
            formatPercent
        );
        bindEffectSlider(vhsSaturationSlider, vhsSaturationVal,
            (value) => { vhsSaturation = value; },
            () => vhsSaturation,
            formatPercent
        );
        if (vhsShadowTintPicker) {
            vhsShadowTintPicker.addEventListener('input', (e) => {
                vhsShadowTint = normalizeHexColor(e.target.value, vhsShadowTint);
                if (vhsShadowTintVal) {
                    vhsShadowTintVal.textContent = vhsShadowTint.toUpperCase();
                }
                triggerNtscRedraw();
                saveSettings();
            });
        }
        bindEffectSlider(vhsTrackingSpeedSlider, vhsTrackingSpeedVal,
            (value) => { vhsTrackingSpeed = value; },
            () => vhsTrackingSpeed,
            (value) => formatFixed(value, 1)
        );
        bindEffectSlider(vhsTrackingOffsetSlider, vhsTrackingOffsetVal,
            (value) => { vhsTrackingOffset = value; },
            () => vhsTrackingOffset,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsTrackingJitterSlider, vhsTrackingJitterVal,
            (value) => { vhsTrackingJitter = value; },
            () => vhsTrackingJitter,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsWaveFrequencySlider, vhsWaveFrequencyVal,
            (value) => { vhsWaveFrequency = value; },
            () => vhsWaveFrequency,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsWaveAmountSlider, vhsWaveAmountVal,
            (value) => { vhsWaveAmount = value; },
            () => vhsWaveAmount,
            (value) => formatFixed(value, 2)
        );
        bindEffectSlider(vhsBottomWarpHeightSlider, vhsBottomWarpHeightVal,
            (value) => { vhsBottomWarpHeight = value; },
            () => vhsBottomWarpHeight,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsBottomWarpOffsetSlider, vhsBottomWarpOffsetVal,
            (value) => { vhsBottomWarpOffset = value; },
            () => vhsBottomWarpOffset,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsBottomWarpJitterSlider, vhsBottomWarpJitterVal,
            (value) => { vhsBottomWarpJitter = value; },
            () => vhsBottomWarpJitter,
            (value) => formatFixed(value, 0)
        );
        bindEffectSlider(vhsStaticLineHeightSlider, vhsStaticLineHeightVal,
            (value) => { vhsStaticLineHeight = value; },
            () => vhsStaticLineHeight,
            (value) => formatFixed(value, 1)
        );
        bindEffectSlider(vhsStaticLineOpacitySlider, vhsStaticLineOpacityVal,
            (value) => { vhsStaticLineOpacity = value; },
            () => vhsStaticLineOpacity,
            formatPercent
        );
        bindEffectSlider(vhsVignettePowerSlider, vhsVignettePowerVal,
            (value) => { vhsVignettePower = value; },
            () => vhsVignettePower,
            (value) => formatFixed(value, 2)
        );
        bindEffectSlider(vhsVignetteBoostSlider, vhsVignetteBoostVal,
            (value) => { vhsVignetteBoost = value; },
            () => vhsVignetteBoost,
            (value) => formatFixed(value, 2)
        );

        if (milkdropPresetList) {
            milkdropPresetList.addEventListener('change', (e) => {
                const checkbox = e.target.closest('.preset-checkbox');
                if (!checkbox) {
                    return;
                }
                const name = checkbox.dataset.presetName || '';
                if (!name) {
                    return;
                }
                if (checkbox.checked) {
                    milkdropDisabledPresetNames.delete(name);
                } else {
                    milkdropDisabledPresetNames.add(name);
                }
                syncMilkdropPresetListState();
                syncMilkdropAnimationState();
                saveSettings();
            });

            milkdropPresetList.addEventListener('click', (e) => {
                const button = e.target.closest('.preset-load-button');
                if (!button) {
                    return;
                }
                milkdropFailedPresetNames.delete(button.dataset.presetName);
                syncMilkdropPresetListState();
                void loadMilkdropPresetByName(button.dataset.presetName, MILKDROP_USER_BLEND_SECONDS, true);
            });
        }

        if (milkdropSelectAllBtn) {
            milkdropSelectAllBtn.addEventListener('click', () => {
                milkdropDisabledPresetNames.clear();
                milkdropFailedPresetNames.clear();
                syncMilkdropPresetListState();
                syncMilkdropAnimationState();
                saveSettings();
            });
        }

        if (milkdropSelectNoneBtn) {
            milkdropSelectNoneBtn.addEventListener('click', () => {
                milkdropDisabledPresetNames = new Set(milkdropPresets.map((entry) => entry.name));
                syncMilkdropPresetListState();
                syncMilkdropAnimationState();
                setMilkdropStatus('No presets selected');
                saveSettings();
            });
        }

        if (milkdropRandomBtn) {
            milkdropRandomBtn.addEventListener('click', () => {
                selectRandomMilkdropPreset(MILKDROP_USER_BLEND_SECONDS, true);
            });
        }

        if (milkdropCycleSongToggle) {
            milkdropCycleSongToggle.addEventListener('change', (e) => {
                milkdropCycleOnSongChange = e.target.checked;
                syncMilkdropControls();
                syncMilkdropAnimationState();
                saveSettings();
            });
        }

        if (milkdropCycleSlider) {
            milkdropCycleSlider.addEventListener('input', (e) => {
                milkdropCycleSeconds = parseInt(e.target.value, 10);
                syncMilkdropControls();
                syncMilkdropAnimationState();
                saveSettings();
            });
        }

        if (milkdropBlendSlider) {
            milkdropBlendSlider.addEventListener('input', (e) => {
                milkdropBlendSeconds = parseFloat(e.target.value);
                syncMilkdropControls();
                saveSettings();
            });
        }

        if (vizOpacitySlider) {
            vizOpacitySlider.addEventListener('input', (e) => {
                visualizerOpacity = clamp01(parseFloat(e.target.value));
                syncMilkdropControls();
                updateVisualizerCanvasOpacity();
                saveSettings();
            });
        }

        // ntscStrengthSlider removed

        function saveSettings() {
            localStorage.setItem(
                'spotifm_player_settings',
                JSON.stringify(getCurrentSettingsSnapshot())
            );
            localStorage.setItem(
                'spotifm_custom_fonts',
                JSON.stringify(customFonts)
            );
        }

        function applySettingsObject(settings) {
            if (!settings || typeof settings !== 'object') {
                return;
            }

            Object.keys(sliderRanges).forEach((key) => {
                const info = sliderRanges[key];
                const sliderEl = document.getElementById(SLIDERS_MAP[key].id);
                if (!sliderEl) {
                    return;
                }
                if (settings[info.minKey] !== undefined) {
                    let val = parseFloat(settings[info.minKey]);
                    if (key === 'lyricFadeCurve') {
                        val = 0.0;
                    }
                    if (Number.isFinite(val)) sliderEl.min = String(val);
                }
                if (settings[info.maxKey] !== undefined) {
                    const val = parseFloat(settings[info.maxKey]);
                    if (Number.isFinite(val)) {
                        let finalVal = val;
                        if (key === 'lyricFadeCurve') {
                            finalVal = Math.max(finalVal, 2.0);
                        }
                        sliderEl.max = String(finalVal);
                    }
                }
                if (settings[info.stepKey] !== undefined) {
                    const val = parseFloat(settings[info.stepKey]);
                    if (Number.isFinite(val)) sliderEl.step = String(val);
                }
            });

            if (settings.autosync !== undefined) {
                isAutosync = !!settings.autosync;
                if (autosyncToggle) {
                    autosyncToggle.checked = isAutosync;
                }
                syncLyricsModeControls();
            }
            if (settings.lyricsVisible !== undefined) {
                lyricsVisible = !!settings.lyricsVisible;
                if (showLyricsToggle) {
                    showLyricsToggle.checked = lyricsVisible;
                }
                syncLyricsModeControls();
            }
            if (settings.lyricTextScale !== undefined) {
                const parsed = parseFloat(settings.lyricTextScale);
                if (Number.isFinite(parsed)) {
                    lyricTextScale = parsed;
                    lyricsSizeSlider.value = String(lyricTextScale);
                }
            }
            if (settings.lyricFadeCurve !== undefined) {
                const parsed = parseFloat(settings.lyricFadeCurve);
                if (Number.isFinite(parsed)) {
                    lyricFadeCurve = parsed;
                    lyricsFadeSlider.value = String(lyricFadeCurve);
                    lyricsFadeVal.textContent = lyricFadeCurve.toFixed(2);
                }
            }
            if (settings.lyricsWrapWidth !== undefined) {
                const parsed = parseInt(settings.lyricsWrapWidth, 10);
                if (Number.isFinite(parsed)) {
                    lyricsWrapWidth = Math.max(40, Math.min(100, parsed));
                }
            }
            if (settings.customFonts !== undefined) {
                setCustomFonts(settings.customFonts);
            }
            if (settings.customLyricFonts !== undefined) {
                setCustomLyricFonts(settings.customLyricFonts);
            }
            if (settings.lyricFont !== undefined) {
                const parsed = normalizeLyricFontName(settings.lyricFont);
                if (parsed) {
                    lyricFont = isCustomLyricFontName(parsed) ? addCustomLyricFont(parsed) : parsed;
                }
            }
            if (settings.customPlayerFonts !== undefined) {
                setCustomPlayerFonts(settings.customPlayerFonts);
            }
            if (settings.playerFont !== undefined) {
                const parsed = normalizeLyricFontName(settings.playerFont);
                if (parsed) {
                    playerFont = isCustomLyricFontName(parsed) ? addCustomPlayerFont(parsed) : parsed;
                }
            }
            if (settings.lyricsBorderWidth !== undefined) {
                const parsed = parseInt(settings.lyricsBorderWidth, 10);
                if (Number.isFinite(parsed)) {
                    lyricsBorderWidth = Math.max(0, Math.min(16, parsed));
                }
            }
            if (settings.lyricsWeight !== undefined) {
                const parsed = parseInt(settings.lyricsWeight, 10);
                if (Number.isFinite(parsed)) {
                    lyricsWeight = Math.max(100, Math.min(900, parsed));
                }
            }
            if (settings.lyricsLineHeight !== undefined) {
                const parsed = parseFloat(settings.lyricsLineHeight);
                if (Number.isFinite(parsed)) {
                    lyricsLineHeight = Math.max(0.8, Math.min(3.0, parsed));
                    console.log("lyricsLineHeight", lyricsLineHeight);
                }
            }
            if (settings.lyricsWordSpacing !== undefined) {
                const parsed = parseInt(settings.lyricsWordSpacing, 10);
                if (Number.isFinite(parsed)) {
                    lyricsWordSpacing = Math.max(-10, Math.min(40, parsed));
                }
            }
            if (settings.lyricsLetterSpacing !== undefined) {
                const parsed = parseFloat(settings.lyricsLetterSpacing);
                if (Number.isFinite(parsed)) {
                    lyricsLetterSpacing = Math.max(-5, Math.min(20, parsed));
                }
            }
            if (settings.autoSyncLyricsOffset !== undefined) {
                const parsed = parseFloat(settings.autoSyncLyricsOffset);
                if (Number.isFinite(parsed)) {
                    autoSyncLyricsOffsetSec = parsed;
                    autoSyncOffsetSlider.value = String(autoSyncLyricsOffsetSec);
                    autoSyncOffsetVal.textContent = formatSignedSeconds(autoSyncLyricsOffsetSec);
                }
            }
            if (settings.delay !== undefined) {
                const parsed = parseFloat(settings.delay);
                if (Number.isFinite(parsed)) {
                    delaySlider.value = String(parsed);
                    manualDelaySec = parsed;
                    delayVal.textContent = manualDelaySec.toFixed(1) + 's';
                }
            }
            if (settings.visualizerEnabled !== undefined) {
                visualizerEnabled = !!settings.visualizerEnabled;
            }
            if (settings.ntscEnabled !== undefined && settings.ntscAmount === undefined) {
                ntscAmount = settings.ntscEnabled ? 1.0 : 0.0;
            }
            if (settings.vhsEnabled !== undefined && settings.vhsAmount === undefined) {
                vhsAmount = settings.vhsEnabled ? 1.0 : 0.0;
            }
            if (settings.ntscAmount !== undefined) {
                const parsed = parseFloat(settings.ntscAmount);
                if (Number.isFinite(parsed)) {
                    ntscAmount = clamp01(parsed);
                }
            }
            if (settings.vhsAmount !== undefined) {
                const parsed = parseFloat(settings.vhsAmount);
                if (Number.isFinite(parsed)) {
                    vhsAmount = clamp01(parsed);
                }
            }
            if (settings.videoEffectOrder !== undefined) {
                if (
                    settings.videoEffectOrder === EFFECT_ORDER_VHS_THEN_NTSC ||
                    settings.videoEffectOrder === EFFECT_ORDER_NTSC_THEN_VHS
                ) {
                    videoEffectOrder = settings.videoEffectOrder;
                }
            }
            if (settings.ntscSmear !== undefined) {
                const parsed = parseFloat(settings.ntscSmear);
                if (Number.isFinite(parsed)) {
                    ntscSmear = Math.max(0, Math.min(2, parsed));
                }
            }
            if (settings.ntscWiggle !== undefined) {
                const parsed = parseFloat(settings.ntscWiggle);
                if (Number.isFinite(parsed)) {
                    ntscWiggle = Math.max(0, Math.min(2, parsed));
                }
            }
            if (settings.ntscWiggleSpeed !== undefined) {
                const parsed = parseFloat(settings.ntscWiggleSpeed);
                if (Number.isFinite(parsed)) {
                    ntscWiggleSpeed = Math.max(0, Math.min(60, parsed));
                }
            }
            if (settings.ntscChromaShift !== undefined) {
                ntscChromaShift = clampNumber(settings.ntscChromaShift, 0, 3, ntscChromaShift);
            }
            if (settings.vhsStrength !== undefined) {
                const parsed = parseFloat(settings.vhsStrength);
                if (Number.isFinite(parsed)) {
                    vhsStrength = Math.max(0, Math.min(2, parsed));
                }
            }
            if (settings.vhsNoise !== undefined) {
                const parsed = parseFloat(settings.vhsNoise);
                if (Number.isFinite(parsed)) {
                    vhsNoise = Math.max(0, Math.min(2, parsed));
                }
            }
            if (settings.vhsGrainSize !== undefined) {
                const parsed = parseFloat(settings.vhsGrainSize);
                if (Number.isFinite(parsed)) {
                    vhsGrainSize = Math.max(0.5, Math.min(8, parsed));
                }
            }
            if (settings.vhsVignette !== undefined) {
                const parsed = parseFloat(settings.vhsVignette);
                if (Number.isFinite(parsed)) {
                    vhsVignette = clamp01(parsed);
                }
            }
            if (settings.vhsBlur !== undefined) {
                const parsed = parseFloat(settings.vhsBlur);
                if (Number.isFinite(parsed)) {
                    vhsBlur = Math.max(0, Math.min(0.5, parsed));
                }
            }
            if (settings.vhsLumaResolution !== undefined) {
                vhsLumaResolution = clampNumber(settings.vhsLumaResolution, 0.1, 1, vhsLumaResolution);
            }
            if (settings.vhsChromaResolution !== undefined) {
                vhsChromaResolution = clampNumber(settings.vhsChromaResolution, 0.01, 0.25, vhsChromaResolution);
            }
            if (settings.vhsLineHeight !== undefined) {
                vhsLineHeight = clampNumber(settings.vhsLineHeight, 1, 8, vhsLineHeight);
            }
            if (settings.vhsSharpen !== undefined) {
                vhsSharpen = clampNumber(settings.vhsSharpen, 0, 5, vhsSharpen);
            }
            if (settings.vhsSharpenRadius !== undefined) {
                vhsSharpenRadius = clampNumber(settings.vhsSharpenRadius, 0, 12, vhsSharpenRadius);
            }
            if (settings.vhsBlackLevel !== undefined) {
                vhsBlackLevel = clampNumber(settings.vhsBlackLevel, 0, 1, vhsBlackLevel);
            }
            if (settings.vhsWhiteLevel !== undefined) {
                vhsWhiteLevel = clampNumber(settings.vhsWhiteLevel, 0, 1, vhsWhiteLevel);
            }
            if (settings.vhsSaturation !== undefined) {
                vhsSaturation = clampNumber(settings.vhsSaturation, 0, 2, vhsSaturation);
            }
            if (settings.vhsShadowTint !== undefined) {
                vhsShadowTint = normalizeHexColor(settings.vhsShadowTint, vhsShadowTint);
            }
            if (settings.vhsTrackingSpeed !== undefined) {
                vhsTrackingSpeed = clampNumber(settings.vhsTrackingSpeed, 1, 20, vhsTrackingSpeed);
            }
            if (settings.vhsTrackingOffset !== undefined) {
                vhsTrackingOffset = clampNumber(settings.vhsTrackingOffset, 0, 40, vhsTrackingOffset);
            }
            if (settings.vhsTrackingJitter !== undefined) {
                vhsTrackingJitter = clampNumber(settings.vhsTrackingJitter, 0, 80, vhsTrackingJitter);
            }
            if (settings.vhsWaveFrequency !== undefined) {
                vhsWaveFrequency = clampNumber(settings.vhsWaveFrequency, 0, 160, vhsWaveFrequency);
            }
            if (settings.vhsWaveAmount !== undefined) {
                vhsWaveAmount = clampNumber(settings.vhsWaveAmount, 0, 5, vhsWaveAmount);
            }
            if (settings.vhsBottomWarpHeight !== undefined) {
                vhsBottomWarpHeight = clampNumber(settings.vhsBottomWarpHeight, 0, 80, vhsBottomWarpHeight);
            }
            if (settings.vhsBottomWarpOffset !== undefined) {
                vhsBottomWarpOffset = clampNumber(settings.vhsBottomWarpOffset, 0, 250, vhsBottomWarpOffset);
            }
            if (settings.vhsBottomWarpJitter !== undefined) {
                vhsBottomWarpJitter = clampNumber(settings.vhsBottomWarpJitter, 0, 150, vhsBottomWarpJitter);
            }
            if (settings.vhsStaticLineHeight !== undefined) {
                vhsStaticLineHeight = clampNumber(settings.vhsStaticLineHeight, 0, 24, vhsStaticLineHeight);
            }
            if (settings.vhsStaticLineOpacity !== undefined) {
                vhsStaticLineOpacity = clampNumber(settings.vhsStaticLineOpacity, 0, 1, vhsStaticLineOpacity);
            }
            if (settings.vhsVignettePower !== undefined) {
                vhsVignettePower = clampNumber(settings.vhsVignettePower, 0.05, 1, vhsVignettePower);
            }
            if (settings.vhsVignetteBoost !== undefined) {
                vhsVignetteBoost = clampNumber(settings.vhsVignetteBoost, 0.5, 4, vhsVignetteBoost);
            }
            if (settings.visualizerOpacity !== undefined) {
                const parsed = parseFloat(settings.visualizerOpacity);
                if (Number.isFinite(parsed)) {
                    visualizerOpacity = Math.max(0.1, Math.min(1, parsed));
                }
            }
            if (settings.milkdropPreset !== undefined) {
                milkdropPresetName = String(settings.milkdropPreset || '');
            }
            if (Array.isArray(settings.milkdropDisabledPresets)) {
                milkdropDisabledPresetNames = new Set(
                    settings.milkdropDisabledPresets.filter((name) => typeof name === 'string')
                );
            }
            if (settings.milkdropCycleOnSongChange !== undefined) {
                milkdropCycleOnSongChange = !!settings.milkdropCycleOnSongChange;
            }
            if (settings.milkdropCycleSeconds !== undefined) {
                const parsed = parseInt(settings.milkdropCycleSeconds, 10);
                if (Number.isFinite(parsed)) {
                    milkdropCycleSeconds = Math.max(5, Math.min(60, parsed));
                }
            }
            if (settings.milkdropBlendSeconds !== undefined) {
                const parsed = parseFloat(settings.milkdropBlendSeconds);
                if (Number.isFinite(parsed)) {
                    milkdropBlendSeconds = Math.max(0, Math.min(8, parsed));
                }
            }
            if (settings.albumArtSize !== undefined) {
                const parsed = parseInt(settings.albumArtSize, 10);
                if (Number.isFinite(parsed)) {
                    albumArtSize = Math.max(32, Math.min(256, parsed));
                }
            }
            if (settings.milkdropFrameLimit !== undefined) {
                const parsed = parseInt(settings.milkdropFrameLimit, 10);
                if (Number.isFinite(parsed)) {
                    milkdropFrameLimit = parsed;
                }
            }
            if (settings.milkdropCanvasSize !== undefined) {
                milkdropCanvasSize = String(settings.milkdropCanvasSize);
            }
            if (settings.milkdropMeshSize !== undefined) {
                milkdropMeshSize = String(settings.milkdropMeshSize);
            }
            if (settings.playerOpacity !== undefined) {
                const parsed = parseFloat(settings.playerOpacity);
                if (Number.isFinite(parsed)) {
                    playerOpacity = Math.max(0.0, Math.min(1.0, parsed));
                    if (playerOpacitySlider) playerOpacitySlider.value = String(playerOpacity);
                    if (playerOpacityVal) playerOpacityVal.textContent = Math.round(playerOpacity * 100) + '%';
                }
            }
            if (settings.playerBorderOpacity !== undefined) {
                const parsed = parseFloat(settings.playerBorderOpacity);
                if (Number.isFinite(parsed)) {
                    playerBorderOpacity = Math.max(0.0, Math.min(1.0, parsed));
                    if (playerBorderSlider) playerBorderSlider.value = String(playerBorderOpacity);
                    if (playerBorderVal) playerBorderVal.textContent = Math.round(playerBorderOpacity * 100) + '%';
                }
            }
            if (settings.playerBlur !== undefined) {
                const parsed = parseInt(settings.playerBlur, 10);
                if (Number.isFinite(parsed)) {
                    playerBlur = Math.max(0, Math.min(40, parsed));
                    if (playerBlurSlider) playerBlurSlider.value = String(playerBlur);
                    if (playerBlurVal) playerBlurVal.textContent = playerBlur + 'px';
                }
            }
            if (settings.playerScale !== undefined) {
                const parsed = parseFloat(settings.playerScale);
                if (Number.isFinite(parsed)) {
                    playerScale = Math.max(0.5, Math.min(2.0, parsed));
                    if (playerScaleSlider) playerScaleSlider.value = String(playerScale);
                    if (playerScaleVal) playerScaleVal.textContent = Math.round(playerScale * 100) + '%';
                }
            }
            if (settings.playerModalWidth !== undefined) {
                const parsed = parseInt(settings.playerModalWidth, 10);
                if (Number.isFinite(parsed)) {
                    playerModalWidth = Math.max(0, Math.min(1120, parsed));
                    if (playerModalWidthSlider) playerModalWidthSlider.value = String(playerModalWidth);
                    if (playerModalWidthVal) playerModalWidthVal.textContent = formatPlayerModalWidth(playerModalWidth);
                }
            }
            if (settings.playerEdgeGap !== undefined) {
                const parsed = parseInt(settings.playerEdgeGap, 10);
                if (Number.isFinite(parsed)) {
                    playerEdgeGap = Math.max(0, Math.min(80, parsed));
                    if (playerEdgeGapSlider) playerEdgeGapSlider.value = String(playerEdgeGap);
                    if (playerEdgeGapVal) playerEdgeGapVal.textContent = Math.round(playerEdgeGap) + 'px';
                }
            }
            if (settings.playerTitleColor !== undefined) {
                playerTitleColor = normalizeHexColor(settings.playerTitleColor, playerTitleColor);
            }
            if (settings.playerArtistColor !== undefined) {
                playerArtistColor = normalizeHexColor(settings.playerArtistColor, playerArtistColor);
            }
            if (settings.playerTitleFontSize !== undefined) {
                const parsed = parseInt(settings.playerTitleFontSize, 10);
                if (Number.isFinite(parsed)) {
                    playerTitleFontSize = Math.max(8, Math.min(72, parsed));
                    if (playerTitleFontSizeSlider) playerTitleFontSizeSlider.value = String(playerTitleFontSize);
                    if (playerTitleFontSizeVal) playerTitleFontSizeVal.textContent = playerTitleFontSize + 'px';
                }
            }
            if (settings.playerArtistFontSize !== undefined) {
                const parsed = parseInt(settings.playerArtistFontSize, 10);
                if (Number.isFinite(parsed)) {
                    playerArtistFontSize = Math.max(8, Math.min(72, parsed));
                    if (playerArtistFontSizeSlider) playerArtistFontSizeSlider.value = String(playerArtistFontSize);
                    if (playerArtistFontSizeVal) playerArtistFontSizeVal.textContent = playerArtistFontSize + 'px';
                }
            }
            if (settings.playerTextGap !== undefined) {
                const parsed = parseInt(settings.playerTextGap, 10);
                if (Number.isFinite(parsed)) {
                    playerTextGap = Math.max(-16, Math.min(32, parsed));
                    if (playerTextGapSlider) playerTextGapSlider.value = String(playerTextGap);
                    if (playerTextGapVal) playerTextGapVal.textContent = playerTextGap + 'px';
                }
            }
            if (settings.playerApplyEffects !== undefined) {
                playerApplyEffects = !!settings.playerApplyEffects;
                if (playerApplyEffectsToggle) playerApplyEffectsToggle.checked = playerApplyEffects;
            }
            if (settings.playerTextHighlight !== undefined) {
                playerTextHighlight = !!settings.playerTextHighlight;
                if (playerTextHighlightToggle) playerTextHighlightToggle.checked = playerTextHighlight;
            }
            if (settings.playerTextBlendMode !== undefined) {
                playerTextBlendMode = normalizePlayerTextBlendMode(settings.playerTextBlendMode);
            }
            if (settings.playerTextLeft !== undefined) {
                playerTextLeft = Math.round(clampNumber(settings.playerTextLeft, 0, window.innerWidth, playerTextLeft));
            }
            if (settings.playerTextBottom !== undefined) {
                playerTextBottom = Math.round(clampNumber(settings.playerTextBottom, 0, window.innerHeight, playerTextBottom));
            }
            if (settings.liquidGlassEnabled !== undefined) {
                liquidGlassEnabled = !!settings.liquidGlassEnabled;
            }
            if (settings.playerAlign !== undefined) {
                playerAlign = normalizePlayerAlign(settings.playerAlign);
            }
            if (settings.showTrackInfo !== undefined) {
                showTrackInfo = !!settings.showTrackInfo;
            }
            if (settings.showAlbumArt !== undefined) {
                showAlbumArt = !!settings.showAlbumArt;
            }
            if (settings.showProgressBar !== undefined) {
                showProgressBar = !!settings.showProgressBar;
            }
            if (settings.showSyncStatus !== undefined) {
                showSyncStatus = !!settings.showSyncStatus;
            }
            if (settings.showListenerNumber !== undefined) {
                showListenerNumber = !!settings.showListenerNumber;
            }
            if (settings.playerMinimized !== undefined) {
                playerMinimized = !!settings.playerMinimized;
                if (playerMinimized) {
                    playerPanel.classList.add('minimized');
                    if (restorePlayerBtn) restorePlayerBtn.style.display = 'flex';
                } else {
                    playerPanel.classList.remove('minimized');
                    if (restorePlayerBtn) restorePlayerBtn.style.display = 'none';
                }
            }

            if (settings.apiKeyEnabled !== undefined) {
                apiKeyEnabled = !!settings.apiKeyEnabled;
                if (apikeyToggle) apikeyToggle.checked = apiKeyEnabled;
                if (apikeySettingsGroup) apikeySettingsGroup.style.display = apiKeyEnabled ? 'flex' : 'none';
                updateSearchTabVisibility();
            }

            if (settings.apiKey !== undefined) {
                apiKey = settings.apiKey;
                if (apikeyInput) apikeyInput.value = apiKey;
                updateSearchTabVisibility();
            }

            updateGlassmorphism();
            applyLyricsVisibility();
            applyLyricsTypography();
            applyPlayerTypography();
            applyBottomDockLayout();
            syncMilkdropControls();
            updateVisualizerCanvasOpacity();
        }

        function loadSettings() {
            applySettingsObject(PROJECT_DEFAULT_SETTINGS);

            // Restore custom fonts list from dedicated storage if present
            try {
                const storedCustomFonts = localStorage.getItem('spotifm_custom_fonts');
                if (storedCustomFonts) {
                    const parsed = JSON.parse(storedCustomFonts);
                    if (Array.isArray(parsed)) {
                        setCustomFonts(parsed);
                    }
                }
            } catch (e) {
                console.error('Error loading custom fonts from dedicated storage:', e);
            }

            const saved = localStorage.getItem('spotifm_player_settings');
            if (!saved) {
                return;
            }

            try {
                applySettingsObject(JSON.parse(saved));
            } catch (e) {
                console.error('Error loading saved settings from localStorage:', e);
            }
        }

        function updateContainerHeight() {
            container.style.height = '';
        }

        function resizeLyricsPadding() {
            if (scrollPanel && lyricsBody) {
                if (lyricsBody.querySelector('.lyric-placeholder')) {
                    lyricsBody.style.paddingTop = '0px';
                    lyricsBody.style.paddingBottom = '0px';
                    lyricsBody.style.height = '100%';
                    return;
                }
                lyricsBody.style.height = '';
                const padHeight = Math.max(100, scrollPanel.clientHeight / 2 - 20);
                lyricsBody.style.paddingTop = padHeight + 'px';
                lyricsBody.style.paddingBottom = padHeight + 'px';
            }
        }

        function initVisualizer() {
            resizeCanvas();
            resizeLyricsPadding();
            if (!resizeHandlerAttached) {
                window.addEventListener('resize', () => {
                    resizeCanvas();
                    updateContainerHeight();
                    updatePlayerViewportConstraints();
                    syncPlayerTextPositionControls();
                    scheduleAlbumArtSizeUpdate();
                    resizeLyricsPadding();
                    updateLyricsHighlight(true);
                });
                resizeHandlerAttached = true;
            }
            updateVisualizerCanvasOpacity();
            void loadMilkdropPresets()
                .then(() => {
                    if (audioCtx) {
                        return loadCurrentMilkdropPreset(0);
                    }
                    return null;
                })
                .catch((error) => {
                    reportMilkdropError('Milkdrop presets failed', '[Milkdrop] Failed during startup:', error);
                });
            if (visualizerEnabled) {
                void loadButterchurnModule();
            }
        }

        // Run local clock syncing inside regular animation frames
        function tick() {
            maybeCommitPendingLyricsTransition();
            if (isPlaying) {
                updateLyricsHighlight();
            }
            updateTrackProgressDisplay();
            updatePlayerTextBlendLayer();
            maybeRecoverAudioStream();
            maybeRecoverVisualizer();
            maybeRecoverLyricsSocket();
            maybeRefreshNowPlayingSnapshot();
            requestAnimationFrame(tick);
        }

        // ==========================================
        // 5. Sleek API Search Modal Integration
        // ==========================================
        const toggleSearchBtn = document.getElementById('toggle-search-btn');
        const closeSearchBtn = document.getElementById('close-search-btn');
        const searchModal = document.getElementById('search-modal');

        function openSearchModal() {
            if (!searchModal || searchModal.classList.contains('show')) {
                return;
            }
            searchModal.style.display = 'flex';
            markChromeActivity();
            window.setTimeout(() => {
                searchModal.classList.add('show');
            }, 10);
            const input = document.getElementById('search-input');
            if (input) {
                input.focus();
            }
        }

        function closeSearchModal() {
            if (!searchModal) {
                return;
            }
            searchModal.classList.remove('show');
            window.setTimeout(() => {
                searchModal.style.display = 'none';
                markChromeActivity();
            }, 250);
        }

        if (toggleSearchBtn && searchModal && closeSearchBtn) {
            toggleSearchBtn.addEventListener('click', openSearchModal);
            closeSearchBtn.addEventListener('click', closeSearchModal);

            searchModal.addEventListener('click', (e) => {
                if (e.target === searchModal) {
                    closeSearchBtn.click();
                }
            });
        }

        // ==========================================
        // 6. Switch Playlist Modal Integration
        // ==========================================
        const togglePlaylistBtn = document.getElementById('toggle-playlist-btn');
        const closePlaylistBtn = document.getElementById('close-playlist-btn');
        const playlistModal = document.getElementById('playlist-modal');
        const playlistListContainer = document.getElementById('playlist-list-container');
        const playlistNameModal = document.getElementById('playlist-name-modal');
        const playlistNameForm = document.getElementById('playlist-name-form');
        const playlistNameInput = document.getElementById('playlist-name-input');
        const playlistNameError = document.getElementById('playlist-name-error');
        const playlistNameClose = document.getElementById('playlist-name-close');
        const playlistNameCancel = document.getElementById('playlist-name-cancel');
        const playerNoticeRegion = document.getElementById('player-notice-region');
        let playlistNameAction = null;
        const pendingPlaylistAdds = new Set();

        function updatePlayerNotice(notice, message, kind = 'info', durationMs = 4200) {
            if (!notice) {
                console.log(`[Notice/${kind}] ${message}`);
                return null;
            }
            if (notice.dismissTimer) {
                window.clearTimeout(notice.dismissTimer);
                notice.dismissTimer = null;
            }
            notice.className = `player-notice ${kind}`;
            notice.textContent = message;
            if (durationMs > 0) {
                notice.dismissTimer = window.setTimeout(() => notice.remove(), durationMs);
            }
            return notice;
        }

        function showPlayerNotice(message, kind = 'info', durationMs = 4200) {
            if (!playerNoticeRegion) {
                console.log(`[Notice/${kind}] ${message}`);
                return null;
            }
            const notice = document.createElement('div');
            playerNoticeRegion.appendChild(notice);
            return updatePlayerNotice(notice, message, kind, durationMs);
        }

        function closePlaylistNameInput() {
            if (!playlistNameModal) return;
            playlistNameModal.classList.remove('show');
            playlistNameAction = null;
            window.setTimeout(() => {
                playlistNameModal.style.display = 'none';
                markChromeActivity();
            }, 250);
        }

        function requestPlaylistName(action) {
            if (!playlistNameModal || !playlistNameInput || !playlistNameError) return;
            playlistNameAction = action;
            playlistNameInput.value = '';
            playlistNameError.textContent = '';
            playlistNameModal.style.display = 'flex';
            markChromeActivity();
            window.setTimeout(() => {
                playlistNameModal.classList.add('show');
                playlistNameInput.focus();
            }, 10);
        }

        if (playlistNameForm) {
            playlistNameForm.addEventListener('submit', (event) => {
                event.preventDefault();
                const name = playlistNameInput ? playlistNameInput.value.trim() : '';
                if (!/^[A-Za-z0-9]+$/.test(name)) {
                    if (playlistNameError) {
                        playlistNameError.textContent = 'Use at least one ASCII letter or number.';
                    }
                    if (playlistNameInput) playlistNameInput.focus();
                    return;
                }
                const action = playlistNameAction;
                closePlaylistNameInput();
                if (action) void action(name);
            });
        }
        if (playlistNameClose) playlistNameClose.addEventListener('click', closePlaylistNameInput);
        if (playlistNameCancel) playlistNameCancel.addEventListener('click', closePlaylistNameInput);
        if (playlistNameModal) {
            playlistNameModal.addEventListener('click', (event) => {
                if (event.target === playlistNameModal) closePlaylistNameInput();
            });
        }

        if (togglePlaylistBtn && playlistModal && closePlaylistBtn) {
            togglePlaylistBtn.addEventListener('click', async () => {
                playlistModal.style.display = 'flex';
                markChromeActivity();
                setTimeout(() => {
                    playlistModal.classList.add('show');
                }, 10);
                await renderPlaylistModalList();
            });

            closePlaylistBtn.addEventListener('click', () => {
                playlistModal.classList.remove('show');
                setTimeout(() => {
                    playlistModal.style.display = 'none';
                    markChromeActivity();
                }, 250);
            });

            playlistModal.addEventListener('click', (e) => {
                if (e.target === playlistModal) {
                    closePlaylistBtn.click();
                }
            });
        }

        const toggleActivePlaylistBtn = document.getElementById('toggle-active-playlist-btn');
        const closeActivePlaylistBtn = document.getElementById('close-active-playlist-btn');
        const activePlaylistModal = document.getElementById('active-playlist-modal');
        const activePlaylistTrackList = document.getElementById('active-playlist-track-list');
        const activePlaylistSummary = document.getElementById('active-playlist-summary');
        let activePlaylistShouldCenterCurrentTrack = false;
        let activePlaylistCenterFrameId = null;

        function scheduleActivePlaylistCurrentTrackCenter() {
            if (
                !activePlaylistShouldCenterCurrentTrack
                || !activePlaylistModal
                || !activePlaylistTrackList
                || activePlaylistModal.style.display === 'none'
            ) {
                return;
            }

            if (activePlaylistCenterFrameId !== null) {
                window.cancelAnimationFrame(activePlaylistCenterFrameId);
            }

            activePlaylistCenterFrameId = window.requestAnimationFrame(() => {
                activePlaylistCenterFrameId = null;
                const currentTrack = activePlaylistTrackList.querySelector(
                    '.active-playlist-track.current'
                );
                if (
                    !activePlaylistShouldCenterCurrentTrack
                    || activePlaylistModal.style.display === 'none'
                    || !currentTrack
                ) {
                    return;
                }

                currentTrack.scrollIntoView({
                    behavior: 'auto',
                    block: 'center',
                    inline: 'nearest',
                });
                activePlaylistShouldCenterCurrentTrack = false;
            });
        }

        function closeActivePlaylistModal() {
            if (!activePlaylistModal) {
                return;
            }
            activePlaylistShouldCenterCurrentTrack = false;
            if (activePlaylistCenterFrameId !== null) {
                window.cancelAnimationFrame(activePlaylistCenterFrameId);
                activePlaylistCenterFrameId = null;
            }
            activePlaylistModal.classList.remove('show');
            if (container) {
                container.classList.remove('active-playlist-open');
            }
            window.setTimeout(() => {
                activePlaylistModal.style.display = 'none';
                markChromeActivity();
            }, 250);
        }

        async function openActivePlaylistModal() {
            if (!activePlaylistModal) {
                return;
            }
            activePlaylistShouldCenterCurrentTrack = true;
            activePlaylistModal.style.display = 'flex';
            if (container) {
                container.classList.add('active-playlist-open');
            }
            markChromeActivity();
            window.setTimeout(() => activePlaylistModal.classList.add('show'), 10);
            await renderActivePlaylistTracks();
            if (activePlaylistModal.style.display !== 'none') {
                activePlaylistShouldCenterCurrentTrack = true;
            }
            scheduleActivePlaylistCurrentTrackCenter();
        }

        if (toggleActivePlaylistBtn && activePlaylistModal && closeActivePlaylistBtn) {
            toggleActivePlaylistBtn.addEventListener('click', () => {
                void openActivePlaylistModal();
            });
            closeActivePlaylistBtn.addEventListener('click', closeActivePlaylistModal);
            activePlaylistModal.addEventListener('click', (event) => {
                if (event.target === activePlaylistModal) {
                    closeActivePlaylistModal();
                }
            });
        }

        let activeSearchCat = 'track';
        const searchCatBtns = document.querySelectorAll('.search-cat-btn');
        searchCatBtns.forEach(btn => {
            btn.addEventListener('click', () => {
                searchCatBtns.forEach(b => {
                    b.classList.remove('active');
                    b.style.borderColor = 'rgba(255,255,255,0.08)';
                    b.style.background = 'rgba(0,0,0,0.2)';
                    b.style.color = '#9ca3af';
                });
                btn.classList.add('active');
                btn.style.borderColor = 'var(--secondary)';
                btn.style.background = 'rgba(245, 158, 11, 0.1)';
                btn.style.color = 'var(--secondary)';
                activeSearchCat = btn.dataset.cat;
                const searchInput = document.getElementById('search-input');
                if (searchInput && searchInput.value.trim()) {
                    performSearch();
                }
            });
        });

        const searchInput = document.getElementById('search-input');
        const searchBtn = document.getElementById('search-btn');
        const searchResults = document.getElementById('search-results');

        async function performSearch() {
            if (!searchInput || !searchResults) return;
            const q = searchInput.value.trim();
            if (!q) return;

            searchResults.innerHTML = '<div style="color: #6b7280; text-align: center; margin-top: 40px; font-size: 14px;">Searching...</div>';

            try {
                let url = `/search/${activeSearchCat}/20?q=${encodeURIComponent(q)}`;
                if (apiKeyEnabled && apiKey) {
                    url += `&api_key=${encodeURIComponent(apiKey)}`;
                }

                const response = await fetch(url);
                if (!response.ok) {
                    if (response.status === 401 || response.status === 403) {
                        searchResults.innerHTML = `<div style="color: #ef4444; text-align: center; margin-top: 40px; font-size: 14px;">Auth Error: ${response.statusText} (${response.status})<br>Check your API Key in Settings.</div>`;
                    } else {
                        searchResults.innerHTML = `<div style="color: #ef4444; text-align: center; margin-top: 40px; font-size: 14px;">Error: ${response.statusText} (${response.status})</div>`;
                    }
                    return;
                }

                const data = await response.json();
                renderSearchResults(data);
            } catch (err) {
                console.error("Search failed:", err);
                searchResults.innerHTML = '<div style="color: #ef4444; text-align: center; margin-top: 40px; font-size: 14px;">Failed to fetch results</div>';
            }
        }

        if (searchBtn) searchBtn.addEventListener('click', performSearch);
        if (searchInput) {
            searchInput.addEventListener('keydown', (e) => {
                if (e.key === 'Enter') {
                    performSearch();
                }
            });
        }

        let playlistNames = ["default"];
        let activePlaylistRefreshPromise = null;

        function buildAuthenticatedUrl(path) {
            const url = new URL(path, window.location.origin);
            if (apiKeyEnabled && apiKey) {
                url.searchParams.set('api_key', apiKey);
            }
            return url.toString();
        }

        async function fetchActivePlaylistTracks() {
            const response = await fetch(buildAuthenticatedUrl('/playlist'), {
                cache: 'no-store',
            });
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }
            const tracks = await response.json();
            if (!Array.isArray(tracks)) {
                throw new Error('Active playlist response was not an array');
            }
            setActivePlaylistHasTracks(tracks.length > 0);
            return tracks;
        }

        function updateActivePlaylistCurrentTrack() {
            if (!activePlaylistTrackList) {
                return;
            }
            let foundCurrentTrack = false;
            activePlaylistTrackList.querySelectorAll('.active-playlist-track').forEach((element) => {
                const isCurrent = !!currentTrackId && element.dataset.trackId === currentTrackId;
                foundCurrentTrack = foundCurrentTrack || isCurrent;
                element.classList.toggle('current', isCurrent);
                element.setAttribute('aria-current', isCurrent ? 'true' : 'false');
                const action = element.querySelector('.active-playlist-track-action');
                if (action) {
                    action.innerHTML = isCurrent
                        ? '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="4" height="14" rx="1"></rect><rect x="15" y="5" width="4" height="14" rx="1"></rect></svg>'
                        : '<svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z"></path></svg>';
                }
            });
            if (foundCurrentTrack) {
                scheduleActivePlaylistCurrentTrackCenter();
            }
        }

        async function playActivePlaylistTrack(trackId, element) {
            const activationPromise = requestAudiblePlayback(false);
            if (element) {
                element.disabled = true;
                element.classList.add('loading');
            }

            try {
                const response = await fetch(
                    buildAuthenticatedUrl(`/playlist/track/${encodeURIComponent(trackId)}/play`)
                );
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}`);
                }
                const nowPlaying = await response.json();
                applyNowPlayingState(nowPlaying, true);
                finalizeRemotePlaybackActivation(activationPromise);
                updateActivePlaylistCurrentTrack();
                closeActivePlaylistModal();
            } catch (error) {
                console.error('[Playlist] Failed to play active playlist track:', error);
                showPlayerNotice('Failed to play that track', 'error');
            } finally {
                if (element) {
                    element.disabled = false;
                    element.classList.remove('loading');
                }
            }
        }

        async function renderActivePlaylistTracks() {
            if (!activePlaylistTrackList || !activePlaylistSummary) {
                return;
            }

            activePlaylistSummary.textContent = 'Loading tracks…';
            activePlaylistTrackList.innerHTML =
                '<div class="active-playlist-empty">Loading tracks…</div>';

            try {
                const tracks = await fetchActivePlaylistTracks();
                const playlistName =
                    (lastTrackData && lastTrackData.active_playlist) || 'Active playlist';
                activePlaylistSummary.textContent =
                    `${playlistName} • ${tracks.length} ${tracks.length === 1 ? 'track' : 'tracks'}`;
                activePlaylistTrackList.innerHTML = '';

                if (tracks.length === 0) {
                    activePlaylistTrackList.innerHTML =
                        '<div class="active-playlist-empty">This playlist is empty.<br>Use search to add something to play.</div>';
                    return;
                }

                tracks.forEach((track, index) => {
                    const trackButton = document.createElement('button');
                    trackButton.type = 'button';
                    trackButton.className = 'active-playlist-track';
                    trackButton.dataset.trackId = track.track_id;
                    trackButton.setAttribute(
                        'aria-label',
                        `Play ${track.track_name || `track ${index + 1}`}`
                    );

                    let cover;
                    if (track.cover_url) {
                        cover = document.createElement('img');
                        cover.className = 'active-playlist-cover';
                        cover.src = track.cover_url;
                        cover.alt = '';
                        cover.loading = 'lazy';
                    } else {
                        cover = document.createElement('span');
                        cover.className = 'active-playlist-cover-placeholder';
                        cover.innerHTML =
                            '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M9 18V5l12-2v13"></path><circle cx="6" cy="18" r="3"></circle><circle cx="18" cy="16" r="3"></circle></svg>';
                    }

                    const copy = document.createElement('span');
                    copy.className = 'active-playlist-track-copy';
                    const title = document.createElement('span');
                    title.className = 'active-playlist-track-title';
                    title.textContent = track.track_name || `Track ${index + 1}`;
                    const artists = document.createElement('span');
                    artists.className = 'active-playlist-track-artists';
                    const artistText = Array.isArray(track.artists)
                        ? track.artists.join(', ')
                        : '';
                    artists.textContent = track.queue_idx === null || track.queue_idx === undefined
                        ? (artistText || 'Unknown artist')
                        : `Queued • ${artistText || 'Unknown artist'}`;
                    copy.append(title, artists);

                    const action = document.createElement('span');
                    action.className = 'active-playlist-track-action';
                    action.setAttribute('aria-hidden', 'true');

                    trackButton.append(cover, copy, action);
                    trackButton.addEventListener('click', () => {
                        void playActivePlaylistTrack(track.track_id, trackButton);
                    });
                    activePlaylistTrackList.appendChild(trackButton);
                });

                updateActivePlaylistCurrentTrack();
            } catch (error) {
                console.error('[Playlist] Failed to load active playlist:', error);
                activePlaylistSummary.textContent = 'Could not load playlist';
                activePlaylistTrackList.innerHTML =
                    '<div class="active-playlist-empty">The active playlist could not be loaded.</div>';
            }
        }

        async function refreshActivePlaylistState() {
            if (IS_SINGLE_FILE_BUILD_SOURCE) {
                return null;
            }
            if (activePlaylistRefreshPromise) {
                return activePlaylistRefreshPromise;
            }

            activePlaylistRefreshPromise = (async () => {
                try {
                    const tracks = await fetchActivePlaylistTracks();
                    const hasTracks = tracks.length > 0;
                    setActivePlaylistHasTracks(hasTracks);
                    return hasTracks;
                } catch (error) {
                    console.warn('[Playlist] Could not determine whether the active playlist is empty:', error);
                    return null;
                } finally {
                    activePlaylistRefreshPromise = null;
                }
            })();

            return activePlaylistRefreshPromise;
        }

        async function fetchPlaylistNames() {
            try {
                let url = '/playlists';
                if (apiKeyEnabled && apiKey) {
                    url += `?api_key=${encodeURIComponent(apiKey)}`;
                }
                const response = await fetch(url);
                if (response.ok) {
                    const data = await response.json();
                    playlistNames = data.map(p => p.name);
                    if (!playlistNames.includes("default")) {
                        playlistNames.unshift("default");
                    }
                }
            } catch (err) {
                console.error("Failed to fetch playlists:", err);
            }
        }

        async function renderPlaylistModalList() {
            if (!playlistListContainer) return;
            playlistListContainer.innerHTML = '<div style="color: #6b7280; text-align: center; padding: 20px; font-size: 14px;">Loading playlists...</div>';

            try {
                let url = '/playlists';
                if (apiKeyEnabled && apiKey) {
                    url += `?api_key=${encodeURIComponent(apiKey)}`;
                }

                const response = await fetch(url);
                if (!response.ok) {
                    playlistListContainer.innerHTML = `<div style="color: #ef4444; text-align: center; padding: 20px; font-size: 14px;">Error: ${response.statusText}</div>`;
                    return;
                }

                const playlists = await response.json();
                playlistListContainer.innerHTML = '';

                playlists.forEach(p => {
                    const itemEl = document.createElement('div');
                    itemEl.style.display = 'flex';
                    itemEl.style.justifyContent = 'space-between';
                    itemEl.style.alignItems = 'center';
                    itemEl.style.padding = '12px 16px';
                    itemEl.style.borderRadius = '12px';
                    itemEl.style.background = 'rgba(255,255,255,0.03)';
                    itemEl.style.border = '1px solid rgba(255,255,255,0.04)';
                    itemEl.style.cursor = 'pointer';
                    itemEl.style.transition = 'all 0.2s';

                    itemEl.addEventListener('mouseenter', () => {
                        itemEl.style.background = 'rgba(255,255,255,0.08)';
                        itemEl.style.borderColor = 'rgba(255,255,255,0.1)';
                    });
                    itemEl.addEventListener('mouseleave', () => {
                        itemEl.style.background = 'rgba(255,255,255,0.03)';
                        itemEl.style.borderColor = 'rgba(255,255,255,0.04)';
                    });

                    itemEl.innerHTML = `
                        <div style="display: flex; flex-direction: column;">
                            <span style="font-weight: bold; color: white; font-size: 14px;">${p.name}</span>
                            <span style="font-size: 12px; color: #9ca3af;">${p.num_tracks} tracks</span>
                        </div>
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="color: var(--secondary);"><polyline points="9 18 15 12 9 6"></polyline></svg>
                    `;

                    itemEl.addEventListener('click', async () => {
                        await switchActivePlaylist(p.name);
                    });

                    playlistListContainer.appendChild(itemEl);
                });

                const createEl = document.createElement('div');
                createEl.style.display = 'flex';
                createEl.style.alignItems = 'center';
                createEl.style.gap = '8px';
                createEl.style.padding = '12px 16px';
                createEl.style.borderRadius = '12px';
                createEl.style.border = '1px dashed rgba(255,255,255,0.15)';
                createEl.style.cursor = 'pointer';
                createEl.style.transition = 'all 0.2s';
                createEl.style.marginTop = '8px';
                createEl.style.justifyContent = 'center';
                createEl.style.color = 'var(--secondary)';
                createEl.style.fontWeight = 'bold';
                createEl.style.fontSize = '13px';

                createEl.addEventListener('mouseenter', () => {
                    createEl.style.background = 'rgba(245, 158, 11, 0.05)';
                    createEl.style.borderColor = 'var(--secondary)';
                });
                createEl.addEventListener('mouseleave', () => {
                    createEl.style.background = 'transparent';
                    createEl.style.borderColor = 'rgba(255,255,255,0.15)';
                });

                createEl.innerHTML = `
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="12" y1="5" x2="12" y2="19"></line><line x1="5" y1="12" x2="19" y2="12"></line></svg>
                    Create New Playlist...
                `;

                createEl.addEventListener('click', async () => {
                    requestPlaylistName(async (name) => {
                        await switchActivePlaylist(name);
                    });
                });

                playlistListContainer.appendChild(createEl);

            } catch (err) {
                console.error("Failed to load playlists:", err);
                playlistListContainer.innerHTML = '<div style="color: #ef4444; text-align: center; padding: 20px; font-size: 14px;">Failed to fetch playlists</div>';
            }
        }

        async function switchActivePlaylist(name) {
            let url = `/playlist/switch/${name}`;
            if (apiKeyEnabled && apiKey) {
                url += `?api_key=${encodeURIComponent(apiKey)}`;
            }

            try {
                const response = await fetch(url);
                if (!response.ok) {
                    showPlayerNotice(`Failed to switch playlist: ${response.statusText} (${response.status})`, 'error');
                    return;
                }
                const tracks = await response.json();
                setActivePlaylistHasTracks(Array.isArray(tracks) && tracks.length > 0);
                if (closePlaylistBtn) closePlaylistBtn.click();
            } catch (err) {
                console.error("Playlist switch request failed:", err);
                showPlayerNotice("Failed to switch playlist", 'error');
            }
        }

        function renderSearchResults(data) {
            if (!searchResults) return;
            searchResults.innerHTML = '';
            if (!data || data.length === 0) {
                searchResults.innerHTML = '<div style="color: #6b7280; text-align: center; margin-top: 40px; font-size: 14px;">No results found</div>';
                return;
            }

            data.forEach(item => {
                let subtext = '';
                let title = '';
                let id = '';

                if (activeSearchCat === 'track') {
                    title = item.track_name;
                    subtext = item.artists ? item.artists.join(', ') : '';
                    id = item.track_id;
                } else if (activeSearchCat === 'artist') {
                    title = item.artist_name;
                    subtext = item.genres && item.genres.length > 0 ? item.genres.join(', ') : 'Artist';
                    id = item.artist_id;
                } else if (activeSearchCat === 'album') {
                    title = item.album_name;
                    subtext = item.artists ? item.artists.join(', ') : '';
                    id = item.album_id;
                } else if (activeSearchCat === 'playlist') {
                    title = item.playlist_name;
                    subtext = `By ${item.owner || ''} • ${item.total_tracks || 0} tracks`;
                    id = item.playlist_id;
                }

                const itemEl = document.createElement('div');
                itemEl.className = 'search-result-item';
                itemEl.dataset.id = id;
                itemEl.dataset.cat = activeSearchCat;
                itemEl.style.display = 'flex';
                itemEl.style.alignItems = 'center';
                itemEl.style.gap = '12px';
                itemEl.style.padding = '10px';
                itemEl.style.borderRadius = '12px';
                itemEl.style.background = 'rgba(255,255,255,0.03)';
                itemEl.style.border = '1px solid rgba(255,255,255,0.04)';
                itemEl.style.cursor = 'pointer';
                itemEl.style.position = 'relative';
                itemEl.style.transition = 'background 0.2s, transform 0.2s';

                const imgUrl = item.cover_url || '';
                const imgStyle = 'width: 50px; height: 50px; border-radius: 8px; background: rgba(0,0,0,0.4); object-fit: cover; flex-shrink: 0;';
                const imgHtml = imgUrl 
                    ? `<img src="${imgUrl}" alt="Cover" style="${imgStyle}">`
                    : `<div style="${imgStyle}; display: flex; align-items: center; justify-content: center; color: #4b5563;"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg></div>`;

                itemEl.innerHTML = `
                    ${imgHtml}
                    <div style="display: flex; flex-direction: column; min-width: 0; flex-grow: 1; margin-right: 90px;">
                        <span style="font-weight: bold; font-size: 14px; color: white; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${title}</span>
                        <span style="font-size: 12px; color: #9ca3af; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${subtext}</span>
                    </div>
                    <div class="result-actions" style="display: flex; gap: 6px; align-items: center; position: absolute; right: 10px; z-index: 10;">
                        <!-- Play Button -->
                        <button class="result-action-btn play-btn" title="Play Now" style="background: rgba(255,255,255,0.05); border: 1px solid rgba(255,255,255,0.08); color: #10b981; width: 28px; height: 28px; cursor: pointer; border-radius: 6px; display: flex; align-items: center; justify-content: center; transition: all 0.2s;">
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                        </button>
                        <!-- Queue Button -->
                        <button class="result-action-btn queue-btn" title="Add to Queue" style="background: rgba(255,255,255,0.05); border: 1px solid rgba(255,255,255,0.08); color: #3b82f6; width: 28px; height: 28px; cursor: pointer; border-radius: 6px; display: flex; align-items: center; justify-content: center; transition: all 0.2s;">
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="8" y1="6" x2="21" y2="6"></line><line x1="8" y1="12" x2="21" y2="12"></line><line x1="8" y1="18" x2="21" y2="18"></line><line x1="3" y1="6" x2="3.01" y2="6"></line><line x1="3" y1="12" x2="3.01" y2="12"></line><line x1="3" y1="18" x2="3.01" y2="18"></line></svg>
                        </button>
                        <!-- Add to Playlist Button -->
                        <button class="result-action-btn playlist-btn" title="Add to Playlist" style="background: rgba(255,255,255,0.05); border: 1px solid rgba(255,255,255,0.08); color: #f59e0b; width: 28px; height: 28px; cursor: pointer; border-radius: 6px; display: flex; align-items: center; justify-content: center; transition: all 0.2s;">
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path><polyline points="14 2 14 8 20 8"></polyline><line x1="12" y1="18" x2="12" y2="12"></line><line x1="9" y1="15" x2="15" y2="15"></line></svg>
                        </button>
                    </div>
                `;

                const playBtn = itemEl.querySelector('.play-btn');
                const queueBtn = itemEl.querySelector('.queue-btn');
                const playlistBtn = itemEl.querySelector('.playlist-btn');

                const hasAll = activePermissions.includes("*");
                const canPlay = hasAll || activePermissions.includes("play");
                const canQueue = hasAll || activePermissions.includes("queue");
                const canPlaylist = hasAll || activePermissions.includes("playlist");

                if (!canPlay && playBtn) playBtn.style.display = 'none';
                if (!canQueue && queueBtn) queueBtn.style.display = 'none';
                if (!canPlaylist && playlistBtn) playlistBtn.style.display = 'none';

                const actionBtns = itemEl.querySelectorAll('.result-action-btn');
                actionBtns.forEach(btn => {
                    btn.addEventListener('mouseenter', () => {
                        btn.style.background = 'rgba(255,255,255,0.15)';
                        btn.style.transform = 'scale(1.1)';
                    });
                    btn.addEventListener('mouseleave', () => {
                        btn.style.background = 'rgba(255,255,255,0.05)';
                        btn.style.transform = 'scale(1)';
                    });
                });

                if (playBtn) {
                    playBtn.addEventListener('click', (e) => {
                        e.stopPropagation();
                        playSearchResult(activeSearchCat, id);
                    });
                }

                if (queueBtn) {
                    queueBtn.addEventListener('click', (e) => {
                        e.stopPropagation();
                        queueSearchResult(activeSearchCat, id);
                    });
                }

                if (playlistBtn) {
                    playlistBtn.addEventListener('click', (e) => {
                        e.stopPropagation();
                        showAddToPlaylistMenu(playlistBtn, activeSearchCat, id);
                    });
                }

                itemEl.addEventListener('click', () => {
                    if (canPlay) {
                        playSearchResult(activeSearchCat, id);
                    }
                });

                searchResults.appendChild(itemEl);
            });
        }

        async function showAddToPlaylistMenu(buttonEl, category, id) {
            await fetchPlaylistNames();

            const existing = document.getElementById('add-to-playlist-dropdown');
            if (existing) existing.remove();

            const dropdown = document.createElement('div');
            dropdown.id = 'add-to-playlist-dropdown';
            dropdown.style.position = 'fixed';
            dropdown.style.zIndex = '11000';
            dropdown.style.background = 'rgba(22, 18, 36, 0.98)';
            dropdown.style.backdropFilter = 'blur(10px)';
            dropdown.style.border = '1px solid var(--border)';
            dropdown.style.borderRadius = '12px';
            dropdown.style.boxShadow = '0 10px 25px rgba(0,0,0,0.5)';
            dropdown.style.display = 'flex';
            dropdown.style.flexDirection = 'column';
            dropdown.style.padding = '6px';
            dropdown.style.minWidth = '160px';
            dropdown.style.maxHeight = '200px';
            dropdown.style.overflowY = 'auto';

            const rect = buttonEl.getBoundingClientRect();
            dropdown.style.top = `${rect.bottom + window.scrollY + 5}px`;
            dropdown.style.left = `${Math.min(window.innerWidth - 180, rect.left + window.scrollX - 80)}px`;

            playlistNames.forEach(name => {
                const opt = document.createElement('div');
                opt.style.padding = '8px 12px';
                opt.style.borderRadius = '8px';
                opt.style.color = '#cbd5e1';
                opt.style.fontSize = '12px';
                opt.style.fontWeight = '600';
                opt.style.cursor = 'pointer';
                opt.style.transition = 'all 0.2s';
                opt.textContent = name;

                opt.addEventListener('mouseenter', () => {
                    opt.style.background = 'rgba(255, 255, 255, 0.08)';
                    opt.style.color = 'white';
                });
                opt.addEventListener('mouseleave', () => {
                    opt.style.background = 'transparent';
                    opt.style.color = '#cbd5e1';
                });

                opt.addEventListener('click', async (e) => {
                    e.stopPropagation();
                    dropdown.remove();
                    await addToPlaylist(name, category, id);
                });

                dropdown.appendChild(opt);
            });

            const createOpt = document.createElement('div');
            createOpt.style.padding = '8px 12px';
            createOpt.style.borderRadius = '8px';
            createOpt.style.color = 'var(--secondary)';
            createOpt.style.fontSize = '12px';
            createOpt.style.fontWeight = 'bold';
            createOpt.style.cursor = 'pointer';
            createOpt.style.borderTop = '1px solid rgba(255,255,255,0.08)';
            createOpt.style.marginTop = '4px';
            createOpt.textContent = '+ New Playlist...';

            createOpt.addEventListener('mouseenter', () => {
                createOpt.style.background = 'rgba(245, 158, 11, 0.05)';
            });
            createOpt.addEventListener('mouseleave', () => {
                createOpt.style.background = 'transparent';
            });

            createOpt.addEventListener('click', async (e) => {
                e.stopPropagation();
                dropdown.remove();
                requestPlaylistName(async (name) => {
                    await addToPlaylist(name, category, id);
                });
            });

            dropdown.appendChild(createOpt);
            document.body.appendChild(dropdown);

            const closeHandler = () => {
                dropdown.remove();
                document.removeEventListener('click', closeHandler);
            };
            setTimeout(() => {
                document.addEventListener('click', closeHandler);
            }, 50);
        }

        async function addToPlaylist(playlistName, category, id) {
            let paramName = '';
            if (category === 'track') paramName = 'tracks[]';
            else if (category === 'album') paramName = 'albums[]';
            else if (category === 'artist') paramName = 'artists[]';
            else if (category === 'playlist') paramName = 'playlists[]';

            const operationKey = `${playlistName}:${category}:${id}`;
            if (pendingPlaylistAdds.has(operationKey)) {
                showPlayerNotice(`Already adding this ${category} to "${playlistName}".`, 'info');
                return;
            }

            pendingPlaylistAdds.add(operationKey);
            const processingNotice = showPlayerNotice(
                `Adding ${category} to "${playlistName}"… Large discographies may take a moment.`,
                'processing',
                0
            );

            let url = `/playlist/${encodeURIComponent(playlistName)}/add?${paramName}=${encodeURIComponent(id)}`;
            if (apiKeyEnabled && apiKey) {
                url += `&api_key=${encodeURIComponent(apiKey)}`;
            }

            try {
                const response = await fetch(url);
                if (!response.ok) {
                    let detail = '';
                    try {
                        const errorPayload = await response.json();
                        detail = errorPayload.error || errorPayload.message || '';
                    } catch (_) {}
                    updatePlayerNotice(
                        processingNotice,
                        detail || `Failed to add: ${response.statusText} (${response.status})`,
                        'error'
                    );
                    return;
                }
                let result = null;
                try {
                    result = await response.json();
                } catch (_) {}
                await refreshActivePlaylistState();
                const addedTracks = result ? Number(result.added_tracks) : NaN;
                const successMessage = Number.isFinite(addedTracks)
                    ? `Added ${addedTracks} track${addedTracks === 1 ? '' : 's'} to "${playlistName}".`
                    : `Added to playlist "${playlistName}" successfully!`;
                updatePlayerNotice(processingNotice, successMessage, 'success');
            } catch (err) {
                console.error("Add to playlist request failed:", err);
                updatePlayerNotice(processingNotice, "Failed to add selection to playlist", 'error');
            } finally {
                pendingPlaylistAdds.delete(operationKey);
            }
        }

        async function queueSearchResult(cat, id) {
            let url = `/queue/${cat}/${id}`;
            const params = [];
            if (apiKeyEnabled && apiKey) {
                params.push(`api_key=${encodeURIComponent(apiKey)}`);
            }
            if (params.length > 0) {
                url += `?${params.join('&')}`;
            }

            try {
                const response = await fetch(url);
                if (!response.ok) {
                    showPlayerNotice(`Failed to queue: ${response.statusText} (${response.status})`, 'error');
                    return;
                }
                setActivePlaylistHasTracks(true);
                showPlayerNotice("Added to queue!", 'success');
                if (closeSearchBtn) closeSearchBtn.click();
            } catch (err) {
                console.error("Queue request failed:", err);
                showPlayerNotice("Failed to queue selection", 'error');
            }
        }

        async function playSearchResult(cat, id) {
            const activationPromise = requestAudiblePlayback(false);
            let url = `/play/${cat}/${id}`;
            const params = [];
            if (apiKeyEnabled && apiKey) {
                params.push(`api_key=${encodeURIComponent(apiKey)}`);
            }
            if (params.length > 0) {
                url += `?${params.join('&')}`;
            }

            try {
                const response = await fetch(url);
                if (!response.ok) {
                    showPlayerNotice(`Failed to play: ${response.statusText} (${response.status})`, 'error');
                    return;
                }
                const nowPlaying = await response.json();
                applyNowPlayingState(nowPlaying, true);
                setActivePlaylistHasTracks(true);
                finalizeRemotePlaybackActivation(activationPromise);
                if (closeSearchBtn) closeSearchBtn.click();
            } catch (err) {
                console.error("Playback request failed:", err);
                showPlayerNotice("Failed to play selection", 'error');
            }
        }

        let activePermissions = ["*"];

        async function checkAuthPermissions() {
            try {
                let url = '/api/privs';
                if (apiKeyEnabled && apiKey) {
                    url += `?api_key=${encodeURIComponent(apiKey)}`;
                }
                const response = await fetch(url);
                if (response.ok) {
                    const data = await response.json();
                    activePermissions = data.permissions || [];
                } else {
                    activePermissions = [];
                }
            } catch (err) {
                console.error("Failed to check auth permissions:", err);
                activePermissions = ["*"]; // default allowed on network failure
            }
            applyAuthPermissions();
        }

        function applyAuthPermissions() {
            const hasAll = activePermissions.includes("*");
            const canSearch = hasAll || activePermissions.includes("search");
            const canPlaylist = hasAll || activePermissions.includes("playlist");
            const canPlay = hasAll || activePermissions.includes("play");

            const toggleSearchBtn = document.getElementById('toggle-search-btn');
            const togglePlaylistBtn = document.getElementById('toggle-playlist-btn');
            const toggleActivePlaylistBtn = document.getElementById('toggle-active-playlist-btn');

            if (toggleSearchBtn) {
                toggleSearchBtn.style.display = (apiKeyEnabled && canSearch) ? 'flex' : 'none';
            }
            if (togglePlaylistBtn) {
                togglePlaylistBtn.style.display = (apiKeyEnabled && canPlaylist) ? 'flex' : 'none';
            }
            if (toggleActivePlaylistBtn) {
                toggleActivePlaylistBtn.style.display =
                    (apiKeyEnabled && canPlaylist && canPlay) ? 'flex' : 'none';
            }

            if (!apiKeyEnabled || !canSearch) {
                if (searchModal && searchModal.classList.contains('show')) {
                    if (closeSearchBtn) closeSearchBtn.click();
                }
            }
            if (!apiKeyEnabled || !canPlaylist) {
                const playlistModal = document.getElementById('playlist-modal');
                if (playlistModal && playlistModal.classList.contains('show')) {
                    const closePlaylistBtn = document.getElementById('close-playlist-btn');
                    if (closePlaylistBtn) closePlaylistBtn.click();
                }
            }
            if (!apiKeyEnabled || !canPlaylist || !canPlay) {
                closeActivePlaylistModal();
            }
        }

        function updateSearchTabVisibility() {
            void checkAuthPermissions();
        }

        let trackInfoResizeObserver = null;
        if (trackInfo && typeof ResizeObserver !== 'undefined') {
            trackInfoResizeObserver = new ResizeObserver(() => {
                scheduleAlbumArtSizeUpdate();
            });
            trackInfoResizeObserver.observe(trackInfo);
        }

        loadSettings();
        applyEffectiveVolume();
        updateGlassmorphism();
        applyLyricsVisibility();
        applyLyricsTypography();
        applyPlayerTypography();
        applyBottomDockLayout();
        updateTrackProgressDisplay();
        scheduleAlbumArtSizeUpdate();
        updateVisualizerCanvasOpacity();
        initVisualizer();
        void initializeAudioStream();
        void refreshActivePlaylistState();
        requestAnimationFrame(tick);

        // Remove no-transition class to allow smooth layout animations on user interaction
        window.addEventListener('load', () => {
            // Use setTimeout to ensure the layout has rendered in its initial state before enabling transitions
            setTimeout(() => {
                if (container) container.classList.remove('no-transition');
            }, 100);
        });
