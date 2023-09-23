# Define the PulseAudio input
input = input.pulseaudio(
    device="auto_null.monitor"
)

# Define the Icecast2 output
output.icecast(
    %vorbis.cbr(samplerate=44100, channels=2, bitrate=320),
#   %mp3(bitrate=192),
#   %vorbis(samplerate=44100, channels=2, quality=0.8), 
#   %vorbis.abr(samplerate=44100, channels=2, bitrate=192, max_bitrate=320, min_bitrate=128),
#   %vorbis.cbr(samplerate=44100, channels=2, bitrate=320),
#   %mp3.abr(bitrate=128, max_bitrate=192, id3v2=true),
    input,
    host="icecast2",
    port=8000,
    password="hackme",
    mount="/listen",
    description="SpotiFM Radio",
    url="https://github.com/waveplate/spotifm"
)
