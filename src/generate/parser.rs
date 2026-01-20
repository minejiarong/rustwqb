use regex::Regex;

pub struct ParsedResult {
    pub exprs: Vec<String>,
    pub total_lines: usize,
    pub rejected_examples: Vec<String>,
}

pub fn validate_prequeue(expr: &str) -> Result<(), String> {
    let s = expr.trim();
    {
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            if bytes[i] == b')' {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'(' {
                    return Err("unexpected_right_paren".to_string());
                }
            }
            i += 1;
        }
    }
    {
        let bytes = s.as_bytes();
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch == b'(' {
                depth += 1;
            } else if ch == b')' {
                let mut k = i;
                while k > 0 && bytes[k - 1].is_ascii_whitespace() {
                    k -= 1;
                }
                if k > 0 && bytes[k - 1] == b',' {
                    return Err("trailing_comma".to_string());
                }
                depth -= 1;
            }
            i += 1;
        }
    }
    {
        let lower = s.to_ascii_lowercase();
        let mut pos = 0usize;
        loop {
            if let Some(idx) = lower[pos..].find("winsorize(") {
                let start = pos + idx + "winsorize(".len();
                let bytes = s.as_bytes();
                let mut depth = 1i32;
                let mut i = start;
                let mut segs: Vec<(usize, usize)> = Vec::new();
                let mut seg_start = start;
                while i < bytes.len() && depth > 0 {
                    let ch = bytes[i];
                    if ch == b'(' {
                        depth += 1;
                    } else if ch == b')' {
                        depth -= 1;
                        if depth == 0 {
                            segs.push((seg_start, i));
                            break;
                        }
                    } else if ch == b',' && depth == 1 {
                        segs.push((seg_start, i));
                        seg_start = i + 1;
                    }
                    i += 1;
                }
                let mut positional = 0usize;
                for (a, b) in segs {
                    let seg = s[a..b].trim();
                    if seg.is_empty() {
                        continue;
                    }
                    let mut d = 0i32;
                    let mut is_named = false;
                    for ch in seg.chars() {
                        if ch == '(' {
                            d += 1;
                        } else if ch == ')' {
                            d -= 1;
                        } else if ch == '=' && d == 0 {
                            is_named = true;
                            break;
                        }
                    }
                    if !is_named {
                        positional += 1;
                    }
                }
                if positional > 1 {
                    return Err("winsorize_arity".to_string());
                }
                pos = (i + 1).min(lower.len());
            } else {
                break;
            }
        }
    }
    Ok(())
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
