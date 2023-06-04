#!/bin/bash

# if more than 5 seconds of silence, send SIGALRM to spotifm so it renews the session
while true; do
        PULSE_SOURCE=spotifm_music.monitor sox -d -n silence 1 1 0% 1 15.0 0%
        pkill -SIGALRM spotifm
	touch /tmp/silence_$(date +%s)
        sleep 5
done
