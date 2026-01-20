use regex::Regex;

pub struct ParsedResult {
    pub exprs: Vec<String>,
    pub total_lines: usize,
    pub rejected_examples: Vec<String>,
}

pub fn sanitize_expression(expr: &str) -> String {
    let re = Regex::new(r"\{[^}]*\}").unwrap();
    let s = re.replace_all(expr, "");
    let s = s.replace('\n', " ");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim().to_string()
}

pub fn parse_alpha_exprs(text: &str) -> ParsedResult {
    let mut out = Vec::new();
    let mut rejected = Vec::new();
    let mut total = 0usize;

    for line in text.lines() {
        total += 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let expr_raw = if let Some(rest) = line.strip_prefix("ALPHA_EXPR:") {
            rest.trim()
        } else {
            line
        };
        let expr = sanitize_expression(expr_raw);

        if expr.len() < 8 {
            if rejected.len() < 5 {
                rejected.push(format!("too_short: {expr}"));
            }
            continue;
        }
        if !expr.contains('(') || !expr.contains(')') {
            if rejected.len() < 5 {
                rejected.push(format!("no_parens: {expr}"));
            }
            continue;
        }
        if !paren_balanced(&expr) {
            if rejected.len() < 5 {
                rejected.push(format!("bad_parens: {expr}"));
            }
            continue;
        }
        if expr.to_ascii_lowercase().contains("reduce_") {
            if rejected.len() < 5 {
                rejected.push(format!("banned_op: {expr}"));
            }
            continue;
        }
        out.push(expr.to_string());
    }

    ParsedResult {
        exprs: out,
        total_lines: total,
        rejected_examples: rejected,
    }
}

fn paren_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}
