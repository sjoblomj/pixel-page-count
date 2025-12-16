use axum::{
    extract::{Query, State},
    routing::get,
    Router,
    response::IntoResponse,
};
use rusqlite::Connection;
use std::{sync::{Arc, Mutex}, net::SocketAddr, path::Path};
use time::OffsetDateTime;

static PIXEL_GIF: &[u8] = b"GIF89a\
\x01\x00\x01\x00\x80\x00\x00\
\x00\x00\x00\xFF\xFF\xFF!\xF9\x04\x01\x00\x00\
\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\
\x00\x02\x02D\x01\x00;";

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
}

#[tokio::main]
async fn main() {
    // Use /data for fly.io volume, fallback to ./data for local development
    let db_path = if Path::new("/data").exists() {
        "/data/analytics.db"
    } else {
        std::fs::create_dir_all("data").unwrap();
        "data/analytics.db"
    };

    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pageviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            domain TEXT NOT NULL,
            page TEXT NOT NULL
        );"
    ).unwrap();

    let state = AppState { db: Arc::new(Mutex::new(conn)) };

    let app = Router::new()
        .route("/counter.gif",  get(count_page_view))
        .route("/stats.json", get(export))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(serde::Deserialize)]
struct Params {
    domain: Option<String>,
    page:   Option<String>,
}

async fn count_page_view(
    State(state):  State<AppState>,
    Query(params): Query<Params>,
) -> impl IntoResponse {
    let domain = params.domain.unwrap_or_else(|| "unknown".into());
    let page = params.page.unwrap_or_else(|| "/unknown".into());
    let ts = OffsetDateTime::now_utc().unix_timestamp();

    let db = state.db.lock().unwrap();
    let _ = db.execute(
        "INSERT INTO pageviews (ts, domain, page) VALUES (?, ?, ?)",
        (ts, domain, page),
    );

    (
        [("Content-Type", "image/gif")],
        PIXEL_GIF
    )
}

async fn export(
    State(state): State<AppState>
) -> impl IntoResponse {

    let db = state.db.lock().unwrap();

    // Fetch all events
    let mut stmt = db.prepare("SELECT ts, domain, page FROM pageviews ORDER BY ts DESC").unwrap();
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    }).unwrap();

    let mut latest = Vec::new();
    let mut pages = std::collections::HashSet::new();

    for row in rows {
        let (ts, domain, page) = row.unwrap();
        pages.insert(page.clone());

        latest.push(serde_json::json!({
            "ts": ts,
            "domain": domain,
            "page": page
        }));
    }

    let summary = serde_json::json!({
        "unique_pages": pages.len(),
        "total_events": latest.len()
    });

    let result = serde_json::json!({
        "summary": summary,
        "latest": latest
    });

    (
        [("Content-Type", "application/json")],
        serde_json::to_string_pretty(&result).unwrap()
    )
}
