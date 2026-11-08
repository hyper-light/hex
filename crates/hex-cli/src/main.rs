//! The `hex` binary.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<Option<String>> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string().ok())
        .collect();
    // The kernel's first process, `hex` is a Shards guest's init, whatever words the kernel passes it.
    #[cfg(target_os = "linux")]
    if hex_cli::guest::runs_as_init(std::process::id(), std::env::args_os().next().as_deref()) {
        return ExitCode::from(hex_cli::guest::init_main());
    }
    // `hex selftest-box <act> …` runs a small program inside a box (hex_cli::selftest); it ends the
    // process itself rather than returning text, so it is handled before the text replies.
    if let [Some(first), rest @ ..] = args.as_slice()
        && first == "selftest-box"
        && rest.iter().all(Option::is_some)
    {
        let acts: Vec<String> = rest.iter().flatten().cloned().collect();
        if let Some(status) = hex_cli::selftest::run(&acts) {
            return ExitCode::from(status);
        }
    }
    // `hex node --socket …` serves the Runtime seam on the node agent's local socket.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let [Some(first), rest @ ..] = args.as_slice()
        && first == "node"
        && rest.iter().all(Option::is_some)
    {
        let rest: Vec<String> = rest.iter().flatten().cloned().collect();
        return ExitCode::from(hex_cli::node::main(&rest));
    }
    // The `oci` class's command line and its re-exec'd container init (Linux only).
    #[cfg(target_os = "linux")]
    if let [Some(first), rest @ ..] = args.as_slice()
        && rest.iter().all(Option::is_some)
    {
        if first == "__oci-init" {
            return ExitCode::from(hex_cli::oci::init());
        }
        if first == "__oci-exec" {
            return ExitCode::from(hex_cli::oci::exec_helper());
        }
        if first == "__oci-seed" {
            return ExitCode::from(hex_cli::oci::seed());
        }
        if first == "oci" {
            let rest: Vec<String> = rest.iter().flatten().cloned().collect();
            return ExitCode::from(hex_cli::oci::main(&rest));
        }
        // `hex guest relay …` relays a Shards guest's vsock to its node agent's socket.
        if first == "guest" {
            let rest: Vec<String> = rest.iter().flatten().cloned().collect();
            return ExitCode::from(hex_cli::guest::main(&rest));
        }
        // `hex box run <limits> -- program args…` runs a program in a process box.
        if first == "box"
            && let [Some(verb), box_args @ ..] = rest
            && verb == "run"
            && box_args.iter().all(Option::is_some)
        {
            let box_args: Vec<String> = box_args.iter().flatten().cloned().collect();
            return ExitCode::from(hex_cli::box_run::main(&box_args));
        }
    }
    let reply = hex_cli::reply(&args);
    let wrote = if reply.to_stderr {
        std::io::stderr().lock().write_all(reply.text.as_bytes())
    } else {
        std::io::stdout().lock().write_all(reply.text.as_bytes())
    };
    // A closed output is a failure the status reports, never a panic.
    match wrote {
        Ok(()) => ExitCode::from(reply.status),
        Err(_) => ExitCode::from(1),
    }
}
