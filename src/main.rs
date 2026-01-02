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

    // Check if we need to migrate from old schema
    let needs_migration = conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='pageviews'",
        [],
        |row| row.get::<_, String>(0)
    ).is_ok() && conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='pageviews'",
        [],
        |row| row.get::<_, String>(0)
    ).unwrap_or_default().contains("ts INTEGER");

    if needs_migration {
        println!("Migrating database from timestamp-based to date-based schema...");

        // Rename old table
        conn.execute("ALTER TABLE pageviews RENAME TO pageviews_old", []).unwrap();

        // Create new table with date-based schema
        conn.execute_batch(
            "CREATE TABLE pageviews (
                domain TEXT NOT NULL,
                page TEXT NOT NULL,
                date TEXT NOT NULL,
                view_count INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (domain, page, date)
            );"
        ).unwrap();

        // Migrate data with aggregation
        conn.execute(
            "INSERT INTO pageviews (domain, page, date, view_count)
             SELECT domain, page, date(ts, 'unixepoch') as date, COUNT(*) as view_count
             FROM pageviews_old
             GROUP BY domain, page, date(ts, 'unixepoch')",
            []
        ).unwrap();

        // Drop old table
        conn.execute("DROP TABLE pageviews_old", []).unwrap();

        println!("Migration completed successfully!");
    } else {
        // Create new table if it doesn't exist
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pageviews (
                domain TEXT NOT NULL,
                page TEXT NOT NULL,
                date TEXT NOT NULL,
                view_count INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (domain, page, date)
            );"
        ).unwrap();
    }

    let state = AppState { db: Arc::new(Mutex::new(conn)) };

    let app = Router::new()
        .route("/counter.gif", get(count_page_view))
        .route("/stats.json",  get(export))
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
    let date = OffsetDateTime::now_utc().date();
    let date_str = format!("{:04}-{:02}-{:02}", date.year(), date.month() as u8, date.day());

    let db = state.db.lock().unwrap();
    let _ = db.execute(
        "INSERT INTO pageviews (domain, page, date, view_count) VALUES (?, ?, ?, 1)
         ON CONFLICT (domain, page, date) DO UPDATE SET view_count = view_count + 1",
        (domain, page, date_str),
    );

    (
        [("Content-Type", "image/gif")],
        PIXEL_GIF
    )
}

async fn export(
    State(state): State<AppState>,
    Query(params): Query<Params>,
) -> impl IntoResponse {

    let db = state.db.lock().unwrap();

    // Fetch pageview records, optionally filtered by domain
    let (query, params_vec): (&str, Vec<&dyn rusqlite::ToSql>) = if let Some(ref domain) = params.domain {
        ("SELECT domain, page, date, view_count FROM pageviews WHERE domain = ? ORDER BY date DESC", vec![domain])
    } else {
        ("SELECT domain, page, date, view_count FROM pageviews ORDER BY date DESC", vec![])
    };

    let mut stmt = db.prepare(query).unwrap();
    let rows = stmt.query_map(params_vec.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?
        ))
    }).unwrap();

    let mut pageviews = Vec::new();
    let mut pages = std::collections::HashSet::new();
    let mut total_views = 0i64;

    for row in rows {
        let (domain, page, date, view_count) = row.unwrap();
        pages.insert(page.clone());
        total_views += view_count;

        pageviews.push(serde_json::json!({
            "domain": domain,
            "page": page,
            "date": date,
            "view_count": view_count
        }));
    }

    let summary = serde_json::json!({
        "unique_pages": pages.len(),
        "total_views": total_views,
        "total_records": pageviews.len()
    });

    let result = serde_json::json!({
        "summary": summary,
        "pageviews": pageviews
    });

    (
        [("Content-Type", "application/json")],
        serde_json::to_string_pretty(&result).unwrap()
    )
}
