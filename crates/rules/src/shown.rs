//! How a page shows a rule's value; shared by `build.rs`, which renders the
//! book's prose with it, and the crate, whose pages quote values.

/// A value with its unit, as prose quotes it: `×4`, `95%`, `250 ms`.
pub fn with_unit(value: &str, unit: &str) -> String {
    match unit {
        "" => value.to_string(),
        "×" => format!("×{value}"),
        "%" | "°" | "°/s" => format!("{value}{unit}"),
        _ => format!("{value} {unit}"),
    }
}

/// A literal as a page shows it: underscores out (a minus as the games'
/// descriptions write it, `-4`).
pub fn literal(literal: &str) -> String {
    let plain = literal.replace('_', "");
    match plain.strip_suffix(".0") {
        Some(whole) => whole.to_string(),
        None => plain,
    }
}

/// A value said as the games' own descriptions say it, by `how`: `m` (JA's
/// units, or units a second, in meters: 40 units ≈ 1 meter), `s` (milli-
/// seconds in seconds), `pct` (thousandths as a percent), `fp` (hundredths
/// of a Force point in Force points), `can` (a yes or no as `can` or
/// `cannot`); its words after it (`6.5 meters`, `1 second`, `50%`). A small
/// amount keeps a place more, so none reads as nothing. An error for a unit
/// `how` does not convert.
pub fn spoken(value: &str, unit: &str, how: &str) -> Result<String, String> {
    if how == "can" {
        return match value {
            "true" => Ok("can".to_string()),
            "false" => Ok("cannot".to_string()),
            _ => Err(format!("`:can` says a yes or no, not `{value}`")),
        };
    }
    let number = value
        .parse::<f64>()
        .map_err(|_| format!("`{value}` is not a number"))?;
    let (amount, one, many) = match (how, unit) {
        ("m", "units") => (number * 0.0254, " meter", " meters"),
        ("m", "units/s") => (number * 0.0254, " meter per second", " meters per second"),
        ("s", "ms") => (number / 1000.0, " second", " seconds"),
        ("pct", "‰") => (number / 10.0, "%", "%"),
        ("fp", "hundredths FP") => (number / 100.0, " Force point", " Force points"),
        _ => return Err(format!("`:{how}` does not say a value in `{unit}`")),
    };
    let places = match amount.abs() {
        size if size == 0.0 || size >= 10.0 => 1,
        size if size >= 1.0 => 1,
        size if size >= 0.1 => 2,
        _ => 3,
    };
    let factor = 10f64.powi(places);
    let rounded = (amount * factor).round() / factor;
    let mut text = format!("{rounded:.*}", places as usize);
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    let words = if rounded == 1.0 { one } else { many };
    Ok(format!("{text}{words}"))
}

/// A value as a page shows it: as the games say it where its unit converts
/// (`spoken`: meters, seconds, a percent, Force points), else with its unit.
pub fn said(value: &str, unit: &str) -> String {
    let how = match unit {
        "units" | "units/s" => "m",
        "ms" => "s",
        "‰" => "pct",
        "hundredths FP" => "fp",
        _ => "",
    };
    spoken(value, unit, how).unwrap_or_else(|_| with_unit(value, unit))
}
