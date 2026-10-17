//! Binary entry point: parse arguments, build the real adapters, run the
//! use case, map the result to an exit code. Kept trivial on purpose;
//! excluded from coverage.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use herdr_command_palette::adapters::clock::SystemClock;
use herdr_command_palette::adapters::files::TempFiles;
use herdr_command_palette::adapters::fzf::Fzf;
use herdr_command_palette::adapters::git::Git;
use herdr_command_palette::adapters::herdr_config::TomlConfigFile;
use herdr_command_palette::adapters::herdr_socket::UnixSocketHerdr;
use herdr_command_palette::adapters::history_db::SqliteHistory;
use herdr_command_palette::adapters::process::Processes;
use herdr_command_palette::adapters::settings_file::TomlSettingsFile;
use herdr_command_palette::adapters::terminal::Tty;
use herdr_command_palette::app::{self, Context};
use herdr_command_palette::cli::Cli;
use herdr_command_palette::domain::env::PluginEnv;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let env = PluginEnv::from_lookup(|name| std::env::var(name).ok());
    let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(app::PROGRAM));
    let cwd = std::env::current_dir().ok();
    let herdr = UnixSocketHerdr::new(&env.socket_path).with_retry(app::retry_policy(&cli.command));
    let context = Context {
        env: &env,
        program: &program,
        cwd: cwd.as_deref(),
        herdr: &herdr,
        config: &TomlConfigFile::new(&env.config_path),
        settings: &TomlSettingsFile::new(app::settings_path(&cli.command, &env)),
        picker: &Fzf::new(),
        spawner: &Processes::new(),
        terminal: &Tty::new(),
        files: &TempFiles::new(),
        clock: &SystemClock,
        history: &SqliteHistory::new(env.history_db()),
        branches: &Git::new(),
    };
    let result = app::run(&cli.command, &context, &mut io::stdout(), &mut io::stderr());
    let _ = io::stdout().flush();
    match result {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!("{}: {error}", app::PROGRAM);
            ExitCode::FAILURE
        }
    }
}
