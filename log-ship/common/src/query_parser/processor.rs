use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::num::ParseIntError;
use std::str::ParseBoolError;
use std::time::{Duration as StdDuration};

use chrono::{DateTime, Duration, Local, NaiveDate, Timelike, TimeZone, ParseError, NaiveTime};
use pest::iterators::{Pair};
use thiserror::Error;
use serde::{Serialize};

#[allow(unused_imports)]
use crate::logging::{setup_with_level, FilterLevel, debug, error, info, warn};
use crate::LogValue;

use crate::query_parser::Rule;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error(transparent)]
    PestError(#[from] pest::error::Error<Rule>),

    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),

    #[error(transparent)]
    ParseBoolError(#[from] ParseBoolError),

    #[error(transparent)]
    ParseError(#[from] ParseError),

    #[error("Expected token: {0}")]
    UnexpectedMissingToken(String),

    #[error("Unexpected rule: {0:?}")]
    UnexpectedRule(Rule),

    #[error("Unexpected value: {0}")]
    UnexpectedValue(String),

    #[error("{0}")]
    InvalidTime(String),

    #[error("{0}")]
    MultipleDisplayCommands(String),
}

#[derive(Serialize, Debug)]
pub enum SearchComparator {
    Equal,
    NotEqual,
    Regex,
    GreaterThan,
    LessThan,
    GreaterEqual,
    LessEqual,
}

impl Display for SearchComparator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s: String = self.into();
        write!(f, "{}", s)
    }
}

impl TryFrom<&str> for SearchComparator {
    type Error = SearchError;

    fn try_from(token: &str) -> Result<Self, Self::Error> {
        match token {
            "=" => Ok(SearchComparator::Equal),
            "!=" => Ok(SearchComparator::NotEqual),
            "~=" => Ok(SearchComparator::Regex),
            "<" => Ok(SearchComparator::LessThan),
            "<=" => Ok(SearchComparator::LessEqual),
            ">" => Ok(SearchComparator::GreaterThan),
            ">=" => Ok(SearchComparator::GreaterEqual),
            _ => Err(SearchError::UnexpectedValue(format!("Unknown comparator: {}", token)))
        }
    }
}

impl From<&SearchComparator> for String {
    fn from(sc: &SearchComparator) -> Self {
        match sc {
            SearchComparator::Equal => { "=" }
            SearchComparator::NotEqual => { "!=" }
            SearchComparator::Regex => { "~=" }
            SearchComparator::GreaterThan => { ">" }
            SearchComparator::LessThan => { "<" }
            SearchComparator::GreaterEqual => { ">=" }
            SearchComparator::LessEqual => { "<=" }
        }.to_string()
    }
}

#[derive(Serialize, Debug)]
pub struct SearchCondition {
    pub field: String,
    pub comparator: SearchComparator,
    pub values: Vec<LogValue>
}

impl Display for SearchCondition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let values = self.values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        write!(f, "{} {} [{}]", self.field, self.comparator.to_string(), values)
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Arg {
    Arg(LogValue),
    ArgArray(Vec<LogValue>),
}

impl Display for Arg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Arg::Arg(arg) => { write!(f, "{:?}", arg) }
            Arg::ArgArray(array) => {
                write!(f, "[{}]", array.iter().map(|lv| format!("{:?}", lv)).collect::<Vec<_>>().join(","))
            }
            // Arg::Function(name, array) => {
            //     write!(f, "{}({})",
            //            name,
            //            array.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")
            //     )
            // }
        }
    }
}

impl From<&str> for Arg {
    fn from(s: &str) -> Self {
        Arg::Arg(LogValue::from(s))
    }
}

impl From<Vec<&str>> for Arg {
    fn from(v: Vec<&str>) -> Self {
        Arg::ArgArray(v.into_iter().map(|s| LogValue::from(s)).collect::<Vec<_>>())
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct CommandDescription {
    pub name: String,
    pub is_display: bool,
    pub args: Vec<Arg>,
    pub named_args: HashMap<String, Arg>,
    pub functions: Vec<(String, Vec<Arg>)>
}

#[derive(Serialize, Debug)]
pub struct ParsedSearch {
    pub query: String, // this is a copy of the full query
    pub start_time: StdDuration,
    pub end_time: StdDuration,
    pub time_span: (usize, usize), // start and end of search time
    pub limit: Option<usize>,
    pub conditions: Vec<SearchCondition>,
    pub conditions_span: (usize, usize), // start and end of the search conditions
    pub commands: Vec<CommandDescription>,
}

fn missing_token(token: &str) -> SearchError {
    SearchError::UnexpectedMissingToken(token.to_string())
}

impl ParsedSearch {
    /// Removes any quotes from the beginning and end of a string
    #[inline]
    fn remove_quotes(arg: &str) -> &str {
        arg.trim_start_matches(&['"', '\'']).trim_end_matches(&['"', '\''])
    }

    pub fn new(query_pair: Pair<Rule>) -> Result<Self, SearchError> {
        // save the whole string
        let query = query_pair.as_str().to_string();

        // grab the first item as it's the search time
        let mut query_pairs = query_pair.into_inner();

        //TODO: Really need to handle timezone
        // convert the various search times into a start & end time, in the local timezone
        let search_time = query_pairs.next().ok_or(missing_token("search_time"))?;
        let time_span = (search_time.as_span().start(), search_time.as_span().end());
        let (start_time, end_time) = Self::process_search_time(search_time)?;

        // convert to Durations since epoch
        let start_time_epoch = StdDuration::from_secs(start_time.timestamp() as u64);
        let end_time_epoch = StdDuration::from_secs(end_time.timestamp() as u64);

        // ensure that the start_time is before end_time
        if start_time_epoch > end_time_epoch {
            let err_str = format!("The start time {} must be before the end time {}", start_time, end_time);
            return Err(SearchError::InvalidTime(err_str));
        }

        debug!("Search time: {} -> {}  ({:?}ms -> {:?}ms)", start_time, end_time, start_time_epoch.as_millis(), end_time_epoch.as_millis());

        // setup defaults
        let mut limit = None;
        let mut conditions = Vec::new();
        let mut condition_start = 0;
        let mut condition_end = 0;

        // various things can come next, including nothing
        while let Some(next_pair) = query_pairs.next() {
            match next_pair.as_rule() {
                Rule::number => {
                    limit = Some(next_pair.as_str().trim().parse::<usize>()?);
                }
                Rule::field_search => {
                    let span = next_pair.as_span();

                    if condition_start == 0 {
                        condition_start = span.start();
                    }

                    condition_end = span.end();

                    let condition = Self::process_field_search(next_pair)?;
                    conditions.push(condition); // add to our list
                },
                Rule::pipe | Rule::EOI => {
                    break;
                },
                _ => {
                    return Err(SearchError::UnexpectedRule(next_pair.as_rule()))
                }
            }
        }

        debug!("Conditions: [{}]", conditions.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(","));

        let mut commands = Vec::<CommandDescription>::new();
        let mut found_display = false;

        // by here we're either done, or we have commands
        while let Some(command_pair) = query_pairs.next() {
            match command_pair.as_rule() {
                Rule::pipe => continue,
                Rule::command => {
                    let command = Self::process_command(command_pair)?;

                    // check to see if we've found a display command
                    if command.is_display {
                        if found_display {
                            let cur_display = commands.last().unwrap();
                            return Err(SearchError::MultipleDisplayCommands(format!("Found multiple display commands: {} and {}", cur_display.name, command.name)));
                        }

                        found_display = true;
                    }

                    commands.push(command);
                }
                Rule::EOI => break,
                _ => return Err(SearchError::UnexpectedRule(command_pair.as_rule()))
            }
        }

        // check to see if we have a display command
        if !found_display {
            let table_cmd = CommandDescription {
                name: "table".to_string(),
                is_display: true,
                args: vec![],
                named_args: Default::default(),
                functions: vec![]
            };

            commands.push(table_cmd);
        }

        return Ok(ParsedSearch {
            query,
            start_time: start_time_epoch,
            end_time: end_time_epoch,
            time_span,
            limit,
            conditions,
            conditions_span: (condition_start, condition_end),
            commands,
        });
    }

    /// Process _either_ absolute_us_time or absolute_eu_time
    fn process_absolute_time(pair: Pair<Rule>) -> Result<DateTime<Local>, SearchError> {
        let time_str = pair.as_str();
        let num_pairs = pair.into_inner().count();

        // debug!("num_pairs: {}", num_pairs);

        let res = match num_pairs {
            // must be HH:MM:SS am/pm
            4 => NaiveTime::parse_from_str(time_str, "%I:%M:%S %p")
                .map_err(|e| SearchError::ParseError(e)),

            // could be: HH:MM:SS or HH:MM am/pm
            3 => NaiveTime::parse_from_str(time_str, "%H:%M:%S")
                    .or_else(|_| NaiveTime::parse_from_str(time_str, "%I:%M %p"))
                    .map_err(|e| SearchError::ParseError(e)),

            // can only be HH:MM
            2 => NaiveTime::parse_from_str(time_str, "%H:%M")
                .map_err(|e| SearchError::ParseError(e)),

            _ => Err(SearchError::InvalidTime(format!("Unknown time format: {}", time_str)))
        }?;

        let now = Local::now();

        let abs_date_time = now
            .with_hour(res.hour()).ok_or(SearchError::InvalidTime(format!("Invalid hours in time: {}", time_str)))?
            .with_minute(res.minute()).ok_or(SearchError::InvalidTime(format!("Invalid minutes in time: {}", time_str)))?
            .with_second(res.second()).ok_or(SearchError::InvalidTime(format!("Invalid seconds in time: {}", time_str)))?;

        Ok(abs_date_time)
    }

    fn process_absolute_date_time(pair: Pair<Rule>) -> Result<DateTime<Local>, SearchError> {
        let date_time_str = pair.as_str().to_string();
        // debug!("process_absolute_date_time: {}", date_time_str);

        let mut pairs = pair.clone().into_inner();
        let date_pair = pairs.next().ok_or(missing_token("absolute_us_date | absolute_eu_date"))?;
        let time_pair = pairs.next().ok_or(missing_token("absolute_time"))?;

        // debug!("date_pair: {:?}", date_pair);
        // debug!("time_pair: {:?}", time_pair);

        let time = Self::process_absolute_time(time_pair)?;

        let naive_date = match date_pair.as_rule() {
            Rule::absolute_us_date => NaiveDate::parse_from_str(date_pair.as_str(), "%m/%d/%Y")?,
            Rule::absolute_eu_date => NaiveDate::parse_from_str(date_pair.as_str(), "%Y-%m-%d")?,
            _ => return Err(SearchError::UnexpectedRule(date_pair.as_rule()))
        };

        let naive_datetime = naive_date.and_time(time.naive_local().time());
        let abs_date_time: DateTime<Local> = Local.from_local_datetime(&naive_datetime)
                                                  .single()
                                                  .ok_or(SearchError::InvalidTime(format!("Invalid start time: {}", date_time_str)))?;

        Ok(abs_date_time)
    }

    /// Convert the search_time Pair into a start and end timestamp
    fn process_search_time(pair: Pair<Rule>) -> Result<(DateTime<Local>, DateTime<Local>), SearchError> {
        // TODO: Use VecDeque so we can pop-front instead of cloning
        let pairs = pair.into_inner().collect::<Vec<_>>();

        if pairs.len() > 1 {
            let start_date_time = match pairs[0].as_rule() {
                Rule::absolute_us_time | Rule::absolute_eu_time => {
                    Self::process_absolute_time(pairs[0].clone())?
                },
                Rule::absolute_date_time => {
                    Self::process_absolute_date_time(pairs[0].clone())?
                },
                _ => {
                    error!("UnexpectedRule for start: {:?}", pairs[0].as_rule());
                    return Err(SearchError::UnexpectedRule(pairs[0].as_rule()))
                }
            };

            let end_date_time = match pairs[1].as_rule() {
                Rule::absolute_us_time | Rule::absolute_eu_time => {
                    Self::process_absolute_time(pairs[1].clone())?
                },
                Rule::absolute_date_time => {
                    Self::process_absolute_date_time(pairs[1].clone())?
                },
                _ => {
                    error!("UnexpectedRule for start: {:?}", pairs[1].as_rule());
                    return Err(SearchError::UnexpectedRule(pairs[1].as_rule()))
                }
            };

            Ok( (start_date_time, end_date_time) )
        } else {
            let now = Local::now();
            let rule = pairs[0].as_rule();

            return match rule {
                Rule::relative_time => {
                    let mut pairs = pairs[0].clone().into_inner();
                    let offset_amt = pairs.next().ok_or(missing_token("number"))?.as_str().parse::<i64>()?;
                    let offset_unit = pairs.next().ok_or(missing_token("time_unit"))?.as_str();

                    match offset_unit {
                        "m" | "M" => {
                            Ok((now - Duration::minutes(offset_amt), now))
                        },
                        "h" | "H" => {
                            Ok((now - Duration::hours(offset_amt), now))
                        },
                        "d" | "D" => {
                            Ok((now - Duration::days(offset_amt), now))
                        },
                        _ => {
                            Err(SearchError::UnexpectedValue(offset_unit.to_string()))
                        }
                    }
                },
                Rule::absolute_us_time | Rule::absolute_eu_time => {
                    let start_time = Self::process_absolute_time(pairs[0].clone())?;

                    Ok((start_time, now))
                },
                Rule::absolute_date_time => {
                    let abs_date_time = Self::process_absolute_date_time(pairs[0].clone())?;

                    Ok((abs_date_time, now))
                }
                _ => {
                    Err(SearchError::UnexpectedRule(pairs[0].as_rule()))
                }
            }
        }
    }

    /// Converts a field_search into
    fn process_field_search(pair: Pair<Rule>) -> Result<SearchCondition, SearchError> {
        let mut pairs = pair.into_inner();

        // we always have 2 pieces
        let field = Self::remove_quotes(pairs.next().ok_or(missing_token("identifier"))?.as_str()).to_string();
        let search_type = pairs.next().ok_or(missing_token("(multi_arg_search | single_arg_search)"))?;

        // we cheat here, because both paths (multi_arg_search & single_arg_search) have comparator ~ basically (arg | array_arg)
        // so instead of matching on search_type, we just unwrap and treat similarly
        // the single_arg_search will _never_ match on Rule::array_arg, but that's OK
        let mut pairs = search_type.into_inner();

        let comparator = pairs.next().ok_or(missing_token("comparator"))?.as_str().try_into()?;
        let arg_type = pairs.next().ok_or(missing_token("arg | array_arg"))?;

        let values = match arg_type.as_rule() {
            Rule::arg => {
                vec![Self::process_arg(arg_type)?]
            },
            Rule::array_arg => {
                let pairs = arg_type.into_inner();
                pairs.map(|p| Self::process_arg(p)).collect::<Result<Vec<_>, _>>()?
            },
            _ => { return Err(SearchError::UnexpectedRule(arg_type.as_rule())) }
        };

        Ok(SearchCondition { field, comparator, values })
    }

    fn process_arg(arg: Pair<Rule>) -> Result<LogValue, SearchError> {
        let arg = arg.into_inner()
                     .next()
                     .ok_or(missing_token("literal | identifier"))?;
        let arg_str = arg.as_str().trim().to_string();

        Ok(match arg.as_rule() {
            Rule::null => {
                LogValue::Null
            }
            Rule::boolean => {
                LogValue::Bool(arg_str.parse::<bool>()?)
            }
            Rule::identifier => {
                // this will "do the right thing" and try to convert to numbers first
                LogValue::from(arg_str.as_str())
            }
            Rule::number | Rule::negative_number => {
                LogValue::Integer(arg_str.parse::<i64>()?)
            }
            Rule::quoted_string => {
                let unquoted_string = &arg_str.as_str()[1..arg_str.len()-1];
                LogValue::String(unquoted_string.to_string())
            }
            _ => { return Err(SearchError::UnexpectedRule(arg.as_rule())); }
        })
    }

    /// Converts a command into a list of commands w/args
    fn process_command(pair: Pair<Rule>) -> Result<CommandDescription, SearchError> {
        let mut pairs = pair.into_inner();

        // start with either a display command, or a "regular" one
        let cmd_pair = pairs.next().ok_or(missing_token("display_command | command_ident"))?;

        let is_display = match cmd_pair.as_rule() {
            Rule::display_command => { true },
            Rule::command_ident => { false },
            _ => return Err(SearchError::UnexpectedRule(cmd_pair.as_rule()))
        };

        // trim off any whitespace, and make lowercase
        let name = cmd_pair.as_str().trim().to_ascii_lowercase();

        let mut args = Vec::new();
        let mut named_args = HashMap::new();
        let mut functions = Vec::new();

        // println!("PAIRS: {:?}", pairs);

        // go through the args: fun, named_arg, array_arg, or arg
        while let Some(arg) = pairs.next() {
            // we'll have either an array_arg, literal, or named_arg
            match arg.as_rule() {
                Rule::func => {
                    let mut pairs = arg.into_inner();
                    let name = pairs.next().ok_or(missing_token("identifier"))?.as_str();
                    debug!("NAME: {}", name);

                    // go through the function arguments
                    let func_args = pairs.map(|p| {
                        match p.as_rule() {
                            Rule::array_arg => {
                                let pairs = p.into_inner();
                                let array_args = pairs.map(|p| Self::process_arg(p)).collect::<Result<Vec<_>, _>>()?;
                                Ok(Arg::ArgArray(array_args))
                            }
                            Rule::arg => {
                                Ok(Arg::Arg(Self::process_arg(p)?))
                            }
                            _ => { Err(SearchError::UnexpectedRule(p.as_rule())) }
                        }
                    }).collect::<Result<Vec<_>, _>>()?;

                    functions.push( (name.to_string(), func_args) );
                }
                Rule::named_arg => {
                    let mut pairs = arg.into_inner();
                    let name = pairs.next().ok_or(missing_token("identifier"))?.as_str();
                    let arg = pairs.next().ok_or(missing_token("arg | array_arg"))?;

                    let arg = match arg.as_rule() {
                        Rule::array_arg => {
                            let pairs = arg.into_inner();
                            let array_args = pairs.map(|p| Self::process_arg(p)).collect::<Result<Vec<_>, _>>()?;
                            Arg::ArgArray(array_args)
                        }
                        Rule::arg => {
                            Arg::Arg(Self::process_arg(arg)?)
                        }
                        _ => { return Err(SearchError::UnexpectedRule(arg.as_rule())) }
                    };

                    named_args.insert(name.to_string(), arg);
                }
                Rule::array_arg => {
                    let pairs = arg.into_inner();
                    let array_args = pairs.map(|p| Self::process_arg(p)).collect::<Result<Vec<_>, _>>()?;

                    args.push(Arg::ArgArray(array_args));
                }
                Rule::arg => {
                    args.push(Arg::Arg(Self::process_arg(arg)?));
                }
                _ => { return Err(SearchError::UnexpectedRule(arg.as_rule())); }
            }
        }

        return Ok(CommandDescription {
            name,
            is_display,
            args,
            named_args,
            functions
        })
    }
}

