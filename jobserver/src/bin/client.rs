//! Client implmentation

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

use clap::clap_app;

use jobserver::{JobServerReq, JobServerResp, Status, SERVER_ADDR};

use prettytable::{cell, row, Table};

fn main() {
    let matches = clap_app! { client =>
        (about: "CLI client for the jobserver")
        (@arg ADDR: --address +takes_value
         "The server IP:PORT (defaults to `localhost:3030`)")

        (@subcommand ping =>
            (about: "Ping the server")
        )

        (@subcommand mkavail =>
            (about: "Make the given machine available with the given class.")
            (@arg ADDR: +required
             "The IP:PORT of the machine")
            (@arg CLASS: +required
             "The class of the machine")
        )

        (@subcommand rmavail =>
            (about: "Remove the given machine from the available pool.")
            (@arg ADDR: +required
             "The IP:PORT of the machine")
        )

        (@subcommand lsavail =>
            (about: "List available machines.")
        )

        (@subcommand setup =>
            (about: "Set up the given machine using the given command")
            (@arg ADDR: +required
             "The IP:PORT of the machine")
            (@arg CMD: +required ...
             "The setup commands, each as a single string")
            (@arg CLASS: --class +takes_value
             "If passed, the machine is added to the class after setup.")
        )

        (@subcommand lsvars =>
            (about: "List variables and their values.")
        )

        (@subcommand setvar =>
            (about: "Set the given variable to be substituted in commands")
            (@arg NAME: +required
             "The variable name")
            (@arg VALUE: +required
             "The class of the machine")
        )

        (@subcommand addjob =>
            (about: "Add a job to be run on the given class of machine.")
            (@arg CLASS: +required
             "The class of machine that can execute the job")
            (@arg CMD: +required
             "The command to execute")
            (@arg CP_PATH:
             "(Optional) The location on this host to copy results to")
        )

        (@subcommand lsjobs =>
            (about: "List all jobs.")
        )

        (@subcommand canceljob =>
            (about: "Cancel a running or scheduled job.")
            (@arg JID: +required {is_usize}
             "The job ID of the job to cancel")
        )

        (@subcommand statjob =>
            (about: "Get information on the status of a job.")
            (@arg JID: +required {is_usize}
             "The job ID of the job")
        )

        (@subcommand clonejob =>
            (about: "Clone a job.")
            (@arg JID: +required {is_usize}
             "The job ID of the job to clone.")
        )
    }
    .setting(clap::AppSettings::SubcommandRequired)
    .setting(clap::AppSettings::DisableVersion)
    .get_matches();

    let addr = matches.value_of("ADDR").unwrap_or(SERVER_ADDR);

    // Form the request
    let request = form_request(&matches);

    // Special case the `lsjobs` command for convenience.
    match request {
        JobServerReq::ListJobs => {
            let job_ids = make_request(addr, request);

            if let JobServerResp::Jobs(mut job_ids) = job_ids {
                // Sort by jid
                job_ids.sort();

                // Print a nice human-readable table
                let mut table = Table::new();

                table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

                table.set_titles(row![ Fwbu =>
                    "Job", "Status", "Class", "Command", "Machine", "Output"
                ]);

                // Query each job's status
                for &jid in job_ids.iter() {
                    let status = make_request(addr, JobServerReq::JobStatus { jid });

                    match status {
                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status: Status::Cancelled,
                        } => {
                            table.add_row(row![b->jid, Fri->"Cancelled", class, cmd, "", ""]);
                        }

                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status: Status::Waiting,
                        } => {
                            table.add_row(row![b->jid, Fb->"Waiting", class, cmd, "", ""]);
                        }

                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status:
                                Status::Done {
                                    machine,
                                    output: None,
                                },
                        } => {
                            table.add_row(row![b->jid, Fm->"Done", class, cmd, machine, ""]);
                        }

                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status:
                                Status::Done {
                                    machine,
                                    output: Some(path),
                                },
                        } => {
                            table.add_row(row![b->jid, Fg->"Done", class, cmd, machine, Fg->path]);
                        }

                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status: Status::Failed { error },
                        } => {
                            table.add_row(row![b->jid, Frbu->"Failed", class, cmd, "", error]);
                        }

                        JobServerResp::JobStatus {
                            jid,
                            cmd,
                            class,
                            status: Status::Running { machine },
                        } => {
                            table.add_row(row![b->jid, Fy->"Running", class, cmd, machine, ""]);
                        }

                        JobServerResp::Jobs(..)
                        | JobServerResp::Ok
                        | JobServerResp::Vars(..)
                        | JobServerResp::JobId(..)
                        | JobServerResp::Machines(..)
                        | JobServerResp::NoSuchJob
                        | JobServerResp::NoSuchMachine => unreachable!(),
                    }
                }

                table.printstd();
            } else {
                unreachable!();
            }
        }

        request => {
            let response = make_request(addr, request);
            println!("Server response: {:?}", response);
        }
    }
}

fn make_request(server_addr: &str, request: JobServerReq) -> JobServerResp {
    // Connect to server
    let mut tcp_stream = TcpStream::connect(server_addr).expect("Unable to connect to server");

    // Send request
    let request = serde_json::to_string(&request).expect("Unable to serialize message");
    tcp_stream
        .write_all(request.as_bytes())
        .expect("Unable to send message to server");

    // Send EOF
    tcp_stream
        .shutdown(Shutdown::Write)
        .expect("Unable to send EOF to server");

    // Wait for response.
    let mut response = String::new();
    tcp_stream
        .read_to_string(&mut response)
        .expect("Unable to read server response");

    serde_json::from_str(&response).expect("Unable to deserialize server response")
}

fn form_request(matches: &clap::ArgMatches<'_>) -> JobServerReq {
    match matches.subcommand() {
        ("ping", Some(_sub_m)) => JobServerReq::Ping,

        ("mkavail", Some(sub_m)) => JobServerReq::MakeAvailable {
            addr: sub_m.value_of("ADDR").unwrap().into(),
            class: sub_m.value_of("CLASS").unwrap().into(),
        },

        ("rmavail", Some(sub_m)) => JobServerReq::RemoveAvailable {
            addr: sub_m.value_of("ADDR").unwrap().into(),
        },

        ("lsavail", Some(_sub_m)) => JobServerReq::ListAvailable,

        ("setup", Some(sub_m)) => JobServerReq::SetUpMachine {
            addr: sub_m.value_of("ADDR").unwrap().into(),
            cmds: sub_m.values_of("CMD").unwrap().map(String::from).collect(),
            class: sub_m.value_of("CLASS").map(Into::into),
        },

        ("setvar", Some(sub_m)) => JobServerReq::SetVar {
            name: sub_m.value_of("NAME").unwrap().into(),
            value: sub_m.value_of("VALUE").unwrap().into(),
        },

        ("addjob", Some(sub_m)) => JobServerReq::AddJob {
            class: sub_m.value_of("CLASS").unwrap().into(),
            cmd: sub_m.value_of("CMD").unwrap().into(),
            cp_results: sub_m.value_of("CP_PATH").map(Into::into),
        },

        ("lsvars", Some(_sub_m)) => JobServerReq::ListVars,

        ("lsjobs", Some(_sub_m)) => JobServerReq::ListJobs,

        ("canceljob", Some(sub_m)) => JobServerReq::CancelJob {
            jid: sub_m.value_of("JID").unwrap().parse().unwrap(),
        },

        ("statjob", Some(sub_m)) => JobServerReq::JobStatus {
            jid: sub_m.value_of("JID").unwrap().parse().unwrap(),
        },

        ("clonejob", Some(sub_m)) => JobServerReq::CloneJob {
            jid: sub_m.value_of("JID").unwrap().parse().unwrap(),
        },

        _ => unreachable!(),
    }
}

fn is_usize(s: String) -> Result<(), String> {
    s.as_str()
        .parse::<usize>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}
