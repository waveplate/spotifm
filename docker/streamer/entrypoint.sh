#!/bin/bash

cd ~

echo "Starting pulseaudio ..."
pulseaudio -D --exit-idle-time=-1
sleep 2
pactl load-module module-null-sink sink_name=spotifm_music sink_properties=device.description=spotifm_music
pactl load-module module-null-sink sink_name=spotifm_announce sink_properties=device.description=spotifm_announce
pactl load-module module-null-sink sink_name=spotifm_master sink_properties=device.description=spotifm_master
sleep 1
pactl load-module module-loopback source=spotifm_music.monitor sink=spotifm_master
pactl load-module module-loopback source=spotifm_announce.monitor sink=spotifm_master

pactl set-default-source spotifm_master.monitor

echo "Starting darkice ..."
darkice -c /etc/darkice.cfg &
sleep 2

echo "Starting keepalive ..."
/keepalive.sh &

echo "Starting spotifm ..."
PULSE_SINK=spotifm_music spotifm /etc/spotifm.json

