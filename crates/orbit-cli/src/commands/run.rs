//! `orbit run` — run a test plan (load testing / automation)

use std::path::PathBuf;

use orbit_config::{Executor, TestPlan};
use orbit_engine::Engine;

use crate::util::read_plan;

pub async fn run_test(
    file: PathBuf,
    env: Option<String>,
    vus_override: Option<u32>,
    duration_override: Option<String>,
    output: String,
    out_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    // Parse the output format first: an unknown format fails immediately instead of after the whole load test (None = default text)
    if output != "text" && orbit_output::ExportFormat::parse(&output).is_none() {
        anyhow::bail!(
            "unknown output format: {output} (supported: text|json|csv|html|junit|jtl|raw-json)"
        );
    }
    let out_format = orbit_output::ExportFormat::parse(&output);

    let mut plan = read_plan(&file)?;
    let out_dir = out_dir.unwrap_or_else(|| PathBuf::from("."));
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    // Apply the environment + CLI overrides. Progress goes to stderr so stdout carries the report only
    if let Some(env_name) = &env {
        orbit_config::apply_environment(&mut plan, env_name)?;
    }
    apply_overrides(&mut plan, vus_override, &duration_override);

    eprintln!("🚀 Running test plan: {}", plan.name);
    if let Some(env_name) = &env {
        eprintln!("🌍 Environment: {}", env_name);
    }
    eprintln!();

    let engine = Engine::new();
    let bus = engine.metrics_bus();
    let want_raw = out_format.is_some_and(orbit_output::ExportFormat::needs_samples);
    if want_raw {
        bus.enable_raw_samples();
    }
    let result = engine.run_plan_with_thresholds(&plan).await?;
    let summary = result.summary;
    for t in &result.threshold_results {
        if t.passed {
            eprintln!("✅ {}", t);
        } else {
            eprintln!("❌ {}", t);
        }
    }

    match out_format {
        Some(orbit_output::ExportFormat::Json) | Some(orbit_output::ExportFormat::Csv) => {
            let ctx = orbit_output::ExportContext::new().with_test_name(&plan.name);
            let report = orbit_output::export(
                out_format.expect("the json/csv branch always has a format"),
                &summary,
                &ctx,
            )?;
            // Machine-readable format: print the report body only
            println!("{}", report.content);
        }
        Some(format) => {
            let samples = bus.raw_samples();
            let ctx = orbit_output::ExportContext::new()
                .with_test_name(&plan.name)
                .with_samples(&samples);
            let report = orbit_output::export(format, &summary, &ctx)?;
            let path = out_dir.join(&report.filename);
            std::fs::write(&path, &report.content)?;
            let kind = match format {
                orbit_output::ExportFormat::Junit => "JUnit",
                orbit_output::ExportFormat::Jtl => "JTL",
                orbit_output::ExportFormat::RawJsonl => "Raw JSON",
                _ => "Report",
            };
            eprintln!("📄 {kind} report saved to {}", path.display());
            if matches!(
                format,
                orbit_output::ExportFormat::Junit | orbit_output::ExportFormat::Jtl
            ) && bus.raw_truncated()
            {
                eprintln!(
                    "⚠️  raw samples exceeded the cap and were truncated (only the first {} kept)",
                    samples.len()
                );
            }
        }
        None => {
            // Default text: print the summary to stdout
            println!("{}", summary);
        }
    }

    // Exit code contract: 0=passed; 1=failed requests exist; 99=thresholds not met (for CI)
    if result.threshold_results.iter().any(|t| !t.passed) {
        std::process::exit(99);
    }
    if summary.total_errors > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Apply the CLI --vus / --duration overrides to the test plan
fn apply_overrides(plan: &mut TestPlan, vus: Option<u32>, duration: &Option<String>) {
    if let Some(vus) = vus {
        for scenario in &mut plan.scenarios {
            match &mut scenario.executor {
                Executor::ConstantVus { vus: ref mut v, .. } => {
                    *v = vus;
                }
                Executor::ConstantArrivalRate {
                    pre_allocated_vus: ref mut p,
                    ..
                } => {
                    *p = vus;
                }
                Executor::RampingVus {
                    start_vus: ref mut s,
                    ..
                } => {
                    *s = vus;
                }
                Executor::Sequential { .. } => { /* no-op */ }
            }
        }
    }
    if let Some(d) = duration {
        for scenario in &mut plan.scenarios {
            match &mut scenario.executor {
                Executor::ConstantVus {
                    duration: ref mut dur,
                    ..
                } => {
                    *dur = d.clone();
                }
                Executor::ConstantArrivalRate {
                    duration: ref mut dur,
                    ..
                } => {
                    *dur = d.clone();
                }
                Executor::RampingVus { .. } => {
                    eprintln!(
                        "⚠️  --duration does not apply to the ramping-vus executor (stages control the duration); ignored"
                    );
                }
                Executor::Sequential { .. } => { /* no-op */ }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_config::Executor;

    fn sample_plan() -> TestPlan {
        let yaml = r#"
name: test
scenarios:
  - name: s1
    executor:
      type: constant-vus
      vus: 5
      duration: 10s
    steps: []
"#;
        orbit_config::from_str(yaml).unwrap()
    }

    #[test]
    fn overrides_vus_for_concurrent_executors() {
        let mut plan = sample_plan();
        apply_overrides(&mut plan, Some(50), &None);
        match &plan.scenarios[0].executor {
            Executor::ConstantVus { vus, .. } => assert_eq!(*vus, 50),
            _ => panic!("expected constant-vus"),
        }
    }

    #[test]
    fn overrides_duration_for_time_based_executors() {
        let mut plan = sample_plan();
        apply_overrides(&mut plan, None, &Some("30s".into()));
        match &plan.scenarios[0].executor {
            Executor::ConstantVus { duration, .. } => assert_eq!(duration, "30s"),
            _ => panic!("expected constant-vus"),
        }
    }

    #[test]
    fn no_overrides_keeps_plan_unchanged() {
        let mut plan = sample_plan();
        apply_overrides(&mut plan, None, &None);
        match &plan.scenarios[0].executor {
            Executor::ConstantVus { vus, duration, .. } => {
                assert_eq!(*vus, 5);
                assert_eq!(duration, "10s");
            }
            _ => panic!("expected constant-vus"),
        }
    }
}
