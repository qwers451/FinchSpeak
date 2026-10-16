//! Shared chat helpers: mentions, reply quotes, attachment info, thumbnails and read state.

use std::collections::HashMap;

use finch_protocol::models::{AttachmentInfo, ChannelUnread, ReplyRef};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Longest quoted part of the original in a reply.
pub const SNIPPET_CHARS: usize = 120;
/// Longest side of a stored thumbnail.
pub const THUMB_MAX_SIDE: u32 = 320;
/// Images larger than this many pixels are kept as plain files (guards against decompression bombs).
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;

/// The first `SNIPPET_CHARS` characters of a message, whitespace collapsed to single spaces.
pub fn snippet(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(SNIPPET_CHARS).collect()
}

/// Logins mentioned as `@login` in `content`, in order of appearance, without duplicates.
///
/// A mention starts at a word boundary (so `me@host.example` is not one) and ends at the first
/// character that cannot be part of a login; trailing dots and dashes belong to the sentence.
pub fn mentioned_logins(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let is_login_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && end - start < 32 && is_login_char(chars[end]) {
                end += 1;
            }
            while end > start && matches!(chars[end - 1], '.' | '-') {
                end -= 1;
            }
            if end > start {
                let login: String = chars[start..end].iter().collect::<String>().to_lowercase();
                if !found.contains(&login) {
                    found.push(login);
                }
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    found
}

/// Ids of the existing users `content` mentions.
pub async fn resolve_mentions(pool: &SqlitePool, content: &str) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for login in mentioned_logins(content) {
        let row: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE username = ? COLLATE NOCASE")
            .bind(&login)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);
        if let Some(id) = row.and_then(|id| Uuid::parse_str(&id).ok())
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// The quote shown above a reply to `reference`; `deleted` once the original is gone.
pub async fn reply_ref(pool: &SqlitePool, reference: Uuid) -> ReplyRef {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT u.display_name, m.content FROM messages m JOIN users u ON u.id = m.author_id WHERE m.id = ?",
    )
    .bind(reference.to_string())
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    match row {
        Some((author_name, content)) => ReplyRef { message_id: reference, author_name, snippet: snippet(&content), deleted: false },
        None => ReplyRef { message_id: reference, author_name: String::new(), snippet: String::new(), deleted: true },
    }
}

type AttachmentRow = (String, String, String, i64, String, Option<i64>, Option<i64>, Option<String>);

pub fn attachment_info(row: AttachmentRow) -> Option<(String, AttachmentInfo)> {
    let (message_id, id, filename, size_bytes, content_type, width, height, thumb_path) = row;
    let uuid = Uuid::parse_str(&id).ok()?;
    Some((
        message_id,
        AttachmentInfo {
            id: uuid,
            url: format!("/api/files/{id}"),
            filename,
            size_bytes: size_bytes as u64,
            content_type,
            width: width.map(|w| w as u32),
            height: height.map(|h| h as u32),
            has_thumb: thumb_path.is_some(),
        },
    ))
}

/// Attachments of the given messages, grouped by message id, in upload order.
pub async fn attachments_for(pool: &SqlitePool, message_ids: &[String]) -> Result<HashMap<String, Vec<AttachmentInfo>>, sqlx::Error> {
    let mut grouped: HashMap<String, Vec<AttachmentInfo>> = HashMap::new();
    if message_ids.is_empty() {
        return Ok(grouped);
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT message_id, id, filename, size_bytes, content_type, width, height, thumb_path FROM attachments WHERE message_id IN (",
    );
    let mut ids = query.separated(", ");
    for id in message_ids {
        ids.push_bind(id);
    }
    ids.push_unseparated(") ORDER BY rowid");
    for row in query.build_query_as::<AttachmentRow>().fetch_all(pool).await? {
        if let Some((message_id, info)) = attachment_info(row) {
            grouped.entry(message_id).or_default().push(info);
        }
    }
    Ok(grouped)
}

/// Mentioned users of the given messages, grouped by message id.
pub async fn mentions_for(pool: &SqlitePool, message_ids: &[String]) -> Result<HashMap<String, Vec<Uuid>>, sqlx::Error> {
    let mut grouped: HashMap<String, Vec<Uuid>> = HashMap::new();
    if message_ids.is_empty() {
        return Ok(grouped);
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT message_id, user_id FROM message_mentions WHERE message_id IN (");
    let mut ids = query.separated(", ");
    for id in message_ids {
        ids.push_bind(id);
    }
    ids.push_unseparated(") ORDER BY rowid");
    for (message_id, user_id) in query.build_query_as::<(String, String)>().fetch_all(pool).await? {
        if let Ok(user_id) = Uuid::parse_str(&user_id) {
            grouped.entry(message_id).or_default().push(user_id);
        }
    }
    Ok(grouped)
}

/// Reply quotes for the given `(message id, reference)` pairs.
pub async fn reply_refs_for(pool: &SqlitePool, references: &[(String, String)]) -> HashMap<String, ReplyRef> {
    let mut quotes = HashMap::new();
    for (message_id, reference) in references {
        if let Ok(reference) = Uuid::parse_str(reference) {
            quotes.insert(message_id.clone(), reply_ref(pool, reference).await);
        }
    }
    quotes
}

/// Unread messages per text channel for `user_id`: messages by others after the user's read mark
/// (or, without one, after the account was created).
pub async fn unread_for(pool: &SqlitePool, user_id: Uuid) -> Result<Vec<ChannelUnread>, sqlx::Error> {
    let user = user_id.to_string();
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT m.channel_id, COUNT(*),
                COALESCE(SUM(EXISTS (SELECT 1 FROM message_mentions mm WHERE mm.message_id = m.id AND mm.user_id = ?1)), 0)
         FROM messages m
         JOIN channels c ON c.id = m.channel_id AND c.channel_type = 'text'
         JOIN users u ON u.id = ?1
         LEFT JOIN read_state rs ON rs.user_id = ?1 AND rs.channel_id = m.channel_id
         WHERE m.author_id != ?1
           AND (m.created_at, m.id) > (COALESCE(rs.last_read_at, u.created_at), COALESCE(rs.last_read_id, ''))
         GROUP BY m.channel_id
         ORDER BY c.position, c.id",
    )
    .bind(&user)
    .fetch_all(pool)
    .await?;
    let mut unread = Vec::with_capacity(rows.len());
    for (channel_id, count, mentions) in rows {
        let Ok(channel) = Uuid::parse_str(&channel_id) else { continue };
        let first: Option<String> = sqlx::query_scalar(
            "SELECT m.id FROM messages m
             JOIN users u ON u.id = ?1
             LEFT JOIN read_state rs ON rs.user_id = ?1 AND rs.channel_id = m.channel_id
             WHERE m.channel_id = ?2 AND m.author_id != ?1
               AND (m.created_at, m.id) > (COALESCE(rs.last_read_at, u.created_at), COALESCE(rs.last_read_id, ''))
             ORDER BY m.created_at, m.id LIMIT 1",
        )
        .bind(&user)
        .bind(&channel_id)
        .fetch_optional(pool)
        .await?;
        unread.push(ChannelUnread {
            channel_id: channel,
            count: count as u32,
            mentions: mentions as u32,
            first_unread_id: first.and_then(|id| Uuid::parse_str(&id).ok()),
        });
    }
    Ok(unread)
}

/// Pixel size and a PNG/JPEG thumbnail (longest side ≤ `THUMB_MAX_SIDE`, never enlarged) of an
/// image, or `None` when `data` is not an image this server can decode safely.
pub fn make_thumbnail(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use image::{ImageDecoder, ImageFormat, ImageReader};
    let reader = ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    let format = reader.format()?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP) {
        return None;
    }
    let decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return None;
    }
    let image = image::load_from_memory_with_format(data, format).ok()?;
    let thumb = if width.max(height) > THUMB_MAX_SIDE { image.thumbnail(THUMB_MAX_SIDE, THUMB_MAX_SIDE) } else { image };
    let mut out = std::io::Cursor::new(Vec::new());
    // Photos stay JPEG, everything else PNG (keeps transparency).
    if format == ImageFormat::Jpeg {
        thumb.to_rgb8().write_to(&mut out, ImageFormat::Jpeg).ok()?;
    } else {
        thumb.write_to(&mut out, ImageFormat::Png).ok()?;
    }
    Some((width, height, out.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_need_a_word_boundary_and_drop_sentence_punctuation() {
        assert_eq!(mentioned_logins("hi @bob, and @Bob. also @carol-d."), vec!["bob", "carol-d"]);
        assert!(mentioned_logins("mail me: alice@bob.example").is_empty());
        assert_eq!(mentioned_logins("(@alice)"), vec!["alice"]);
        assert!(mentioned_logins("@").is_empty());
        assert!(mentioned_logins("@.").is_empty());
    }

    #[test]
    fn mention_scanning_never_panics_on_odd_input() {
        for text in ["@@@@", "@ж", "a@b@c@d", "@\u{1F600}x", &"@a".repeat(1000)] {
            let _ = mentioned_logins(text);
        }
    }

    #[test]
    fn snippets_are_cut_by_characters_not_bytes() {
        let long = "ж".repeat(300);
        assert_eq!(snippet(&long).chars().count(), SNIPPET_CHARS);
        assert_eq!(snippet("a \n  b\t c"), "a b c");
    }
}
