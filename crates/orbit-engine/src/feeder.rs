//! Data source module - supports CSV/JSON data-driven testing
//!
//!  CSV file data source with sequential/random/shuffle modes

use std::collections::HashMap;
use std::path::Path;

/// Data source error
#[derive(Debug, thiserror::Error)]
pub enum FeederError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("CSV parse error: {0}")]
    Csv(String),
    #[error("Column not found: {0}")]
    ColumnNotFound(String),
}

/// Data source mode
#[derive(Debug, Clone)]
pub enum FeederMode {
    /// Read sequentially (wrapping around)
    Sequential,
    /// Read randomly
    Random,
    /// Shuffle once, then read sequentially
    Shuffle,
}

/// CSV data source
pub struct CsvFeeder {
    rows: Vec<HashMap<String, String>>,
    mode: FeederMode,
    index: usize,
}

impl CsvFeeder {
    /// Load from a CSV file
    pub fn from_file(path: &Path, mode: FeederMode) -> Result<Self, FeederError> {
        let content = std::fs::read_to_string(path).map_err(|e| FeederError::Io(e.to_string()))?;
        Self::from_str(&content, mode)
    }

    /// Load from a CSV string
    pub fn from_str(csv: &str, mode: FeederMode) -> Result<Self, FeederError> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_reader(csv.as_bytes());

        let headers: Vec<String> = reader
            .headers()
            .map_err(|e| FeederError::Csv(e.to_string()))?
            .iter()
            .map(|h| h.to_string())
            .collect();

        let mut rows = Vec::new();
        for result in reader.records() {
            let record = result.map_err(|e| FeederError::Csv(e.to_string()))?;
            let mut row = HashMap::new();
            for (i, value) in record.iter().enumerate() {
                if i < headers.len() {
                    row.insert(headers[i].clone(), value.to_string());
                }
            }
            rows.push(row);
        }

        if rows.is_empty() {
            return Err(FeederError::Csv("CSV file is empty".to_string()));
        }

        let mut feeder = Self {
            rows,
            mode: mode.clone(),
            index: 0,
        };
        if matches!(mode, FeederMode::Shuffle) {
            feeder.shuffle();
        }
        Ok(feeder)
    }

    /// Get the next row
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> HashMap<String, String> {
        if self.rows.is_empty() {
            return HashMap::new();
        }

        match self.mode {
            FeederMode::Sequential | FeederMode::Shuffle => {
                let row = self.rows[self.index].clone();
                self.index = (self.index + 1) % self.rows.len();
                row
            }
            FeederMode::Random => {
                use std::time::SystemTime;
                let nanos = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .subsec_nanos();
                let idx = nanos as usize % self.rows.len();
                self.rows[idx].clone()
            }
        }
    }

    /// Get the total number of rows
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn shuffle(&mut self) {
        use std::time::SystemTime;
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64;
        // Simple Fisher-Yates shuffle with time-based seed
        let len = self.rows.len();
        let mut rng_state = seed;
        for i in (1..len).rev() {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng_state as usize) % (i + 1);
            self.rows.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_feeder_sequential() {
        let csv = "username,password\nalice,pass1\nbob,pass2\ncarol,pass3";
        let mut feeder = CsvFeeder::from_str(csv, FeederMode::Sequential).unwrap();
        assert_eq!(feeder.len(), 3);

        let row1 = feeder.next();
        assert_eq!(row1.get("username").unwrap(), "alice");

        let row2 = feeder.next();
        assert_eq!(row2.get("username").unwrap(), "bob");

        // Wrap around to the first row
        let row3 = feeder.next();
        assert_eq!(row3.get("username").unwrap(), "carol");

        let row4 = feeder.next();
        assert_eq!(row4.get("username").unwrap(), "alice");
    }

    #[test]
    fn test_csv_feeder_shuffle() {
        let csv = "id,value\n1,a\n2,b\n3,c\n4,d\n5,e";
        let mut feeder = CsvFeeder::from_str(csv, FeederMode::Shuffle).unwrap();
        assert_eq!(feeder.len(), 5);
        // Still iterates normally after shuffling
        let row = feeder.next();
        assert!(row.contains_key("id"));
    }
}
