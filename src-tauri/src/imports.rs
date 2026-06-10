use mailparse::MailHeaderMap;
use rusqlite::params;
use std::fs;
use uuid::Uuid;

use crate::db::unix_ts_now;

pub fn import_mbox(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    file_path: &str,
) -> Result<usize, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("读取文件失败: {e}"))?;

    let messages = split_mbox(&content);
    import_messages(db, account_id, folder_path, &messages)
}

pub fn import_eml(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    file_path: &str,
) -> Result<usize, String> {
    let content = fs::read(file_path)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    let raw = String::from_utf8_lossy(&content).to_string();

    if let Ok(count) = try_import_single(db, account_id, folder_path, &raw) {
        return Ok(count);
    }

    // Try splitting as mbox as fallback
    let messages = split_mbox(&raw);
    import_messages(db, account_id, folder_path, &messages)
}

fn split_mbox(content: &str) -> Vec<String> {
    let mut messages = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        if line.starts_with("From ") && !line.starts_with("From:") && !current.is_empty() {
            messages.push(std::mem::take(&mut current));
        }
        if !current.is_empty() || !line.trim().is_empty() {
            current.push_str(line);
            current.push('\n');
        }
    }

    if !current.trim().is_empty() {
        messages.push(current);
    }

    messages
}

fn try_import_single(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    raw: &str,
) -> Result<usize, String> {
    let parsed = mailparse::parse_mail(raw.as_bytes())
        .map_err(|_| "无法解析邮件格式".to_string())?;
    let (subject, from_name, from_email, to_emails, date_ts, body) = extract_fields(&parsed);
    save_message(db, account_id, folder_path, &subject, &from_name, &from_email, &to_emails, date_ts, &body)?;
    Ok(1)
}

fn import_messages(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    messages: &[String],
) -> Result<usize, String> {
    let mut count = 0usize;
    for raw in messages {
        if raw.trim().is_empty() {
            continue;
        }
        match try_import_single(db, account_id, folder_path, raw) {
            Ok(_) => count += 1,
            Err(_) => { /* skip unparseable messages */ }
        }
    }
    Ok(count)
}

fn extract_fields(parsed: &mailparse::ParsedMail) -> (String, String, String, Vec<String>, i64, String) {
    let subject = parsed.headers
        .get_first_value("Subject")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "(无主题)".to_string());

    let from_raw = parsed.headers
        .get_first_value("From")
        .unwrap_or_else(|| String::new());

    let (from_name, from_email) = if let (Some(l), Some(r)) = (from_raw.rfind('<'), from_raw.rfind('>')) {
        if l < r {
            let email = from_raw[l + 1..r].trim().to_string();
            let name = from_raw[..l].trim().trim_matches('"').to_string();
            (if name.is_empty() { email.clone() } else { name }, email)
        } else {
            (from_raw.clone(), from_raw)
        }
    } else {
        (from_raw.clone(), from_raw)
    };

    let to_raw = parsed.headers
        .get_first_value("To")
        .unwrap_or_default();
    let to_emails: Vec<String> = to_raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let (Some(l), Some(r)) = (s.rfind('<'), s.rfind('>')) {
                if l < r { s[l + 1..r].trim().to_string() } else { s.to_string() }
            } else {
                s.to_string()
            }
        })
        .collect();

    let date_ts = parsed.headers
        .get_first_value("Date")
        .and_then(|d| parse_email_date(d.trim()))
        .unwrap_or_else(|| unix_ts_now());

    let body = extract_text_body(parsed);

    (subject, from_name, from_email, to_emails, date_ts, body)
}

fn extract_text_body(parsed: &mailparse::ParsedMail) -> String {
    if parsed.subparts.is_empty() {
        let body = parsed.get_body().unwrap_or_default();
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            crate::text::html_to_text(&body)
        } else {
            body
        }
    } else {
        for p in &parsed.subparts {
            if p.ctype.mimetype.eq_ignore_ascii_case("text/plain") {
                return p.get_body().unwrap_or_default();
            }
        }
        for p in &parsed.subparts {
            if p.ctype.mimetype.eq_ignore_ascii_case("text/html") {
                return crate::text::html_to_text(&p.get_body().unwrap_or_default());
            }
        }
        parsed.subparts.first().map(|p| extract_text_body(p)).unwrap_or_default()
    }
}

fn parse_email_date(date_str: &str) -> Option<i64> {
    // Try parsing common email date formats
    // e.g. "Thu, 01 Jan 2024 12:00:00 +0000"
    let s = date_str.trim();
    // Strip day-of-week prefix if present
    let s = if s.contains(',') { s.split(',').nth(1)?.trim() } else { s };
    // Split into parts: "01 Jan 2024 12:00:00 +0000"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 4 { return None; }

    let months: std::collections::HashMap<&str, u32> = [
        ("Jan", 1), ("Feb", 2), ("Mar", 3), ("Apr", 4), ("May", 5), ("Jun", 6),
        ("Jul", 7), ("Aug", 8), ("Sep", 9), ("Oct", 10), ("Nov", 11), ("Dec", 12),
    ].iter().cloned().collect();

    let day: i64 = parts[0].parse().ok()?;
    let month: u32 = *months.get(parts.get(1)?)?;
    let year: i32 = parts.get(2)?.parse().ok()?;
    let time_parts: Vec<&str> = parts.get(3)?.split(':').collect();
    if time_parts.len() < 3 { return None; }
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = time_parts[2].parse().ok()?;

    // Use a simple epoch-based calculation
    // Days from Unix epoch for each year-month-day
    let days_before_month: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let mut days = (year as i64 - 1970) * 365;
    days += ((year as i64 - 1969) / 4) - ((year as i64 - 1901) / 100) + ((year as i64 - 1601) / 400);
    days += days_before_month[month as usize];
    if month > 2 && is_leap { days += 1; }
    days += day as i64 - 1;
    let secs = days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    Some(secs)
}

fn save_message(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    subject: &str,
    from_name: &str,
    from_email: &str,
    to_emails: &[String],
    date_ts: i64,
    body: &str,
) -> Result<(), String> {
    let id = Uuid::new_v4().to_string();
    let now = unix_ts_now();
    let snippet = body.lines().next().unwrap_or("").to_string();
    let to_json = serde_json::to_string(to_emails).unwrap_or_else(|_| "[]".to_string());

    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO messages (
               id, account_id, folder_path, subject, from_name, from_email,
               to_emails, date_ts, snippet, body, body_fetched, is_read, created_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1, ?11)",
            params![id, account_id, folder_path, subject, from_name, from_email, to_json, date_ts, snippet, body, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}
