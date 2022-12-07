use std::collections::HashMap;
use std::fs::{OpenOptions};

use std::path::PathBuf;
use std::sync::Arc;


use anyhow::{anyhow, bail, Context};
use clap::{Arg, Command, arg, ArgAction};
use maplit::hashmap;
use stream_cancel::Tripwire;
use futures::future::join_all;
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::Semaphore;

use plugins::*; // import all the plugins

use crate::common::config_utils::{find_config_file, parse_config_file};
use crate::common::logging::{setup_with_level, FilterLevel, debug, error, info, warn};
use crate::common::logging::setup_with_level_location;

use crate::config_file::{ConfigFile, merge_globals};
use crate::plugin::{Args, Plugin};

mod common;
mod config_file;
mod plugins;
mod plugin;
mod event;

const CONFIG_FILE_NAME: &str = "log-ship.toml";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    // setup the args
    let args = Command::new("log-ship")
        .about("The most versatile log shipper!")
        .arg(Arg::new("debug").long("debug").hide(true).required(false).action(ArgAction::SetTrue))
        .arg(Arg::new("log_file").long("log-file").help("Optional log file location").required(false).value_name("LOG_FILE"))
        .arg(Arg::new("config_file").long("config-file").help("Optional config file location").required(false).value_name("CONFIG_FILE"))
        .arg(arg!(--check "Check the config file, and print the routes").action(ArgAction::SetTrue))
        .get_matches();

    // first figure out if we've enabled debug logging
    let log_level = if args.get_flag("debug") {
        FilterLevel::Debug
    } else {
        FilterLevel::Info
    };

    // see if a config-file was specified on the command line
    let config_file_path = if let Some(config_file_arg) = args.get_one::<String>("config_file") {
        let config_file_path = PathBuf::from(config_file_arg);

        if !config_file_path.exists() || !config_file_path.is_file() {
            bail!("The configuration file specified on the command line ({}) was not found", config_file_path.display());
        }

        config_file_path
    } else {
        match find_config_file(CONFIG_FILE_NAME) {
            Ok(path) => path,
            Err(checked_paths) => {
                eprintln!("The configuration file ({}) for log-ship was not found", CONFIG_FILE_NAME);
                eprintln!("The following places were checked:");

                for path in checked_paths {
                    eprintln!("\t{}", path.display());
                }

                bail!("Cannot start without a config file");
            }
        }
    };

    // parse the config file
    let config_file: ConfigFile = parse_config_file(config_file_path.clone())?;

    debug!("CONFIG: {:#?}", config_file);

    // sanity check the config file
    let check_config = args.get_flag("check");

    if let Err(e) = config_file.sanity_check(check_config) {
        error!("Error checking the config file: {}", e);
        bail!(e);
    }

    if check_config {
        return Ok( () );
    }

    // get the log_file
    let op_log_file = if let Some(log_file_arg) = args.get_one::<String>("log_file") {
        Some(PathBuf::from(log_file_arg))
    } else {
        config_file.globals.log_file.clone()
    };

    // setup the logging
    let _logger = if let Some(ref log_file_path) = op_log_file {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)
            .with_context(|| format!("opening log_file: {}", log_file_path.display()))
            ?;

        setup_with_level_location(log_level, log_file)
    } else {
        setup_with_level(log_level)
    };

    // creates a map of name -> factory for each plugin
    let input_plugins = hashmap! {
        FileInput::name() => FileInput::factory(),
        JournaldInput::name() => JournaldInput::factory(),
        StdInput::name() => StdInput::factory(),
    };
    let output_plugins = hashmap! {
        StdOutput::name() => StdOutput::factory(),
        TcpSocketOutput::name() => TcpSocketOutput::factory(),
        UnixSocketOutput::name() => UnixSocketOutput::factory(),
        SpeedTest::name() => SpeedTest::factory(),
    };
    let transform_plugins = hashmap! {
        PythonScript::name() => PythonScript::factory(),
        InsertFieldTransform::name() => InsertFieldTransform::factory(),
        InsertTimestampTransform::name() => InsertTimestampTransform::factory(),
    };

    info!("Starting log-ship with config file: {}", config_file_path.display());

    // get the configuration of each plugin type
    let input_configs = config_file.inputs.into_iter().map(|i| (i.name, (i.input_type, merge_globals(&i.args, &config_file.globals)))).collect::<HashMap<_,_>>();
    let transform_configs = config_file.transforms.into_iter().map(|t| (t.name, (t.transform_type, merge_globals(&t.args, &config_file.globals)))).collect::<HashMap<_,_>>();
    let output_configs = config_file.outputs.into_iter().map(|o| (o.name, (o.output_type, merge_globals(&o.args, &config_file.globals)))).collect::<HashMap<_,_>>();

    // create a tripwire used to stop all processing
    let (trigger, tripwire) = Tripwire::new();

    // create a list of all the route handles
    let num_routes = config_file.routes.len();
    let mut route_join_handles = Vec::with_capacity(num_routes);
    let route_semaphore = Arc::new(Semaphore::new(num_routes));

    // go through the routes trying to configure them
    for route in config_file.routes {
        // create 2 lists: inputs & transforms; outputs
        // these are passed into the tokio "thread"
        // connected up, and run in reverse order
        let mut input_transform_list = Vec::with_capacity(route.transforms.len() + 1);

        // get the configuration, and create an instance of the plugin
        let (plugin_type, args) = input_configs.get(route.input.as_str())
            .ok_or(anyhow!("In route {}, the input {} was not found. Ensure the config file has an [[input]] entry with the appropriate name", route.name, route.input))?;
        let input_plugin = input_plugins.get(plugin_type.as_str())
            .ok_or(anyhow!("No input plugin of type {} found", plugin_type))?(args.clone(), tripwire.clone())?;

        input_transform_list.push(input_plugin);

        // go through the list of transformations
        for transform in route.transforms.iter() {
            let (plugin_type, args) = transform_configs.get(transform.as_str())
                .ok_or(anyhow!("In route {}, the transform {} was not found. Ensure the config file has a [[transform]] entry with the appropriate name", route.name, transform))?;
            let transform_plugin = transform_plugins.get(plugin_type.as_str())
                .ok_or(anyhow!("No transform plugin of type {} found", plugin_type))?(args.clone(), tripwire.clone())?;

            input_transform_list.push(transform_plugin);
        }

        // setup the output
        let (plugin_type, args) = output_configs.get(route.output.as_str())
            .ok_or(anyhow!("In route {}, the output {} was not found. Ensure the config file has an [[output]] entry with the appropriate name", route.name, route.output))?;
        let mut output_plugin = output_plugins.get(plugin_type.as_str())
            .ok_or(anyhow!("No output plugin of type {} found", plugin_type))?(args.clone(), tripwire.clone())?;

        info!("Constructed route {}", route.name);

        let sem_clone = route_semaphore.clone();

        // spawn an instance of this route
        route_join_handles.push(tokio::spawn(async move {
            let _permit = sem_clone.acquire().await.expect("Error getting permit");

            // go through and hook-up all the receivers
            for i in 1..input_transform_list.len() {
                let recv = input_transform_list[i-1].get_receiver();
                input_transform_list[i].connect_receiver(recv);
            }

            let mut plugin_join_handles = Vec::with_capacity(input_transform_list.len() + 1);

            // hook-up the output
            let recv = input_transform_list.last().expect("Nothing in input_transform_list!!!").get_receiver();
            output_plugin.connect_receiver(recv);

            // call run after it's been hooked up
            plugin_join_handles.push(tokio::spawn(async move { output_plugin.run().await }));

            // go backwards through the inputs calling run on them
            for mut p in input_transform_list.into_iter().rev() {
                plugin_join_handles.push(tokio::spawn(async move { p.run().await }));
            }

            // TODO: maybe try_join???
            // wait for everything to finish
            join_all(plugin_join_handles).await;
        }));
    }

    info!("Running all routes");

    let mut sig_term = signal::unix::signal(SignalKind::terminate()).context("Attempting to setup signal handler for terminate")?;

    // wait for the routes to finish (probably an error), or the Ctrl-C signal
    tokio::select! {
        all_results = join_all(route_join_handles) => {
            info!("All routes finished");

            for res in all_results {
                if let Err(e) = res {
                    error!("{}", e);
                }
            }
        },

        _ = signal::ctrl_c() => {
            warn!("Got Ctrl-C; shutting down");

            trigger.cancel();
        },

        _ = sig_term.recv() => {
            warn!("Got SIGTERM; shutting down");

            trigger.cancel();
        }
    }

    // make sure everyone is done
    while route_semaphore.available_permits() < num_routes {
        tokio::task::yield_now().await;
    }


    Ok( () )
}
