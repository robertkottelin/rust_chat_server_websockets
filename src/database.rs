use anyhow::{Result};
use bcrypt::{verify, hash, DEFAULT_COST};
use sqlite::{Connection};
use std::sync::Arc;
use tokio::{
    sync::{Mutex},
};

lazy_static::lazy_static! {
    pub static ref DB_MUTEX: Arc<Mutex<Connection>> = {
        let conn = Connection::open("users.db").expect("Failed to open users.db");
        Arc::new(Mutex::new(conn))
    };
}

pub async fn database(username: &str, password: &str) -> Result<()> {
    // Hash the password using bcrypt
    let hashed_password = hash(password, DEFAULT_COST)?;

    // println!("{:?}", hashed_password);

    // Create the users table if it does not exist
    let create_table_query = "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE, password TEXT NOT NULL);";
    let conn = DB_MUTEX.lock().await;
    conn.execute(create_table_query)?;

    // Insert the user into the users table
    let insert_user_query = format!(
        "INSERT INTO users (username, password) VALUES ('{}', '{}');",
        username, hashed_password
    );
    conn.execute(&insert_user_query)?;

    Ok(())
}

pub async fn user_auth(username: &str, password: &str) -> Result<bool> {
    let query = format!(
        "SELECT password FROM users WHERE username = '{}';",
        username
    );

    let conn = DB_MUTEX.lock().await;
    let mut password_hash: String = String::new();
    conn.iterate(&query, |row| {
        password_hash = row[0].1.unwrap().to_owned();
        true
    })?;

    let is_authenticated = verify(password, &password_hash)?;

    Ok(is_authenticated)
}