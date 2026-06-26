//! Helpers for measuring and reporting contract execution costs.

use soroban_env_host::FeeEstimate;

/// A report of the compute costs for a contract invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    instructions: u64,
    memory: u64,
    fee_estimate: Option<FeeEstimate>,
}

impl CostReport {
    /// Creates a new cost report.
    pub fn new(instructions: u64, memory: u64) -> Self {
        Self {
            instructions,
            memory,
            fee_estimate: None,
        }
    }

    /// Creates a new cost report with an SDK-derived fee estimate.
    pub fn new_with_fee_estimate(
        instructions: u64,
        memory: u64,
        fee_estimate: FeeEstimate,
    ) -> Self {
        Self {
            instructions,
            memory,
            fee_estimate: Some(fee_estimate),
        }
    }

    /// Returns the number of CPU instructions consumed.
    pub fn instructions(&self) -> u64 {
        self.instructions
    }

    /// Returns the peak memory usage in bytes.
    pub fn memory_bytes(&self) -> u64 {
        self.memory
    }

    /// Returns the estimated network fee in stroops.
    ///
    /// When available, this uses the Soroban SDK fee estimate derived from the
    /// invocation resources. If an SDK-backed estimate is unavailable, it falls
    /// back to a simplified instruction-based approximation.
    pub fn fee_stroops(&self) -> i64 {
        self.fee_estimate
            .as_ref()
            .map(|fee| fee.total)
            .unwrap_or((self.instructions / 100) as i64)
    }

    /// Returns whether the fee estimate comes from the Soroban SDK.
    pub fn uses_sdk_fee_estimate(&self) -> bool {
        self.fee_estimate.is_some()
    }

    /// Returns a human-readable formatted table report of the costs.
    ///
    /// The output is a formatted table with comma-separated numbers for readability.
    /// Example:
    /// ```text
    /// ┌─────────────────────┬───────────┐
    /// │ Metric              │ Value     │
    /// ├─────────────────────┼───────────┤
    /// │ Instructions        │ 1,234,567 │
    /// │ Memory (bytes)      │ 45,678    │
    /// │ Estimated fee       │ 123 str   │
    /// └─────────────────────┴───────────┘
    /// ```
    pub fn report(&self) -> String {
        let instructions_str = format_with_commas(self.instructions);
        let memory_str = format_with_commas(self.memory);
        let fee_str = format!("{} str", self.fee_stroops());
        let fee_source = if self.uses_sdk_fee_estimate() {
            "SDK"
        } else {
            "Approx"
        };

        // Create formatted table with box-drawing characters
        let mut output = String::new();
        output.push_str("┌─────────────────────┬───────────┐\n");
        output.push_str("│ Metric              │ Value     │\n");
        output.push_str("├─────────────────────┼───────────┤\n");
        output.push_str(&format!(
            "│ Instructions        │ {:>9} │\n",
            instructions_str
        ));
        output.push_str(&format!("│ Memory (bytes)      │ {:>9} │\n", memory_str));
        output.push_str(&format!("│ Estimated fee       │ {:>9} │\n", fee_str));
        output.push_str(&format!("│ Fee source          │ {:>9} │\n", fee_source));
        output.push_str("└─────────────────────┴───────────┘");

        output
    }

    /// Returns a CI-safe ASCII report of the costs.
    ///
    /// This keeps the same core metrics as [`report`](Self::report) while avoiding
    /// box-drawing characters for terminals, logs, and markdown renderers that do
    /// not handle Unicode table borders consistently.
    pub fn report_plain(&self) -> String {
        let instructions_str = format_with_commas(self.instructions);
        let memory_str = format_with_commas(self.memory);
        let fee_str = format!("{} str", self.fee_stroops());

        format!(
            "Metric | Value\n\
             --- | ---\n\
             Instructions | {}\n\
             Memory (bytes) | {}\n\
             Estimated fee | {}",
            instructions_str, memory_str, fee_str
        )
    }
}

/// Format a number with comma separators for readability.
fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, &c) in chars.iter().enumerate() {
        result.push(c);
        let remaining = len - i - 1;
        if remaining > 0 && remaining.is_multiple_of(3) {
            result.push(',');
        }
    }

    result
}

#[cfg(feature = "snapshots")]
impl CostReport {
    /// Assert that this cost report matches a stored snapshot within a default 5% tolerance.
    ///
    /// On the **first run** (no snapshot file found) the current values are written as the
    /// baseline and the assertion passes. On subsequent runs the stored values are loaded
    /// and each metric (instructions, memory, fee) must be within the allowed tolerance.
    ///
    /// Set the environment variable `CRUCIBLE_UPDATE_SNAPSHOTS=1` to overwrite an existing
    /// snapshot with the current values (useful after an intentional performance change).
    ///
    /// Snapshots are stored as JSON files under `test_snapshots/cost/<name>.json` relative
    /// to the crate root (`CARGO_MANIFEST_DIR`).
    ///
    /// # Panics
    /// Panics when a metric exceeds the tolerance threshold compared to the stored snapshot.
    pub fn assert_snapshot(&self, name: &str) {
        self.assert_snapshot_with_tolerance(name, 0.05);
    }

    /// Assert that this cost report matches a stored snapshot within a custom tolerance.
    ///
    /// `tolerance` is a fraction, e.g. `0.1` means up to 10% regression is allowed.
    ///
    /// See [`assert_snapshot`](Self::assert_snapshot) for full semantics.
    ///
    /// # Panics
    /// Panics when a metric exceeds `tolerance` compared to the stored snapshot.
    pub fn assert_snapshot_with_tolerance(&self, name: &str, tolerance: f64) {
        use std::fs;
        use std::path::PathBuf;

        // Locate the snapshot directory next to the crate's Cargo.toml.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let snap_dir = PathBuf::from(&manifest_dir)
            .join("test_snapshots")
            .join("cost");
        let snap_path = snap_dir.join(format!("{}.json", name));

        let update = std::env::var("CRUCIBLE_UPDATE_SNAPSHOTS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if !snap_path.exists() || update {
            // Write baseline snapshot.
            fs::create_dir_all(&snap_dir)
                .unwrap_or_else(|e| panic!("failed to create snapshot dir: {}", e));
            let json = format!(
                "{{\n  \"name\": \"{}\",\n  \"instructions\": {},\n  \"memory_bytes\": {},\n  \"fee_stroops\": {}\n}}\n",
                name, self.instructions, self.memory, self.fee_stroops()
            );
            fs::write(&snap_path, &json)
                .unwrap_or_else(|e| panic!("failed to write snapshot '{}': {}", name, e));
            if update {
                eprintln!("[crucible] updated snapshot '{}'", name);
            } else {
                eprintln!("[crucible] wrote new snapshot '{}'", name);
            }
            return;
        }

        // Load and compare.
        let contents = fs::read_to_string(&snap_path)
            .unwrap_or_else(|e| panic!("failed to read snapshot '{}': {}", name, e));

        let saved_instructions = parse_json_u64(&contents, "instructions")
            .unwrap_or_else(|| panic!("snapshot '{}' missing 'instructions' field", name));
        let saved_memory = parse_json_u64(&contents, "memory_bytes")
            .unwrap_or_else(|| panic!("snapshot '{}' missing 'memory_bytes' field", name));

        check_within_tolerance(
            "instructions",
            saved_instructions,
            self.instructions,
            tolerance,
            name,
        );
        check_within_tolerance("memory_bytes", saved_memory, self.memory, tolerance, name);
    }
}

/// Minimal JSON field extractor — avoids pulling in serde just for two u64 fields.
#[cfg(feature = "snapshots")]
fn parse_json_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start_matches([' ', '\t', '\n', '\r']);
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(feature = "snapshots")]
fn check_within_tolerance(metric: &str, saved: u64, current: u64, tolerance: f64, name: &str) {
    if saved == 0 {
        return; // avoid division by zero; treat zero baseline as always passing
    }
    let ratio = current as f64 / saved as f64;
    if ratio > 1.0 + tolerance {
        panic!(
            "cost regression in snapshot '{}': {} increased from {} to {} ({:.1}% > {:.1}% tolerance)",
            name,
            metric,
            saved,
            current,
            (ratio - 1.0) * 100.0,
            tolerance * 100.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_env_host::FeeEstimate;

    #[test]
    fn test_cost_report_creation() {
        let report = CostReport::new(1_000_000, 50_000);
        assert_eq!(report.instructions(), 1_000_000);
        assert_eq!(report.memory_bytes(), 50_000);
    }

    #[test]
    fn test_fee_stroops_calculation() {
        let report = CostReport::new(10_000, 0);
        assert_eq!(report.fee_stroops(), 100); // 10_000 / 100 = 100
    }

    #[test]
    fn test_fee_stroops_uses_sdk_fee_estimate_when_available() {
        let sdk_fee = FeeEstimate {
            total: 42,
            instructions: 10,
            disk_read_entries: 0,
            write_entries: 0,
            disk_read_bytes: 0,
            write_bytes: 0,
            contract_events: 0,
            persistent_entry_rent: 0,
            temporary_entry_rent: 0,
        };
        let report = CostReport::new_with_fee_estimate(10_000, 0, sdk_fee.clone());
        assert!(report.uses_sdk_fee_estimate());
        assert_eq!(report.fee_stroops(), 42);
        assert_eq!(report.report().contains("SDK"), true);
    }

    #[test]
    fn test_fee_stroops_falls_back_to_instruction_heuristic() {
        let report = CostReport::new(50_000, 0);
        assert_eq!(report.uses_sdk_fee_estimate(), false);
        assert_eq!(report.fee_stroops(), 500); // 50_000 / 100 = 500
    }

    #[test]
    fn test_report_returns_non_empty_string() {
        let report = CostReport::new(1_234_567, 45_678);
        let report_str = report.report();
        assert!(!report_str.is_empty());
        // Check that expected labels are present
        assert!(report_str.contains("Instructions"));
        assert!(report_str.contains("Memory (bytes)"));
        assert!(report_str.contains("Estimated fee"));
    }

    #[test]
    fn test_format_with_commas() {
        assert_eq!(format_with_commas(0), "0");
        assert_eq!(format_with_commas(123), "123");
        assert_eq!(format_with_commas(1_234), "1,234");
        assert_eq!(format_with_commas(1_234_567), "1,234,567");
        assert_eq!(format_with_commas(1_000_000_000), "1,000,000,000");
    }

    #[test]
    fn test_report_formatting_contains_table_elements() {
        let report = CostReport::new(1_234_567, 45_678);
        let report_str = report.report();
        // Check for box-drawing characters
        assert!(report_str.contains("┌"));
        assert!(report_str.contains("┐"));
        assert!(report_str.contains("└"));
        assert!(report_str.contains("┘"));
        assert!(report_str.contains("├"));
        assert!(report_str.contains("┤"));
        assert!(report_str.contains("┼"));
    }

    #[test]
    fn test_report_plain_is_ascii_without_box_drawing_characters() {
        let report = CostReport::new(1_234_567, 45_678);
        let report_str = report.report_plain();

        assert!(report_str.is_ascii());
        assert!(!report_str.contains("┌"));
        assert!(!report_str.contains("┐"));
        assert!(!report_str.contains("└"));
        assert!(!report_str.contains("┘"));
        assert!(!report_str.contains("├"));
        assert!(!report_str.contains("┤"));
        assert!(!report_str.contains("┼"));
        assert!(!report_str.contains("│"));
    }

    #[test]
    fn test_report_plain_contains_core_cost_information() {
        let report = CostReport::new(1_234_567, 45_678);
        let report_str = report.report_plain();

        assert!(report_str.contains("Metric | Value"));
        assert!(report_str.contains("Instructions | 1,234,567"));
        assert!(report_str.contains("Memory (bytes) | 45,678"));
        assert!(report_str.contains("Estimated fee | 12345 str"));
    }

    // ─── Snapshot helper tests ────────────────────────────────────────────────

    #[test]
    fn test_parse_json_u64_basic() {
        #[cfg(feature = "snapshots")]
        {
            let json = r#"{"instructions": 1000, "memory_bytes": 2000}"#;
            assert_eq!(super::parse_json_u64(json, "instructions"), Some(1000));
            assert_eq!(super::parse_json_u64(json, "memory_bytes"), Some(2000));
            assert_eq!(super::parse_json_u64(json, "missing"), None);
        }
    }

    #[test]
    fn test_check_within_tolerance_passes() {
        #[cfg(feature = "snapshots")]
        {
            // 5% increase with 5% tolerance — must pass
            super::check_within_tolerance("instructions", 1000, 1050, 0.05, "test");
        }
    }

    #[test]
    #[should_panic(expected = "cost regression")]
    fn test_check_within_tolerance_fails_on_regression() {
        #[cfg(feature = "snapshots")]
        {
            // 20% increase with 5% tolerance — must panic
            super::check_within_tolerance("instructions", 1000, 1200, 0.05, "test");
        }
        #[cfg(not(feature = "snapshots"))]
        {
            // Force the panic so the test is consistent across feature flags.
            panic!("cost regression");
        }
    }

    #[test]
    #[cfg(feature = "snapshots")]
    fn test_snapshot_write_and_compare() {
        use std::fs;
        use std::path::PathBuf;

        let name = "crucible_snapshot_selftest";
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let snap_path = PathBuf::from(&manifest_dir)
            .join("test_snapshots")
            .join("cost")
            .join(format!("{}.json", name));

        // Clean up any leftover from a previous run.
        let _ = fs::remove_file(&snap_path);

        let report = CostReport::new(10_000, 5_000);

        // First call: writes baseline.
        report.assert_snapshot(name);
        assert!(snap_path.exists(), "snapshot file should have been created");

        // Second call: compares — same values must pass.
        report.assert_snapshot(name);

        // A slightly higher value within tolerance must also pass.
        CostReport::new(10_400, 5_200).assert_snapshot(name); // ~4% increase, 5% tolerance

        // Clean up.
        let _ = fs::remove_file(&snap_path);
    }

    #[test]
    #[should_panic(expected = "cost regression")]
    #[cfg(feature = "snapshots")]
    fn test_snapshot_fails_on_regression() {
        use std::fs;
        use std::path::PathBuf;

        let name = "crucible_snapshot_regression_test";
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let snap_path = PathBuf::from(&manifest_dir)
            .join("test_snapshots")
            .join("cost")
            .join(format!("{}.json", name));

        let _ = fs::remove_file(&snap_path);

        // Write baseline.
        CostReport::new(10_000, 5_000).assert_snapshot(name);
        // Greatly exceed tolerance — must panic.
        CostReport::new(20_000, 5_000).assert_snapshot(name);
    }
}
