mod cli;
mod comments;
mod md;
mod status;
mod ui;

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

/// Opens files in the editor. Unless asked to stay in the foreground, the
/// editor runs detached so that `margin open plan.md` returns at once, which
/// is what an agent's shell tool needs. An already running editor opens the
/// files itself.
fn open(files: Vec<PathBuf>, foreground: bool) -> anyhow::Result<i32> {
    let stay = foreground
        || std::env::var_os("MARGIN_FOREGROUND").is_some()
        || std::env::var_os("MARGIN_SCRIPT").is_some();
    if stay {
        return ui::run(files);
    }
    use std::os::unix::process::CommandExt;
    let files: Vec<PathBuf> = files
        .into_iter()
        .map(|f| std::path::absolute(&f).unwrap_or(f))
        .collect();
    // Report problems here; the detached editor has no terminal.
    for f in &files {
        comments::canonical_doc_path(f)?;
        comments::read_doc(f)?;
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
