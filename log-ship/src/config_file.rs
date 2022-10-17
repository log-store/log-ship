use std::path::PathBuf;


use anyhow::bail;
use serde::{Serialize, Deserialize};
use toml::Value;
use toml::value::Table;

use crate::Args;


#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigFile {
    // Globals that always have defaults
    pub globals: Globals,

    #[serde(rename = "input")]
    pub inputs: Vec<Input>,

    #[serde(rename = "transform")]
    pub transforms: Vec<Transform>,

    #[serde(rename = "output")]
    pub outputs: Vec<Output>,

    #[serde(rename = "route")]
    pub routes: Vec<Route>,
}

impl ConfigFile {
    /// Sanity checks the configuration file, and optionally prints the routes
    pub fn sanity_check(&self, print_route: bool) -> anyhow::Result<()> {
        if self.globals.channel_size < 1 {
            bail!("Channel size too small; it should be between 2 and 1024");
        } else if self.globals.channel_size > 1024 {
            bail!("Channel size too large; it should be between 2 and 1024");
        }

        // make sure we have routes
        if self.routes.is_empty() {
            bail!("No routes specified");
        }

        // go through the routes, and make sure they specify known inputs, transforms, and outputs
        for route in self.routes.iter() {
            if let Some(input) = self.inputs.iter().find(|i| i.name == route.input) {
                if print_route {
                    println!("▶ {} ◀", route.name);
                    println!("INPUT: {}", input.name);
                }
            } else {
                bail!("Input {} not found for route {}", route.input, route.name);
            }

            let mut transforms = Vec::with_capacity(route.transforms.len());

            for transform_name in route.transforms.iter() {
                if self.transforms.iter().find(|t| t.name == *transform_name).is_some() {
                    transforms.push(transform_name.as_str());
                } else {
                    bail!("Transform {} not found for route {}", transform_name, route.name);
                }
            }

            if print_route {
                println!("⮱ TRANSFORMS: {}", transforms.join(" → "));
            }

            if self.outputs.iter().find(|o| o.name == *route.output).is_some() {
                if print_route {
                    println!("  ⮱ OUTPUT: {}", route.output.as_str());
                    println!();
                }
            } else {
                bail!("Output {} not found for route {}", route.output, route.name);
            }
        }

        Ok( () )
    }
}

const fn default_channel_size() -> i64 { 128 }

#[derive(Serialize, Deserialize, Debug)]
pub struct Globals {
    #[serde(default = "default_channel_size")]
    pub channel_size: i64,

    pub log_file: Option<PathBuf>
}

pub fn merge_globals(args: &Args, globals: &Globals) -> Args {
    let mut ret = args.iter()
                      .map(|(k, v)| (k.clone(), v.clone()))
                      .collect::<Args>();

    // manually list-out the globals here
    ret.insert("channel_size".to_string(), Value::Integer(globals.channel_size));

    ret
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Input {
    pub name: String,

    #[serde(rename = "type")]
    pub input_type: String,

    pub description: Option<String>,

    #[serde(default)]
    pub args: Args,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Output {
    pub name: String,

    #[serde(rename = "type")]
    pub output_type: String,

    pub description: Option<String>,

    #[serde(default)]
    pub args: Table,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Transform {
    pub name: String,

    #[serde(rename = "type")]
    pub transform_type: String,

    pub description: Option<String>,

    #[serde(default)]
    pub args: Table,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Route {
    pub name: String,

    pub input: String,

    #[serde(default)]
    pub transforms: Vec<String>,

    pub output: String,
}

