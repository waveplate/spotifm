use crate::state::AppState;
use serde::Serialize;
use std::{convert::Infallible, sync::Arc};
use warp::Reply;

#[derive(Serialize)]
struct OAuthStatusResponse {
    status: &'static str,
    player_url: String,
}

pub fn authorization_transition_page(phase: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Authorization Successful - Spotifm</title>
    <style>
        :root {{ color-scheme: dark; }}
        * {{ box-sizing: border-box; }}
        body {{
            min-height: 100vh;
            margin: 0;
            display: grid;
            place-items: center;
            padding: 24px;
            overflow: hidden;
            font-family: Inter, Outfit, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
            color: #f8fafc;
            background:
                radial-gradient(circle at 20% 15%, rgba(168, 85, 247, .24), transparent 38%),
                radial-gradient(circle at 85% 80%, rgba(217, 70, 239, .18), transparent 40%),
                #09070f;
        }}
        .card {{
            width: min(480px, 100%);
            padding: 42px;
            text-align: center;
            border: 1px solid rgba(255, 255, 255, .18);
            border-radius: 28px;
            background: rgba(22, 18, 36, .78);
            backdrop-filter: blur(24px) saturate(125%);
            box-shadow: 0 28px 80px rgba(0, 0, 0, .55);
        }}
        .status-icon {{
            width: 72px;
            height: 72px;
            margin: 0 auto 24px;
            display: grid;
            place-items: center;
            border-radius: 50%;
            color: #fff;
            background: linear-gradient(135deg, #a855f7, #d946ef);
            box-shadow: 0 0 34px rgba(168, 85, 247, .35);
        }}
        .spinner {{
            width: 30px;
            height: 30px;
            border: 3px solid rgba(255, 255, 255, .32);
            border-top-color: #fff;
            border-radius: 50%;
            animation: spin .8s linear infinite;
        }}
        .check {{ display: none; width: 34px; height: 34px; }}
        .ready .spinner {{ display: none; }}
        .ready .check {{ display: block; }}
        @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
        h1 {{ margin: 0 0 12px; font-size: clamp(26px, 6vw, 34px); letter-spacing: -.03em; }}
        p {{ margin: 0; color: #aeb5c4; line-height: 1.65; }}
        .phase {{ margin-top: 10px; color: #d8b4fe; font-size: 13px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }}
        .button {{
            display: none;
            width: 100%;
            margin-top: 28px;
            padding: 14px 20px;
            border-radius: 999px;
            color: #fff;
            font-weight: 750;
            text-decoration: none;
            background: linear-gradient(135deg, #a855f7, #d946ef);
            box-shadow: 0 12px 28px rgba(168, 85, 247, .28);
            transition: transform .18s ease, filter .18s ease;
        }}
        .button:hover {{ transform: translateY(-2px); filter: brightness(1.08); }}
        .ready .button {{ display: inline-block; }}
    </style>
</head>
<body>
    <main class="card" id="authorization-card">
        <div class="status-icon">
            <span class="spinner" aria-hidden="true"></span>
            <svg class="check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="m5 12 4 4L19 6"/></svg>
        </div>
        <h1 id="authorization-title">Checking authorization…</h1>
        <p id="authorization-message">Spotifm is verifying your Spotify session and checking whether another authorization is needed.</p>
        <div class="phase">{phase}</div>
        <a class="button" id="player-link" href="/">Open Web Player</a>
    </main>
    <script>
        (() => {{
            const card = document.getElementById('authorization-card');
            const title = document.getElementById('authorization-title');
            const message = document.getElementById('authorization-message');
            const playerLink = document.getElementById('player-link');

            async function checkAuthorization() {{
                try {{
                    const response = await fetch('/oauth/status', {{ cache: 'no-store' }});
                    if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
                    const result = await response.json();

                    if (result.status === 'authorization_required') {{
                        title.textContent = 'Additional authorization required…';
                        message.textContent = `Continuing with ${{result.phase || 'Spotify'}} authorization.`;
                        window.setTimeout(() => window.location.assign('/oauth'), 500);
                        return;
                    }}

                    if (result.status === 'authorized') {{
                        card.classList.add('ready');
                        title.textContent = 'Authorization Successful!';
                        message.textContent = 'Spotifm is connected to Spotify and ready to play.';
                        playerLink.href = result.player_url || '/';
                        return;
                    }}
                }} catch (_) {{
                    // The temporary OAuth server is briefly replaced between phases.
                }}
                window.setTimeout(checkAuthorization, 750);
            }}

            checkAuthorization();
        }})();
    </script>
</body>
</html>"#
    )
}

pub async fn handle_oauth_status(state: Arc<AppState>) -> Result<impl Reply, Infallible> {
    Ok(warp::reply::json(&OAuthStatusResponse {
        status: "authorized",
        player_url: state.cli.player_endpoint.clone(),
    }))
}

pub async fn handle_oauth_page() -> Result<impl Reply, Infallible> {
    Ok(warp::http::Response::builder()
        .header("content-type", "text/html; charset=utf-8")
        .header("cache-control", "no-store")
        .body(authorization_transition_page("Spotify connected"))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::authorization_transition_page;

    #[test]
    fn transition_page_checks_readiness_before_showing_the_player() {
        let page = authorization_transition_page("Playback");

        assert!(page.contains("Checking authorization…"));
        assert!(page.contains("fetch('/oauth/status'"));
        assert!(page.contains("Authorization Successful!"));
        assert!(page.contains("Open Web Player"));
    }
}
