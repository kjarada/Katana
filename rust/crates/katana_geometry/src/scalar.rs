pub(crate) fn min(first: f64, second: f64) -> f64 {
    if second < first { second } else { first }
}

pub(crate) fn max(first: f64, second: f64) -> f64 {
    if first < second { second } else { first }
}
