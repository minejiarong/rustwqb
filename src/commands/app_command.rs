use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum AppCommand {
    Catch {
        alpha_id: String,
    },
    Backtest {
        expr: String,
    },
    BacktestsClear,
    AlphasClear,
    GenerateStart {
        model: String,
        batch: usize,
        interval_sec: u64,
        region: Option<String>,
        universe: Option<String>,
        delay: Option<i32>,
        sample_size: usize,
        auto_backtest: bool,
    },
    GenerateOnce {
        model: String,
        batch: usize,
        region: Option<String>,
        universe: Option<String>,
        delay: Option<i32>,
        sample_size: usize,
        auto_backtest: bool,
    },
    GenerateStop,
    GetDetail {
        expr: String,
    },
    Help,
    Quit,
    FieldsSync,
    FieldStats,
    FieldSample {
        region: Option<String>,
        universe: Option<String>,
        delay: Option<i32>,
        n: usize,
    },
    Unknown(String),
}

impl FromStr for AppCommand {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(AppCommand::Unknown("".to_string()));
        }

        match parts[0] {
            "alpha" | "alphas" => {
                if parts.get(1) == Some(&"clear") {
                    Ok(AppCommand::AlphasClear)
                } else {
                    Ok(AppCommand::Unknown("用法: alphas clear".to_string()))
                }
            }
            "fields" => {
                if parts.get(1) == Some(&"sync") {
                    Ok(AppCommand::FieldsSync)
                } else if parts.get(1) == Some(&"stats") {
                    Ok(AppCommand::FieldStats)
                } else if parts.get(1) == Some(&"sample") {
                    let region = parts.get(2).map(|s| s.to_string());
                    let universe = parts.get(3).map(|s| s.to_string());
                    let delay = parts.get(4).and_then(|s| s.parse::<i32>().ok());
                    let n = parts
                        .get(5)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(300);
                    Ok(AppCommand::FieldSample {
                        region,
                        universe,
                        delay,
                        n,
                    })
                } else {
                    Ok(AppCommand::Unknown("用法: fields sync | fields stats | fields sample [region] [universe] [delay] [n]".to_string()))
                }
            }
            "catch" => {
                if let Some(id) = parts.get(1) {
                    Ok(AppCommand::Catch {
                        alpha_id: id.to_string(),
                    })
                } else {
                    Ok(AppCommand::Unknown("用法: catch <alpha_id>".to_string()))
                }
            }
            "backtest" => {
                if parts.get(1) == Some(&"clear") {
                    Ok(AppCommand::BacktestsClear)
                } else {
                    let expr = parts[1..].join(" ");
                    if !expr.is_empty() {
                        Ok(AppCommand::Backtest { expr })
                    } else {
                        Ok(AppCommand::Unknown("用法: backtest <expr> | backtest clear".to_string()))
                    }
                }
            }
            "generate" => {
                match parts.get(1).map(|s| *s) {
                    Some("stop") => Ok(AppCommand::GenerateStop),
                    Some("loop") => {
                        let n = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                        let interval_sec = parts
                            .get(3)
                            .and_then(|s| parse_interval_seconds(s))
                            .unwrap_or(5);
                        let provider = std::env::var("LLM_PROVIDER")
                            .unwrap_or_else(|_| "openrouter".to_string())
                            .to_lowercase();
                        let mut idx = 4usize;
                        let mut model = if provider == "cerebras" {
                            "llama-3.3-70b".to_string()
                        } else {
                            "deepseek/deepseek-r1".to_string()
                        };
                        if let Some(tok) = parts.get(idx) {
                            let t = tok.to_string();
                            if !is_region_code(&t) {
                                model = t;
                                idx += 1;
                            }
                        }
                        let region = parts.get(idx).map(|s| s.to_string());
                        let universe = parts.get(idx + 1).map(|s| s.to_string());
                        let delay = parts.get(idx + 2).and_then(|s| s.parse::<i32>().ok());
                        let sample_size = parts
                            .get(idx + 3)
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(300);
                        let auto_backtest = parts
                            .get(idx + 4)
                            .map(|s| s.to_ascii_lowercase())
                            .map(|s| matches!(s.as_str(), "1" | "true" | "yes" | "on" | "bt" | "backtest"))
                            .unwrap_or(true);
                        Ok(AppCommand::GenerateStart {
                            model,
                            batch: n,
                            interval_sec,
                            region,
                            universe,
                            delay,
                            sample_size,
                            auto_backtest,
                        })
                    }
                    Some("once") => {
                        let n = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
                        let provider = std::env::var("LLM_PROVIDER")
                            .unwrap_or_else(|_| "openrouter".to_string())
                            .to_lowercase();
                        let mut idx = 3usize;
                        let mut model = if provider == "cerebras" {
                            "llama-3.3-70b".to_string()
                        } else {
                            "deepseek/deepseek-r1".to_string()
                        };
                        if let Some(tok) = parts.get(idx) {
                            let t = tok.to_string();
                            if !is_region_code(&t) {
                                model = t;
                                idx += 1;
                            }
                        }
                        let region = parts.get(idx).map(|s| s.to_string());
                        let universe = parts.get(idx + 1).map(|s| s.to_string());
                        let delay = parts.get(idx + 2).and_then(|s| s.parse::<i32>().ok());
                        let sample_size = parts
                            .get(idx + 3)
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(300);
                        let auto_backtest = parts
                            .get(idx + 4)
                            .map(|s| s.to_ascii_lowercase())
                            .map(|s| matches!(s.as_str(), "1" | "true" | "yes" | "on" | "bt" | "backtest"))
                            .unwrap_or(true);
                        Ok(AppCommand::GenerateOnce {
                            model,
                            batch: n,
                            region,
                            universe,
                            delay,
                            sample_size,
                            auto_backtest,
                        })
                    }
                    Some(n_str) => Ok(AppCommand::Unknown(format!("未知的 generate 子命令: {}", n_str))),
                    None => Ok(AppCommand::Unknown("用法: generate loop <n> <sec> [model] [region] [universe] [delay] [sample_size] [auto_backtest] | generate once <n> [model] [region] [universe] [delay] [sample_size] [auto_backtest] | generate stop".to_string())),
                }
            }
            "__INTERNAL_GET_DETAIL__" => {
                let expr = parts[1..].join(" ");
                Ok(AppCommand::GetDetail { expr })
            }
            "help" | "h" => Ok(AppCommand::Help),
            "quit" | "q" | "exit" => Ok(AppCommand::Quit),
            _ => Ok(AppCommand::Unknown(format!("未知命令: {}", parts[0]))),
        }
    }
}

fn is_region_code(s: &str) -> bool {
    s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase())
}

fn parse_interval_seconds(s: &str) -> Option<u64> {
    let raw = s.trim();
    if raw.is_empty() {
        return None;
    }
    let t = raw.to_ascii_lowercase();
    if let Ok(v) = t.parse::<u64>() {
        return Some(v);
    }

    let parse_num = |x: &str| x.trim().parse::<u64>().ok();

    for (suffix, mul) in [
        ("s", 1u64),
        ("sec", 1u64),
        ("secs", 1u64),
        ("m", 60u64),
        ("min", 60u64),
        ("mins", 60u64),
        ("minute", 60u64),
        ("minutes", 60u64),
        ("h", 3600u64),
        ("hr", 3600u64),
        ("hrs", 3600u64),
        ("hour", 3600u64),
        ("hours", 3600u64),
    ] {
        if let Some(prefix) = t.strip_suffix(suffix) {
            if let Some(v) = parse_num(prefix) {
                return v.checked_mul(mul);
            }
        }
    }

    None
}
