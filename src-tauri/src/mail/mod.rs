use crate::models::AccountSettings;
use base64::Engine;
use imap_proto::types::{BodyContentCommon, BodyContentSinglePart, BodyParams, BodyStructure};
use lettre::{
    message::{header::ContentType, Mailbox, Message},
    transport::smtp::authentication::{Credentials, Mechanism},
    SmtpTransport, Transport,
};
use mailparse::{DispositionType, MailHeaderMap, ParsedMail};
use native_tls::TlsConnector;
use rusqlite::{params, params_from_iter};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use uuid::Uuid;

const REMOTE_STATE_CHECK_INTERVAL_SECS: i64 = 15 * 60;
const REMOTE_STATE_REFRESH_CHUNK_SIZE: usize = 200;
const BODY_PREVIEW_BYTES: usize = 4096;

struct AttachmentMeta {
    filename: String,
    mime_type: String,
    size_bytes: i64,
    content: Option<Vec<u8>>,
    content_id: Option<String>,
    disposition: String,
}

pub struct AttachmentSelector {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_id: Option<String>,
    pub disposition: String,
    pub attachment_index: Option<i64>,
}

pub struct FetchedAttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub content_id: Option<String>,
    pub disposition: String,
    pub attachment_index: i64,
}

pub struct FetchedMessageBody {
    pub body: String,
    pub body_html: Option<String>,
    pub attachments: Vec<FetchedAttachmentMeta>,
}

#[derive(Debug, Clone, Copy)]
struct FolderSyncState {
    last_uid: i64,
    uid_validity: Option<i64>,
    remote_state_checked_at: i64,
}

pub struct RemoteFolder {
    pub path: String,
    pub name: String,
    pub delimiter: Option<String>,
    pub selectable: bool,
}

pub enum MailAuth {
    Password(String),
    OAuth2(String),
}

impl MailAuth {
    fn secret(&self) -> &str {
        match self {
            MailAuth::Password(value) | MailAuth::OAuth2(value) => value,
        }
    }

    fn is_oauth2(&self) -> bool {
        matches!(self, MailAuth::OAuth2(_))
    }
}

struct OAuth2ImapAuth {
    user: String,
    access_token: String,
}

impl imap::Authenticator for OAuth2ImapAuth {
    type Response = String;

    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

fn header_value(parsed: &ParsedMail, name: &str) -> Option<String> {
    parsed
        .headers
        .get_first_value(name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_from(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let (Some(l), Some(r)) = (raw.rfind('<'), raw.rfind('>')) {
        if l < r {
            let email = raw[l + 1..r].trim().to_string();
            let name = raw[..l].trim().trim_matches('"').to_string();
            return (if name.is_empty() { email.clone() } else { name }, email);
        }
    }
    (raw.to_string(), raw.to_string())
}

fn map_imap_login_error(error: impl ToString) -> String {
    let message = error.to_string();
    let lower = message.to_lowercase();

    if lower.contains("application-specific password required")
        || lower.contains("app password")
        || lower.contains("support.google.com/accounts/answer/185833")
    {
        return "Gmail 要求使用应用专用密码，不能直接填写 Google 账号密码。请先在 Google 账号中开启两步验证，然后生成 App Password，并把生成的 16 位应用专用密码填到这里。帮助：https://support.google.com/accounts/answer/185833".to_string();
    }

    if lower.contains("authenticate failed") || lower.contains("authenticationfailed") {
        return "邮箱服务器拒绝 IMAP 登录。请确认邮箱已开启 IMAP/SMTP，并使用该服务商要求的授权码或应用专用密码。Outlook/Microsoft 账号建议使用“快捷登录（OAuth 登录）”。".to_string();
    }

    message
}

fn decode_imap_modified_utf7(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after_amp = &rest[start + 1..];

        if after_amp.starts_with('-') {
            out.push('&');
            rest = &after_amp[1..];
            continue;
        }

        let Some(end) = after_amp.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };

        let encoded = &after_amp[..end];
        let mut standard = encoded.replace(',', "/");
        while standard.len() % 4 != 0 {
            standard.push('=');
        }

        match base64::engine::general_purpose::STANDARD.decode(standard) {
            Ok(bytes) => {
                let units = bytes
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>();
                match String::from_utf16(&units) {
                    Ok(decoded) => out.push_str(&decoded),
                    Err(_) => out.push_str(&rest[start..start + end + 2]),
                }
            }
            Err(_) => out.push_str(&rest[start..start + end + 2]),
        }

        rest = &after_amp[end + 1..];
    }

    out.push_str(rest);
    out
}

fn extract_text_body(parsed: &ParsedMail) -> String {
    if parsed.subparts.is_empty() {
        let body = parsed.get_body().unwrap_or_default();
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            return crate::text::html_to_text(&body);
        }
        return body;
    }
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
    parsed
        .subparts
        .first()
        .map(extract_text_body)
        .unwrap_or_default()
}

fn extract_html_body(parsed: &ParsedMail) -> Option<String> {
    if parsed.subparts.is_empty() {
        if parsed.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            let body = parsed.get_body().unwrap_or_default();
            if !body.trim().is_empty() {
                return Some(body);
            }
        }
        return None;
    }
    for p in &parsed.subparts {
        if p.ctype.mimetype.eq_ignore_ascii_case("text/html") {
            let body = p.get_body().unwrap_or_default();
            if !body.trim().is_empty() {
                return Some(body);
            }
        }
    }
    // Try nested multipart/alternative
    for p in &parsed.subparts {
        if p.ctype.mimetype.eq_ignore_ascii_case("multipart/alternative") {
            if let Some(html) = extract_html_body(p) {
                return Some(html);
            }
        }
    }
    None
}

fn decode_body_preview(header: &[u8], text: Option<&[u8]>) -> String {
    let Some(text) = text.filter(|value| !value.is_empty()) else {
        return String::new();
    };

    let mut combined = Vec::with_capacity(header.len() + text.len() + 4);
    combined.extend_from_slice(header);
    combined.extend_from_slice(b"\r\n");
    combined.extend_from_slice(text);

    if let Ok(parsed) = mailparse::parse_mail(&combined) {
        let body = extract_text_body(&parsed);
        if !body.trim().is_empty() {
            return truncate_preview_text(&body);
        }
    }

    let fallback = String::from_utf8_lossy(text).to_string();
    truncate_preview_text(&crate::text::html_to_text(&fallback))
}

fn truncate_preview_text(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 2048;
    value
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn extract_attachments(parsed: &ParsedMail, include_content: bool) -> Vec<AttachmentMeta> {
    let mut attachments = Vec::new();
    collect_attachments(parsed, &mut attachments, include_content);
    attachments
}

fn extract_bodystructure_attachments(bodystructure: &BodyStructure<'_>) -> Vec<AttachmentMeta> {
    let mut attachments = Vec::new();
    collect_bodystructure_attachments(bodystructure, &mut attachments);
    attachments
}

fn collect_bodystructure_attachments(
    bodystructure: &BodyStructure<'_>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    match bodystructure {
        BodyStructure::Basic { common, other, .. } | BodyStructure::Text { common, other, .. } => {
            push_bodystructure_attachment(common, other, attachments);
        }
        BodyStructure::Message {
            common,
            other,
            body,
            ..
        } => {
            let before = attachments.len();
            push_bodystructure_attachment(common, other, attachments);
            if attachments.len() == before {
                collect_bodystructure_attachments(body, attachments);
            }
        }
        BodyStructure::Multipart { bodies, .. } => {
            for body in bodies {
                collect_bodystructure_attachments(body, attachments);
            }
        }
    }
}

fn push_bodystructure_attachment(
    common: &BodyContentCommon<'_>,
    other: &BodyContentSinglePart<'_>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    let disposition = common
        .disposition
        .as_ref()
        .map(|value| value.ty.trim().to_lowercase())
        .unwrap_or_else(|| "inline".to_string());
    let filename = common
        .disposition
        .as_ref()
        .and_then(|value| param_value(&value.params, "filename"))
        .or_else(|| param_value(&common.ty.params, "name"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let is_attachment = disposition.eq_ignore_ascii_case("attachment");
    if !is_attachment && filename.is_none() {
        return;
    }

    attachments.push(AttachmentMeta {
        filename: filename.unwrap_or_else(|| "attachment".to_string()),
        mime_type: format!(
            "{}/{}",
            common.ty.ty.to_ascii_lowercase(),
            common.ty.subtype.to_ascii_lowercase()
        ),
        size_bytes: other.octets as i64,
        content: None,
        content_id: other.id.map(clean_content_id),
        disposition,
    });
}

fn param_value<'a>(params: &BodyParams<'a>, key: &str) -> Option<&'a str> {
    params.as_ref()?.iter().find_map(|(param_key, value)| {
        if param_key.eq_ignore_ascii_case(key) {
            Some(*value)
        } else {
            None
        }
    })
}

fn clean_content_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

fn collect_attachments(
    parsed: &ParsedMail,
    attachments: &mut Vec<AttachmentMeta>,
    include_content: bool,
) {
    for part in &parsed.subparts {
        collect_attachments(part, attachments, include_content);
    }

    if !parsed.subparts.is_empty() {
        return;
    }

    let disposition = parsed.get_content_disposition();
    let filename = disposition
        .params
        .get("filename")
        .or_else(|| parsed.ctype.params.get("name"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let is_attachment = matches!(disposition.disposition, DispositionType::Attachment);
    if !is_attachment && filename.is_none() {
        return;
    }

    let content = if include_content {
        Some(
            parsed
                .get_body_raw()
                .unwrap_or_else(|_| parsed.raw_bytes.to_vec()),
        )
    } else {
        None
    };
    let size_bytes = content
        .as_ref()
        .map(|content| content.len() as i64)
        .unwrap_or(parsed.raw_bytes.len() as i64);
    let disposition_name = match disposition.disposition {
        DispositionType::Inline => "inline".to_string(),
        DispositionType::Attachment => "attachment".to_string(),
        DispositionType::FormData => "form-data".to_string(),
        DispositionType::Extension(value) => value,
    };

    attachments.push(AttachmentMeta {
        filename: filename.unwrap_or_else(|| "attachment".to_string()),
        mime_type: parsed.ctype.mimetype.clone(),
        size_bytes,
        content,
        content_id: header_value(parsed, "Content-ID").map(|v| {
            v.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        }),
        disposition: disposition_name,
    });
}

pub fn sync_imap_folder(
    db: &crate::db::Db,
    account_id: &str,
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    limit: usize,
    force_remote_state_check: bool,
) -> Result<usize, String> {
    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let session = authenticate_imap_client(client, settings, auth)?;
        return sync_imap_session(
            db,
            account_id,
            session,
            folder_path,
            limit,
            force_remote_state_check,
        );
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let session = authenticate_imap_client(client, settings, auth)?;

    sync_imap_session(
        db,
        account_id,
        session,
        folder_path,
        limit,
        force_remote_state_check,
    )
}

pub fn list_imap_folders(
    settings: &AccountSettings,
    auth: &MailAuth,
) -> Result<Vec<RemoteFolder>, String> {
    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        let folders = list_session_folders(&mut session)?;
        let _ = session.logout();
        return Ok(folders);
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    let folders = list_session_folders(&mut session)?;
    let _ = session.logout();
    Ok(folders)
}

pub fn fetch_imap_attachment_content(
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    imap_uid: i64,
    selector: &AttachmentSelector,
) -> Result<Vec<u8>, String> {
    if imap_uid <= 0 {
        return Err("该附件缺少远端邮件 UID，无法按需下载。请重新同步这封邮件。".to_string());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        let content =
            fetch_session_attachment_content(&mut session, folder_path, imap_uid, selector)?;
        let _ = session.logout();
        return Ok(content);
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    let content = fetch_session_attachment_content(&mut session, folder_path, imap_uid, selector)?;
    let _ = session.logout();
    Ok(content)
}

pub fn fetch_imap_message_body(
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    imap_uid: i64,
) -> Result<FetchedMessageBody, String> {
    if imap_uid <= 0 {
        return Err("该邮件缺少远端 UID，无法按需下载正文。请重新同步这封邮件。".to_string());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        let body = fetch_session_message_body(&mut session, folder_path, imap_uid)?;
        let _ = session.logout();
        return Ok(body);
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    let body = fetch_session_message_body(&mut session, folder_path, imap_uid)?;
    let _ = session.logout();
    Ok(body)
}

fn fetch_session_attachment_content<T: Read + Write>(
    session: &mut imap::Session<T>,
    folder_path: &str,
    imap_uid: i64,
    selector: &AttachmentSelector,
) -> Result<Vec<u8>, String> {
    session.select(folder_path).map_err(|e| e.to_string())?;
    let fetches = session
        .uid_fetch(imap_uid.to_string(), "(BODY.PEEK[] UID)")
        .map_err(|e| e.to_string())?;
    let fetch = fetches
        .iter()
        .next()
        .ok_or_else(|| "服务器上找不到这封邮件，附件无法下载。".to_string())?;
    let body = fetch
        .body()
        .ok_or_else(|| "服务器没有返回邮件正文，附件无法下载。".to_string())?;
    let parsed = mailparse::parse_mail(body).map_err(|e| e.to_string())?;
    let attachments = extract_attachments(&parsed, true);

    attachments
        .into_iter()
        .enumerate()
        .find_map(|(index, attachment)| {
            if attachment_matches_selector(index, &attachment, selector) {
                attachment.content
            } else {
                None
            }
        })
        .ok_or_else(|| "服务器返回的邮件中没有匹配的附件。请刷新或重新同步这封邮件。".to_string())
}

fn fetch_session_message_body<T: Read + Write>(
    session: &mut imap::Session<T>,
    folder_path: &str,
    imap_uid: i64,
) -> Result<FetchedMessageBody, String> {
    session.select(folder_path).map_err(|e| e.to_string())?;
    let fetches = session
        .uid_fetch(imap_uid.to_string(), "(BODY.PEEK[] UID)")
        .map_err(|e| e.to_string())?;
    let fetch = fetches
        .iter()
        .next()
        .ok_or_else(|| "服务器上找不到这封邮件，正文无法下载。".to_string())?;
    let body = fetch
        .body()
        .ok_or_else(|| "服务器没有返回邮件正文。".to_string())?;
    let parsed = mailparse::parse_mail(body).map_err(|e| e.to_string())?;
    let text_body = extract_text_body(&parsed);
    let html_body = extract_html_body(&parsed);
    let attachments = extract_attachments(&parsed, false)
        .into_iter()
        .enumerate()
        .map(|(index, attachment)| FetchedAttachmentMeta {
            filename: attachment.filename,
            mime_type: attachment.mime_type,
            size_bytes: attachment.size_bytes,
            content_id: attachment.content_id,
            disposition: attachment.disposition,
            attachment_index: index as i64,
        })
        .collect();

    Ok(FetchedMessageBody {
        body: text_body,
        body_html: html_body,
        attachments,
    })
}

fn attachment_matches_selector(
    index: usize,
    attachment: &AttachmentMeta,
    selector: &AttachmentSelector,
) -> bool {
    if let Some(expected_index) = selector.attachment_index {
        return expected_index >= 0 && expected_index as usize == index;
    }

    attachment.filename == selector.filename
        && attachment
            .mime_type
            .eq_ignore_ascii_case(&selector.mime_type)
        && attachment.content_id == selector.content_id
        && attachment
            .disposition
            .eq_ignore_ascii_case(&selector.disposition)
        && (selector.size_bytes <= 0 || attachment.size_bytes == selector.size_bytes)
}

fn authenticate_imap_client<T: Read + Write>(
    client: imap::Client<T>,
    settings: &AccountSettings,
    auth: &MailAuth,
) -> Result<imap::Session<T>, String> {
    match auth {
        MailAuth::Password(password) => client
            .login(settings.imap_username.as_str(), password)
            .map_err(|(e, _)| map_imap_login_error(e)),
        MailAuth::OAuth2(access_token) => {
            let oauth = OAuth2ImapAuth {
                user: settings.imap_username.clone(),
                access_token: access_token.clone(),
            };
            client
                .authenticate("XOAUTH2", &oauth)
                .map_err(|(e, _)| map_imap_login_error(e))
        }
    }
}

fn list_session_folders<T: Read + Write>(
    session: &mut imap::Session<T>,
) -> Result<Vec<RemoteFolder>, String> {
    let names = session.list(None, Some("*")).map_err(|e| e.to_string())?;
    let mut out = Vec::new();

    for item in names.iter() {
        let path = item.name().to_string();
        let delimiter = item.delimiter().map(|v| v.to_string());
        let raw_name = delimiter
            .as_deref()
            .and_then(|d| path.rsplit(d).next())
            .filter(|v| !v.is_empty())
            .unwrap_or(path.as_str())
            .to_string();
        let name = decode_imap_modified_utf7(&raw_name);
        let selectable = !item
            .attributes()
            .iter()
            .any(|attr| matches!(attr, imap::types::NameAttribute::NoSelect));

        out.push(RemoteFolder {
            path,
            name,
            delimiter,
            selectable,
        });
    }

    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(out)
}

fn sync_imap_session<T: Read + Write>(
    db: &crate::db::Db,
    account_id: &str,
    mut session: imap::Session<T>,
    folder_path: &str,
    limit: usize,
    force_remote_state_check: bool,
) -> Result<usize, String> {
    let mailbox = session.select(folder_path).map_err(|e| e.to_string())?;
    let selected_uid_validity = mailbox.uid_validity.map(i64::from);
    let mut sync_state = read_folder_sync_state(db, account_id, folder_path)?;
    let mut last_uid = sync_state.last_uid;

    if let (Some(saved), Some(current)) = (sync_state.uid_validity, selected_uid_validity) {
        if saved != current {
            reset_local_folder_for_uid_validity_change(db, account_id, folder_path)?;
            sync_state = FolderSyncState {
                last_uid: 0,
                uid_validity: selected_uid_validity,
                remote_state_checked_at: 0,
            };
            last_uid = 0;
        }
    }

    let query = if last_uid > 0 {
        format!("UID {}:*", last_uid + 1)
    } else {
        "ALL".to_string()
    };
    let uids = session.uid_search(query).map_err(|e| e.to_string())?;
    let mut uid_vec: Vec<u32> = uids.into_iter().collect();
    uid_vec.sort();

    maybe_refresh_cached_remote_state(
        db,
        &mut session,
        account_id,
        folder_path,
        &sync_state,
        selected_uid_validity,
        mailbox.exists as i64,
        force_remote_state_check,
    )?;

    let start = if last_uid > 0 {
        0
    } else {
        uid_vec.len().saturating_sub(limit)
    };
    let mut targets: Vec<u32> = uid_vec[start..].to_vec();

    if targets.is_empty() {
        update_folder_sync_state(
            db,
            account_id,
            folder_path,
            selected_uid_validity,
            uid_vec
                .iter()
                .max()
                .map(|uid| *uid as i64)
                .unwrap_or(last_uid),
            last_uid,
            None,
        )?;
        let _ = session.logout();
        return Ok(0);
    }

    let existing_uids = existing_imap_uids(db, account_id, folder_path, &targets)?;
    targets.retain(|uid| !existing_uids.contains(&(*uid as i64)));

    if targets.is_empty() {
        if let Some(max_uid) = uid_vec.iter().max() {
            update_folder_sync_state(
                db,
                account_id,
                folder_path,
                selected_uid_validity,
                (*max_uid) as i64,
                last_uid,
                None,
            )?;
        }
        let _ = session.logout();
        return Ok(0);
    }

    let mut inserted = 0usize;
    let mut max_seen_uid = last_uid;

    for chunk in targets.chunks(25).rev() {
        let fetch_set = chunk
            .iter()
            .rev()
            .map(|uid| uid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetch_items = format!(
            "(BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.{}> BODYSTRUCTURE FLAGS UID RFC822.SIZE)",
            BODY_PREVIEW_BYTES
        );
        let fetches = session
            .uid_fetch(fetch_set, fetch_items)
            .map_err(|e| e.to_string())?;

        for f in fetches.iter() {
            let imap_uid = f.uid.unwrap_or(0);
            if imap_uid == 0 {
                continue;
            }

            let header_bytes = match f.header() {
                Some(v) => v,
                None => continue,
            };

            let parsed = mailparse::parse_mail(header_bytes).map_err(|e| e.to_string())?;
            let subject =
                header_value(&parsed, "Subject").unwrap_or_else(|| "(no subject)".to_string());
            let from_raw = header_value(&parsed, "From").unwrap_or_else(|| "unknown".to_string());
            let (from_name, from_email) = parse_from(&from_raw);
            let date_ts = header_value(&parsed, "Date")
                .and_then(|v| mailparse::dateparse(&v).ok())
                .map(|t| t as i64)
                .unwrap_or_else(crate::db::unix_ts_now);
            let body = decode_body_preview(header_bytes, f.text());
            let attachments = f
                .bodystructure()
                .map(extract_bodystructure_attachments)
                .unwrap_or_default();
            let snippet = body.lines().next().unwrap_or("").to_string();
            let is_read = flags_include_seen(f.flags());
            let to_emails = header_value(&parsed, "To")
                .map(|v| serde_json::to_string(&vec![v]).unwrap_or_else(|_| "[]".to_string()))
                .unwrap_or_else(|| "[]".to_string());
            let source_id = header_value(&parsed, "Message-Id")
                .or_else(|| header_value(&parsed, "Message-ID"))
                .map(|v| format!("imap:msgid:{v}"))
                .unwrap_or_else(|| format!("imap:uid:{imap_uid}"));

            let now = crate::db::unix_ts_now();
            let msg_id = Uuid::new_v4().to_string();

            let did_insert = db.with_conn_mut(|conn| {
                let tx = conn.transaction().map_err(|e| e.to_string())?;
                let changed = tx
                    .execute(
                        "INSERT INTO messages (
                            id, account_id, folder_path, subject, from_name,
                            from_email, to_emails, date_ts, snippet, body,
                            body_fetched, is_read, created_at, source_id, imap_uid
                         )
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12, ?13, ?14)
                     ON CONFLICT(account_id, folder_path, imap_uid) DO NOTHING",
                        params![
                            msg_id,
                            account_id,
                            folder_path,
                            subject,
                            from_name,
                            from_email,
                            to_emails,
                            date_ts,
                            snippet,
                            body,
                            if is_read { 1i64 } else { 0i64 },
                            now,
                            source_id,
                            imap_uid as i64
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                if changed > 0 {
                    insert_attachment_metadata(&tx, &msg_id, &attachments, now)?;
                }

                tx.commit().map_err(|e| e.to_string())?;
                Ok(changed > 0)
            })?;

            if did_insert {
                inserted += 1;
            }
            max_seen_uid = max_seen_uid.max(imap_uid as i64);
        }
    }

    update_folder_sync_state(
        db,
        account_id,
        folder_path,
        selected_uid_validity,
        max_seen_uid,
        last_uid,
        None,
    )?;

    let _ = session.logout();
    Ok(inserted)
}

fn existing_imap_uids(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    uids: &[u32],
) -> Result<HashSet<i64>, String> {
    if uids.is_empty() {
        return Ok(HashSet::new());
    }

    db.with_conn(|conn| {
        let placeholders = std::iter::repeat("?")
            .take(uids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT imap_uid FROM messages WHERE account_id = ? AND folder_path = ? AND imap_uid IN ({placeholders})"
        );
        let params = std::iter::once(account_id.to_string())
            .chain(std::iter::once(folder_path.to_string()))
            .chain(uids.iter().map(|uid| (*uid as i64).to_string()));
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        let mut out = HashSet::new();
        for row in rows {
            out.insert(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

fn insert_attachment_metadata(
    tx: &rusqlite::Transaction<'_>,
    message_id: &str,
    attachments: &[AttachmentMeta],
    now: i64,
) -> Result<(), String> {
    for (attachment_index, attachment) in attachments.iter().enumerate() {
        let content: Option<&[u8]> = attachment.content.as_deref();
        tx.execute(
            "INSERT INTO attachments (
               id, message_id, filename, mime_type, size_bytes,
               content, content_id, disposition, attachment_index, created_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                Uuid::new_v4().to_string(),
                message_id,
                attachment.filename.as_str(),
                attachment.mime_type.as_str(),
                attachment.size_bytes,
                content,
                attachment.content_id.as_deref(),
                attachment.disposition.as_str(),
                attachment_index as i64,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn read_folder_sync_state(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
) -> Result<FolderSyncState, String> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT last_uid, uid_validity, remote_state_checked_at
             FROM folder_sync_state
             WHERE account_id = ?1 AND folder_path = ?2",
            params![account_id, folder_path],
            |row| {
                Ok(FolderSyncState {
                    last_uid: row.get(0)?,
                    uid_validity: row.get(1)?,
                    remote_state_checked_at: row.get(2)?,
                })
            },
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(FolderSyncState {
                last_uid: 0,
                uid_validity: None,
                remote_state_checked_at: 0,
            }),
            _ => Err(e),
        })
        .map_err(|e| e.to_string())
    })
}

fn maybe_refresh_cached_remote_state<T: Read + Write>(
    db: &crate::db::Db,
    session: &mut imap::Session<T>,
    account_id: &str,
    folder_path: &str,
    sync_state: &FolderSyncState,
    uid_validity: Option<i64>,
    remote_exists: i64,
    force: bool,
) -> Result<(), String> {
    let now = crate::db::unix_ts_now();
    let local_count = local_imap_message_count(db, account_id, folder_path)?;
    let check_is_stale =
        now.saturating_sub(sync_state.remote_state_checked_at) >= REMOTE_STATE_CHECK_INTERVAL_SECS;

    if !force && !check_is_stale && local_count <= remote_exists {
        return Ok(());
    }

    let local_uids = local_imap_uids(db, account_id, folder_path)?;
    if !local_uids.is_empty() {
        refresh_cached_flags_and_deletions(session, db, account_id, folder_path, &local_uids)?;
    }

    update_folder_sync_state(
        db,
        account_id,
        folder_path,
        uid_validity,
        sync_state.last_uid,
        sync_state.last_uid,
        Some(now),
    )
}

fn local_imap_message_count(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
) -> Result<i64, String> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT COUNT(*)
             FROM messages
             WHERE account_id = ?1 AND folder_path = ?2 AND imap_uid IS NOT NULL",
            params![account_id, folder_path],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())
    })
}

fn local_imap_uids(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
) -> Result<Vec<i64>, String> {
    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT imap_uid
                 FROM messages
                 WHERE account_id = ?1 AND folder_path = ?2 AND imap_uid IS NOT NULL
                 ORDER BY imap_uid ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![account_id, folder_path], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

fn refresh_cached_flags_and_deletions<T: Read + Write>(
    session: &mut imap::Session<T>,
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    local_uids: &[i64],
) -> Result<(), String> {
    let mut remote_flags = HashMap::<i64, bool>::new();

    for chunk in local_uids.chunks(REMOTE_STATE_REFRESH_CHUNK_SIZE) {
        let uid_set = imap_uid_set(chunk);
        if uid_set.is_empty() {
            continue;
        }

        let fetches = session
            .uid_fetch(uid_set, "(FLAGS UID)")
            .map_err(|e| e.to_string())?;
        for fetch in fetches.iter() {
            let Some(uid) = fetch.uid else {
                continue;
            };
            remote_flags.insert(uid as i64, flags_include_seen(fetch.flags()));
        }
    }

    let deleted_uids = local_uids
        .iter()
        .copied()
        .filter(|uid| !remote_flags.contains_key(uid))
        .collect::<Vec<_>>();

    update_cached_imap_flags(db, account_id, folder_path, &remote_flags)?;
    delete_local_messages_by_uids(db, account_id, folder_path, &deleted_uids)
}

fn update_cached_imap_flags(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    remote_flags: &HashMap<i64, bool>,
) -> Result<(), String> {
    if remote_flags.is_empty() {
        return Ok(());
    }

    db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (uid, seen) in remote_flags {
            let is_read = if *seen { 1i64 } else { 0i64 };
            tx.execute(
                "UPDATE messages
                 SET is_read = ?1
                 WHERE account_id = ?2
                   AND folder_path = ?3
                   AND imap_uid = ?4
                   AND is_read <> ?1",
                params![is_read, account_id, folder_path, uid],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn reset_local_folder_for_uid_validity_change(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
) -> Result<(), String> {
    db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        delete_local_folder_messages_tx(&tx, account_id, folder_path)?;
        tx.execute(
            "DELETE FROM folder_sync_state WHERE account_id = ?1 AND folder_path = ?2",
            params![account_id, folder_path],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn delete_local_messages_by_uids(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    uids: &[i64],
) -> Result<(), String> {
    if uids.is_empty() {
        return Ok(());
    }

    db.with_conn_mut(|conn| {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for chunk in uids.chunks(REMOTE_STATE_REFRESH_CHUNK_SIZE) {
            delete_local_message_uid_chunk_tx(&tx, account_id, folder_path, chunk)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

fn delete_local_folder_messages_tx(
    tx: &rusqlite::Transaction<'_>,
    account_id: &str,
    folder_path: &str,
) -> Result<(), String> {
    tx.execute(
        "DELETE FROM message_tags
         WHERE message_id IN (
           SELECT id FROM messages WHERE account_id = ?1 AND folder_path = ?2
         )",
        params![account_id, folder_path],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM attachments
         WHERE message_id IN (
           SELECT id FROM messages WHERE account_id = ?1 AND folder_path = ?2
         )",
        params![account_id, folder_path],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM messages WHERE account_id = ?1 AND folder_path = ?2",
        params![account_id, folder_path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn delete_local_message_uid_chunk_tx(
    tx: &rusqlite::Transaction<'_>,
    account_id: &str,
    folder_path: &str,
    uids: &[i64],
) -> Result<(), String> {
    if uids.is_empty() {
        return Ok(());
    }

    let placeholders = std::iter::repeat("?")
        .take(uids.len())
        .collect::<Vec<_>>()
        .join(",");

    let params = std::iter::once(account_id.to_string())
        .chain(std::iter::once(folder_path.to_string()))
        .chain(uids.iter().map(ToString::to_string));

    let sql = format!(
        "DELETE FROM message_tags
         WHERE message_id IN (
           SELECT id FROM messages
           WHERE account_id = ? AND folder_path = ? AND imap_uid IN ({placeholders})
         )"
    );
    tx.execute(&sql, params_from_iter(params))
        .map_err(|e| e.to_string())?;

    let params = std::iter::once(account_id.to_string())
        .chain(std::iter::once(folder_path.to_string()))
        .chain(uids.iter().map(ToString::to_string));
    let sql = format!(
        "DELETE FROM attachments
         WHERE message_id IN (
           SELECT id FROM messages
           WHERE account_id = ? AND folder_path = ? AND imap_uid IN ({placeholders})
         )"
    );
    tx.execute(&sql, params_from_iter(params))
        .map_err(|e| e.to_string())?;

    let params = std::iter::once(account_id.to_string())
        .chain(std::iter::once(folder_path.to_string()))
        .chain(uids.iter().map(ToString::to_string));
    let sql = format!(
        "DELETE FROM messages
         WHERE account_id = ? AND folder_path = ? AND imap_uid IN ({placeholders})"
    );
    tx.execute(&sql, params_from_iter(params))
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn flags_include_seen(flags: &[imap::types::Flag<'_>]) -> bool {
    flags
        .iter()
        .any(|flag| format!("{flag}").eq_ignore_ascii_case("\\Seen"))
}

fn update_folder_sync_state(
    db: &crate::db::Db,
    account_id: &str,
    folder_path: &str,
    uid_validity: Option<i64>,
    max_seen_uid: i64,
    last_uid: i64,
    remote_state_checked_at: Option<i64>,
) -> Result<(), String> {
    if max_seen_uid <= last_uid && uid_validity.is_none() && remote_state_checked_at.is_none() {
        return Ok(());
    }

    let next_last_uid = max_seen_uid.max(last_uid);
    let now = crate::db::unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO folder_sync_state (
               account_id, folder_path, last_uid, uid_validity,
               remote_state_checked_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, 0), ?6)
             ON CONFLICT(account_id, folder_path) DO UPDATE SET
               last_uid = MAX(folder_sync_state.last_uid, excluded.last_uid),
               uid_validity = COALESCE(excluded.uid_validity, folder_sync_state.uid_validity),
               remote_state_checked_at = CASE
                 WHEN ?5 IS NULL THEN folder_sync_state.remote_state_checked_at
                 ELSE excluded.remote_state_checked_at
               END,
               updated_at = excluded.updated_at",
            params![
                account_id,
                folder_path,
                next_last_uid,
                uid_validity,
                remote_state_checked_at,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn mark_imap_message_seen(
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    imap_uid: i64,
) -> Result<(), String> {
    if imap_uid <= 0 {
        return Ok(());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        mark_session_message_seen(&mut session, folder_path, imap_uid)?;
        let _ = session.logout();
        return Ok(());
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    mark_session_message_seen(&mut session, folder_path, imap_uid)?;
    let _ = session.logout();
    Ok(())
}

pub fn move_imap_messages(
    settings: &AccountSettings,
    auth: &MailAuth,
    source_folder: &str,
    target_folder: &str,
    imap_uids: &[i64],
) -> Result<(), String> {
    let uid_set = imap_uid_set(imap_uids);
    if uid_set.is_empty() {
        return Ok(());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        move_session_messages(&mut session, source_folder, target_folder, &uid_set)?;
        let _ = session.logout();
        return Ok(());
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    move_session_messages(&mut session, source_folder, target_folder, &uid_set)?;
    let _ = session.logout();
    Ok(())
}

fn mark_session_message_seen<T: Read + Write>(
    session: &mut imap::Session<T>,
    folder_path: &str,
    imap_uid: i64,
) -> Result<(), String> {
    session.select(folder_path).map_err(|e| e.to_string())?;
    session
        .uid_store(imap_uid.to_string(), "+FLAGS.SILENT (\\Seen)")
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn move_session_messages<T: Read + Write>(
    session: &mut imap::Session<T>,
    source_folder: &str,
    target_folder: &str,
    uid_set: &str,
) -> Result<(), String> {
    session.select(source_folder).map_err(|e| e.to_string())?;

    match session.uid_mv(uid_set, target_folder) {
        Ok(_) => Ok(()),
        Err(move_error) => {
            session
                .uid_copy(uid_set, target_folder)
                .map_err(|copy_error| {
                    format!(
                        "服务器移动邮件失败：{move_error}；复制到目标文件夹也失败：{copy_error}"
                    )
                })?;
            session
                .uid_store(uid_set, "+FLAGS.SILENT (\\Deleted)")
                .map_err(|e| e.to_string())?;
            session.uid_expunge(uid_set).map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

fn imap_uid_set(imap_uids: &[i64]) -> String {
    let mut uids = imap_uids
        .iter()
        .copied()
        .filter(|uid| *uid > 0)
        .collect::<Vec<_>>();
    uids.sort_unstable();
    uids.dedup();
    uids.iter()
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn send_smtp(
    settings: &AccountSettings,
    auth: &MailAuth,
    from_email: &str,
    from_name: &str,
    to_emails: &[String],
    subject: &str,
    body: &str,
    is_html: bool,
) -> Result<Vec<u8>, String> {
    let from_mb: Mailbox = format!("{from_name} <{from_email}>")
        .parse::<Mailbox>()
        .map_err(|e| e.to_string())?;

    let mut builder = Message::builder().from(from_mb).subject(subject);
    for to in to_emails {
        let mb: Mailbox = to.parse::<Mailbox>().map_err(|e| e.to_string())?;
        builder = builder.to(mb);
    }

    let content_type = if is_html {
        ContentType::TEXT_HTML
    } else {
        ContentType::TEXT_PLAIN
    };

    let email = builder
        .header(content_type)
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    let raw_message = email.formatted();

    let creds = Credentials::new(settings.smtp_username.clone(), auth.secret().to_string());

    let mut transport_builder = if settings.smtp_port == 465 {
        SmtpTransport::relay(settings.smtp_host.as_str())
            .map_err(|e| e.to_string())?
            .tls(lettre::transport::smtp::client::Tls::Wrapper(
                lettre::transport::smtp::client::TlsParameters::new(settings.smtp_host.clone())
                    .map_err(|e| e.to_string())?,
            ))
    } else if settings.smtp_tls {
        SmtpTransport::starttls_relay(settings.smtp_host.as_str()).map_err(|e| e.to_string())?
    } else {
        SmtpTransport::builder_dangerous(settings.smtp_host.as_str())
    };

    transport_builder = transport_builder
        .port(settings.smtp_port as u16)
        .credentials(creds);
    if auth.is_oauth2() {
        transport_builder = transport_builder.authentication(vec![Mechanism::Xoauth2]);
    }
    let transport = transport_builder.build();
    transport.send(&email).map_err(|e| e.to_string())?;
    Ok(raw_message)
}

pub fn append_imap_message(
    settings: &AccountSettings,
    auth: &MailAuth,
    folder_path: &str,
    raw_message: &[u8],
) -> Result<(), String> {
    if folder_path.trim().is_empty() {
        return Ok(());
    }

    if settings.imap_tls {
        let tls = TlsConnector::builder().build().map_err(|e| e.to_string())?;
        let client = if settings.imap_port == 143 {
            imap::connect_starttls(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        } else {
            imap::connect(
                (settings.imap_host.as_str(), settings.imap_port as u16),
                settings.imap_host.as_str(),
                &tls,
            )
            .map_err(|e| e.to_string())?
        };

        let mut session = authenticate_imap_client(client, settings, auth)?;
        append_session_message(&mut session, folder_path, raw_message)?;
        let _ = session.logout();
        return Ok(());
    }

    let stream = TcpStream::connect((settings.imap_host.as_str(), settings.imap_port as u16))
        .map_err(|e| e.to_string())?;
    let client = imap::Client::new(stream);
    let mut session = authenticate_imap_client(client, settings, auth)?;
    append_session_message(&mut session, folder_path, raw_message)?;
    let _ = session.logout();
    Ok(())
}

fn append_session_message<T: Read + Write>(
    session: &mut imap::Session<T>,
    folder_path: &str,
    raw_message: &[u8],
) -> Result<(), String> {
    session
        .append_with_flags(folder_path, raw_message, &[imap::types::Flag::Seen])
        .map_err(|e| e.to_string())
}
