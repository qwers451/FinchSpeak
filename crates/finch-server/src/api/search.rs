//! Searching messages.

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    /// Only this channel.
    pub channel: Option<Uuid>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub message_id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub timestamp: i64,
}

/// Longest search text taken into account.
const MAX_QUERY_CHARS: usize = 100;
const MAX_TERMS: usize = 8;

/// Turn what a person typed into a safe FTS5 expression: every word is a quoted phrase (so
/// operators such as OR, NEAR, `-` and `:` lose their meaning), the last one also matches prefixes.
/// `None` when nothing searchable is left.
pub fn fts_expression(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(MAX_TERMS)
        .map(str::to_lowercase)
        .collect();
    let last = terms.len().checked_sub(1)?;
    Some(
        terms
            .iter()
            .enumerate()
            .map(|(i, term)| if i == last { format!("\"{term}\"*") } else { format!("\"{term}\"") })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// GET /api/search?q=&channel=&limit=&offset=: newest matches first.
pub async fn search_handler(
    _auth: AuthUser,
    State(state): State<ArcSharedState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, ServerError> {
    let Some(expression) = fts_expression(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let limit = i64::from(query.limit.unwrap_or(30).clamp(1, 100));
    let offset = i64::from(query.offset.unwrap_or(0).min(10_000));
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(
        "SELECT m.id, m.channel_id, c.name, m.author_id, u.display_name, m.content, m.created_at
         FROM messages_fts f
         JOIN messages m ON m.rowid = f.rowid
         JOIN channels c ON c.id = m.channel_id
         JOIN users u ON u.id = m.author_id
         WHERE messages_fts MATCH ?1 AND (?2 IS NULL OR m.channel_id = ?2)
         ORDER BY m.created_at DESC, m.id DESC
         LIMIT ?3 OFFSET ?4",
    )
    .bind(&expression)
    .bind(query.channel.map(|id| id.to_string()))
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(message_id, channel_id, channel_name, author_id, author_name, content, timestamp)| SearchHit {
                message_id,
                channel_id,
                channel_name,
                author_id,
                author_name,
                content,
                timestamp,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_quoted_phrases_and_the_last_one_matches_prefixes() {
        assert_eq!(fts_expression("Hello wor").as_deref(), Some("\"hello\" \"wor\"*"));
        assert_eq!(fts_expression("привет Мир").as_deref(), Some("\"привет\" \"мир\"*"));
    }

    #[test]
    fn operators_and_punctuation_lose_their_meaning() {
        assert_eq!(fts_expression("a OR b NEAR(c) -d \"e\" f:g *").as_deref(), Some("\"a\" \"or\" \"b\" \"near\" \"c\" \"d\" \"e\" \"f\"*"));
        assert_eq!(fts_expression("\"\"\"\"") , None);
        assert_eq!(fts_expression("   "), None);
        assert_eq!(fts_expression("!!! ???"), None);
    }

    #[test]
    fn long_input_is_cut() {
        let long = "word ".repeat(200);
        assert_eq!(fts_expression(&long).unwrap().matches('"').count(), MAX_TERMS * 2);
    }
}
