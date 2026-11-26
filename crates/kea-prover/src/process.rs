//! Running a tool with a wall-clock cap and measuring the CPU it used.
//!
//! Provers misbehave in two ways that matter here: they run forever, and they ignore a polite
//! stop while a solver is mid-query. So each tool runs in its own process group; at the cap the
//! whole group gets SIGTERM, and SIGKILL after a grace period.
//!
//! CPU is measured per run, not per engine: several runs share one machine, so the engine's
//! own child-usage counter would mix them. GNU `time` wraps the tool and reports the user and
//! system time of the tool and every process it waited for, which includes its solvers.

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

/// Where GNU `time` is installed on the build machine.
const GNU_TIME: &str = "/usr/bin/time";

/// How long a run may take.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Wall-clock cap.
    pub wall: Duration,
    /// Time between SIGTERM and SIGKILL once the cap is hit.
    pub grace: Duration,
}

/// What a finished run reported.
#[derive(Clone, Debug)]
pub struct Finished {
    /// The tool's exit code, when it exited on its own.
    pub exit_code: Option<i32>,
    /// Whether the cap was hit.
    pub timed_out: bool,
    /// User plus system CPU seconds of the tool and its children.
    pub cpu_seconds: f64,
    /// Wall-clock time.
    pub wall: Duration,
}

impl Finished {
    /// Whether the tool exited with status 0.
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// One tool invocation.
#[derive(Clone, Debug)]
pub struct Run<'a> {
    /// The program.
    pub program: &'a str,
    /// Its arguments.
    pub args: Vec<String>,
    /// The working directory.
    pub dir: &'a Path,
    /// Extra environment.
    pub env: Vec<(String, String)>,
    /// Where standard output goes.
    pub stdout: PathBuf,
    /// Where standard error goes.
    pub stderr: PathBuf,
    /// The limits.
    pub limits: Limits,
}

/// Runs the tool to completion or to its cap.
///
/// # Errors
/// When the tool cannot be started or its output files cannot be created.
pub async fn run(spec: Run<'_>) -> std::io::Result<Finished> {
    // `time` runs inside `spec.dir`, so it gets the bare name; joined to a relative `spec.dir` it
    // would point below itself, `time` could not open it, and the tool would never start.
    let usage_name = format!(".cpu-{}", std::process::id());
    let usage_file = spec.dir.join(&usage_name);
    let mut command = Command::new(GNU_TIME);
    command
        .arg("-o")
        .arg(&usage_name)
        .args(["-f", "%U %S", spec.program])
        .args(&spec.args)
        .current_dir(spec.dir)
        // Tools run model-written code, so they inherit nothing but a path and a home: the
        // engine's own secrets (its API key) must never be readable from a test.
        .env_clear()
        .envs(["PATH", "HOME"].into_iter().filter_map(|key| Some((key, std::env::var(key).ok()?))))
        .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&spec.stdout)?)
        .stderr(std::fs::File::create(&spec.stderr)?)
        .process_group(0)
        .kill_on_drop(true);
    let started = Instant::now();
    let mut child = command.spawn()?;
    let group = child.id().and_then(|id| i32::try_from(id).ok()).map(Pid::from_raw);
    let (status, timed_out) = if let Ok(status) = tokio::time::timeout(spec.limits.wall, child.wait()).await {
        (status?, false)
    } else {
        if let Some(group) = group {
            let _ = killpg(group, Signal::SIGTERM);
        }
        let polite = tokio::time::timeout(spec.limits.grace, child.wait()).await;
        // `time` itself dies on SIGTERM while a solver that ignores it would live on as an
        // orphan, so whatever happened, nothing in the group outlives the grace period.
        if let Some(group) = group {
            let _ = killpg(group, Signal::SIGKILL);
        }
        let status = if let Ok(status) = polite { status? } else { child.wait().await? };
        (status, true)
    };
    let wall = started.elapsed();
    let cpu_seconds = read_usage(&usage_file).unwrap_or(wall.as_secs_f64());
    let _ = std::fs::remove_file(&usage_file);
    Ok(Finished { exit_code: if timed_out { None } else { status.code() }, timed_out, cpu_seconds, wall })
}

/// User plus system seconds from GNU `time`'s output file. A killed run may leave no usable
/// line, in which case the caller counts wall time instead.
fn read_usage(path: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let mut fields = line.split_whitespace().map(str::parse::<f64>);
    match (fields.next(), fields.next()) {
        (Some(Ok(user)), Some(Ok(system))) => Some(user + system),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_lines_add_user_and_system_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("usage");
        std::fs::write(&file, "Command terminated by signal 15\n12.50 0.75\n").unwrap();
        assert!((read_usage(&file).unwrap() - 13.25).abs() < 1e-9);
        std::fs::write(&file, "garbage\n").unwrap();
        assert_eq!(read_usage(&file), None);
    }

    #[tokio::test]
    async fn tools_see_no_secrets_from_the_engine() {
        if !Path::new(GNU_TIME).exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let finished = run(Run {
            program: "sh",
            args: vec!["-c".into(), "env > seen".into()],
            dir: dir.path(),
            env: vec![("RH_RPC".into(), "https://rpc.example".into())],
            stdout: dir.path().join("out"),
            stderr: dir.path().join("err"),
            limits: Limits { wall: Duration::from_secs(10), grace: Duration::from_secs(1) },
        })
        .await
        .unwrap();
        assert!(finished.succeeded());
        let seen = std::fs::read_to_string(dir.path().join("seen")).unwrap();
        assert!(seen.contains("RH_RPC=https://rpc.example"));
        assert!(seen.lines().all(|line| !line.starts_with("ANTHROPIC_") && !line.starts_with("CARGO_")), "{seen}");
    }

    #[tokio::test]
    async fn a_relative_working_directory_runs_the_tool() {
        if !Path::new(GNU_TIME).exists() {
            return;
        }
        // Relative to the test's directory, as `--work work` is to the engine's.
        let dir = tempfile::tempdir_in(".").unwrap();
        let relative = Path::new(dir.path().file_name().unwrap());
        let finished = run(Run {
            program: "sh",
            args: vec!["-c".into(), "echo ran > ran".into()],
            dir: relative,
            env: vec![],
            stdout: relative.join("out"),
            stderr: relative.join("err"),
            limits: Limits { wall: Duration::from_secs(10), grace: Duration::from_secs(1) },
        })
        .await
        .unwrap();
        assert!(finished.succeeded(), "{}", std::fs::read_to_string(relative.join("err")).unwrap_or_default());
        assert!(relative.join("ran").exists());
    }

    #[tokio::test]
    async fn the_cap_stops_a_process_that_will_not_finish() {
        if !Path::new(GNU_TIME).exists() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let finished = run(Run {
            program: "sh",
            args: vec!["-c".into(), "trap '' TERM; sleep 30".into()],
            dir: dir.path(),
            env: vec![],
            stdout: dir.path().join("out"),
            stderr: dir.path().join("err"),
            limits: Limits { wall: Duration::from_millis(300), grace: Duration::from_millis(300) },
        })
        .await
        .unwrap();
        assert!(finished.timed_out);
        assert!(finished.wall < Duration::from_secs(5));
    }
}
