//! History pagination must keep database work bounded independently of page size.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use finch_server::{
    api::create_router, db::init_db, session::SessionManager,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};
use uuid::Uuid;

/// These scenarios create several users without invitations.
fn open_registration() -> finch_server::config::ServerConfig {
    let mut config = finch_server::config::ServerConfig::default();
    config.auth.registration = finch_server::config::RegistrationMode::Open;
    config
}


#[derive(Clone, Default)]
struct AttachmentQueries(Arc<Mutex<Vec<u64>>>);

#[derive(Default)]
struct QueryFields {
    statement: String,
    rows: u64,
}

impl Visit for QueryFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" {
            self.statement = format!("{value:?}");
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "rows_returned" {
            self.rows = value;
        }
    }
}

impl<S: Subscriber> Layer<S> for AttachmentQueries {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut fields = QueryFields::default();
        event.record(&mut fields);
        if fields.statement.contains("SELECT") && fields.statement.contains("FROM attachments") {
            self.0.lock().unwrap().push(fields.rows);
        }
    }
}

async fn history(
    app: &axum::Router,
    token: &str,
    channel: &str,
    query: &str,
) -> anyhow::Result<Value> {
    let request = Request::builder()
        .uri(format!("/api/channels/{channel}/messages?{query}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(serde_json::from_slice(
        &response.into_body().collect().await?.to_bytes(),
    )?)
}

#[tokio::test]
async fn history_batches_only_page_attachments_and_preserves_pagination() -> anyhow::Result<()> {
    // This integration-test binary owns its global subscriber, including SQLite worker threads.
    let queries = AttachmentQueries::default();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(queries.clone()))?;
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), SessionManager::new());
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"username": "reader", "password": "password_123"}).to_string(),
        ))?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let auth: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    let token = auth["token"].as_str().unwrap();
    let user = auth["user"]["id"].as_str().unwrap();
    let channel: String = sqlx::query_scalar("SELECT id FROM channels WHERE name = 'general'")
        .fetch_one(&pool)
        .await?;
    let ids: Vec<String> = (1..=5).map(|id| Uuid::from_u128(id).to_string()).collect();
    for (id, timestamp) in ids.iter().zip([10_i64, 20, 30, 30, 30]) {
        sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(id).bind(&channel).bind(user).bind(id).bind(timestamp).execute(&pool).await?;
    }
    // Reverse UUID order for the two attachments on message 4: retain insertion order.
    for (id, message, filename) in [
        (11_u128, Some(&ids[0]), "old"),
        (12, Some(&ids[1]), "lookahead"),
        (15, Some(&ids[3]), "first.png"),
        (14, Some(&ids[3]), "second.txt"),
        (16, Some(&ids[4]), "latest"),
        (17, None, "unlinked"),
    ] {
        sqlx::query("INSERT INTO attachments (id, uploader_id, message_id, filename, size_bytes, content_type, storage_path, created_at) VALUES (?, ?, ?, ?, 42, 'text/plain', 'unused', 1)")
            .bind(Uuid::from_u128(id).to_string()).bind(user).bind(message).bind(filename)
            .execute(&pool).await?;
    }

    queries.0.lock().unwrap().clear();
    let page = history(&app, token, &channel, "limit=3").await?;
    assert_eq!(page["channel_id"], channel);
    assert_eq!(page["has_more"], true);
    assert_eq!(
        page["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![ids[2].as_str(), ids[3].as_str(), ids[4].as_str()]
    );
    assert_eq!(page["messages"][0]["attachments"], json!([]));
    assert_eq!(
        page["messages"][1]["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["filename"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["first.png", "second.txt"]
    );
    assert_eq!(page["messages"][1]["attachments"][0]["size_bytes"], 42);
    assert_eq!(
        page["messages"][1]["attachments"][0]["content_type"],
        "text/plain"
    );
    assert_eq!(
        page["messages"][1]["attachments"][0]["url"],
        format!("/api/files/{}", Uuid::from_u128(15))
    );
    // Actual SQLite query events prove constant query count and exclude the lookahead/unlinked rows.
    assert_eq!(
        *queries.0.lock().unwrap(),
        vec![3],
        "one attachment SELECT returning only page attachments"
    );

    queries.0.lock().unwrap().clear();
    let older = history(
        &app,
        token,
        &channel,
        &format!("limit=3&before_id={}", ids[2]),
    )
    .await?;
    assert_eq!(older["has_more"], false);
    assert_eq!(
        older["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![ids[0].as_str(), ids[1].as_str()]
    );
    assert_eq!(*queries.0.lock().unwrap(), vec![2]);
    let before = history(&app, token, &channel, "limit=3&before=30").await?;
    assert_eq!(before, older);

    queries.0.lock().unwrap().clear();
    let empty = history(&app, token, &channel, "before=10").await?;
    assert_eq!(empty["messages"], json!([]));
    assert_eq!(empty["has_more"], false);
    assert!(
        queries.0.lock().unwrap().is_empty(),
        "empty pages need no attachment SELECT"
    );
    let single = history(&app, token, &channel, "limit=0").await?;
    assert_eq!(single["messages"].as_array().unwrap().len(), 1);
    assert_eq!(single["messages"][0]["id"], ids[4]);
    assert_eq!(single["has_more"], true);
    let request = Request::builder()
        .uri(format!("/api/channels/{channel}/messages"))
        .body(Body::empty())?;
    assert_eq!(
        app.oneshot(request).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    pool.close().await;
    Ok(())
}
