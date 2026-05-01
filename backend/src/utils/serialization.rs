use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Display;
use std::str::FromStr;

/// Specialized serialization for types that should be strings in JSON.
pub fn serialize_as_string<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Display,
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Serializes DateTime<Utc> as a Unix timestamp in milliseconds.
pub fn serialize_dt_as_millis<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_i64(dt.timestamp_millis())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StellarAsset {
    Native,
    AlphaNum4 { code: String, issuer: String },
    AlphaNum12 { code: String, issuer: String },
}

impl Display for StellarAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StellarAsset::Native => write!(f, "native"),
            StellarAsset::AlphaNum4 { code, issuer } => write!(f, "{code}:{issuer}"),
            StellarAsset::AlphaNum12 { code, issuer } => write!(f, "{code}:{issuer}"),
        }
    }
}

impl FromStr for StellarAsset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "native" {
            Ok(StellarAsset::Native)
        } else {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() != 2 {
                return Err("Invalid asset format, expected 'code:issuer'".to_string());
            }
            let code = parts[0].to_string();
            let issuer = parts[1].to_string();
            if code.len() <= 4 {
                Ok(StellarAsset::AlphaNum4 { code, issuer })
            } else {
                Ok(StellarAsset::AlphaNum12 { code, issuer })
            }
        }
    }
}

impl Serialize for StellarAsset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for StellarAsset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        StellarAsset::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[derive(Serialize, Deserialize)]
    struct TestData {
        #[serde(serialize_with = "serialize_as_string")]
        large_int: i128,
        #[serde(
            serialize_with = "serialize_as_string",
            deserialize_with = "deserialize_from_string"
        )]
        amount: Decimal,
        #[serde(serialize_with = "serialize_dt_as_millis")]
        timestamp: DateTime<Utc>,
        asset: StellarAsset,
    }

    fn deserialize_from_string<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: std::fmt::Display,
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        T::from_str(&s).map_err(serde::de::Error::custom)
    }

    #[test]
    fn test_stellar_asset_serialization() {
        let asset = StellarAsset::AlphaNum4 {
            code: "USDC".to_string(),
            issuer: "GBBD...".to_string(),
        };
        let data = TestData {
            large_int: 123456789012345678901234567890_i128,
            amount: dec!(100.50),
            timestamp: Utc::now(),
            asset,
        };

        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"asset\":\"USDC:GBBD...\""));
        assert!(json.contains("\"amount\":\"100.50\""));
        assert!(json.contains("\"large_int\":\"123456789012345678901234567890\""));

        // Verify asset roundtrip independently (it uses string serde natively).
        let asset_json = serde_json::to_string(&data.asset).unwrap();
        let decoded_asset: StellarAsset = serde_json::from_str(&asset_json).unwrap();
        assert_eq!(decoded_asset, data.asset);
    }
}
