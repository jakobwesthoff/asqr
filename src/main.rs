// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod terminal;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = asqr::cli::Cli::parse();
    asqr::cli::run(
        cli,
        asqr::cli::environment_selection(),
        terminal::run_terminal_ui,
    )
    .into()
}
