use rusqlite::params;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct BaiduResponse {
    #[serde(default)]
    trans_result: Vec<BaiduTransResult>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_msg: Option<String>,
}

#[derive(Deserialize)]
struct BaiduTransResult {
    #[allow(dead_code)]
    src: String,
    dst: String,
}

pub fn translate_text(
    db: &crate::db::Db,
    text: &str,
    to_lang: &str,
    appid_override: Option<&str>,
    secret_override: Option<&str>,
) -> Result<String, String> {
    let appid = appid_override
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未配置翻译 API 密钥。请在 设置 → 翻译 中填入百度翻译 APP ID 和密钥。\n获取方式：https://fanyi-api.baidu.com/".to_string())?;
    let secret = secret_override
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未配置翻译密钥".to_string())?;

    let source_hash = hex_hash(text);

    // Check cache first
    let cached_result: Option<String> = db.with_conn(|conn| {
        let row = conn.query_row(
            "SELECT translated_text FROM translation_cache
             WHERE source_hash = ?1 AND target_lang = ?2
             ORDER BY created_at DESC LIMIT 1",
            params![source_hash, to_lang],
            |row| row.get::<_, String>(0),
        );
        match row {
            Ok(text) => Ok(Some(text)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })?;
    if let Some(cached) = cached_result {
        return Ok(cached);
    }

    let salt = rand_salt();
    let sign_input = format!("{appid}{text}{salt}{secret}");
    let sign = md5_hash(&sign_input);

    // Build form body manually for correct encoding
    let body = format!(
        "q={}&from=auto&to={}&appid={}&salt={}&sign={}",
        urlencoding::encode(text),
        urlencoding::encode(to_lang),
        urlencoding::encode(&appid),
        urlencoding::encode(&salt),
        urlencoding::encode(&sign),
    );

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://fanyi-api.baidu.com/api/trans/vip/translate")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .map_err(|e| format!("翻译请求失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("翻译服务 HTTP 错误: {}", response.status()));
    }

    let result: BaiduResponse = response.json().map_err(|e| format!("解析翻译结果失败: {e}"))?;

    if let Some(code) = result.error_code {
        let msg = result.error_msg.unwrap_or_default();
        return Err(format!("百度翻译错误 ({}): {}", code, msg));
    }

    let translated = result
        .trans_result
        .into_iter()
        .map(|t| t.dst)
        .collect::<Vec<_>>()
        .join("\n");

    if translated.is_empty() {
        return Err("百度翻译返回了空结果".to_string());
    }

    // Cache the result
    let now = crate::db::unix_ts_now();
    let _ = db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO translation_cache (source_hash, source_text, source_lang, target_lang, translated_text, created_at)
             VALUES (?1, ?2, 'auto', ?3, ?4, ?5)",
            params![source_hash, text, to_lang, translated, now],
        )
        .map_err(|e| e.to_string())
    });

    Ok(translated)
}

fn hex_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn rand_salt() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}", ts % 100000)
}

fn md5_hash(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}