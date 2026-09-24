//! Export context

use orbit_metrics::MetricSample;

/// Export context: carries the non-summary inputs needed for export (test name, raw samples, ...)
#[derive(Debug, Clone, Default)]
pub struct ExportContext<'a> {
    test_name: Option<&'a str>,
    samples: Option<&'a [MetricSample]>,
}

impl<'a> ExportContext<'a> {
    /// Empty context
    pub fn new() -> Self {
        Self {
            test_name: None,
            samples: None,
        }
    }

    /// Set the test name
    pub fn with_test_name(mut self, name: &'a str) -> Self {
        self.test_name = Some(name);
        self
    }

    /// Set the raw samples
    pub fn with_samples(mut self, samples: &'a [MetricSample]) -> Self {
        self.samples = Some(samples);
        self
    }

    /// Test name (defaults to "Load Test")
    pub fn test_name(&self) -> &str {
        self.test_name.unwrap_or("Load Test")
    }

    /// Raw samples (may be empty)
    pub fn samples(&self) -> Option<&'a [MetricSample]> {
        self.samples
    }
}
