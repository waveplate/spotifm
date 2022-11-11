#!/bin/bash
#set -x
USER="$1"
PASS="$2"

function play {
	echo pkill -f spotify-fm -9
	echo spotify-fm "$USER" "$PASS" "$1" pulseaudio
}

function skip {
	echo "skip"
}

while read line
do
	action=""
	trackId=""

	for i in $(echo "$line"|grep -oE '^(\w+=\w+&?)+'|tr '&' ' '); do eval ${i}; done;

	case $action in
		play)
			play $trackId
			;;
		skip)
			skip
			;;
	esac


done < <(socat -v TCP-LISTEN:4040,reuseaddr,fork system:'echo "HTTP/1.1 200 OK\r\n"' 2>&1)
#done < <(socat - TCP-LISTEN:4040,fork,reuseaddr|grep -oE '^(\w+=\w+&?)+')
