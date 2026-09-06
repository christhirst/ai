use chrono::{Datelike, Days, Months, NaiveDate};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalType {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateIntervalStep {
    pub timeframe_label: String,
    pub start_date: String,
    pub end_date: String,
}

/// Parses an interval string into IntervalType.
/// Supports "daily" / "dayly", "weekly", "monthly", "yearly" (case-insensitive).
pub fn parse_interval(raw: &str) -> Result<IntervalType, String> {
    match raw.trim().to_lowercase().as_str() {
        "daily" | "dayly" | "day" => Ok(IntervalType::Daily),
        "weekly" | "week" => Ok(IntervalType::Weekly),
        "monthly" | "month" => Ok(IntervalType::Monthly),
        "yearly" | "year" | "annually" | "annual" => Ok(IntervalType::Yearly),
        other => Err(format!(
            "Unsupported interval '{other}'. Supported intervals: 'daily', 'weekly', 'monthly', 'yearly'."
        )),
    }
}

/// Parses flexible date string formats: "YYYY", "YYYY-MM", or "YYYY-MM-DD".
/// If `is_end` is true, returns the last valid day of that period (e.g. "2000" -> 2000-12-31, "2000-02" -> 2000-02-29).
pub fn parse_flexible_date(raw: &str, is_end: bool) -> Result<NaiveDate, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("Date string cannot be empty".to_string());
    }

    // Check YYYY-MM-DD
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d);
    }

    // Check YYYY-MM
    let ym_re = Regex::new(r"^(\d{4})-(\d{1,2})$").map_err(|e| e.to_string())?;
    if let Some(caps) = ym_re.captures(s) {
        let year: i32 = caps[1].parse().map_err(|e| format!("Invalid year: {e}"))?;
        let month: u32 = caps[2].parse().map_err(|e| format!("Invalid month: {e}"))?;
        if !(1..=12).contains(&month) {
            return Err(format!("Invalid month {month} in date '{s}'"));
        }
        if !is_end {
            return NaiveDate::from_ymd_opt(year, month, 1)
                .ok_or_else(|| format!("Invalid date '{s}'"));
        } else {
            // Find last day of month
            let next_month = if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
            }
            .ok_or_else(|| format!("Invalid next month calculation for '{s}'"))?;
            return next_month
                .checked_sub_days(Days::new(1))
                .ok_or_else(|| format!("Invalid end of month for '{s}'"));
        }
    }

    // Check YYYY
    let y_re = Regex::new(r"^(\d{4})$").map_err(|e| e.to_string())?;
    if let Some(caps) = y_re.captures(s) {
        let year: i32 = caps[1].parse().map_err(|e| format!("Invalid year: {e}"))?;
        if !is_end {
            return NaiveDate::from_ymd_opt(year, 1, 1)
                .ok_or_else(|| format!("Invalid year '{s}'"));
        } else {
            return NaiveDate::from_ymd_opt(year, 12, 31)
                .ok_or_else(|| format!("Invalid year '{s}'"));
        }
    }

    Err(format!(
        "Unable to parse date '{s}'. Supported formats: 'YYYY', 'YYYY-MM', 'YYYY-MM-DD'."
    ))
}

/// Generates a list of date intervals between start and end date inclusive.
pub fn generate_interval_steps(
    interval: IntervalType,
    start_str: &str,
    end_str: &str,
) -> Result<Vec<DateIntervalStep>, String> {
    let start_date = parse_flexible_date(start_str, false)?;
    let end_date = parse_flexible_date(end_str, true)?;

    if start_date > end_date {
        return Err(format!(
            "start_date ({}) cannot be after end_date ({})",
            start_date.format("%Y-%m-%d"),
            end_date.format("%Y-%m-%d")
        ));
    }

    let mut steps = Vec::new();

    match interval {
        IntervalType::Daily => {
            let mut curr = start_date;
            while curr <= end_date {
                let date_str = curr.format("%Y-%m-%d").to_string();
                steps.push(DateIntervalStep {
                    timeframe_label: date_str.clone(),
                    start_date: date_str.clone(),
                    end_date: date_str,
                });
                curr = match curr.checked_add_days(Days::new(1)) {
                    Some(d) => d,
                    None => break,
                };
            }
        }
        IntervalType::Weekly => {
            let mut curr = start_date;
            while curr <= end_date {
                let step_end = match curr.checked_add_days(Days::new(6)) {
                    Some(d) => {
                        if d > end_date {
                            end_date
                        } else {
                            d
                        }
                    }
                    None => end_date,
                };

                let start_s = curr.format("%Y-%m-%d").to_string();
                let end_s = step_end.format("%Y-%m-%d").to_string();
                let label = format!("{start_s} to {end_s}");

                steps.push(DateIntervalStep {
                    timeframe_label: label,
                    start_date: start_s,
                    end_date: end_s,
                });

                curr = match curr.checked_add_days(Days::new(7)) {
                    Some(d) => d,
                    None => break,
                };
            }
        }
        IntervalType::Monthly => {
            let mut curr_month_start = NaiveDate::from_ymd_opt(start_date.year(), start_date.month(), 1)
                .ok_or_else(|| "Failed to construct month start date".to_string())?;

            while curr_month_start <= end_date {
                let next_month_start = match curr_month_start.checked_add_months(Months::new(1)) {
                    Some(d) => d,
                    None => break,
                };
                let month_end = match next_month_start.checked_sub_days(Days::new(1)) {
                    Some(d) => d,
                    None => break,
                };

                let step_start = if curr_month_start < start_date {
                    start_date
                } else {
                    curr_month_start
                };
                let step_end = if month_end > end_date {
                    end_date
                } else {
                    month_end
                };

                let label = curr_month_start.format("%Y-%m").to_string();

                steps.push(DateIntervalStep {
                    timeframe_label: label,
                    start_date: step_start.format("%Y-%m-%d").to_string(),
                    end_date: step_end.format("%Y-%m-%d").to_string(),
                });

                curr_month_start = next_month_start;
            }
        }
        IntervalType::Yearly => {
            let mut curr_year = start_date.year();
            let end_year = end_date.year();

            while curr_year <= end_year {
                let year_start = NaiveDate::from_ymd_opt(curr_year, 1, 1)
                    .ok_or_else(|| "Failed to construct year start".to_string())?;
                let year_end = NaiveDate::from_ymd_opt(curr_year, 12, 31)
                    .ok_or_else(|| "Failed to construct year end".to_string())?;

                let step_start = if year_start < start_date {
                    start_date
                } else {
                    year_start
                };
                let step_end = if year_end > end_date {
                    end_date
                } else {
                    year_end
                };

                let label = format!("{curr_year}");

                steps.push(DateIntervalStep {
                    timeframe_label: label,
                    start_date: step_start.format("%Y-%m-%d").to_string(),
                    end_date: step_end.format("%Y-%m-%d").to_string(),
                });

                curr_year += 1;
            }
        }
    }

    Ok(steps)
}

/// Dynamically injects or replaces the timeframe in a natural language prompt template.
///
/// 1. Replaces explicit placeholders: `{{timeframe}}` or `{{date}}`.
/// 2. If present, replaces lines matching `- Timeframe: "..."` with `- Timeframe: "{timeframe}"`.
/// 3. If "### Target Task:" header is present, appends `- Timeframe: "{timeframe}"` under it.
/// 4. Otherwise, prepends `- Timeframe: "{timeframe}"\n\n` to the prompt.
pub fn inject_timeframe_into_prompt(template: &str, timeframe: &str) -> String {
    if template.contains("{{timeframe}}") {
        return template.replace("{{timeframe}}", timeframe);
    }
    if template.contains("{{date}}") {
        return template.replace("{{date}}", timeframe);
    }

    let re_quoted = Regex::new(r#"(?im)^(\s*-?\s*timeframe\s*:\s*)"[^"]*""#).unwrap();
    if re_quoted.is_match(template) {
        return re_quoted
            .replace(template, format!("${{1}}\"{timeframe}\""))
            .to_string();
    }

    let re_unquoted = Regex::new(r#"(?im)^(\s*-?\s*timeframe\s*:\s*)[^\r\n]+"#).unwrap();
    if re_unquoted.is_match(template) {
        return re_unquoted
            .replace(template, format!("${{1}}\"{timeframe}\""))
            .to_string();
    }

    // Insert under "### Target Task:" if present
    if let Some(idx) = template.find("### Target Task:") {
        let after_header = idx + "### Target Task:".len();
        let mut result = template[..after_header].to_string();
        result.push_str(&format!("\n- Timeframe: \"{timeframe}\""));
        result.push_str(&template[after_header..]);
        return result;
    }

    format!("- Timeframe: \"{timeframe}\"\n\n{template}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval_aliases() {
        assert_eq!(parse_interval("daily").unwrap(), IntervalType::Daily);
        assert_eq!(parse_interval("Dayly").unwrap(), IntervalType::Daily);
        assert_eq!(parse_interval("WEEKLY").unwrap(), IntervalType::Weekly);
        assert_eq!(parse_interval("monthly").unwrap(), IntervalType::Monthly);
        assert_eq!(parse_interval("yearly").unwrap(), IntervalType::Yearly);
        assert_eq!(parse_interval("annual").unwrap(), IntervalType::Yearly);
        assert!(parse_interval("hourly").is_err());
    }

    #[test]
    fn test_parse_flexible_date() {
        assert_eq!(
            parse_flexible_date("2000-02-15", false).unwrap(),
            NaiveDate::from_ymd_opt(2000, 2, 15).unwrap()
        );
        assert_eq!(
            parse_flexible_date("2000-02", false).unwrap(),
            NaiveDate::from_ymd_opt(2000, 2, 1).unwrap()
        );
        // Leap year end of February 2000 is 29
        assert_eq!(
            parse_flexible_date("2000-02", true).unwrap(),
            NaiveDate::from_ymd_opt(2000, 2, 29).unwrap()
        );
        // Non-leap year end of February 2001 is 28
        assert_eq!(
            parse_flexible_date("2001-02", true).unwrap(),
            NaiveDate::from_ymd_opt(2001, 2, 28).unwrap()
        );
        // Year parsing
        assert_eq!(
            parse_flexible_date("2000", false).unwrap(),
            NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()
        );
        assert_eq!(
            parse_flexible_date("2000", true).unwrap(),
            NaiveDate::from_ymd_opt(2000, 12, 31).unwrap()
        );
    }

    #[test]
    fn test_generate_monthly_interval_steps() {
        let steps = generate_interval_steps(IntervalType::Monthly, "2000-01", "2000-03").unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].timeframe_label, "2000-01");
        assert_eq!(steps[0].start_date, "2000-01-01");
        assert_eq!(steps[0].end_date, "2000-01-31");

        assert_eq!(steps[1].timeframe_label, "2000-02");
        assert_eq!(steps[1].start_date, "2000-02-01");
        assert_eq!(steps[1].end_date, "2000-02-29"); // leap year

        assert_eq!(steps[2].timeframe_label, "2000-03");
        assert_eq!(steps[2].start_date, "2000-03-01");
        assert_eq!(steps[2].end_date, "2000-03-31");
    }

    #[test]
    fn test_generate_weekly_interval_steps() {
        let steps =
            generate_interval_steps(IntervalType::Weekly, "2000-01-01", "2000-01-16").unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].start_date, "2000-01-01");
        assert_eq!(steps[0].end_date, "2000-01-07");
        assert_eq!(steps[1].start_date, "2000-01-08");
        assert_eq!(steps[1].end_date, "2000-01-14");
        assert_eq!(steps[2].start_date, "2000-01-15");
        assert_eq!(steps[2].end_date, "2000-01-16");
    }

    #[test]
    fn test_generate_daily_interval_steps() {
        let steps =
            generate_interval_steps(IntervalType::Daily, "2000-02-28", "2000-03-01").unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].timeframe_label, "2000-02-28");
        assert_eq!(steps[1].timeframe_label, "2000-02-29");
        assert_eq!(steps[2].timeframe_label, "2000-03-01");
    }

    #[test]
    fn test_generate_yearly_interval_steps() {
        let steps = generate_interval_steps(IntervalType::Yearly, "1999", "2001").unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].timeframe_label, "1999");
        assert_eq!(steps[0].start_date, "1999-01-01");
        assert_eq!(steps[0].end_date, "1999-12-31");
        assert_eq!(steps[1].timeframe_label, "2000");
        assert_eq!(steps[2].timeframe_label, "2001");
    }

    #[test]
    fn test_inject_timeframe_into_prompt_placeholder() {
        let template = "Task:\n- Location: \"Germany\"\n- Timeframe: \"{{timeframe}}\"\n- Scope: Crime";
        let res = inject_timeframe_into_prompt(template, "2000-05");
        assert!(res.contains("- Timeframe: \"2000-05\""));
        assert!(!res.contains("{{timeframe}}"));
    }

    #[test]
    fn test_inject_timeframe_into_prompt_replaces_existing() {
        let template = "### Target Task:\n- Location: \"Germany\"\n- Timeframe: \"2000-02\"\n- Scope: Crime";
        let res = inject_timeframe_into_prompt(template, "2000-06");
        assert!(res.contains("- Timeframe: \"2000-06\""));
        assert!(!res.contains("2000-02"));
    }

    #[test]
    fn test_inject_timeframe_into_prompt_inserts_under_header() {
        let template = "### Target Task:\n- Location: \"Germany\"\n- Scope: Crime";
        let res = inject_timeframe_into_prompt(template, "2000-07");
        assert!(res.contains("### Target Task:\n- Timeframe: \"2000-07\"\n- Location: \"Germany\""));
    }

    #[test]
    fn test_inject_timeframe_into_prompt_prepends_fallback() {
        let template = "Please find crime news articles.";
        let res = inject_timeframe_into_prompt(template, "2000-08");
        assert_eq!(res, "- Timeframe: \"2000-08\"\n\nPlease find crime news articles.");
    }
}
