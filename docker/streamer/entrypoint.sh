#!/bin/bash

cd ~

echo "Starting pulseaudio ..."
pulseaudio -D --exit-idle-time=-1
sleep 2

echo "Starting liquidsoap ..."
liquidsoap /etc/liquidsoap/pulse.liq --daemon
sleep 2

echo "Starting keepalive ..."
/keepalive.sh &

echo "Starting spotifm ..."

spotifm /etc/spotifm.json