#[macro_use]
extern crate pest_derive;

use pest::Parser;
use common::logging::debug;
use common::init_test_logger;
use crate::processor::{ParsedSearch};


mod processor;

#[derive(Parser)]
#[grammar="query_grammar.pest"]
struct QueryParser;

fn run_test(query: &str) {
    debug!("Attempting to parse: {}", query);

    let query_pair = QueryParser::parse(Rule::query, query).unwrap().next().unwrap();
    match ParsedSearch::new(query_pair) {
        Err(e) => {
            panic!("Error parsing query: {} - {:?}", query, e);
        }
        Ok(search) => {
            println!("{} -> {:?}", query, search);
        }
    }

}

fn main() {
    init_test_logger();

    // time tests
    // run_test(r#"00:47:32"#);
    // run_test(r#"12:47:32 am"#);
    // run_test(r#"12:47:32 AM"#);
    // run_test(r#"00:47"#);
    // run_test(r#"12:47 am"#);
    // run_test(r#"02/21/2020 12:47:32"#);
    // run_test(r#"02/21/2020 12:47"#);
    // run_test(r#"2002-11-23 12:47:32"#);
    // run_test(r#"2002-11-23 12:47"#);
    // run_test(r#"2002-11-23 12:47:32 to 2002-12-23 14:47:32 "#);
    // run_test(r#"00:47:32 to 16:47:32"#);
    // run_test(r#"1h"#);
    run_test(r#"1d"#);

    run_test(r#"5d clientip = ["157.47.0.0", "127.0.0.1"] timestamp = 894022409"#);
    run_test(r#"5d clientip = "157.47.0.0" timestamp = 894022409"#);
    run_test(r#"5d clientip = null timestamp = 894022409"#);
    run_test(r#"1m | dedup"#);
    run_test(r#"1h field="value""#);
    run_test(r#"1h field="value" other_field=1234"#);
    run_test(r#"1h | dedup"#);
    run_test(r#"1h field="value" | dedup"#);
    run_test(r#"1h field="value" field="value" | dedup"#);

    run_test(r#"1d a=7 b="hill"| chart"#);
    run_test(r#"1d a=-7 b="hill"| chart a=7"#);
    run_test(r#"1d a=7 b="hill"| bob "blah""#);
    run_test(r#"1d a=7 b="hill"| blah | chart"#);
    run_test(r#"1d a = -7 b="hill"| bob [23, true, "seven"] | chart"#);
    run_test(r#"1d a = 7 b="hill"| bob "blah" [23, true, "seven"] | chart"#);
    run_test(r#"1d a=-7 b="hill"| bob true some="blah" foo=[23, true, "seven"] | chart"#);
    run_test(r#"1d a=7 b="hill"| foo "blah" [23, true, "seven"] true | bob "[blah]" n1=[23, true, "seven"] n2=true | chart"#);

    run_test(r#"1d a=7 b!="hill"| bob blah"#);
    run_test(r#"1d a = -7 b!="hill"| bob [23, true, seven] | chart"#);
    run_test(r#"1d a = 7 b!="hill"| bob blah [23, true, seven] | chart"#);
    run_test(r#"1d a=-7 b!="hill"| bob true some=blah foo=[23, true, seven] | chart"#);
    run_test(r#"1d a=7 b!="hill"| foo blah [23, true, seven] true | bob "[blah]" n1=[23, true, seven] n2=true | chart"#);

    run_test(r#"5d 7 clientip = "157.47.0.0" timestamp = 894022409"#);
    run_test(r#"1h 23"#);
    run_test(r#"1m 1234| dedup"#);
    run_test(r#"1h 12 field="value""#);
    run_test(r#"1h 1 field="value" other_field=1234"#);
    run_test(r#"1h 12 | dedup"#);
    run_test(r#"1h 1 field="value" | dedup"#);
    run_test(r#"1h 123 field="value" field="value" | dedup"#);

    run_test(r#"1d 1230 a=7 b="hill"| chart"#);
    run_test(r#"1d 1230 a=-7 b="hill"| chart a=7"#);
    run_test(r#"1d 1230 a=7 b="hill"| bob "blah""#);
    run_test(r#"1d 1230 a=7 b="hill"| blah | chart"#);
    run_test(r#"1d 1230 a = -7 b="hill"| bob [23, true, "seven"] | chart"#);
    run_test(r#"1d 1230 a = 7 b="hill"| bob "blah" [23, true, "seven"] | chart"#);
    run_test(r#"1d 1230 a=-7 b="hill"| bob true some="blah" foo=[23, true, "seven"] | chart"#);
    run_test(r#"1d 1230 a=7 b="hill"| foo "blah" [23, true, "seven"] true | bob "[blah]" n1=[23, true, "seven"] n2=true | chart"#);

    run_test(r#"1d 1230 a=7 b="hill"| bob blah"#);
    run_test(r#"1d 1230 a = -7 b="hill"| bob [23, true, seven] | chart"#);
    run_test(r#"1d 1230 a = 7 b="hill"| bob blah [23, true, seven] | chart"#);
    run_test(r#"1d 1230 a=-7 b="hill"| bob true some=blah foo=[23, true, seven] | chart"#);
    run_test(r#"1d 1230 a=7 b="hill"| foo blah [23, true, seven] true | bob "[blah]" n1=[23, true, seven] n2=true | chart"#);

    run_test(r#"1d | agg median(ip)"#);
    run_test(r#"1d | agg median([ip, "blah"])"#);
    run_test(r#"1d | agg median("blah", 23)"#);
    run_test(r#"1d | agg median([true, "blah"], 23)"#);
    run_test(r#"1d | agg median([true, "blah"], [1, blah], 23)"#);
    run_test(r#"1d | agg median(ip) blah foo=bar"#);
    run_test(r#"1d | agg median(ip) blah foo=bar mean(something)"#);
    run_test(r#"1d | bucket t span="5m" | agg count by=bar"#);
    run_test(r#"1d | bucket t 5m | agg count by=bar"#);
    run_test(r#"1d | bucket t 5 | agg count by=bar"#);
    run_test(r#"1d | bucket t span=5 | agg count by=bar"#);
    run_test(r#"1d | bucket t -5 | agg count by=bar"#);
    run_test(r#"1d | bucket t span=true | agg count by=bar"#);

}
