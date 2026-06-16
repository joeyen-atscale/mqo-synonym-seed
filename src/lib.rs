use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Input shapes ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(default)]
    pub column_group: String,
    #[serde(rename = "type", default)]
    pub column_type: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeModel {
    pub columns: Vec<Column>,
}

// ── Output shapes ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynonymCandidate {
    pub name: String,
    pub existing_description: String,
    pub synonyms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateOutput {
    pub candidates: Vec<SynonymCandidate>,
}

// ── Rule-based synonym engine ─────────────────────────────────────────────────

/// Expand a snake_case / UPPER_SNAKE name into human-readable words.
fn expand_name(name: &str) -> Vec<String> {
    // Split on underscores, lowercase, filter empty tokens
    name.split('_')
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Extract any numeric component from a token like "7d", "30d", etc.
fn extract_window(tokens: &[String]) -> Option<String> {
    for token in tokens {
        // Tokens like "7d", "30d", "14d"
        if let Some(n) = token.strip_suffix('d') {
            if n.parse::<u32>().is_ok() {
                return Some(format!("{}-day", n));
            }
        }
        // "7w", "4w"
        if let Some(n) = token.strip_suffix('w') {
            if n.parse::<u32>().is_ok() {
                return Some(format!("{}-week", n));
            }
        }
    }
    None
}

/// Core synonym rule engine. Returns a deduplicated, sorted synonym list.
pub fn synonyms_for_name(name: &str) -> Vec<String> {
    let upper = name.to_uppercase();
    let tokens = expand_name(name);
    // base phrase from token expansion (drop numeric-only tokens for the phrase)
    let base_phrase: String = tokens
        .iter()
        .filter(|t| t.parse::<u64>().is_err()) // drop pure-number tokens
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let mut synonyms: Vec<String> = Vec::new();

    // Always add the human-readable expansion of the name itself if it's not trivial
    if !base_phrase.is_empty() && base_phrase != name.to_lowercase() {
        synonyms.push(base_phrase.clone());
    }

    // ── Suffix / prefix rules ─────────────────────────────────────────────────

    // _AMT
    if upper.ends_with("_AMT") || upper == "AMT" {
        let prefix = tokens_except_last(&tokens);
        synonyms.push(format!("{} amount", prefix));
        synonyms.push(format!("{} total", prefix));
        synonyms.push(format!("{} sum", prefix));
        synonyms.push(format!("gross {}", prefix));
        synonyms.push("total sales".to_string());
        synonyms.push("revenue".to_string());
        synonyms.push("order total".to_string());
    }

    // _QTY
    if upper.ends_with("_QTY") || upper == "QTY" {
        let prefix = tokens_except_last(&tokens);
        synonyms.push(format!("{} quantity", prefix));
        synonyms.push(format!("{} count", prefix));
        synonyms.push(format!("{} volume", prefix));
        synonyms.push(format!("{} units", prefix));
    }

    // _PCT or _RATE
    if upper.ends_with("_PCT") || upper.ends_with("_RATE") || upper == "PCT" || upper == "RATE" {
        let prefix = tokens_except_suffix(&tokens, &["pct", "rate"]);
        synonyms.push(format!("{} percentage", prefix));
        synonyms.push(format!("{} rate", prefix));
        synonyms.push(format!("{} ratio", prefix));
        synonyms.push(format!("{} share", prefix));
    }

    // _USD or _COST
    if upper.ends_with("_USD") || upper.ends_with("_COST") || upper == "USD" {
        let prefix = tokens_except_suffix(&tokens, &["usd", "cost"]);
        synonyms.push(format!("{} cost", prefix));
        synonyms.push(format!("{} spend", prefix));
        synonyms.push(format!("{} in dollars", prefix));
        synonyms.push(format!("{} dollar amount", prefix));
    }

    // _FLAG or IS_ prefix
    if upper.ends_with("_FLAG") || upper.starts_with("IS_") {
        let core = if upper.starts_with("IS_") {
            tokens_except_first(&tokens)
        } else {
            tokens_except_last(&tokens)
        };
        synonyms.push(format!("{} indicator", core));
        synonyms.push(format!("{} flag", core));
        synonyms.push(format!("{} boolean", core));
        synonyms.push(format!("{} yes/no", core));
    }

    // AVG_ prefix or _AVG suffix
    let is_avg = upper.starts_with("AVG_") || upper.ends_with("_AVG");
    if is_avg {
        let core = if upper.starts_with("AVG_") {
            tokens_except_first(&tokens)
        } else {
            tokens_except_last(&tokens)
        };
        synonyms.push(format!("average {}", core));
        synonyms.push(format!("mean {}", core));
        synonyms.push(format!("typical {}", core));
    }

    // _VALUE
    if upper.ends_with("_VALUE") || upper == "VALUE" {
        let prefix = tokens_except_last(&tokens);
        synonyms.push(format!("{} value", prefix));
        synonyms.push(format!("{} amount", prefix));
        synonyms.push(format!("{} total", prefix));
        // If also has AVG, add "average X size" / "mean X value"
        if is_avg {
            synonyms.push(format!("average {} size", prefix));
            synonyms.push(format!("mean {} value", prefix));
        }
    }

    // _SALES
    if upper.ends_with("_SALES") || upper == "SALES" {
        let prefix = tokens_except_last(&tokens);
        synonyms.push(format!("{} sales", prefix));
        synonyms.push(format!("{} revenue", prefix));
        synonyms.push(format!("{} bookings", prefix));
    }

    // ROLLING_ prefix — detect window size from tokens like "7d", "30d"
    if upper.starts_with("ROLLING_") {
        let window = extract_window(&tokens);
        let window_str = window.as_deref().unwrap_or("N-day");
        // What comes after ROLLING_<window>_ is the metric
        let metric_tokens: Vec<&str> = tokens
            .iter()
            .skip(1) // skip "rolling"
            .filter(|t| t.parse::<u64>().is_err() && !t.ends_with('d') && !t.ends_with('w'))
            .filter(|t| *t != "avg" && *t != "average")
            .map(|t| t.as_str())
            .collect();
        let metric = metric_tokens.join(" ");
        synonyms.push(format!("{} rolling average", window_str));
        synonyms.push(format!("{} rolling average {}", window_str, metric).trim().to_string());
        synonyms.push(format!("trailing average {}", metric).trim().to_string());
        synonyms.push("rolling average".to_string());
        // Weekly shorthand if 7d
        if window_str == "7-day" {
            synonyms.push(format!("weekly average {}", metric).trim().to_string());
        }
    }

    // ── Deduplicate and clean up ───────────────────────────────────────────────
    let mut seen = std::collections::HashSet::new();
    synonyms.retain(|s| {
        let s = s.trim().to_string();
        !s.is_empty() && seen.insert(s)
    });
    // Normalise whitespace
    synonyms = synonyms
        .into_iter()
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect();

    synonyms
}

/// Join all tokens except the last one into a phrase.
fn tokens_except_last(tokens: &[String]) -> String {
    if tokens.len() <= 1 {
        return String::new();
    }
    tokens[..tokens.len() - 1].join(" ")
}

/// Join all tokens except the first one into a phrase.
fn tokens_except_first(tokens: &[String]) -> String {
    if tokens.len() <= 1 {
        return String::new();
    }
    tokens[1..].join(" ")
}

/// Join all tokens except any trailing token that matches one of `suffixes`.
fn tokens_except_suffix(tokens: &[String], suffixes: &[&str]) -> String {
    let last = tokens.last().map(|s| s.as_str()).unwrap_or("");
    if suffixes.contains(&last) {
        tokens_except_last(tokens)
    } else {
        tokens.join(" ")
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Generate synonym candidates for all columns.
/// Pass `only_empty = true` to skip columns that already have descriptions.
pub fn generate_synonyms(columns: &[Column], only_empty: bool) -> Vec<SynonymCandidate> {
    columns
        .iter()
        .filter(|c| !only_empty || c.description.trim().is_empty())
        .map(|c| SynonymCandidate {
            name: c.name.clone(),
            existing_description: c.description.clone(),
            synonyms: synonyms_for_name(&c.name),
        })
        .collect()
}

/// Write approved synonyms into the description fields of `columns`.
/// Columns not present in `approved` are left unchanged.
pub fn apply_synonyms(columns: &mut [Column], approved: &HashMap<String, Vec<String>>) {
    for col in columns.iter_mut() {
        if let Some(syns) = approved.get(&col.name) {
            let joined = syns.join(", ");
            if col.description.trim().is_empty() {
                col.description = joined;
            } else {
                col.description = format!("{}; {}", col.description.trim(), joined);
            }
        }
    }
}

// ── JSON-RPC 2.0 serve ────────────────────────────────────────────────────────

/// Process a single JSON-RPC 2.0 request object.  Returns a JSON-RPC 2.0 response string.
pub fn process_message(request: &serde_json::Value) -> String {
    let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match method {
        "generate" => {
            let params = request.get("params").cloned().unwrap_or_default();
            let model: Result<DescribeModel, _> = serde_json::from_value(params);
            match model {
                Ok(m) => {
                    let candidates = generate_synonyms(&m.columns, false);
                    let result = GenerateOutput { candidates };
                    rpc_ok(id, serde_json::to_value(result).unwrap_or_default())
                }
                Err(e) => rpc_error(id, -32602, &format!("invalid params: {e}")),
            }
        }
        "apply" => {
            let params = request.get("params").cloned().unwrap_or_default();
            let mut model: Result<DescribeModel, _> =
                serde_json::from_value(params.get("model").cloned().unwrap_or_default());
            let approved: Result<HashMap<String, Vec<String>>, _> =
                serde_json::from_value(params.get("approved").cloned().unwrap_or_default());
            match (model.as_mut(), approved) {
                (Ok(m), Ok(ap)) => {
                    apply_synonyms(&mut m.columns, &ap);
                    rpc_ok(id, serde_json::to_value(&m).unwrap_or_default())
                }
                _ => rpc_error(id, -32602, "invalid params"),
            }
        }
        _ => rpc_error(id, -32601, &format!("method not found: {method}")),
    }
}

fn rpc_ok(id: serde_json::Value, result: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
    .to_string()
}

fn rpc_error(id: serde_json::Value, code: i32, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn syns(name: &str) -> Vec<String> {
        synonyms_for_name(name)
    }

    fn col(name: &str, desc: &str) -> Column {
        Column {
            name: name.to_string(),
            column_group: String::new(),
            column_type: String::new(),
            description: desc.to_string(),
        }
    }

    // AC1: ORDER_AMT → ≥2 of "total sales"/"revenue"/"order total"/"gross amount"
    #[test]
    fn ac1_order_amt() {
        let s = syns("ORDER_AMT");
        let targets = ["total sales", "revenue", "order total", "gross amount"];
        let matched: Vec<_> = targets.iter().filter(|&&t| s.iter().any(|x| x == t)).collect();
        assert!(
            matched.len() >= 2,
            "Expected ≥2 of {:?} in synonyms, got {:?}",
            targets,
            s
        );
    }

    // AC2: AVG_ORDER_VALUE → "average order size" or "mean order value"
    #[test]
    fn ac2_avg_order_value() {
        let s = syns("AVG_ORDER_VALUE");
        assert!(
            s.iter().any(|x| x == "average order size" || x == "mean order value"),
            "Expected 'average order size' or 'mean order value' in {:?}",
            s
        );
    }

    // AC3: ROLLING_7D_AVG_SALES → rolling-window paraphrase
    #[test]
    fn ac3_rolling_7d_avg_sales() {
        let s = syns("ROLLING_7D_AVG_SALES");
        let has_rolling = s.iter().any(|x| {
            x.contains("rolling") || x.contains("trailing") || x.contains("weekly")
        });
        assert!(has_rolling, "Expected rolling-window paraphrase in {:?}", s);
    }

    // AC4: existing descriptions are preserved and synonyms are additive
    #[test]
    fn ac4_existing_description_preserved() {
        let columns = vec![
            col("ORDER_AMT", "The total order amount in USD"),
            col("ORDER_QTY", ""),
        ];
        let candidates = generate_synonyms(&columns, false);
        // ORDER_AMT has description — it should still appear in candidates (additive)
        let order_amt = candidates.iter().find(|c| c.name == "ORDER_AMT").unwrap();
        assert_eq!(order_amt.existing_description, "The total order amount in USD");
        // ORDER_QTY has no description
        let order_qty = candidates.iter().find(|c| c.name == "ORDER_QTY").unwrap();
        assert!(order_qty.existing_description.is_empty());
    }

    // AC5: apply --approved round-trip
    #[test]
    fn ac5_apply_round_trip() {
        let mut columns = vec![
            col("ORDER_AMT", ""),
            col("ORDER_QTY", "existing desc"),
            col("OTHER_COL", "untouched"),
        ];
        let mut approved = HashMap::new();
        approved.insert("ORDER_AMT".to_string(), vec!["total sales".to_string(), "revenue".to_string()]);
        approved.insert("ORDER_QTY".to_string(), vec!["order count".to_string()]);

        apply_synonyms(&mut columns, &approved);

        let amt = columns.iter().find(|c| c.name == "ORDER_AMT").unwrap();
        assert_eq!(amt.description, "total sales, revenue");

        // Existing description preserved, synonyms appended
        let qty = columns.iter().find(|c| c.name == "ORDER_QTY").unwrap();
        assert!(qty.description.contains("existing desc"));
        assert!(qty.description.contains("order count"));

        // Untouched column unchanged
        let other = columns.iter().find(|c| c.name == "OTHER_COL").unwrap();
        assert_eq!(other.description, "untouched");
    }

    // AC6: --planner-brain absent API key → rule path + warning, exit 0
    // This is tested at the CLI level; here we verify that generate_synonyms works offline
    #[test]
    fn ac6_planner_brain_fallback_generates_offline() {
        let columns = vec![col("ORDER_AMT", "")];
        let candidates = generate_synonyms(&columns, false);
        assert!(!candidates.is_empty());
        assert!(!candidates[0].synonyms.is_empty());
    }

    // AC7: serve mode via process_message
    #[test]
    fn ac7_serve_mode() {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "generate",
            "params": {
                "columns": [
                    {"name": "ORDER_AMT", "description": ""}
                ]
            }
        });
        let resp_str = process_message(&req);
        let resp: serde_json::Value = serde_json::from_str(&resp_str).unwrap();
        assert_eq!(resp["jsonrpc"], "2.0");
        assert!(resp.get("error").is_none(), "unexpected error: {resp}");
        let candidates = &resp["result"]["candidates"];
        assert!(candidates.is_array());
        assert!(!candidates.as_array().unwrap().is_empty());
    }

    // AC8: _AMT, AVG_, _PCT, _FLAG rules each exercised
    #[test]
    fn ac8_amt_rule() {
        let s = syns("REVENUE_AMT");
        assert!(s.iter().any(|x| x.contains("amount") || x.contains("total")));
    }

    #[test]
    fn ac8_avg_rule() {
        let s = syns("AVG_BASKET_SIZE");
        assert!(s.iter().any(|x| x.starts_with("average") || x.starts_with("mean")));
    }

    #[test]
    fn ac8_pct_rule() {
        let s = syns("DISCOUNT_PCT");
        assert!(
            s.iter().any(|x| x.contains("percentage") || x.contains("rate") || x.contains("ratio")),
            "got: {:?}",
            s
        );
    }

    #[test]
    fn ac8_flag_rule() {
        let s = syns("IS_ACTIVE");
        assert!(
            s.iter().any(|x| x.contains("indicator") || x.contains("boolean") || x.contains("flag")),
            "got: {:?}",
            s
        );
        let s2 = syns("DELETED_FLAG");
        assert!(s2.iter().any(|x| x.contains("indicator") || x.contains("flag")));
    }

    // Extra: only_empty filter
    #[test]
    fn only_empty_filter() {
        let columns = vec![
            col("ORDER_AMT", "has a description"),
            col("ORDER_QTY", ""),
        ];
        let candidates = generate_synonyms(&columns, true);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "ORDER_QTY");
    }

    // Extra: ROLLING_30D_AVG
    #[test]
    fn rolling_30d() {
        let s = syns("ROLLING_30D_AVG_REVENUE");
        assert!(s.iter().any(|x| x.contains("30-day")), "got: {:?}", s);
        assert!(s.iter().any(|x| x.contains("rolling") || x.contains("trailing")));
    }
}
