use rusqlite::Connection;
use crate::db::migrations::run_migrations;

/// Creates an isolated in-memory database with the current schema and seed data.
pub fn in_memory_database() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory SQLite database");
    run_migrations(&conn).expect("run Sujood database migrations");
    conn
}
