mod processor;

use pest::Parser;
pub use crate::query_parser::processor::{
    ParsedSearch,
    SearchError,
    SearchComparator,
    Arg
};

#[allow(unused_imports)]
use crate::logging::{setup_with_level, FilterLevel, debug, error, info, warn};

#[derive(Parser)]
#[grammar="query_parser/query_grammar.pest"]
struct QueryParser;

/// Given a search query, try to construct a [Search] object
pub fn parse_query(query: &str) -> Result<ParsedSearch, SearchError> {
    debug!("Attempting to parse query: {}", query);

    let query_pair = QueryParser::parse(Rule::query, query)?
        .next()
        .ok_or(SearchError::UnexpectedMissingToken("".to_string()))?;

    ParsedSearch::new(query_pair)
}

pub use processor::CommandDescription;
