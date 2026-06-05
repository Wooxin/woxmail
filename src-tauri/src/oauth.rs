use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const GMAIL_SCOPE: &str = "https://mail.google.com/ openid email profile";
const OUTLOOK_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const OUTLOOK_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const OUTLOOK_SCOPE: &str = "offline_access openid email profile https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send";
pub const DEFAULT_GMAIL_CLIENT_ID: &str =
    "171866449816-5ful4dh0qf8n6ml860972fv7u1onkb30.apps.googleusercontent.com";
pub const DEFAULT_OUTLOOK_CLIENT_ID: &str = "d398e2a0-59cb-4c6c-81c6-cbd9b7c1d366";

#[derive(Debug, Clone)]
pub struct OAuthTokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct GmailOAuthResult {
    pub email: String,
    pub name: String,
    pub tokens: OAuthTokenSet,
}

#[derive(Debug, Clone)]
pub struct OutlookOAuthResult {
    pub email: String,
    pub name: String,
    pub tokens: OAuthTokenSet,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftIdClaims {
    email: Option<String>,
    preferred_username: Option<String>,
    name: Option<String>,
}

pub fn run_gmail_oauth(
    client_id: &str,
    auth_url_opener: impl FnOnce(String) -> Result<(), String>,
) -> Result<GmailOAuthResult, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/gmail/callback");
    let state = pkce_random();
    let verifier = pkce_random();
    let challenge = pkce_challenge(&verifier);

    let auth_url = format!(
        "{AUTH_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        enc(client_id),
        enc(&redirect_uri),
        enc(GMAIL_SCOPE),
        enc(&state),
        enc(&challenge),
    );

    auth_url_opener(auth_url)?;
    let code = wait_for_callback(listener, &state, "Gmail")?;
    let (tokens, _) = exchange_code(
        "Google",
        TOKEN_URL,
        client_id,
        &redirect_uri,
        &verifier,
        &code,
    )?;
    let (email, name) = fetch_userinfo(&tokens.access_token)?;

    Ok(GmailOAuthResult {
        email,
        name,
        tokens,
    })
}

pub fn run_outlook_oauth(
    client_id: &str,
    auth_url_opener: impl FnOnce(String) -> Result<(), String>,
) -> Result<OutlookOAuthResult, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://localhost:{port}/oauth/outlook/callback");
    let state = pkce_random();
    let verifier = pkce_random();
    let challenge = pkce_challenge(&verifier);

    let auth_url = format!(
        "{OUTLOOK_AUTH_URL}?response_type=code&client_id={}&redirect_uri={}&response_mode=query&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=select_account",
        enc(client_id),
        enc(&redirect_uri),
        enc(OUTLOOK_SCOPE),
        enc(&state),
        enc(&challenge),
    );

    auth_url_opener(auth_url)?;
    let code = wait_for_callback(listener, &state, "Outlook")?;
    let (tokens, id_token) = exchange_code(
        "Outlook",
        OUTLOOK_TOKEN_URL,
        client_id,
        &redirect_uri,
        &verifier,
        &code,
    )?;
    let (email, name) = parse_microsoft_identity(id_token.as_deref())?;

    Ok(OutlookOAuthResult {
        email,
        name,
        tokens,
    })
}

pub fn refresh_access_token(
    provider: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<OAuthTokenSet, String> {
    let provider_name = oauth_provider_name(provider)?;
    let token_url = token_url_for(provider)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    let token = response
        .json::<TokenResponse>()
        .map_err(|e| e.to_string())?;
    token_set_from_response(provider_name, token, refresh_token.to_string())
}

fn exchange_code(
    provider_name: &str,
    token_url: &str,
    client_id: &str,
    redirect_uri: &str,
    verifier: &str,
    code: &str,
) -> Result<(OAuthTokenSet, Option<String>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .map_err(|e| e.to_string())?;
    let token = response
        .json::<TokenResponse>()
        .map_err(|e| e.to_string())?;
    let refresh_token = token.refresh_token.clone().ok_or_else(|| {
        format!("{provider_name} 没有返回 refresh token。请重新网页登录，并确认授权页允许 Wox Mail 访问邮箱。")
    })?;
    let id_token = token.id_token.clone();
    let tokens = token_set_from_response(provider_name, token, refresh_token)?;
    Ok((tokens, id_token))
}

fn token_set_from_response(
    provider_name: &str,
    token: TokenResponse,
    refresh_token: String,
) -> Result<OAuthTokenSet, String> {
    if let Some(error) = token.error {
        let desc = token.error_description.unwrap_or_default();
        return Err(format!("{provider_name} OAuth 授权失败：{error} {desc}")
            .trim()
            .to_string());
    }

    let access_token = token
        .access_token
        .ok_or_else(|| format!("{provider_name} OAuth 没有返回 access token"))?;
    let expires_in = token.expires_in.unwrap_or(3600).max(60);
    Ok(OAuthTokenSet {
        access_token,
        refresh_token,
        expires_at: crate::db::unix_ts_now() + expires_in,
    })
}

fn fetch_userinfo(access_token: &str) -> Result<(String, String), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let user = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .map_err(|e| e.to_string())?
        .json::<UserInfoResponse>()
        .map_err(|e| e.to_string())?;
    let email = user
        .email
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "Google OAuth 没有返回邮箱地址".to_string())?;
    let name = user
        .name
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| email.clone());
    Ok((email, name))
}

fn parse_microsoft_identity(id_token: Option<&str>) -> Result<(String, String), String> {
    let token = id_token.ok_or_else(|| "Outlook OAuth 没有返回用户身份信息，请重新登录。".to_string())?;
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| "Outlook OAuth 返回的身份信息格式无效。".to_string())?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| format!("Outlook OAuth 身份信息解析失败：{e}"))?;
    let claims: MicrosoftIdClaims = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Outlook OAuth 身份信息解析失败：{e}"))?;
    let email = claims
        .email
        .or(claims.preferred_username)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Outlook OAuth 没有返回邮箱地址。".to_string())?;
    let name = claims
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| email.clone());
    Ok((email, name))
}

fn token_url_for(provider: &str) -> Result<&'static str, String> {
    match provider {
        "gmail" => Ok(TOKEN_URL),
        "outlook" => Ok(OUTLOOK_TOKEN_URL),
        _ => Err(format!("暂不支持刷新 {provider} OAuth token")),
    }
}

fn oauth_provider_name(provider: &str) -> Result<&'static str, String> {
    match provider {
        "gmail" => Ok("Google"),
        "outlook" => Ok("Outlook"),
        _ => Err(format!("暂不支持 {provider} OAuth")),
    }
}

fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    provider_name: &str,
) -> Result<String, String> {
    listener.set_nonblocking(false).map_err(|e| e.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let mut buffer = [0u8; 8192];
    let len = stream.read(&mut buffer).map_err(|e| e.to_string())?;
    let request = String::from_utf8_lossy(&buffer[..len]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "无法读取 OAuth 回调请求".to_string())?;
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let params = parse_query(query);

    let result = if params.get("state").map(String::as_str) != Some(expected_state) {
        Err("OAuth state 校验失败，请重新登录".to_string())
    } else if let Some(error) = params.get("error") {
        Err(format!("{provider_name} 授权被拒绝：{error}"))
    } else {
        params
            .get("code")
            .cloned()
            .ok_or_else(|| "OAuth 回调缺少授权 code".to_string())
    };

    let body = match &result {
        Ok(_) => match provider_name {
            "Outlook" => "<html><body><h2>Outlook 授权完成</h2><p>可以回到 Wox Mail 了。</p></body></html>",
            _ => "<html><body><h2>Gmail 授权完成</h2><p>可以回到 Wox Mail 了。</p></body></html>",
        },
        Err(_) => match provider_name {
            "Outlook" => "<html><body><h2>Outlook 授权失败</h2><p>请回到 Wox Mail 重试。</p></body></html>",
            _ => "<html><body><h2>Gmail 授权失败</h2><p>请回到 Wox Mail 重试。</p></body></html>",
        },
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    result
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((
                urlencoding::decode(key).ok()?.to_string(),
                urlencoding::decode(value).ok()?.to_string(),
            ))
        })
        .collect()
}

fn pkce_random() -> String {
    UuidParts::new().join("")
}

struct UuidParts;

impl UuidParts {
    fn new() -> [String; 3] {
        [
            uuid::Uuid::new_v4().simple().to_string(),
            uuid::Uuid::new_v4().simple().to_string(),
            uuid::Uuid::new_v4().simple().to_string(),
        ]
    }
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn enc(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}
