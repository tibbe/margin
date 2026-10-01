mod cli;

#[cfg(target_os = "macos")]
mod macos;

use clap::Parser;
use margin_core::comments;
use margin_core::comments::handoff;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// How long `margin open` gives the editor to show the files: the default
/// wait for an Apple event reply, such as the one delivering them on macOS.
/// It only bounds a launch that failed; a healthy one returns sooner.
const SHOW_TIMEOUT: Duration = Duration::from_secs(60);

/// Waits until an editor shows each of the files, so that `margin wait`
/// right after `margin open` finds them shown.
fn await_shown(files: &[PathBuf]) -> anyhow::Result<()> {
    let start = Instant::now();
    for f in files {
        while handoff::viewers(f)? == 0 {
            if start.elapsed() > SHOW_TIMEOUT {
                anyhow::bail!(
                    "Margin didn't open {} within {} seconds",
                    f.display(),
                    SHOW_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

/// Opens files in the editor. Unless asked to stay in the foreground, the
/// editor runs detached so that `margin open plan.md` returns as soon as the
/// editor shows the files, which is what an agent's shell tool needs. An
/// already running editor opens the files itself.
fn open(files: Vec<PathBuf>, foreground: bool) -> anyhow::Result<i32> {
    let files: Vec<PathBuf> = files
        .into_iter()
        .map(|f| std::path::absolute(&f).unwrap_or(f))
        .collect();
    // Report problems here; the detached editor has no terminal.
    for f in &files {
        comments::canonical_doc_path(f)?;
        comments::read_doc(f)?;
    }
    launch(files, foreground)
}

#[cfg(not(target_os = "macos"))]
fn launch(files: Vec<PathBuf>, foreground: bool) -> anyhow::Result<i32> {
    use std::os::unix::process::CommandExt;
    let stay = foreground
        || std::env::var_os("MARGIN_FOREGROUND").is_some()
        || std::env::var_os("MARGIN_SCRIPT").is_some();
    if stay {
        return margin_gtk::run(files);
    }
    std::process::Command::new(std::env::current_exe()?)
        .args(&files)
        .env("MARGIN_FOREGROUND", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    await_shown(&files)?;
    Ok(0)
}

/// Hands files to the containing app. With `foreground`, waits until the
/// app quits.
#[cfg(target_os = "macos")]
fn launch(files: Vec<PathBuf>, foreground: bool) -> anyhow::Result<i32> {
    // A standalone CLI could only pick an app by bundle ID, which can match
    // any build Launch Services has registered, so it opens none.
    let Some(bundle) = macos::app_bundle() else {
        anyhow::bail!(
            "this margin isn't inside Margin.app, so it can't open documents; \
             use Margin.app/Contents/Helpers/margin"
        );
    };
    // Launch Services only opens files that exist; a new document starts
    // empty either way.
    for f in &files {
        if !f.exists() {
            std::fs::write(f, "")?;
        }
    }
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.arg("-a").arg(&bundle);
    if foreground {
        cmd.arg("-W");
    }
    let status = cmd.args(&files).status()?;
    if !status.success() {
        anyhow::bail!("could not start {}", bundle.display());
    }
    if !foreground {
        await_shown(&files)?;
    }
    Ok(0)
}

fn main() -> ExitCode {
    let args = cli::Cli::parse();
    let result = match args.command {
        None => open(args.files, args.foreground),
        Some(cli::Command::Open { files }) => open(files, args.foreground),
        Some(cmd) => cli::run(cmd),
    };
    match result {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("margin: {e:#}");
            ExitCode::from(1)
        }
    }
}
