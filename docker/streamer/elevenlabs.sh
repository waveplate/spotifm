#!/bin/bash

curl -X "POST" \
  "https://api.elevenlabs.io/v1/text-to-speech/$2" \
  --header "accept: audio/mpeg" \
  --header "xi-api-key: $1" \
  --header "Content-Type: application/json" \
  --data "{
    \"text\": \"$3\",
    \"model_id\": \"eleven_monolingual_v1\",
    \"voice_settings\": {
      \"stability\": 0,
      \"similarity_boost\": 0
    }
  }" \
  -o /tmp/output.mp3