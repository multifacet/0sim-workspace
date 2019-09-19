//! Server implmentation

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use clap::clap_app;

use crossbeam::channel::{select, unbounded, Receiver, TryRecvError};

use jobserver::{
    cmd_replace_machine, cmd_replace_vars, cmd_to_path, JobServerReq, JobServerResp, Status,
    SERVER_ADDR,
};

use log::{debug, error, info, warn};

const RUNNER: &str = "/nobackup/research-workspace/runner/";

/// The server's state.
#[derive(Debug)]
struct Server {
    // Lock ordering:
    // - machines
    // - jobs
    // - setup_tasks
    /// Maps available machines to their classes.
    machines: Arc<Mutex<HashMap<String, MachineStatus>>>,

    /// Any variables set by the client. Used for replacement in command strings.
    variables: Arc<Mutex<HashMap<String, String>>>,

    /// Information about jobs, by job ID.
    jobs: Arc<Mutex<HashMap<usize, Job>>>,

    /// Information about matrices, by ID.
    matrices: Arc<Mutex<HashMap<usize, Matrix>>>,

    /// Setup tasks. They are assigned a job ID, but are kind of weird because they can have
    /// multiple commands and are assigned a machine at creation time.
    setup_tasks: Arc<Mutex<HashMap<usize, SetupTask>>>,

    /// The next job ID to be assigned.
    next_jid: AtomicUsize,

    /// The path to the runner. Never changes.
    runner: String,
}

/// Information about a single job.
#[derive(Clone, Debug)]
struct Job {
    /// The job's ID.
    jid: usize,

    /// The command (without replacements).
    cmd: String,

    /// The class of the machines that can run this job.
    class: String,

    /// The location to copy results, if any.
    cp_results: Option<String>,

    /// The current status of the job.
    status: Status,

    /// The mapping of variables at the time the job was created.
    variables: HashMap<String, String>,
}

/// Information about a single setup task.
#[derive(Clone, Debug)]
struct SetupTask {
    /// The job's ID.
    jid: usize,

    /// The machine we are setting up.
    machine: String,

    /// The commands (without replacements).
    cmds: Vec<String>,

    /// The current command to be run.
    current_cmd: usize,

    /// The class of the machines that can run this job.
    class: Option<String>,

    /// The current status of the job.
    status: Status,

    /// The mapping of variables at the time the job was created.
    variables: HashMap<String, String>,
}

/// A collection of jobs that run over the cartesian product of some set of variables.
#[derive(Clone, Debug)]
struct Matrix {
    /// This matrix's ID.
    id: usize,

    /// The command (without replacements).
    cmd: String,

    /// The class of the machines that can run this job.
    class: String,

    /// The location to copy results, if any.
    cp_results: Option<String>,

    /// The variables and their possible values.
    variables: HashMap<String, Vec<String>>,

    /// A list of jobs in this matrix.
    jids: Vec<usize>,
}

/// Information about a single machine.
#[derive(Debug, PartialEq, Eq, Hash)]
struct MachineStatus {
    /// The class of the machine.
    class: String,

    /// What job it is running, if any.
    running: Option<usize>,
}

impl Server {
    /// Creates a new server. Not listening yet.
    pub fn new(runner: String) -> Self {
        Self {
            machines: Arc::new(Mutex::new(HashMap::new())),
            variables: Arc::new(Mutex::new(HashMap::new())),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            setup_tasks: Arc::new(Mutex::new(HashMap::new())),
            matrices: Arc::new(Mutex::new(HashMap::new())),
            next_jid: AtomicUsize::new(0),
            runner,
        }
    }

    pub fn listen(&self, listen_addr: &str) {
        let listener = match TcpListener::bind(listen_addr) {
            Ok(listener) => listener,
            Err(e) => {
                error!("Unable to listen at `{}`: {}", listen_addr, e);
                info!("Exiting");
                std::process::exit(1);
            }
        };

        // accept incoming streams and process them.
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => match self.handle_client(stream) {
                    Ok(()) => {}
                    Err(e) => error!("Error while handling client: {}", e),
                },
                Err(e) => error!("Error while handling client: {}", e),
            }
        }
    }
}

impl Server {
    fn handle_client(&self, mut client: TcpStream) -> std::io::Result<()> {
        let peer_addr = client.peer_addr()?;
        info!("Handling request from {}", peer_addr);

        let mut request = String::new();
        client.read_to_string(&mut request)?;

        let request: JobServerReq = serde_json::from_str(&request)?;

        info!("(request) {}: {:?}", peer_addr, request);

        client.shutdown(Shutdown::Read)?;

        let response = self.handle_request(request)?;

        info!("(response) {}: {:?}", peer_addr, response);

        let response = serde_json::to_string(&response)?;

        client.write_all(response.as_bytes())?;

        Ok(())
    }

    fn handle_request(&self, request: JobServerReq) -> std::io::Result<JobServerResp> {
        use JobServerReq::*;

        let response = match request {
            Ping => JobServerResp::Ok,

            MakeAvailable { addr, class } => {
                let mut locked = self.machines.lock().unwrap();

                // Check if the machine is already there, since it may be running a job.
                let old = locked.get(&addr);

                let running_job = if let Some(old_class) = old {
                    warn!(
                        "Removing {} from old class {}. New class is {}",
                        addr, old_class.class, class
                    );

                    old_class.running
                } else {
                    None
                };

                info!(
                    "Add machine {}/{} with running job: {:?}",
                    addr, class, running_job
                );

                // Add machine
                locked.insert(
                    addr.clone(),
                    MachineStatus {
                        class,
                        running: running_job,
                    },
                );

                // Respond
                JobServerResp::Ok
            }

            RemoveAvailable { addr } => {
                if let Some(old_class) = self.machines.lock().unwrap().remove(&addr) {
                    info!("Removed machine {}/{}", addr, old_class.class);

                    // Cancel any running jobs on the machine.
                    if let Some(running) = old_class.running {
                        self.cancel_job(running);
                    }

                    JobServerResp::Ok
                } else {
                    error!("No such machine: {}", addr);
                    JobServerResp::NoSuchMachine
                }
            }

            ListAvailable => JobServerResp::Machines(
                self.machines
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(addr, info)| (addr.clone(), info.class.clone()))
                    .collect(),
            ),

            ListVars => JobServerResp::Vars(self.variables.lock().unwrap().clone()),

            SetUpMachine { addr, class, cmds } => {
                let jid = self.next_jid.fetch_add(1, Ordering::Relaxed);

                info!(
                    "Create setup task with ID {}. Machine: {}. Cmds: {:?}",
                    jid, addr, cmds
                );

                let variables = self.variables.lock().unwrap().clone();

                self.setup_tasks.lock().unwrap().insert(
                    jid,
                    SetupTask {
                        jid,
                        cmds,
                        class,
                        current_cmd: 0,
                        machine: addr,
                        status: Status::Waiting,
                        variables,
                    },
                );

                JobServerResp::JobId(jid)
            }

            SetVar { name, value } => {
                let old = self
                    .variables
                    .lock()
                    .unwrap()
                    .insert(name.clone(), value.clone());

                info!("Set {}={}", name, value);

                if let Some(old_value) = old {
                    warn!(
                        "Old value of {} was {}. New value is {}",
                        name, old_value, value
                    );
                }

                // Respond
                JobServerResp::Ok
            }

            AddJob {
                class,
                cmd,
                cp_results,
            } => {
                let jid = self.next_jid.fetch_add(1, Ordering::Relaxed);

                info!("Added job {} with class {}: {}", jid, class, cmd);

                let variables = self.variables.lock().unwrap().clone();

                self.jobs.lock().unwrap().insert(
                    jid,
                    Job {
                        jid,
                        cmd,
                        class,
                        cp_results,
                        status: Status::Waiting,
                        variables,
                    },
                );

                JobServerResp::JobId(jid)
            }

            ListJobs => {
                let mut jobs: Vec<_> = self.jobs.lock().unwrap().keys().map(|&k| k).collect();
                jobs.extend(self.setup_tasks.lock().unwrap().keys());

                JobServerResp::Jobs(jobs)

                // drop locks
            }

            CancelJob { jid } => self.cancel_job(jid),

            JobStatus { jid } => {
                if let Some(job) = self.jobs.lock().unwrap().get(&jid) {
                    info!("Stating job {}, {:?}", jid, job);

                    JobServerResp::JobStatus {
                        jid,
                        class: job.class.clone(),
                        cmd: job.cmd.clone(),
                        status: job.status.clone(),
                        variables: job.variables.clone(),
                    }
                } else if let Some(setup_task) = self.setup_tasks.lock().unwrap().get(&jid) {
                    info!("Stating setup task {}, {:?}", jid, setup_task);

                    JobServerResp::JobStatus {
                        jid,
                        class: setup_task
                            .class
                            .as_ref()
                            .map(Clone::clone)
                            .unwrap_or("".into()),
                        cmd: setup_task.cmds[setup_task.current_cmd].clone(),
                        status: setup_task.status.clone(),
                        variables: setup_task.variables.clone(),
                    }
                } else {
                    error!("No such job: {}", jid);
                    JobServerResp::NoSuchJob
                }
            }

            CloneJob { jid } => {
                let mut locked_jobs = self.jobs.lock().unwrap();

                if let Some(job) = locked_jobs.get(&jid).map(Clone::clone) {
                    let new_jid = self.next_jid.fetch_add(1, Ordering::Relaxed);

                    info!("Cloning job {} to job {}, {:?}", jid, new_jid, job);

                    locked_jobs.insert(
                        new_jid,
                        Job {
                            jid: new_jid,
                            cmd: job.cmd.clone(),
                            class: job.class.clone(),
                            cp_results: job.cp_results.clone(),
                            status: Status::Waiting,
                            variables: job.variables.clone(),
                        },
                    );

                    JobServerResp::JobId(new_jid)
                } else {
                    error!("No such job: {}", jid);
                    JobServerResp::NoSuchJob
                }
            }

            AddMatrix {
                mut vars,
                cmd,
                class,
                cp_results,
            } => {
                let id = self.next_jid.fetch_add(1, Ordering::Relaxed);

                // Get the set of base variables, some of which may be overridden by the matrix
                // variables in the template.
                vars.extend(
                    self.variables
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| (k.to_owned(), vec![v.to_owned()])),
                );

                info!(
                    "Create matrix with ID {}. Cmd: {:?}, Vars: {:?}",
                    id, cmd, vars
                );

                let mut jids = vec![];

                // Create a new job for every element in the cartesian product of the variables.
                for config in jobserver::cartesian_product(&vars) {
                    let jid = self.next_jid.fetch_add(1, Ordering::Relaxed);
                    jids.push(jid);

                    let cmd = cmd_replace_vars(&cmd, &config);

                    info!(
                        "[Matrix {}] Added job {} with class {}: {}",
                        id, jid, class, cmd
                    );

                    self.jobs.lock().unwrap().insert(
                        jid,
                        Job {
                            jid,
                            cmd,
                            class: class.clone(),
                            cp_results: cp_results.clone(),
                            status: Status::Waiting,
                            variables: config,
                        },
                    );
                }

                self.matrices.lock().unwrap().insert(
                    id,
                    Matrix {
                        id,
                        cmd,
                        class,
                        cp_results,
                        variables: vars,
                        jids,
                    },
                );

                JobServerResp::MatrixId(id)
            }

            StatMatrix { id } => {
                if let Some(matrix) = self.matrices.lock().unwrap().get(&id) {
                    info!("Stating matrix {}, {:?}", id, matrix);

                    JobServerResp::MatrixStatus {
                        id,
                        class: matrix.class.clone(),
                        cp_results: matrix.cp_results.clone(),
                        cmd: matrix.cmd.clone(),
                        jobs: matrix.jids.clone(),
                        variables: matrix.variables.clone(),
                    }
                } else {
                    error!("No such matrix: {}", id);
                    JobServerResp::NoSuchMatrix
                }
            }
        };

        Ok(response)
    }
}

impl Server {
    /// Start the thread that actually gets stuff done...
    pub fn start_work_thread(self: Arc<Self>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || Self::work_thread(self))
    }

    /// Does the actual work...
    fn work_thread(self: Arc<Self>) {
        // A mapping of running jobs to their thread handles.
        let mut running_job_handles = HashMap::new();

        // Loop over all jobs and setup tasks to see if they can be scheduled on any of the
        // available machines.
        //
        // We don't really try to be efficient here because the amount of time to run the jobs will
        // dwarf the amount of time to run this loop...
        loop {
            {
                let mut locked_machines = self.machines.lock().unwrap();
                let mut locked_jobs = self.jobs.lock().unwrap();

                // Compute a list of idle machines.
                let mut idle_machines: Vec<_> = locked_machines
                    .iter_mut()
                    .filter(|m| m.1.running.is_none())
                    .collect();

                // Find a waiting job that can run on one of the available machines.
                let waiting_job = locked_jobs.iter_mut().find(|j| {
                    if let Status::Waiting = j.1.status {
                        idle_machines.iter().any(|m| m.1.class == j.1.class)
                    } else {
                        false
                    }
                });

                // If there is a task to run, run it.
                if let Some(can_run) = waiting_job {
                    // Get a machine for it to run on. Safe because we already checked that a
                    // machine is available, and we are holding the lock.
                    let machine = idle_machines
                        .iter_mut()
                        .find(|m| m.1.class == can_run.1.class)
                        .unwrap();

                    // Mark the machine and job as running.
                    machine.1.running = Some(*can_run.0);
                    can_run.1.status = Status::Running {
                        machine: machine.0.clone(),
                    };

                    let this = Arc::clone(&self);
                    let jid = *can_run.0;
                    let (sender, receiver) = unbounded();
                    let handle = std::thread::spawn(move || {
                        this.job_thread(jid, receiver);
                    });

                    info!("Running job {} on machine {}", can_run.0, machine.0);

                    running_job_handles.insert(*can_run.0, (handle, sender));
                } else {
                    debug!("No jobs can run.");
                }

                // drop locks
            }

            // Also, start any waiting setup tasks.
            {
                let mut locked_setup_tasks = self.setup_tasks.lock().unwrap();

                for (&jid, setup_task) in locked_setup_tasks.iter_mut() {
                    if let Status::Waiting = setup_task.status {
                        // Mark the task as running.
                        setup_task.status = Status::Running {
                            machine: setup_task.machine.clone(),
                        };

                        let this = Arc::clone(&self);
                        let (sender, receiver) = unbounded();
                        let handle = std::thread::spawn(move || {
                            this.setup_task_thread(jid, receiver);
                        });

                        info!(
                            "Running setup task {} on machine {}",
                            jid, setup_task.machine
                        );

                        running_job_handles.insert(jid, (handle, sender));
                    }
                }

                // drop locks
            }

            // Finally, check for any cancelled task to signal and remove. We are not really
            // in a hurry, so we can be a bit lazy about this for simplicity.
            {
                let mut locked_jobs = self.jobs.lock().unwrap();

                let to_cancel = locked_jobs.iter().find_map(|(&jid, j)| {
                    if let Status::Cancelled = j.status {
                        Some(jid)
                    } else {
                        None
                    }
                });

                if let Some(to_cancel) = to_cancel {
                    let cancelled = locked_jobs.remove(&to_cancel).unwrap(); // safe because we just checked.
                    if let Some((_, cancel_chan)) = running_job_handles.remove(&cancelled.jid) {
                        let _ = cancel_chan.send(());
                    }
                }

                // drop locks
            }

            {
                let mut locked_setup_tasks = self.setup_tasks.lock().unwrap();

                let to_cancel = locked_setup_tasks.iter().find_map(|(&jid, j)| {
                    if let Status::Cancelled = j.status {
                        Some(jid)
                    } else {
                        None
                    }
                });

                if let Some(to_cancel) = to_cancel {
                    let cancelled = locked_setup_tasks.remove(&to_cancel).unwrap(); // safe because we just checked.
                    if let Some((_, cancel_chan)) = running_job_handles.remove(&cancelled.jid) {
                        let _ = cancel_chan.send(());
                    }
                }

                // drop locks
            }

            // Sleep a bit.
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// Mark the given job as cancelled.
    fn cancel_job(&self, jid: usize) -> JobServerResp {
        // We set the status to cancelled and let the job server handle the rest.

        if let Some(job) = self.jobs.lock().unwrap().get_mut(&jid) {
            info!("Cancelling job {}, {:?}", jid, job);

            job.status = Status::Cancelled;

            JobServerResp::Ok
        } else if let Some(setup_task) = self.setup_tasks.lock().unwrap().get_mut(&jid) {
            info!("Stating setup task {}, {:?}", jid, setup_task);

            setup_task.status = Status::Cancelled;

            JobServerResp::Ok
        } else {
            error!("No such job: {}", jid);
            JobServerResp::NoSuchJob
        }
    }

    /// A thread to run the given job.
    fn job_thread(self: Arc<Self>, jid: usize, cancel_chan: Receiver<()>) {
        // Find the command and machine to use. We do a lot of checking whether the job is
        // cancelled.
        let (cmd, machine, cp_results, variables) = {
            let locked_jobs = self.jobs.lock().unwrap();
            if let Some(job) = locked_jobs.get(&jid) {
                (
                    job.cmd.clone(),
                    if let Status::Running { ref machine } = job.status {
                        machine.clone()
                    } else {
                        error!(
                            "Unable to find machine assignment for job {}: {:?}",
                            jid, job
                        );
                        return;
                    },
                    job.cp_results.clone(),
                    job.variables.clone(),
                )
            } else {
                match cancel_chan.try_recv() {
                    Err(TryRecvError::Disconnected) | Ok(()) => {
                        warn!("Job {} was cancelled before running.", jid);
                    }
                    Err(TryRecvError::Empty) => {
                        error!("Job {} was not found.", jid);
                    }
                }

                return;
            }

            // drop locks
        };

        match cancel_chan.try_recv() {
            Err(TryRecvError::Disconnected) | Ok(()) => {
                warn!(
                    "Job {} was cancelled before running. Cmd {}, Machine {}",
                    jid, cmd, machine
                );
                return;
            }
            _ => {}
        }

        // Actually run the command now.
        let result = Self::run_cmd(jid, &machine, &cmd, cancel_chan, &variables, &self.runner);

        // Check the results.
        match result {
            Ok(Some(results_path)) => {
                info!("Job {} completed with results path: {}", jid, results_path);

                // Update status
                {
                    let mut locked_jobs = self.jobs.lock().unwrap();
                    if let Some(job) = locked_jobs.get_mut(&jid) {
                        job.status = Status::Done {
                            machine: machine.clone(),
                            output: Some(results_path.clone()),
                        };
                    } else {
                        // doesn't matter, since we're done anyway
                    }

                    // drop locks
                }

                if let Some(cp_results) = cp_results {
                    // Copy via SCP
                    info!("Copying results (job {}) to {}", jid, cp_results);

                    // HACK: assume all machine names are in the form HOSTNAME:PORT, and all
                    // results are output to `vm_shared/results/` on the remote.
                    let machine_ip = machine.split(":").next().unwrap();

                    let scp_result = std::process::Command::new("scp")
                        .arg(&format!(
                            "{}:vm_shared/results/{}",
                            machine_ip, results_path
                        ))
                        .arg(cp_results)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output();

                    match scp_result {
                        Ok(..) => info!("Finished copying results for job {}.", jid),
                        Err(e) => error!("Error copy results for job {}: {}", jid, e),
                    }
                } else {
                    warn!("Discarding results even though they were produced");
                }
            }

            Ok(None) => {
                // Update status
                {
                    let mut locked_jobs = self.jobs.lock().unwrap();
                    if let Some(job) = locked_jobs.get_mut(&jid) {
                        job.status = Status::Done {
                            machine: machine.clone(),
                            output: None,
                        };
                    } else {
                        // doesn't matter, since we're done anyway
                    }

                    // drop locks
                }

                if let Some(cp_results) = cp_results {
                    warn!(
                        "Job {} completed with no results, but a path was given to \
                         copy results to: {}",
                        jid, cp_results
                    );
                } else {
                    info!("Job {} completed with no results", jid);
                }
            }

            Err(e) => {
                error!(
                    "Job {}, cmd {}, machine{} terminated with error {}",
                    jid, cmd, machine, e
                );

                // Update status
                {
                    let mut locked_jobs = self.jobs.lock().unwrap();
                    if let Some(job) = locked_jobs.get_mut(&jid) {
                        job.status = Status::Failed {
                            machine: Some(machine.clone()),
                            error: format!("{}", e),
                        };
                    } else {
                        // doesn't matter, since we're done anyway
                    }

                    // drop locks
                }
            }
        }

        // Mark machine as available again
        {
            let mut locked_machines = self.machines.lock().unwrap();
            let locked_machine = locked_machines.get_mut(&machine);

            if let Some(locked_machine) = locked_machine {
                info!("Releasing machine {:?}", locked_machine);
                locked_machine.running = None;
            } else {
                // Not sure why this would happen, but clearly it doesn't matter...
                error!("Unable to release machine {}", machine);
            }
        }
    }

    /// A thread that runs a setup task.
    fn setup_task_thread(self: Arc<Self>, jid: usize, cancel_chan: Receiver<()>) {
        // Get the task.
        let setup_task = if let Some(setup_task) = self.setup_tasks.lock().unwrap().get(&jid) {
            // We make a local copy so we don't need to hold the lock.
            setup_task.clone()
        } else {
            match cancel_chan.try_recv() {
                Err(TryRecvError::Disconnected) | Ok(()) => {
                    warn!("Setup task {} was cancelled before running.", jid);
                }
                Err(TryRecvError::Empty) => {
                    error!("Setup task {} was not found.", jid);
                }
            }

            return;
        };

        // Check for cancellation.
        match cancel_chan.try_recv() {
            Err(TryRecvError::Disconnected) | Ok(()) => {
                warn!(
                    "Setup task {} was cancelled before running. {:?}",
                    jid, setup_task
                );
                return;
            }
            _ => {}
        }

        let variables = self.variables.lock().unwrap().clone();

        // Execute all cmds
        for (i, cmd) in setup_task.cmds.iter().enumerate() {
            // Check for cancellation.
            match cancel_chan.try_recv() {
                Err(TryRecvError::Disconnected) | Ok(()) => {
                    warn!(
                        "Setup task {} was cancelled while running. {:?}",
                        jid, setup_task
                    );
                    return;
                }
                _ => {}
            }

            // Update the progress...
            {
                let mut locked_tasks = self.setup_tasks.lock().unwrap();
                let task = locked_tasks.get_mut(&jid);

                if let Some(task) = task {
                    task.current_cmd = i;
                } else {
                    match cancel_chan.try_recv() {
                        Err(TryRecvError::Disconnected) | Ok(()) => {
                            warn!("Setup task {} was cancelled while running.", jid);
                        }
                        Err(TryRecvError::Empty) => {
                            error!("Setup task {} was not found.", jid);
                        }
                    }

                    return;
                }

                // drop locks
            }

            let result = Self::run_cmd(
                jid,
                &setup_task.machine,
                cmd,
                cancel_chan.clone(),
                &variables,
                &self.runner,
            );

            match result {
                Ok(Some(results_path)) => {
                    warn!(
                        "Setup task {} produced results at {} {}",
                        jid, setup_task.machine, results_path
                    );
                }

                Ok(None) => {
                    info!("Setup task {} completed", jid);
                }

                Err(e) => {
                    error!(
                        "Setup task {} cmd {} terminated with error {}. Aborting setup task {:?}",
                        jid, cmd, e, setup_task
                    );

                    // Update status
                    {
                        let mut locked_setup_tasks = self.setup_tasks.lock().unwrap();
                        if let Some(job) = locked_setup_tasks.get_mut(&jid) {
                            job.status = Status::Failed {
                                machine: Some(setup_task.machine.clone()),
                                error: format!("{}", e),
                            };
                        } else {
                            // doesn't matter, since we're done anyway
                        }

                        // drop locks
                    }

                    return;
                }
            }
        }

        // Set the status of the task to done!
        {
            let mut locked_tasks = self.setup_tasks.lock().unwrap();
            let task = locked_tasks.get_mut(&jid);

            if let Some(task) = task {
                task.status = Status::Done {
                    machine: task.machine.clone(),
                    output: None,
                };
            } else {
                match cancel_chan.try_recv() {
                    Err(TryRecvError::Disconnected) | Ok(()) => {
                        warn!("Setup task {} was cancelled while running.", jid);
                    }
                    Err(TryRecvError::Empty) => {
                        error!("Setup task {} was not found.", jid);
                    }
                }

                return;
            }

            // drop locks
        }

        // If the class is set, make the machine available
        if let Some(class) = setup_task.class {
            info!(
                "Adding machine {} to class {} after setup task {} completed.",
                setup_task.machine, class, jid
            );

            let mut locked = self.machines.lock().unwrap();

            // Check if the machine is already there, since it may be running a job.
            let old = locked.get(&setup_task.machine);

            let running_job = if let Some(old_class) = old {
                error!(
                    "After setup task {}: Removing {} from old class {}. New class is {}",
                    jid, setup_task.machine, old_class.class, class
                );

                old_class.running
            } else {
                None
            };

            info!(
                "Add machine {}/{} with running job: {:?}",
                setup_task.machine, class, running_job
            );

            // Add machine
            locked.insert(
                setup_task.machine.clone(),
                MachineStatus {
                    class,
                    running: running_job,
                },
            );

            // drop locks
        }
    }

    fn run_cmd(
        jid: usize,
        machine: &str,
        cmd: &str,
        cancel_chan: Receiver<()>,
        variables: &HashMap<String, String>,
        runner: &str,
    ) -> std::io::Result<Option<String>> {
        let cmd = cmd_replace_machine(&cmd_replace_vars(&cmd, variables), &machine);

        // Open a tmp file for the cmd output
        let tmp_file_name = cmd_to_path(&cmd);
        let mut tmp_file = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(&tmp_file_name)?;

        let stderr_file_name = format!("{}.err", tmp_file_name);
        let stderr_file = OpenOptions::new()
            .truncate(true)
            .write(true)
            .create(true)
            .open(&stderr_file_name)?;

        // Run the command, piping output to a buf reader.
        let mut child = std::process::Command::new("cargo")
            .args(&["run", "--", "--print_results_path"])
            .args(&cmd.split_whitespace().collect::<Vec<_>>())
            .current_dir(runner)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .spawn()?;

        // HACK: the std API doesn't allow us to kill the child while reading stdout, so instead we
        // take the pid and just send the kill signal manually.
        let child_pid = child.id();

        let reader = BufReader::new(child.stdout.as_mut().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "Could not capture standard output.",
            )
        })?);

        // Wait for the output to check for the results path. We do this in a side thread and let
        // the main thread poll for cancellations.
        let (results_path_chan_s, results_path_chan_r) = unbounded();

        let results_path = crossbeam::scope(|s| {
            s.spawn(|_| {
                let mut results_path = None;
                reader
                    .lines()
                    .filter_map(|line| line.ok())
                    .for_each(|line| {
                        // Check if there was a results path printed.
                        if line.starts_with("RESULTS: ") {
                            results_path = Some(line[9..].to_string());
                        }

                        match writeln!(tmp_file, "{}", line) {
                            Ok(..) => {}
                            Err(e) => {
                                error!("Unable to write to tmp file: {} {}", e, line);
                            }
                        }
                    });

                // Receiver may have closed
                let _ = results_path_chan_s.send(results_path);
            });

            // Wait for either the child to finish or the cancel signal from the server.
            select! {
                recv(results_path_chan_r) -> results_path => match results_path {
                    Ok(results_path) => {
                        info!("Job {} completed. Results path: {:?}", jid, results_path);
                        Ok(results_path)
                    }
                    Err(chan_err) => {
                        error!("Job completed, but there was an error reading results \
                               path from thread: {}", chan_err);
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "Could not capture standard output.",
                        ))
                    }
                },
                recv(cancel_chan) -> _ => {
                    info!("Killing job {}, cmd {}, machine {}", jid, cmd, machine);

                    // SIGKILL the child
                    unsafe {
                        libc::kill(child_pid as i32, 9);
                    }

                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Job was cancelled."
                    ))
                },
            }
        });

        let results_path = match results_path {
            Ok(results_path) => results_path,
            Err(err) => {
                error!("Thread panicked while running job {}: {:?}", jid, err);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Thread panicked",
                ));
            }
        };

        let exit = child.wait()?;

        if exit.success() {
            results_path
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Job failed.",
            ))
        }
    }
}

fn main() {
    let matches = clap_app! { jobserver =>
        (about: "Serves jobs to machines")
        (@arg ADDR: --addr +takes_value
         "The IP:ADDR for the server to listen on for commands \
         (defaults to `localhost:3030`)")
        (@arg RUNNER: --runner +takes_value
         "Path to the runner cargo workspace \
         (defaults to /nobackup/research-workspace/runner/)")
    }
    .get_matches();

    let addr = matches.value_of("ADDR").unwrap_or(SERVER_ADDR.into());
    let runner = matches.value_of("RUNNER").unwrap_or(RUNNER);

    // Start logger
    env_logger::init();

    info!("Starting server at {}", addr);

    // Listen for client requests on the main thread, while we do work in the background.
    let server = Arc::new(Server::new(runner.into()));
    Arc::clone(&server).start_work_thread();
    server.listen(addr);
}
