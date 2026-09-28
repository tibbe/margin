mod cli;

use clap::Parser;
use margin_core::comments;
use std::path::PathBuf;
use std::process::ExitCode;

/// Opens files in the editor. Unless asked to stay in the foreground, the
/// editor runs detached so that `margin open plan.md` returns at once, which
/// is what an agent's shell tool needs. An already running editor opens the
/// files itself.
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
    Ok(0)
}

/// Hands the files to Margin.app through Launch Services, which starts it
/// or reuses the running one. With `foreground`, waits until it quits.
#[cfg(target_os = "macos")]
fn launch(files: Vec<PathBuf>, foreground: bool) -> anyhow::Result<i32> {
    // Launch Services only opens files that exist; a new document starts
    // empty either way.
    for f in &files {
        if !f.exists() {
            std::fs::write(f, "")?;
        }
    }
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.args(["-b", "io.github.tibbe.Margin"]);
    if foreground {
        cmd.arg("-W");
    }
    let status = cmd.args(&files).status()?;
    if !status.success() {
        anyhow::bail!("could not start Margin.app; is it installed?");
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
