//! `orbit history` — show recent request history (reads the orbit-data snapshot, same source as the app data)

use crate::util::data_file;

pub fn history_command(limit: usize) -> anyhow::Result<()> {
    let path = data_file();
    if !path.exists() {
        println!("No run history found. Run a test first with: orbit run <file>");
        return Ok(());
    }
    let content = std::fs::read_to_string(&path)?;
    let snap: orbit_data::Snapshot = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse data file {}: {}", path.display(), e))?;
    let entries = &snap.data.history;
    if entries.is_empty() {
        println!("No run history found.");
        return Ok(());
    }

    println!("Recent runs ({} total):", entries.len().min(limit));
    for (i, entry) in entries.iter().take(limit).enumerate() {
        let status = entry
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".to_string());
        let duration = entry
            .duration
            .map(|d| format!("{}ms", d))
            .unwrap_or_else(|| "-".to_string());
        let size = entry
            .size
            .map(|b| {
                if b >= 1024 * 1024 {
                    format!("{:.1}MB", b as f64 / (1024.0 * 1024.0))
                } else if b >= 1024 {
                    format!("{:.1}KB", b as f64 / 1024.0)
                } else {
                    format!("{}B", b)
                }
            })
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  {}. {} | {} {} | {} | {} | {}",
            i + 1,
            entry.name,
            entry.method,
            entry.url,
            status,
            duration,
            size
        );
    }
    Ok(())
}
