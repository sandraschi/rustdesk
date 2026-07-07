/// rustdesk++ OAuth login for public RustDesk server.
/// 
/// The public RustDesk server (rs-ny.rustdesk.com) now requires OAuth login
/// (Google/GitHub) due to botnet abuse. This module implements the flow:
/// 1. Open browser to OAuth URL
/// 2. Start local HTTP server for callback
/// 3. Exchange code for token
/// 4. Store token for subsequent connections
///
/// Reference: https://github.com/rustdesk/rustdesk/wiki/Login-required-for-public-server

use hbb_common::{config::Config, log};
use std::collections::HashMap;

const TOKEN_KEY: &str = "access_token";
const OAUTH_URL: &str = "https://api.rustdesk.com/oauth/authorize";

/// Get a stored OAuth token, or trigger login flow.
pub async fn ensure_token() -> Result<String, String> {
    // Check for existing token
    let token = Config::get_option(TOKEN_KEY);
    if !token.is_empty() {
        log::info!("Using stored OAuth token");
        return Ok(token);
    }

    // No token — trigger login flow
    log::info!("No OAuth token found, starting login flow...");
    let token = login_flow().await?;

    // Store the token
    Config::set_option(TOKEN_KEY.to_string(), token.clone());
    Ok(token)
}

async fn login_flow() -> Result<String, String> {
    // Phase 1: Open browser to OAuth page
    let state = generate_state();
    let url = format!("{}?response_type=code&state={}", OAUTH_URL, state);

    // Open browser
    if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(&["/c", "start", &url])
            .spawn()
            .map_err(|e| format!("open browser: {}", e))?;
    } else {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("open browser: {}", e))?;
    }

    // Phase 2: Start local callback server
    let token = start_callback_server().await?;

    log::info!("OAuth login successful");
    Ok(token)
}

fn generate_state() -> String {
    use hbb_common::rand;
    let state: String = (0..16)
        .map(|_| {
            let idx = rand::random::<usize>() % 36;
            "abcdefghijklmnopqrstuvwxyz0123456789".chars().nth(idx).unwrap()
        })
        .collect();
    state
}

async fn start_callback_server() -> Result<String, String> {
    // Start a minimal HTTP server on 127.0.0.1:10810 to receive the OAuth callback
    let listener = std::net::TcpListener::bind("127.0.0.1:10810")
        .map_err(|e| format!("callback server bind: {}", e))?;

    log::info!("OAuth callback server listening on 127.0.0.1:10810");

    match listener.accept() {
        Ok((mut stream, _)) => {
            let mut buf = [0; 4096];
            let n = std::io::Read::read(&mut stream, &mut buf)
                .map_err(|e| format!("callback read: {}", e))?;

            let request = String::from_utf8_lossy(&buf[..n]);

            let code = parse_code_from_request(&request)?;

            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Login successful!</h1><p>You can close this window.</p></body></html>";
            std::io::Write::write_all(&mut stream, response.as_bytes())
                .map_err(|e| format!("respond: {}", e))?;

            exchange_code_for_token(&code).await
        }
        Err(e) => Err(format!("callback accept: {}", e)),
    }
}

fn parse_code_from_request(request: &str) -> Result<String, String> {
    // Parse GET parameters
    if let Some(query) = request.split(' ').nth(1) {
        for param in query.split('&') {
            if let Some(code) = param.strip_prefix("/?code=") {
                let code = code.split('&').next().unwrap_or(code);
                return Ok(code.to_string());
            }
            if let Some(code) = param.strip_prefix("?code=") {
                let code = code.split('&').next().unwrap_or(code);
                return Ok(code.to_string());
            }
        }
    }
    Err("No authorization code in callback URL".into())
}

async fn exchange_code_for_token(code: &str) -> Result<String, String> {
    // POST to the RustDesk token exchange endpoint
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.rustdesk.com/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", "rustdesk-cli"),
        ])
        .send()
        .await
        .map_err(|e| format!("token exchange request: {}", e))?;

    let body: HashMap<String, String> = resp
        .json()
        .await
        .map_err(|e| format!("token exchange response: {}", e))?;

    body.get("access_token")
        .cloned()
        .ok_or_else(|| "No access_token in response".into())
}
