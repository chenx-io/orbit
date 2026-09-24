//! `orbit validate` — validate a test plan

use std::path::PathBuf;

use crate::util::read_plan;

pub fn validate_plan(file: PathBuf) -> anyhow::Result<()> {
    let plan = read_plan(&file)?;
    println!("✅ Valid test plan: {}", plan.name);
    println!("   Scenarios: {}", plan.scenarios.len());
    for (i, scenario) in plan.scenarios.iter().enumerate() {
        println!(
            "   Scenario {}: {} ({} steps)",
            i + 1,
            scenario.name,
            scenario.steps.len()
        );
    }
    Ok(())
}
