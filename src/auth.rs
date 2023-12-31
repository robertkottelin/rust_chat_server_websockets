use anyhow::{Result};

use crate::database;

pub async fn authenticate_user(
    username: &str,
    password: &str,
) -> Result<()> {
    let mut authenticated = false;

    while !authenticated {
        // Check if the user is authenticated
        if database::user_auth(&username, &password)
            .await
            .unwrap_or(false)
        {
            authenticated = true;
        } else {
            // If the user is not authenticated, store their credentials and add them to the database
            database::database(&username, &password)
                .await
                .expect("Failed to add user to the database");
        }
    }

    Ok(())
}