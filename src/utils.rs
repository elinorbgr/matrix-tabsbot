pub fn parse_amount(txt: &str) -> Option<i32> {
    let mut splits = txt.split('.').map(|s| (s.len(), s.parse::<i32>()));
    match (splits.next(), splits.next(), splits.next()) {
        (Some((_, Ok(units))), Some((d, Ok(mut cents))), None) => {
            if d > 2 {
                return None;
            } else if d == 1 {
                cents *= 10;
            }
            if units < 0 {
                return None;
            }
            Some(units * 100 + cents)
        }
        (Some((_, Ok(units))), None, None) => if units >= 0 {
            Some(units * 100)
        } else {
            None
        },
        _ => None,
    }
}

pub fn format_amount(amount: i32) -> String {
    let (sign, amount) = if amount < 0 {
        ("-", -amount)
    } else {
        ("", amount)
    };
    let units = amount / 100;
    let cents = amount % 100;
    format!(
        "{}{}.{}{}",
        sign,
        units,
        if cents < 10 { "0" } else { "" },
        cents
    )
}