use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
pub struct Arguments {
    #[arg(
        short,
        long,
        value_name = "MODULE:APP",
        default_value = "echo_server:app"
    )]
    pub module: String,
    #[arg(short, long, value_name = "SOCK_FILE", default_value = "/tmp/worker-1")]
    pub sock: PathBuf,
}
