//! `orbit compare` — compare two load test results (plain percentage and statistical regression checks)

use std::path::Path;
use std::path::PathBuf;

use crate::util::separator;

/// Read and validate load test JSON (the output of orbit run --output json); fails when key fields are missing
fn load_summary(path: &Path, keys: &[&str]) -> anyhow::Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        anyhow::anyhow!(
            "{} is not valid JSON (generate it with `orbit run --output json`): {}",
            path.display(),
            e
        )
    })?;
    for key in keys {
        if !v.get(*key).and_then(|x| x.as_f64()).is_some() {
            anyhow::bail!(
                "{} is missing the numeric field '{}' (generate it with `orbit run --output json`)",
                path.display(),
                key
            );
        }
    }
    Ok(v)
}

fn pct_change(baseline: f64, current: f64) -> f64 {
    if baseline > 0.0 {
        ((current - baseline) / baseline) * 100.0
    } else {
        0.0
    }
}

pub fn compare_command(
    baseline: PathBuf,
    current: PathBuf,
    max_regression: f64,
) -> anyhow::Result<()> {
    let base = load_summary(&baseline, &["p95_ms", "rps", "error_rate"])?;
    let curr = load_summary(&current, &["p95_ms", "rps", "error_rate"])?;

    let bp95 = base["p95_ms"].as_f64().unwrap();
    let cp95 = curr["p95_ms"].as_f64().unwrap();
    let brps = base["rps"].as_f64().unwrap();
    let crps = curr["rps"].as_f64().unwrap();
    let berr = base["error_rate"].as_f64().unwrap();
    let cerr = curr["error_rate"].as_f64().unwrap();

    println!("📊 Load Test Comparison");
    println!("  Baseline: {}", baseline.display());
    println!("  Current:  {}", current.display());
    println!("{}", separator());
    println!(
        "  p95:  {:.1}ms → {:.1}ms ({:+.1}%)",
        bp95,
        cp95,
        pct_change(bp95, cp95)
    );
    println!(
        "  RPS:  {:.1} → {:.1} ({:+.1}%)",
        brps,
        crps,
        pct_change(brps, crps)
    );
    println!("  Err:  {:.2}% → {:.2}%", berr * 100.0, cerr * 100.0);

    if pct_change(bp95, cp95) > max_regression {
        println!(
            "
⚠️  REGRESSION DETECTED: p95 increased by {:.1}% (threshold: {:.0}%)",
            pct_change(bp95, cp95),
            max_regression
        );
        std::process::exit(99);
    } else {
        println!(
            "
✅ No significant regression detected."
        );
    }

    Ok(())
}

pub fn regression_check(
    baseline: PathBuf,
    current: PathBuf,
    max_zscore: f64,
) -> anyhow::Result<()> {
    let base = load_summary(&baseline, &["p95_ms", "p99_ms"])?;
    let curr = load_summary(&current, &["p95_ms", "p99_ms"])?;

    let bp95 = base["p95_ms"].as_f64().unwrap();
    let cp95 = curr["p95_ms"].as_f64().unwrap();
    let bp99 = base["p99_ms"].as_f64().unwrap();
    let cp99 = curr["p99_ms"].as_f64().unwrap();

    let p95_change = pct_change(bp95, cp95);
    let p99_change = pct_change(bp99, cp99);

    println!("📈 Statistical Regression Check");
    println!("  p95: {:.1}ms → {:.1}ms ({:+.1}%)", bp95, cp95, p95_change);
    println!("  p99: {:.1}ms → {:.1}ms ({:+.1}%)", bp99, cp99, p99_change);
    println!("  Threshold: ±{:.0}%", max_zscore);

    if p95_change > max_zscore || p99_change > max_zscore {
        println!(
            "
⚠️  Statistical regression detected! Latency increased beyond {:.0}% threshold.",
            max_zscore
        );
        std::process::exit(99);
    } else {
        println!(
            "
✅ No statistical regression detected."
        );
    }

    Ok(())
}
