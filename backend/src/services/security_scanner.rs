#![allow(dead_code)]

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub description: String,
    pub line_number: Option<u32>,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    pub contract_name: String,
    pub findings: Vec<SecurityFinding>,
    pub risk_score: f64,
    pub scanned_at: chrono::DateTime<Utc>,
    pub passed: bool,
}

pub struct SecurityScanner {
    #[allow(dead_code)]
    db: PgPool,
}

impl SecurityScanner {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn scan(
        &self,
        contract_name: &str,
        source_code: &str,
    ) -> Result<SecurityReport, sqlx::Error> {
        let mut findings = Vec::new();

        // Check for unsafe blocks
        if source_code.contains("unsafe {") || source_code.contains("unsafe{") {
            findings.push(SecurityFinding {
                id: Uuid::new_v4().to_string(),
                severity: "high".to_string(),
                title: "Unsafe block detected".to_string(),
                description: "The contract contains unsafe Rust code which bypasses memory safety guarantees.".to_string(),
                line_number: source_code.lines().enumerate().find(|(_, l)| l.contains("unsafe")).map(|(i, _)| i as u32 + 1),
                recommendation: "Remove unsafe blocks or document why they are necessary and safe.".to_string(),
            });
        }

        // Check for missing auth on sensitive operations
        let has_transfer = source_code.contains("transfer") || source_code.contains("mint");
        let has_auth =
            source_code.contains("require_auth") || source_code.contains("require_auth_for_args");
        if has_transfer && !has_auth {
            findings.push(SecurityFinding {
                id: Uuid::new_v4().to_string(),
                severity: "medium".to_string(),
                title: "Missing authorization check".to_string(),
                description:
                    "Sensitive operations (transfer/mint) found without require_auth checks."
                        .to_string(),
                line_number: None,
                recommendation:
                    "Add require_auth() or require_auth_for_args() before sensitive operations."
                        .to_string(),
            });
        }

        // Check for potential integer overflow (absence of checked arithmetic)
        let has_arithmetic = source_code.contains(" + ") || source_code.contains(" * ");
        let has_checked = source_code.contains("checked_add")
            || source_code.contains("checked_mul")
            || source_code.contains("saturating_");
        if has_arithmetic && !has_checked {
            findings.push(SecurityFinding {
                id: Uuid::new_v4().to_string(),
                severity: "medium".to_string(),
                title: "Potential integer overflow".to_string(),
                description: "Arithmetic operations found without checked or saturating variants."
                    .to_string(),
                line_number: None,
                recommendation:
                    "Use checked_add, checked_mul, or saturating_add to prevent overflow."
                        .to_string(),
            });
        }

        // Check for hardcoded addresses
        if source_code.contains("G") && source_code.contains("AAAA") {
            findings.push(SecurityFinding {
                id: Uuid::new_v4().to_string(),
                severity: "low".to_string(),
                title: "Hardcoded address detected".to_string(),
                description: "A Stellar address appears to be hardcoded in the contract source."
                    .to_string(),
                line_number: None,
                recommendation: "Store addresses in contract storage or pass them as parameters."
                    .to_string(),
            });
        }

        let risk_score: f64 = findings
            .iter()
            .map(|f| match f.severity.as_str() {
                "critical" => 25.0,
                "high" => 15.0,
                "medium" => 5.0,
                "low" => 1.0,
                _ => 0.0,
            })
            .sum::<f64>()
            .min(100.0);

        Ok(SecurityReport {
            contract_name: contract_name.to_string(),
            findings,
            risk_score,
            scanned_at: Utc::now(),
            passed: risk_score < 50.0,
        })
    }
}
