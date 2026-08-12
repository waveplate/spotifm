# HTTPS and nginx

[documentation index](README.md) · [project readme](../README.md)

## certificates

spotifm serves the player, audio stream, websocket, and REST API over HTTP and HTTPS:

- HTTP: `http://<host>:3333`
- HTTPS: `https://<host>:3443`

spotifm generates and reuses a self-signed certificate and private key at:

```text
$XDG_CACHE_HOME/spotifm/tls/cert.pem
$XDG_CACHE_HOME/spotifm/tls/key.pem
```

when `XDG_CACHE_HOME` is unset, the files are stored under `~/.cache/spotifm/tls/`.

to use another certificate:

```toml
tls_cert = "/etc/letsencrypt/live/radio.example/fullchain.pem"
tls_key = "/etc/letsencrypt/live/radio.example/privkey.pem"
```

or:

```sh
spotifm --tls-cert /path/to/fullchain.pem --tls-key /path/to/privkey.pem
```

<section>

## nginx reverse proxy

<details>
<summary>nginx configuration</summary>

start spotifm:

```sh
./target/release/spotifm --no-browser
```

or with docker:

```sh
docker run --rm -d \
  --name spotifm \
  -p 127.0.0.1:3333:3333 \
  -p 8898:8898 \
  -v spotifm-cache:/root/.cache/spotifm \
  -v spotifm-data:/root/.local/share/spotifm \
  waveplate/spotifm:latest
```

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 80;
    server_name radio.example;
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    server_name radio.example;

    ssl_certificate /etc/letsencrypt/live/radio.example/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/radio.example/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:3333;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 1d;
    }
}
```

```sh
sudo nginx -t && sudo systemctl reload nginx
```

visit `https://radio.example/oauth` to authorize.

</details>

</section>
