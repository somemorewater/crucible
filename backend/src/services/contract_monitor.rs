#![allow(dead_code)]

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorRule {
    pub id: String,
    pub contract_address: String,
    pub rule_type: String,
    pub threshold: f64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorAlert {
    pub id: String,
    pub rule_id: String,
    pub contract_address: String,
    pub message: String,
    pub severity: String,
    pub triggered_at: chrono::DateTime<Utc>,
    pub resolved: bool,
}

#[derive(Clone)]
pub struct ContractMonitor {
    rules: Arc<RwLock<Vec<MonitorRule>>>,
    alerts: Arc<RwLock<Vec<MonitorAlert>>>,
}

impl ContractMonitor {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(vec![])),
            alerts: Arc::new(RwLock::new(vec![])),
        }
    }

    pub async fn add_rule(
        &self,
        contract_address: &str,
        rule_type: &str,
        threshold: f64,
    ) -> MonitorRule {
        let rule = MonitorRule {
            id: Uuid::new_v4().to_string(),
            contract_address: contract_address.to_string(),
            rule_type: rule_type.to_string(),
            threshold,
            enabled: true,
        };
        self.rules.write().await.push(rule.clone());
        rule
    }

    pub async fn remove_rule(&self, id: &str) -> bool {
        let mut rules = self.rules.write().await;
        let before = rules.len();
        rules.retain(|r| r.id != id);
        rules.len() < before
    }

    pub async fn list_rules(&self, contract_address: Option<&str>) -> Vec<MonitorRule> {
        let rules = self.rules.read().await;
        match contract_address {
            Some(addr) => rules
                .iter()
                .filter(|r| r.contract_address == addr)
                .cloned()
                .collect(),
            None => rules.clone(),
        }
    }

    pub async fn check_and_alert(
        &self,
        contract_address: &str,
        metric_value: f64,
    ) -> Vec<MonitorAlert> {
        let rules = self.rules.read().await;
        let mut new_alerts = Vec::new();

        for rule in rules
            .iter()
            .filter(|r| r.enabled && r.contract_address == contract_address)
        {
            if metric_value > rule.threshold {
                let severity = if metric_value > rule.threshold * 2.0 {
                    "critical"
                } else if metric_value > rule.threshold * 1.5 {
                    "warning"
                } else {
                    "info"
                };
                let alert = MonitorAlert {
                    id: Uuid::new_v4().to_string(),
                    rule_id: rule.id.clone(),
                    contract_address: contract_address.to_string(),
                    message: format!(
                        "Rule '{}' triggered: value {:.2} exceeded threshold {:.2}",
                        rule.rule_type, metric_value, rule.threshold
                    ),
                    severity: severity.to_string(),
                    triggered_at: Utc::now(),
                    resolved: false,
                };
                new_alerts.push(alert);
            }
        }
        drop(rules);

        let mut alerts = self.alerts.write().await;
        alerts.extend(new_alerts.clone());
        new_alerts
    }

    pub async fn list_alerts(
        &self,
        contract_address: Option<&str>,
        resolved: Option<bool>,
    ) -> Vec<MonitorAlert> {
        let alerts = self.alerts.read().await;
        alerts
            .iter()
            .filter(|a| {
                contract_address.map_or(true, |addr| a.contract_address == addr)
                    && resolved.map_or(true, |r| a.resolved == r)
            })
            .cloned()
            .collect()
    }

    pub async fn resolve_alert(&self, id: &str) -> bool {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.resolved = true;
            return true;
        }
        false
    }
}

impl Default for ContractMonitor {
    fn default() -> Self {
        Self::new()
    }
}
