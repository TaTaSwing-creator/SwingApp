use anyhow::{anyhow, Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use super::AppSettings;

#[derive(Serialize)]
struct ConfigUpsert<'a> {
    username: &'a str,
    config_json: &'a AppSettings,
}

#[derive(Deserialize)]
struct ConfigRow {
    config_json: AppSettings,
}

fn endpoint() -> Result<String> {
    let supabase_url = std::env::var("SUPABASE_URL")
        .context("SUPABASE_URL is not configured")?;
    let supabase_url = supabase_url.trim_end_matches('/');
    if supabase_url.is_empty() {
        return Err(anyhow!("SUPABASE_URL is empty"));
    }

    Ok(format!("{supabase_url}/rest/v1/configs"))
}

fn client() -> Result<Client> {
    let anon_key = std::env::var("SUPABASE_ANON_KEY")
        .context("SUPABASE_ANON_KEY is not configured")?;

    Client::builder()
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::HeaderName::from_static("apikey"),
                anon_key
                    .parse()
                    .context("SUPABASE_ANON_KEY is not a valid header value")?,
            );
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {anon_key}")
                    .parse()
                    .context("failed to create Supabase authorization header")?,
            );
            headers
        })
        .build()
        .context("failed to create Supabase HTTP client")
}

/// Saves a user's settings by inserting or updating the row identified by username.
pub async fn save_config(username: &str, config: &AppSettings) -> Result<()> {
    if username.is_empty() {
        return Err(anyhow!("username is empty"));
    }

    let response = client()?
        .post(endpoint()?)
        .header(
            reqwest::header::HeaderName::from_static("prefer"),
            "resolution=merge-duplicates,return=minimal",
        )
        .json(&ConfigUpsert {
            username,
            config_json: config,
        })
        .send()
        .await
        .context("failed to send config to Supabase")?;

    response
        .error_for_status()
        .context("Supabase rejected config upsert")?;
    Ok(())
}

/// Loads the settings row whose username exactly matches the supplied username.
pub async fn load_config(username: &str) -> Result<AppSettings> {
    if username.is_empty() {
        return Err(anyhow!("username is empty"));
    }

    let response = client()?
        .get(endpoint()?)
        .query(&[("username", format!("eq={username}")), ("limit", "1".to_string())])
        .send()
        .await
        .context("failed to request config from Supabase")?;

    if response.status() == StatusCode::NOT_FOUND {
        return Err(anyhow!("Supabase config endpoint was not found"));
    }

    let rows: Vec<ConfigRow> = response
        .error_for_status()
        .context("Supabase rejected config load")?
        .json()
        .await
        .context("failed to deserialize Supabase config response")?;

    rows.into_iter()
        .next()
        .map(|row| row.config_json)
        .ok_or_else(|| anyhow!("no cloud config found for username {username}"))
}
