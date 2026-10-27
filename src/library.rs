//! The Innuto collection: tracks, analysis results, cue points, and playlists
//! stored in an unencrypted SQLite database.

use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::analysis::Analysis;

const SCHEMA: &str = "
CREATE TABLE track (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT,
    artist TEXT,
    duration_ms INTEGER,
    sample_rate INTEGER,
    bpm REAL,
    first_beat_ms REAL,
    preview BLOB,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    analyzed_at INTEGER
);
CREATE TABLE cue (
    id INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    hot_slot INTEGER,
    position_ms REAL NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    UNIQUE (track_id, hot_slot)
);
CREATE TABLE playlist (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER REFERENCES playlist(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_folder INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE playlist_track (
    playlist_id INTEGER NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
);
";

#[derive(Debug, Clone)]
pub struct TrackRow {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_ms: Option<i64>,
    pub bpm: Option<f64>,
    pub first_beat_ms: Option<f64>,
    pub preview: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Cue {
    pub hot_slot: Option<u8>,
    pub position_ms: f64,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub is_folder: bool,
}

pub struct Library {
    conn: Connection,
}

/// `<platform data dir>/Innuto/library.db`, shared by the CLI and the app.
pub fn default_path() -> Result<std::path::PathBuf> {
    Ok(dirs::data_dir().context("no platform data directory; pass --db")?.join("Innuto").join("library.db"))
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => conn.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = 1; COMMIT;"))?,
            1 => {}
            v => bail!("library schema version {v} is newer than this Innuto build"),
        }
        Ok(Library { conn })
    }

    /// Adds a track if its path is new; returns its id either way.
    pub fn add_track(&self, path: &str, title: Option<&str>, artist: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO track (path, title, artist) VALUES (?1, ?2, ?3) ON CONFLICT(path) DO NOTHING",
            params![path, title, artist],
        )?;
        Ok(self.conn.query_row("SELECT id FROM track WHERE path = ?1", [path], |r| r.get(0))?)
    }

    pub fn save_analysis(&self, id: i64, a: &Analysis, duration_ms: i64, sample_rate: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE track SET bpm = ?2, first_beat_ms = ?3, preview = ?4, duration_ms = ?5,
             sample_rate = ?6, analyzed_at = unixepoch() WHERE id = ?1",
            params![id, a.bpm, a.first_beat_ms, a.preview, duration_ms, sample_rate],
        )?;
        Ok(())
    }

    /// Replaces the beat grid with one taken from another application's analysis.
    pub fn set_beat_grid(&self, id: i64, bpm: f64, first_beat_ms: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE track SET bpm = ?2, first_beat_ms = ?3 WHERE id = ?1",
            params![id, bpm, first_beat_ms],
        )?;
        Ok(())
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<TrackRow> {
        Ok(TrackRow {
            id: r.get(0)?,
            path: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            duration_ms: r.get(4)?,
            bpm: r.get(5)?,
            first_beat_ms: r.get(6)?,
            preview: r.get(7)?,
        })
    }

    const TRACK_COLUMNS: &'static str =
        "t.id, t.path, t.title, t.artist, t.duration_ms, t.bpm, t.first_beat_ms, t.preview";

    pub fn track(&self, id: i64) -> Result<TrackRow> {
        self.conn
            .query_row(&format!("SELECT {} FROM track t WHERE t.id = ?1", Self::TRACK_COLUMNS), [id], Self::row)
            .optional()?
            .with_context(|| format!("no track with id {id}"))
    }

    /// Tracks matching `filter` against title, artist, or path; all when empty.
    pub fn tracks(&self, filter: &str) -> Result<Vec<TrackRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM track t WHERE ?1 = '' OR t.title LIKE ?2 OR t.artist LIKE ?2 OR t.path LIKE ?2
             ORDER BY t.artist, t.title, t.path",
            Self::TRACK_COLUMNS
        ))?;
        let like = format!("%{filter}%");
        let rows = stmt.query_map(params![filter, like], Self::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_cue(&self, track_id: i64, cue: &Cue) -> Result<()> {
        match cue.hot_slot {
            Some(slot) => self.conn.execute(
                "INSERT INTO cue (track_id, hot_slot, position_ms, label) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(track_id, hot_slot) DO UPDATE SET position_ms = excluded.position_ms,
                 label = excluded.label",
                params![track_id, slot, cue.position_ms, cue.label],
            )?,
            None => self.conn.execute(
                "INSERT INTO cue (track_id, position_ms, label) VALUES (?1, ?2, ?3)",
                params![track_id, cue.position_ms, cue.label],
            )?,
        };
        Ok(())
    }

    pub fn delete_hot_cue(&self, track_id: i64, slot: u8) -> Result<()> {
        self.conn.execute("DELETE FROM cue WHERE track_id = ?1 AND hot_slot = ?2", params![track_id, slot])?;
        Ok(())
    }

    pub fn cues(&self, track_id: i64) -> Result<Vec<Cue>> {
        let mut stmt = self.conn.prepare(
            "SELECT hot_slot, position_ms, label FROM cue WHERE track_id = ?1 ORDER BY position_ms",
        )?;
        let rows = stmt.query_map([track_id], |r| {
            Ok(Cue { hot_slot: r.get(0)?, position_ms: r.get(1)?, label: r.get(2)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_playlist(&self, name: &str, parent: Option<i64>, is_folder: bool) -> Result<i64> {
        if let Some(p) = parent {
            let folder: Option<bool> = self
                .conn
                .query_row("SELECT is_folder FROM playlist WHERE id = ?1", [p], |r| r.get(0))
                .optional()?;
            match folder {
                Some(true) => {}
                Some(false) => bail!("playlist {p} is not a folder"),
                None => bail!("no folder with id {p}"),
            }
        }
        self.conn.execute(
            "INSERT INTO playlist (parent_id, name, is_folder) VALUES (?1, ?2, ?3)",
            params![parent, name, is_folder],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        let is_folder: bool = self
            .conn
            .query_row("SELECT is_folder FROM playlist WHERE id = ?1", [playlist_id], |r| r.get(0))
            .optional()?
            .with_context(|| format!("no playlist with id {playlist_id}"))?;
        if is_folder {
            bail!("{playlist_id} is a folder; add tracks to a playlist inside it");
        }
        self.conn.execute(
            "INSERT INTO playlist_track (playlist_id, track_id, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_track WHERE playlist_id = ?1))",
            params![playlist_id, track_id],
        )?;
        Ok(())
    }

    pub fn playlists(&self) -> Result<Vec<PlaylistRow>> {
        let mut stmt = self.conn.prepare("SELECT id, parent_id, name, is_folder FROM playlist ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(PlaylistRow { id: r.get(0)?, parent_id: r.get(1)?, name: r.get(2)?, is_folder: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<TrackRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM playlist_track p JOIN track t ON t.id = p.track_id
             WHERE p.playlist_id = ?1 ORDER BY p.position",
            Self::TRACK_COLUMNS
        ))?;
        let rows = stmt.query_map([playlist_id], Self::row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_are_deduplicated_by_path_and_searchable() {
        let lib = Library::open_in_memory().unwrap();
        let a = lib.add_track("/m/a.mp3", Some("Alpha"), Some("DJ One")).unwrap();
        assert_eq!(lib.add_track("/m/a.mp3", None, None).unwrap(), a);
        lib.add_track("/m/b.mp3", Some("Beta"), Some("DJ Two")).unwrap();
        assert_eq!(lib.tracks("").unwrap().len(), 2);
        let found = lib.tracks("two").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title.as_deref(), Some("Beta"));
    }

    #[test]
    fn hot_cues_replace_by_slot() {
        let lib = Library::open_in_memory().unwrap();
        let t = lib.add_track("/m/a.mp3", None, None).unwrap();
        lib.set_cue(t, &Cue { hot_slot: Some(0), position_ms: 1000.0, label: "A".into() }).unwrap();
        lib.set_cue(t, &Cue { hot_slot: Some(0), position_ms: 2000.0, label: "A2".into() }).unwrap();
        lib.set_cue(t, &Cue { hot_slot: None, position_ms: 500.0, label: String::new() }).unwrap();
        let cues = lib.cues(t).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].position_ms, 2000.0);
    }

    #[test]
    fn playlists_nest_in_folders_and_keep_order() {
        let lib = Library::open_in_memory().unwrap();
        let folder = lib.create_playlist("Gigs", None, true).unwrap();
        let pl = lib.create_playlist("Friday", Some(folder), false).unwrap();
        assert!(lib.create_playlist("Bad", Some(pl), false).is_err());
        assert!(lib.add_to_playlist(folder, 1).is_err());
        let a = lib.add_track("/m/a.mp3", None, None).unwrap();
        let b = lib.add_track("/m/b.mp3", None, None).unwrap();
        lib.add_to_playlist(pl, b).unwrap();
        lib.add_to_playlist(pl, a).unwrap();
        let ids: Vec<i64> = lib.playlist_tracks(pl).unwrap().iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![b, a]);
    }
}
