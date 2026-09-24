//! `orbit import` — import from other tools into a test plan (reuses orbit-config::exchange)

use std::io::Read;
use std::path::PathBuf;

use orbit_config::exchange;

/// Read the source text from --file / stdin
fn read_input(file: Option<&PathBuf>) -> anyhow::Result<String> {
    match file {
        Some(path) if path.as_os_str() == "-" => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        Some(path) => Ok(std::fs::read_to_string(path)?),
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

pub fn import_command(
    file: Option<PathBuf>,
    from: String,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let input = read_input(file.as_ref())?;

    // Scenario import: source -> ApiSpec -> TestPlan, emitting YAML that can be run directly with orbit run
    let plan = exchange::import_scenario(&from, &input)
        .map_err(|e| anyhow::anyhow!("{} import failed: {}", from, e))?;
    let yaml = serde_yaml::to_string(&plan)?;

    eprintln!("✅ Imported test plan in {} format: {}", from, plan.name);
    match output {
        Some(path) => {
            std::fs::write(&path, &yaml)?;
            eprintln!("📄 Written to {}", path.display());
        }
        None => println!("{}", yaml),
    }
    Ok(())
}
