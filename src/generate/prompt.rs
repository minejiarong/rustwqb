use crate::generate::context::OperatorCatalog;
use regex::Regex;

pub struct PromptBuilder {
    operators: OperatorCatalog,
}

impl PromptBuilder {
    pub fn new(operators: OperatorCatalog) -> Self {
        Self { operators }
    }

    pub fn build(&self, n: usize) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Generate {n} unique alpha factor expressions for WorldQuant BRAIN FASTEXPR."
        ));
        lines.push("Return ONLY the expressions, one per line.".to_string());
        lines.push(
            "Each line MUST start with 'ALPHA_EXPR:' followed by the expression.".to_string(),
        );
        lines.push("No markdown, no explanations.".to_string());
        lines.push("Do NOT include any curly braces {} or annotations.".to_string());
        lines.push("Do NOT append trailing markers like {CR}, {…}, comments or metadata.".to_string());
        lines.push("".to_string());
        lines.push(
            "Example format (use placeholders; do NOT reuse placeholders as real fields):"
                .to_string(),
        );
        lines.push("ALPHA_EXPR:ts_rank([FIELD], 20)".to_string());
        lines.push("ALPHA_EXPR:group_zscore(ts_mean([FIELD], 10), [GROUP_FIELD])".to_string());
        lines.push("".to_string());

        if !self.operators.by_category.is_empty() {
            lines.push("Operators (compact hints):".to_string());
            for (cat, list) in &self.operators.by_category {
                let mut line = String::new();
                line.push_str(cat);
                line.push_str(": ");
                let mut first = true;
                for op in list.iter().take(20) {
                    if is_banned(&op.name) {
                        continue;
                    }
                    let mut item = String::new();
                    item.push_str(&op.name);
                    let sig = op
                        .definition
                        .as_ref()
                        .map(|d| compact_signature(d))
                        .or_else(|| op.op_type.clone())
                        .unwrap_or_default();
                    if !sig.is_empty() {
                        item.push_str("(");
                        item.push_str(&sig);
                        item.push_str(")");
                    }
                    if let Some(scope) = &op.scope {
                        let abbr = scope_abbr(scope);
                        if !abbr.is_empty() {
                            item.push_str("{");
                            item.push_str(&abbr);
                            item.push_str("}");
                        }
                    }
                    if let Some(level) = &op.level {
                        if !level.is_empty() && level != "ALL" {
                            item.push_str("[");
                            item.push_str(level);
                            item.push(']');
                        }
                    }
                    if let Some(desc) = &op.description {
                        let short = smart_truncate(desc, 64);
                        if !short.is_empty() {
                            item.push_str(": ");
                            item.push_str(&short);
                        }
                    }
                    if !first {
                        line.push_str(" | ");
                    }
                    first = false;
                    if (line.len() + item.len()) > 400 {
                        break;
                    }
                    line.push_str(&item);
                }
                lines.push(line);
            }
            lines.push("".to_string());
        }

        lines.push("STRICT COMPLEXITY GUIDELINES:".to_string());
        lines.push("1. 每个表达式必须至少使用 3 个运算符，且覆盖≥2类（如 ts_* + group_* + arithmetic/logical）。".to_string());
        lines.push("2. 每个表达式必须引用≥2个不同的数据字段（不要只用同一个字段）。".to_string());
        lines.push("3. 至少包含一个时间序列运算符（ts_*）并提供正整数lookback，以及一个分组运算符（group_*）。".to_string());
        lines.push("4. 优先使用嵌套组合：例如 group_neutralize(ts_rank(FIELD_ID, 30) - ts_mean(OTHER_FIELD_ID, 20), GROUP_FIELD)。".to_string());
        lines.push(
            "5. 避免简单形式（单一运算符、统一的极小lookback如1、或重复相同模板）。".to_string(),
        );
        lines.push("6. 尽量混合使用低频字段以提升多样性。".to_string());

        lines.join("\n")
    }

    pub fn build_with_fields(
        &self,
        n: usize,
        fields: &[String],
        region: Option<&str>,
        universe: Option<&str>,
        delay: Option<i32>,
    ) -> String {
        self.build_with_field_groups(n, fields, &[] as &[String], region, universe, delay)
    }

    pub fn build_with_field_groups(
        &self,
        n: usize,
        non_event_fields: &[String],
        event_fields: &[String],
        region: Option<&str>,
        universe: Option<&str>,
        delay: Option<i32>,
    ) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Generate {n} unique alpha factor expressions for WorldQuant BRAIN FASTEXPR."
        ));
        lines.push("Return ONLY the expressions, one per line.".to_string());
        lines.push(
            "Each line MUST start with 'ALPHA_EXPR:' followed by the expression.".to_string(),
        );
        lines.push("No markdown, no explanations.".to_string());
        lines.push("Do NOT include any curly braces {} or annotations.".to_string());
        lines.push("Do NOT append trailing markers like {CR}, {…}, comments or metadata.".to_string());
        lines.push("".to_string());

        if region.is_some() || universe.is_some() || delay.is_some() {
            let r = region.unwrap_or("N/A");
            let u = universe.unwrap_or("N/A");
            let d = delay
                .map(|x| x.to_string())
                .unwrap_or_else(|| "N/A".to_string());
            lines.push(format!("Context: region={r}, universe={u}, delay={d}"));
        }

        if !non_event_fields.is_empty() || !event_fields.is_empty() {
            lines.push("Available Fields sample (use real field IDs below):".to_string());

            if !non_event_fields.is_empty() {
                let preview: Vec<&String> = non_event_fields.iter().take(50).collect();
                let joined = preview
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("NON_EVENT: ({})", joined));
            }

            if !event_fields.is_empty() {
                let preview: Vec<&String> = event_fields.iter().take(50).collect();
                let joined = preview
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("EVENT: ({})", joined));
            }

            lines.push("".to_string());
        }

        lines.push("Example format (use provided fields; avoid placeholders):".to_string());
        lines.push("ALPHA_EXPR:ts_rank(FIELD_ID_HERE, 20)".to_string());
        lines.push(
            "ALPHA_EXPR:group_zscore(ts_mean(FIELD_ID_HERE, 10), GROUP_FIELD_ID)".to_string(),
        );
        lines.push("".to_string());

        if !self.operators.by_category.is_empty() {
            lines.push("Operators (compact hints):".to_string());
            for (cat, list) in &self.operators.by_category {
                let mut line = String::new();
                line.push_str(cat);
                line.push_str(": ");
                let mut first = true;
                for op in list.iter().take(20) {
                    if is_banned(&op.name) {
                        continue;
                    }
                    let mut item = String::new();
                    item.push_str(&op.name);
                    let sig = op
                        .definition
                        .as_ref()
                        .map(|d| compact_signature(d))
                        .or_else(|| op.op_type.clone())
                        .unwrap_or_default();
                    if !sig.is_empty() {
                        item.push_str("(");
                        item.push_str(&sig);
                        item.push_str(")");
                    }
                    if let Some(scope) = &op.scope {
                        let abbr = scope_abbr(scope);
                        if !abbr.is_empty() {
                            item.push_str("{");
                            item.push_str(&abbr);
                            item.push_str("}");
                        }
                    }
                    if let Some(level) = &op.level {
                        if !level.is_empty() && level != "ALL" {
                            item.push_str("[");
                            item.push_str(level);
                            item.push(']');
                        }
                    }
                    if let Some(desc) = &op.description {
                        let short = smart_truncate(desc, 64);
                        if !short.is_empty() {
                            item.push_str(": ");
                            item.push_str(&short);
                        }
                    }
                    if !first {
                        line.push_str(" | ");
                    }
                    first = false;
                    if (line.len() + item.len()) > 400 {
                        break;
                    }
                    line.push_str(&item);
                }
                lines.push(line);
            }
            lines.push("".to_string());
        }

        lines.join("\n")
    }
}

fn is_banned(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "reduce_ir"
        || n == "reduce_avg"
        || n == "reduce_max"
        || n == "reduce_sum"
        || n == "reduce_min"
}

fn scope_abbr(scope: &[String]) -> String {
    let mut s = String::new();
    for v in scope {
        match v.as_str() {
            "COMBO" => s.push('C'),
            "REGULAR" => s.push('R'),
            "SELECTION" => s.push('S'),
            _ => {}
        }
    }
    s
}

fn smart_truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    if let Some(pos) = s[..max].rfind([' ', ',', '；', ';', '。', '.']) {
        end = pos;
    }
    s[..end].trim().to_string()
}

fn compact_signature(def: &str) -> String {
    let re_fn = Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)").unwrap();
    if let Some(caps) = re_fn.captures(def) {
        let args_raw = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let mut args = args_raw.replace(" ", "");
        let re_filter = Regex::new(r"filter=([A-Za-z0-9_]+)").unwrap();
        args = re_filter.replace_all(&args, "filter").to_string();
        return args;
    }
    smart_truncate(def, 48)
}
