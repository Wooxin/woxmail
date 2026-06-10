pub fn html_to_text(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut suppress_until: Option<&'static str> = None;

    while let Some(start) = rest.find('<') {
        let text = &rest[..start];
        if suppress_until.is_none() {
            append_decoded_text(&mut out, text);
        }
        rest = &rest[start + 1..];

        let Some(end) = rest.find('>') else {
            out.push('<');
            append_decoded_text(&mut out, rest);
            return normalize_text(&out);
        };

        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let tag_name = tag_name(tag);
        let closing = tag.trim_start().starts_with('/');

        if let Some(target) = suppress_until {
            if closing && tag_name == target {
                suppress_until = None;
            }
            continue;
        }

        match tag_name.as_str() {
            "script" | "style" if !closing => {
                suppress_until = Some(if tag_name == "script" {
                    "script"
                } else {
                    "style"
                })
            }
            "br" => out.push('\n'),
            "p" | "div" | "section" | "article" | "header" | "footer" | "tr" | "table"
            | "blockquote"
                if closing =>
            {
                out.push('\n')
            }
            "li" if !closing => {
                out.push('\n');
                out.push_str("- ");
            }
            _ => {}
        }
    }

    if suppress_until.is_none() {
        append_decoded_text(&mut out, rest);
    }

    normalize_text(&out)
}

fn append_decoded_text(out: &mut String, value: &str) {
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }

        let mut entity = String::new();
        while let Some(next) = chars.peek().copied() {
            if next == ';' {
                chars.next();
                break;
            }
            if entity.len() >= 32 || next.is_whitespace() || next == '&' {
                out.push('&');
                out.push_str(&entity);
                entity.clear();
                break;
            }
            entity.push(next);
            chars.next();
        }

        if entity.is_empty() {
            if chars.peek().is_none() {
                out.push('&');
            }
            continue;
        }

        if let Some(decoded) = decode_entity(&entity) {
            out.push(decoded);
        } else {
            out.push('&');
            out.push_str(&entity);
            out.push(';');
        }
    }
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn tag_name(tag: &str) -> String {
    tag.trim_start()
        .trim_start_matches('/')
        .trim_start_matches('!')
        .trim_start_matches('?')
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_text(value: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    let mut newline_count = 0usize;

    for ch in value.replace("\r\n", "\n").replace('\r', "\n").chars() {
        if ch == '\n' {
            pending_space = false;
            newline_count += 1;
            if newline_count <= 2 && !out.ends_with('\n') {
                out.push('\n');
            } else if newline_count == 2 {
                out.push('\n');
            }
            continue;
        }

        if ch.is_whitespace() {
            if !out.is_empty() && !out.ends_with('\n') {
                pending_space = true;
            }
            continue;
        }

        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        newline_count = 0;
        out.push(ch);
    }

    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::html_to_text;

    #[test]
    fn converts_basic_html_to_readable_text() {
        let html = "<div>Hello&nbsp;<b>Wox</b></div><p>Mail &amp; Search</p>";
        assert_eq!(html_to_text(html), "Hello Wox\nMail & Search");
    }

    #[test]
    fn skips_script_and_style_content() {
        let html = "<style>.x{}</style><p>Safe</p><script>alert(1)</script><p>Text</p>";
        assert_eq!(html_to_text(html), "Safe\nText");
    }
}
