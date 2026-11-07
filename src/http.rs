//! Port of `HttpMarshal`: GET with 5 retries, `b64-gzip` decoding, file downloads with progress.

use std::io::{Read, Write};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()
}

pub fn get_string(url: &str, headers: &[(&str, &str)]) -> Result<String> {
    let mut last_err = anyhow!("GET failed! URL: {url}\nBad behaviour");
    for _ in 0..5 {
        match try_get_string(url, headers) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = anyhow!("GET failed! URL: {url}\n{e:#}"),
        }
    }
    Err(last_err)
}

fn try_get_string(url: &str, headers: &[(&str, &str)]) -> Result<String> {
    let mut req = agent(Duration::from_secs(90)).get(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let resp = req.call()?;
    let b64_gzip = resp
        .header("Content-Encoding")
        .map(|v| v.split(',').any(|p| p.trim() == "b64-gzip"))
        .unwrap_or(false);
    let body = resp.into_string()?;

    if b64_gzip && !body.is_empty() {
        let raw = base64::engine::general_purpose::STANDARD.decode(body.trim())?;
        if raw.len() < 4 {
            return Err(anyhow!("invalid b64-gzip payload"));
        }
        let len = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        let mut out = Vec::with_capacity(len);
        flate2::read::GzDecoder::new(&raw[4..]).take(len as u64).read_to_end(&mut out)?;
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    Ok(body)
}

pub fn get_json<T: serde::de::DeserializeOwned>(url: &str, headers: &[(&str, &str)]) -> Result<T> {
    let text = get_string(url, headers)?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON from {url}"))
}

/// Downloads `url` to `local_path`, reporting integer percentages through `progress`.
/// `progress` may return an error to abort the download (used for cancellation).
pub fn get_file(url: &str, local_path: &str, mut progress: impl FnMut(i32) -> Result<()>) -> Result<()> {
    let mut last_err = anyhow!("GET failed! URL: {url}");
    for attempt in 1..=5 {
        match try_get_file(url, local_path, &mut progress) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if e.is::<Cancelled>() {
                    return Err(e);
                }
                last_err = anyhow!("GET failed! URL: {url}\n{e:#}");
                if attempt < 5 {
                    progress(0)?;
                }
            }
        }
    }
    Err(last_err)
}

fn try_get_file(url: &str, local_path: &str, progress: &mut impl FnMut(i32) -> Result<()>) -> Result<()> {
    let resp = agent(Duration::from_secs(5 * 3600)).get(url).call()?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let mut reader = resp.into_reader();
    let mut file = std::io::BufWriter::new(std::fs::File::create(local_path)?);
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let mut last = -1;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if let Some(total) = total.filter(|t| *t > 0) {
            let p = ((done as f64 / total as f64) * 100.0) as i32;
            if p != last {
                last = p;
                progress(p)?;
            }
        }
    }
    file.flush()?;
    if let Some(total) = total
        && done != total {
            return Err(anyhow!("connection closed after {done} of {total} bytes"));
        }
    Ok(())
}

/// Error used to abort long-running operations.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}
