//! Common definitions for the client and server.

use std::collections::HashMap;

use itertools::Itertools;

use serde::{Deserialize, Serialize};

/// The address where the server listens.
pub const SERVER_ADDR: &str = "127.0.0.1:3030";

/// A request to the jobserver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobServerReq {
    /// Used for debugging.
    Ping,

    /// Add the given machine to the list of available machines
    MakeAvailable {
        /// The IP:PORT of the machine.
        addr: String,

        /// The class of the machine.
        class: String,
    },

    /// Remove the given machine from the list of available machines
    RemoveAvailable {
        /// The IP:PORT of the machine.
        addr: String,
    },

    /// List the available machines known to the server.
    ListAvailable,

    /// Set up a machine and optionally make it available in the given class.
    SetUpMachine {
        /// The IP:PORT of the machine.
        addr: String,

        /// The class of the machine.
        class: Option<String>,

        /// The setup commands to execute in order.
        ///
        /// The commands may use any existing variables known to the server.
        cmds: Vec<String>,
    },

    /// Set the value of a variable.
    SetVar { name: String, value: String },

    /// List all set variables and their values.
    ListVars,

    /// Add a job to be run on the given class of machine.
    AddJob {
        /// The class of machine allowed to run this job.
        class: String,

        /// The command of the job.
        ///
        /// The command may use any existing variables known to the server.
        cmd: String,

        /// The location to copy results, if any.
        cp_results: Option<String>,
    },

    /// Get a list of job IDs.
    ListJobs,

    /// Cancel a running or scheduled job.
    CancelJob {
        /// The job ID of the job to cancel.
        jid: usize,
    },

    /// Get information on the status of a job.
    JobStatus {
        /// The job ID of the job.
        jid: usize,
    },

    /// Clone a running or scheduled job. That is, create a new job with the same properties as the
    /// given job.
    CloneJob {
        /// The job ID of the job to cancel.
        jid: usize,
    },

    /// Start a matrix with the given variables and command template.
    AddMatrix {
        /// The variables and their values, which we take the Cartesian Product over.
        vars: HashMap<String, Vec<String>>,

        /// The command of the job.
        ///
        /// The command may use any existing variables known to the server and variables from the
        /// set above.
        cmd: String,

        /// The class of machine allowed to run this job.
        class: String,

        /// The location to copy results, if any.
        cp_results: Option<String>,
    },

    StatMatrix {
        /// The ID of the matrix.
        id: usize,
    },
}

/// A response to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobServerResp {
    /// Succeeded. No return value.
    Ok,

    /// Succeeded. A list of available machines and their classes.
    Machines(HashMap<String, String>),

    /// A list of job IDs.
    Jobs(Vec<usize>),

    /// A list of variables and their values.
    Vars(HashMap<String, String>),

    /// Succeeded. The job ID of a created job.
    JobId(usize),

    /// Succeeded. The matrix ID of a created matrix.
    MatrixId(usize),

    /// Succeeded. The status of a job.
    JobStatus {
        class: String,
        cmd: String,
        jid: usize,
        status: Status,
        variables: HashMap<String, String>,
    },

    /// Succeeded. The status of a matrix.
    MatrixStatus {
        /// The command template.
        cmd: String,

        /// The class of machine allowed to run this job.
        class: String,

        /// The location to copy results, if any.
        cp_results: Option<String>,

        /// The matrix ID
        id: usize,

        /// The job IDs that comprise the matrix.
        jobs: Vec<usize>,

        /// The variables in the matrix
        variables: HashMap<String, Vec<String>>,
    },

    /// Error. The requested machine does not exist.
    NoSuchMachine,

    /// Error. No such job.
    NoSuchJob,

    /// Error. No such matrix.
    NoSuchMatrix,
}

/// The status of a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    /// The job is waiting to run.
    Waiting,

    /// The job is currently running.
    Running {
        /// The machine the job is running on.
        machine: String,
    },

    /// The job finished runnning.
    Done {
        /// The machine the job is ran on.
        machine: String,

        /// The name of the output files, if any.
        output: Option<String>,
    },

    /// The job was cancelled.
    Cancelled,

    /// The job produced an error.
    Failed {
        /// The machine where the job was running when the failure occured, if any.
        machine: Option<String>,

        /// The error that caused the failure.
        error: String,
    },
}

pub fn cmd_replace_vars(cmd: &str, vars: &HashMap<String, String>) -> String {
    vars.iter().fold(cmd.to_string(), |cmd, (key, value)| {
        cmd.replace(&format!("{{{}}}", key), &value)
    })
}

pub fn cmd_replace_machine(cmd: &str, machine: &str) -> String {
    cmd.replace("{MACHINE}", &machine)
}

pub fn cmd_to_path(cmd: &str) -> String {
    let mut name = format!(
        "/tmp/{}",
        cmd.replace(" ", "_")
            .replace("{", "_")
            .replace("}", "_")
            .replace("/", "_")
    );
    name.truncate(200);
    name
}

// Gets the cartesian product of the given set of variables and their sets of possible values.
pub fn cartesian_product<'v>(
    vars: &'v HashMap<String, Vec<String>>,
) -> impl Iterator<Item = HashMap<String, String>> + 'v {
    vars.iter()
        .map(|(k, vs)| vs.iter().map(move |v| (k.clone(), v.clone())))
        .multi_cartesian_product()
        .map(|config: Vec<(String, String)>| config.into_iter().collect())
}
