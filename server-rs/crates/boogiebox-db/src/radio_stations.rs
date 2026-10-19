//! Internet radio stations: shared by all users, edited by admins, exposed to the web player (through the
//! server's stream proxy) and to Subsonic apps (`getInternetRadioStations`).
//! See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-6).

use rusqlite::{params, Connection, OptionalExtension};

/// Longest station name.
pub const MAX_NAME_CHARS: usize = 200;
/// Longest stream or homepage URL.
pub const MAX_URL_CHARS: usize = 2048;

/// Creates `internet_radio_stations` (part of migration `2026-10-19-subsonic-phase-2-5`).
pub fn ensure_radio_station_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS internet_radio_stations (
           id            TEXT PRIMARY KEY,
           name          TEXT NOT NULL,
           stream_url    TEXT NOT NULL,
           homepage_url  TEXT,
           created_at    TEXT NOT NULL DEFAULT (datetime('now')),
           updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
         );",
    )
}

/// One station.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    pub stream_url: String,
    pub homepage_url: Option<String>,
}

/// Validated station fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationInput {
    pub name: String,
    pub stream_url: String,
    pub homepage_url: Option<String>,
}

/// Why station input was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationError {
    Name,
    StreamUrl,
    HomepageUrl,
}

impl StationError {
    pub fn message(self) -> &'static str {
        match self {
            StationError::Name => "Station name must be 1–200 characters",
            StationError::StreamUrl => "Stream URL must be an http:// or https:// address",
            StationError::HomepageUrl => "Homepage must be an http:// or https:// address",
        }
    }
}

/// True for an absolute http(s) URL with a host, no whitespace or control characters, ≤ 2048 characters.
pub fn valid_http_url(raw: &str) -> bool {
    if raw.chars().count() > MAX_URL_CHARS
        || raw.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let lower = raw.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if let Some(r) = lower.strip_prefix("https://") {
        r
    } else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("");
    !host.is_empty() && !host.starts_with(':')
}

impl StationInput {
    /// Trims and validates raw fields; an empty homepage means none.
    pub fn new(
        name: &str,
        stream_url: &str,
        homepage_url: Option<&str>,
    ) -> Result<Self, StationError> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(StationError::Name);
        }
        let stream_url = stream_url.trim();
        if !valid_http_url(stream_url) {
            return Err(StationError::StreamUrl);
        }
        let homepage_url = match homepage_url.map(str::trim).filter(|h| !h.is_empty()) {
            Some(h) if valid_http_url(h) => Some(h.to_owned()),
            Some(_) => return Err(StationError::HomepageUrl),
            None => None,
        };
        Ok(StationInput {
            name: name.to_owned(),
            stream_url: stream_url.to_owned(),
            homepage_url,
        })
    }
}

fn map_station(row: &rusqlite::Row<'_>) -> rusqlite::Result<RadioStation> {
    Ok(RadioStation {
        id: row.get(0)?,
        name: row.get(1)?,
        stream_url: row.get(2)?,
        homepage_url: row.get(3)?,
    })
}

/// All stations by name.
pub fn list_stations(conn: &Connection) -> rusqlite::Result<Vec<RadioStation>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, stream_url, homepage_url FROM internet_radio_stations
         ORDER BY name COLLATE NOCASE, id",
    )?;
    let rows = stmt.query_map([], map_station)?;
    rows.collect()
}

/// One station.
pub fn get_station(conn: &Connection, id: &str) -> rusqlite::Result<Option<RadioStation>> {
    conn.query_row(
        "SELECT id, name, stream_url, homepage_url FROM internet_radio_stations WHERE id = ?1",
        [id],
        map_station,
    )
    .optional()
}

/// Adds a station and returns it.
pub fn create_station(conn: &Connection, input: &StationInput) -> rusqlite::Result<RadioStation> {
    let id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO internet_radio_stations(id, name, stream_url, homepage_url) VALUES (?1, ?2, ?3, ?4)",
        params![id, input.name, input.stream_url, input.homepage_url],
    )?;
    Ok(RadioStation {
        id,
        name: input.name.clone(),
        stream_url: input.stream_url.clone(),
        homepage_url: input.homepage_url.clone(),
    })
}

/// Replaces a station's fields; `None` when it does not exist.
pub fn update_station(
    conn: &Connection,
    id: &str,
    input: &StationInput,
) -> rusqlite::Result<Option<RadioStation>> {
    let n = conn.execute(
        "UPDATE internet_radio_stations SET name = ?2, stream_url = ?3, homepage_url = ?4,
           updated_at = datetime('now') WHERE id = ?1",
        params![id, input.name, input.stream_url, input.homepage_url],
    )?;
    if n == 0 {
        return Ok(None);
    }
    get_station(conn, id)
}

/// Deletes a station; true when it existed.
pub fn delete_station(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM internet_radio_stations WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn url_validation() {
        assert!(valid_http_url("http://radio.example.com:8000/stream"));
        assert!(valid_http_url("HTTPS://ice.example.org/live.mp3?x=1"));
        assert!(valid_http_url("http://192.168.1.5/live"));
        assert!(!valid_http_url("ftp://example.com/a"));
        assert!(!valid_http_url("file:///etc/passwd"));
        assert!(!valid_http_url("http://"));
        assert!(!valid_http_url("http:///path"));
        assert!(!valid_http_url("http://:80/x"));
        assert!(!valid_http_url("http://exa mple.com"));
        assert!(!valid_http_url("javascript:alert(1)"));
        assert!(!valid_http_url(&format!(
            "http://a.com/{}",
            "x".repeat(MAX_URL_CHARS)
        )));
    }

    #[test]
    fn input_validation() {
        let ok = StationInput::new("  Jazz FM ", " http://jazz.example/live ", Some("  ")).unwrap();
        assert_eq!(ok.name, "Jazz FM");
        assert_eq!(ok.stream_url, "http://jazz.example/live");
        assert_eq!(ok.homepage_url, None);
        assert_eq!(
            StationInput::new(" ", "http://a.b", None),
            Err(StationError::Name)
        );
        assert_eq!(
            StationInput::new(&"n".repeat(MAX_NAME_CHARS + 1), "http://a.b", None),
            Err(StationError::Name)
        );
        assert_eq!(
            StationInput::new("x", "a.b", None),
            Err(StationError::StreamUrl)
        );
        assert_eq!(
            StationInput::new("x", "http://a.b", Some("mailto:me")),
            Err(StationError::HomepageUrl)
        );
        assert!(!StationError::Name.message().is_empty());
    }

    #[test]
    fn crud() {
        let conn = db();
        let a = create_station(
            &conn,
            &StationInput::new("Zed", "http://z.example/s", None).unwrap(),
        )
        .unwrap();
        let b = create_station(
            &conn,
            &StationInput::new("alpha", "https://a.example/s", Some("https://a.example")).unwrap(),
        )
        .unwrap();
        let names: Vec<String> = list_stations(&conn)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["alpha", "Zed"]);
        assert_eq!(get_station(&conn, &b.id).unwrap().unwrap(), b);

        let edited = update_station(
            &conn,
            &a.id,
            &StationInput::new("Zed 2", "http://z2.example", None).unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(edited.name, "Zed 2");
        assert!(update_station(
            &conn,
            "missing",
            &StationInput::new("q", "http://q.q", None).unwrap()
        )
        .unwrap()
        .is_none());
        assert!(delete_station(&conn, &a.id).unwrap());
        assert!(!delete_station(&conn, &a.id).unwrap());
        assert!(get_station(&conn, &a.id).unwrap().is_none());
    }
}
