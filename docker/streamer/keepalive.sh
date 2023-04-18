#!/bin/bash

# if more than 5 seconds of silence, send SIGALRM to spotifm so it renews the session
while true; do
        PULSE_SOURCE=auto_null.monitor sox -d -n silence 1 1 0% 1 5.0 0%
        pkill -SIGALRM spotifm
        sleep 5
done
