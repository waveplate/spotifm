#!/bin/bash
echo "Starting pulseaudio..."
su - user -c "pulseaudio -D --exit-idle-time=-1"
sleep 2

echo "Starting Icecast2..."
/etc/init.d/icecast2 start
sleep 2

echo "Starting darkice..."
su - user -c "darkice -c /etc/darkice.cfg &"
sleep 2

echo "Starting spotifyd..."
su - user -c "spotify-server.sh $SPOTIFY_USER $SPOTIFY_PASS"

