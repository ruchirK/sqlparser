use crate::ast::ParsedDateTime;
use crate::parser::{DateTimeField, ParserError};

pub(crate) fn tokenize_interval(
    value: &str,
    include_timezone: bool,
) -> Result<(Vec<IntervalToken>, Vec<IntervalToken>), ParserError> {
    let mut toks = vec![];
    let mut num_buf = String::with_capacity(4);
    fn parse_num(n: &str, idx: usize, is_fraction: bool) -> Result<IntervalToken, ParserError> {
        // TODO need to check if n is empty
        if is_fraction == true {
            let raw: u32 = n.parse().map_err(|e| {
                ParserError::ParserError(format!("couldn't parse fraction of second {}: {}", n, e))
            })?;
            // this is guaranteed to be ascii, so len is fine
            let chars = n.len() as u32;
            let multiplicand = 1_000_000_000 / 10_u32.pow(chars);

            return Ok(IntervalToken::Nanos(raw * multiplicand));
        }

        Ok(IntervalToken::Num(n.parse().map_err(|e| {
            ParserError::ParserError(format!(
                "Unable to parse value as a number at index {}: {}",
                idx, e
            ))
        })?))
    };

    let mut is_frac = false;
    let mut after_time_value = false;
    for (i, chr) in value.chars().enumerate() {
        match chr {
            '-' => {
                // TODO abstract away the number handling functionality to a function
                // dashes at the beginning mean make it negative
                if !num_buf.is_empty() {
                    toks.push(parse_num(&num_buf, i, is_frac)?);
                    num_buf.clear();
                }
                toks.push(IntervalToken::Dash);
                is_frac = false;
                // TODO note that this + 'z' can also designate the start of a timezone
            }
            ' ' => {
                toks.push(parse_num(&num_buf, i, is_frac)?);
                num_buf.clear();
                toks.push(IntervalToken::Space);
                is_frac = false;
            }
            ':' => {
                toks.push(parse_num(&num_buf, i, is_frac)?);
                num_buf.clear();
                toks.push(IntervalToken::Colon);
                is_frac = false;
                after_time_value = true;
            }
            '.' => {
                toks.push(parse_num(&num_buf, i, is_frac)?);
                num_buf.clear();
                toks.push(IntervalToken::Dot);
                is_frac = true;
            }
            '+' => {
                // Not sure if I need to do more to deal with the fractional bit
                // TODO push the fractional processing bit to a function
                if include_timezone != true || after_time_value != true {
                    // TODO Not sure if I need to throw this error here
                    return Err(ParserError::TokenizerError(format!(
                        "Invalid character at offset {} in {}: {:?}",
                        i, value, chr
                    )));
                }

                // TODO
                // here I need to get a slice of the string from i..end
                // and send it to a different function to parse the substring
                // for timezone info
                toks.push(parse_num(&num_buf, 0, is_frac)?);
                let timezone_toks = tokenize_timezone(value.get(i..).unwrap_or(""))?;
                return Ok((toks, timezone_toks));
            }
            chr if chr.is_digit(10) => num_buf.push(chr),
            chr => {
                return Err(ParserError::TokenizerError(format!(
                    "Invalid character at offset {} in {}: {:?}",
                    i, value, chr
                )))
            }
        }
    }
    if !num_buf.is_empty() {
        toks.push(parse_num(&num_buf, 0, is_frac)?);
    }
    Ok((toks, vec![]))
}

/// Get the tokens that you *might* end up parsing starting with a most significant unit
///
/// For example, parsing `INTERVAL '9-5 4:3' MONTH` is *illegal*, but you
/// should interpret that as `9 months 5 days 4 hours 3 minutes`. This function
/// doesn't take any perspective on what things should be, it just teslls you
/// what the user might have meant.
fn potential_interval_tokens(from: &DateTimeField) -> Vec<IntervalToken> {
    use DateTimeField::*;
    use IntervalToken::*;

    let all_toks = [
        Num(0), // year
        Dash,
        Num(0), // month
        Dash,
        Num(0), // day
        Space,
        Num(0), // hour
        Colon,
        Num(0), // minute
        Colon,
        Num(0), // second
        Dot,
        Nanos(0), // Nanos
    ];
    let offset = match from {
        Year => 0,
        Month => 2,
        Day => 4,
        Hour => 6,
        Minute => 8,
        Second => 10,
        // TODO throw an error here
        TimezoneOffsetSecond => 0,
    };
    all_toks[offset..].to_vec()
}

fn potential_timezone_tokens() -> Vec<IntervalToken> {
    use IntervalToken::*;
    let all = [Plus, Num(0), Colon, Num(0)];

    all[..].to_vec()
}

fn tokenize_timezone(value: &str) -> Result<Vec<IntervalToken>, ParserError> {
    let mut toks = vec![];
    let mut num_buf = String::with_capacity(4);
    fn parse_num(n: &str, idx: usize) -> Result<IntervalToken, ParserError> {
        Ok(IntervalToken::Num(n.parse().map_err(|e| {
            ParserError::ParserError(format!(
                "Unable to parse value as a number at index {}: {}",
                idx, e
            ))
        })?))
    };
    for (i, chr) in value.chars().enumerate() {
        match chr {
            '-' => {
                num_buf.clear();
                toks.push(IntervalToken::Dash);
            }
            ' ' => {
                toks.push(parse_num(&num_buf, i)?);
                num_buf.clear();
                toks.push(IntervalToken::Space);
            }
            ':' => {
                toks.push(parse_num(&num_buf, i)?);
                num_buf.clear();
                toks.push(IntervalToken::Colon);
            }
            '+' => {
                num_buf.clear();
                toks.push(IntervalToken::Plus);
            }
            chr if chr.is_digit(10) => num_buf.push(chr),
            chr => {
                return Err(ParserError::TokenizerError(format!(
                    "Invalid character at offset {} in {}: {:?}",
                    i, value, chr
                )))
            }
        }
    }
    if !num_buf.is_empty() {
        toks.push(parse_num(&num_buf, 0)?);
    }
    Ok(toks)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntervalToken {
    Dash,
    Space,
    Colon,
    Dot,
    Plus,
    Num(u64),
    Nanos(u32),
}

pub(crate) fn build_parsed_datetime(
    tokens: &[IntervalToken],
    leading_field: &DateTimeField,
    value: &str,
    timezone_tokens: &[IntervalToken],
) -> Result<ParsedDateTime, ParserError> {
    use IntervalToken::*;

    let expected = potential_interval_tokens(&leading_field);
    let mut actual = tokens.iter().peekable();

    let is_positive = match actual.peek() {
        Some(val) if val == &&IntervalToken::Dash => {
            actual.next();
            false
        }
        _ => true,
    };
    let mut current_field = leading_field.clone();
    let mut pdt = ParsedDateTime {
        is_positive,
        ..Default::default()
    };
    let mut seconds_seen = 0;
    for (i, (atok, etok)) in actual.zip(&expected).enumerate() {
        match (atok, etok) {
            (Dash, Dash) | (Space, Space) | (Colon, Colon) | (Dot, Dot) => {
                /* matching punctuation */
            }
            (Num(val), Num(_)) => {
                let val = *val;
                match current_field {
                    DateTimeField::Year => pdt.year = Some(val),
                    DateTimeField::Month => {
                        if val < 1 {
                            return parser_err!("Invalid Month {} in {}", val, value);
                        }
                        pdt.month = Some(val)
                    }
                    DateTimeField::Day => {
                        if val < 1 {
                            return parser_err!("Invalid Day {} in {}", val, value);
                        }
                        pdt.day = Some(val)
                    }
                    DateTimeField::Hour => pdt.hour = Some(val),
                    DateTimeField::Minute => pdt.minute = Some(val),
                    DateTimeField::Second if seconds_seen == 0 => {
                        seconds_seen += 1;
                        pdt.second = Some(val);
                    }
                    DateTimeField::Second => {
                        return parser_err!("Too many numbers to parse as a second at {}", val)
                    }
                    // TODO fix that
                    DateTimeField::TimezoneOffsetSecond => {
                        pdt.timezone_offset_second = Some(val as i64)
                    }
                }
                if current_field != DateTimeField::Second {
                    current_field = current_field
                        .into_iter()
                        .next()
                        .expect("Exhausted day iterator");
                }
            }
            (Nanos(val), Nanos(_)) if seconds_seen == 1 => pdt.nano = Some(*val),
            (provided, expected) => {
                return parser_err!(
                    "Invalid interval part at offset {}: '{}' provided {:?} but expected {:?}",
                    i,
                    value,
                    provided,
                    expected,
                )
            }
        }
    }

    if timezone_tokens.is_empty() != true {
        let expected = potential_timezone_tokens(); // TODO add a arg for the tz tokens list here to select the right one
        let mut actual = timezone_tokens.iter().peekable();

        let is_positive = match actual.peek() {
            Some(val) if val == &&IntervalToken::Dash => {
                actual.next();
                false
            }
            _ => true,
        };

        let mut hours_seen = false;
        let mut minutes_seen = false;
        let mut tz_offset: i64 = 0;

        for (i, (atok, etok)) in actual.zip(&expected).enumerate() {
            match (atok, etok) {
                (Dash, Dash) | (Space, Space) | (Colon, Colon) | (Dot, Dot) | (Plus, Plus) => {
                    /* matching punctuation */
                }
                (Num(val), Num(_)) => {
                    if hours_seen == false {
                        // TODO validate the range here
                        tz_offset += (val * 60 * 60) as i64;
                        hours_seen = true;
                    } else if minutes_seen == false {
                        tz_offset += (val * 60) as i64;
                        minutes_seen = true;
                    } // add extra error case TODO
                }
                (provided, expected) => {
                    return parser_err!(
                        "Invalid interval time zone part at offset {}: '{}' provided {:?} but expected {:?}",
                        i,
                        value,
                        provided,
                        expected,
                    )
                }
            }
        }

        if is_positive == false {
            tz_offset *= -1;
        }

        pdt.timezone_offset_second = Some(tz_offset);
    }

    Ok(pdt)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::*;

    #[test]
    fn test_potential_interval_tokens() {
        use DateTimeField::*;
        use IntervalToken::*;
        assert_eq!(
            potential_interval_tokens(&Year),
            vec![
                Num(0),
                Dash,
                Num(0),
                Dash,
                Num(0),
                Space,
                Num(0),
                Colon,
                Num(0),
                Colon,
                Num(0),
                Dot,
                Nanos(0),
            ]
        );

        assert_eq!(
            potential_interval_tokens(&Day),
            vec![
                Num(0),
                Space,
                Num(0),
                Colon,
                Num(0),
                Colon,
                Num(0),
                Dot,
                Nanos(0),
            ]
        );
    }
}
