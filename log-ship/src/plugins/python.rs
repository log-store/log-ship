use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString, PyTuple};
use stream_cancel::{StreamExt, Tripwire};
use serde_json::{Value as JsonValue};

use tokio::sync::{broadcast, Semaphore};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::{StreamExt as TokioStreamExt};
use tokio_stream::wrappers::BroadcastStream;
use toml::Value;

use crate::common::logging::{debug, error};
use crate::{Args, connect_receiver, create_event_stream, create_sender_semaphore, get_receiver, Plugin, recv_event, send_event};
use crate::event::{Event};
use crate::plugin::{PluginType, ChannelType};

const DEFAULT_FUNCTION_NAME: &str = "process";


/// Transformer plugin that processes through Python
pub struct PythonScript {
    python_function: PyObject,
    arg_type: String, //TODO: Change to an enum
    tripwire: Tripwire,
    receiver: Option<Receiver<ChannelType>>,
    sender: Sender<ChannelType>,
    semaphore: Arc<Semaphore>,
}

#[async_trait]
impl Plugin for PythonScript {
    fn name() -> &'static str where Self: Sized {
        "python"
    }

    async fn new(args: Args, tripwire: Tripwire) -> anyhow::Result<Box<PluginType>> where Self: Sized {
        debug!("PythonScript args: {:#?}", args);

        let file_path = args.get("path").ok_or(anyhow!("Could not find 'path' arg for python transformer"))?;
        let file_path = file_path.as_str().ok_or(anyhow!("The 'path' arg for python transformer does not appear to be a string"))?;
        let mut file = File::open(file_path).context(format!("Error opening Python script: {}", file_path))?;
        let mut code = String::new();

        // read in the code
        file.read_to_string(&mut code).context(format!("Error reading Python script: {}", file_path))?;

        // get the name of the function to call
        let default_function_name = Value::String(DEFAULT_FUNCTION_NAME.to_string());
        let function_name = args.get("function")
            .unwrap_or(&default_function_name)
            .as_str()
            .ok_or(anyhow!("The 'function' arg for python transformer does not appear to be a string"))?;

        // parse out the code, and find the appropriate function
        let python_function = Python::with_gil(|py| -> anyhow::Result<PyObject> {
            let module = PyModule::from_code(py, code.as_str(), file_path, "log-ship")
                .context(format!("Unable to parse code find in script {}", file_path))?;
            let func = module.getattr(function_name)
                .context(format!("Unable to find a function named '{}' in script {}", function_name, file_path))?;

            Ok(func.into_py(py))
        })?;

        // get the type to pass to the function
        let arg_type = args.get("arg_type")
            .ok_or(anyhow!("Could not find 'arg_type' for python transformer"))?;
        let arg_type = arg_type.as_str().ok_or(anyhow!("The 'arg_type' arg for python transformer does not appear to be a string"))?;

        // validate the type
        if arg_type != "str" && arg_type != "dict" {
            return Err(anyhow!("The 'arg_type' for python transformer must be one of: str or dict"));
        }

        // setup the channel
        let (sender, semaphore) = create_sender_semaphore!(args);

        Ok(Box::new(PythonScript {
            python_function,
            arg_type: arg_type.to_string(),
            tripwire,
            receiver: None,
            sender,
            semaphore,
        }))
    }

    async fn run(&mut self) {
        debug!("Python running...");

        let mut event_stream = create_event_stream!(self);

        while let Some(event) = event_stream.next().await {
            let (event, callback) = recv_event!(event);

            // convert the event to the correct type based upon what's being asked for
            let event = match (event, self.arg_type.as_str()) {
                (Event::None, _) => continue,
                (Event::Json(json), "str") => { Event::String(json.to_string()) },
                (Event::String(string), "dict") => {
                    match serde_json::from_str(string.as_str()) {
                        Err(_e) => {
                            error!("While trying to run a Python script which requested the log as a dict, non-JSON log provided");
                            return;
                        }
                        Ok(json) => Event::Json(json)
                    }
                },
                (event, _) => event
            };

            // call the Python function
            let call_res = Python::with_gil(|py| -> anyhow::Result<Option<Event>> {
                let args = match event {
                    Event::None => {
                        error!("Event::None; should never happen");
                        return Ok(None)
                    }
                    Event::Json(json) => {
                        let dict = PyDict::new(py);
                        let json = json.as_object().ok_or(anyhow!("Error converting log to JSON"))?;

                        // go through the JSON adding everything to the dict
                        for (k,v) in json.into_iter() {
                            if v.is_u64() {
                                dict.set_item(k, v.as_u64().unwrap())?;
                            } else if v.is_i64() {
                                dict.set_item(k, v.as_i64().unwrap())?;
                            } else if v.is_f64() {
                                dict.set_item(k, v.as_f64().unwrap())?;
                            } else if v.is_boolean() {
                                dict.set_item(k, v.as_bool().unwrap())?;
                            } else if v.is_null() {
                                dict.set_item(k, py.None())?;
                            } else if v.is_string() {
                                dict.set_item(k, v.as_str().unwrap())?;
                            } else {
                                // everything else we just treat as a string
                                dict.set_item(k, v.to_string())?;
                            }
                        }

                        PyTuple::new(py, &[dict])
                    }
                    Event::String(string) => {
                        let s = PyString::new(py, string.as_str());
                        PyTuple::new(py, &[s])
                    }
                };

                // call the function, and extract the result as a string
                let ret_obj = self.python_function.call1(py, args).context(format!("Error calling Python function"))?;

                // simply skip if it's none
                if ret_obj.is_none(py) {
                    return Ok(None)
                }

                let dict: HashMap<String, PyObject> = ret_obj.extract(py)?;
                let mut json_map = serde_json::Map::new();

                for (key, value) in dict.into_iter() {
                    if value.as_ref(py).is_instance_of::<PyString>()? {
                        let s: String = value.extract(py)?;
                        json_map.insert(key, JsonValue::from(s));
                    } else if value.as_ref(py).is_instance_of::<PyInt>()? {
                        let i: i64 = value.extract(py)?;
                        json_map.insert(key, JsonValue::from(i));
                    } else if value.as_ref(py).is_instance_of::<PyFloat>()? {
                        let f: f64 = value.extract(py)?;
                        json_map.insert(key, JsonValue::from(f));
                    } else if value.as_ref(py).is_instance_of::<PyBool>()? {
                        let b: bool = value.extract(py)?;
                        json_map.insert(key, JsonValue::from(b));
                    } else if value.is_none(py) {
                        json_map.insert(key, JsonValue::Null);
                    } else {
                        return Err(anyhow!("Error getting type for PyObject: {:?}", value));
                    }
                }

                Ok(Some(Event::Json(JsonValue::from(json_map))))
            });

            // debug!("Python sending: {:?}", event);

            // send the event
            match call_res {
                Err(e) => { error!("Error running Python script: {:?}", e); }
                Ok(op_event) => {
                    if let Some(event) = op_event {
                        send_event!(self, event, callback);
                    }
                }
            }
        }
    }

    // boilerplate methods
    get_receiver!{}
    connect_receiver!{}
}