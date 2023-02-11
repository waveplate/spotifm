#!/bin/bash

cd ~

echo "Starting pulseaudio ..."
pulseaudio -D --exit-idle-time=-1
sleep 2

echo "Starting darkice ..."
darkice -c /etc/darkice.cfg &
sleep 2

echo "Starting spotify-fm ..."
spotify-fm $SPOTIFY_USER $SPOTIFY_PASS "$SPOTIFY_URI" pulseaudio

