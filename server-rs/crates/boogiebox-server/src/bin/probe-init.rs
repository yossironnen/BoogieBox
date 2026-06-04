//! Defines the Probe Init command-line utility for BoogieBox server maintenance.

use boogiebox_db::init_db;
use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = env::args().collect();
    let p = args.get(1).cloned().unwrap_or_else(default_probe_path);
    match init_db(Path::new(&p)) {
        Ok(i) => {
            println!(
                "OK db_path={} journal={:?}",
                i.db_path.display(),
                i.journal_mode
            );
        }
        Err(e) => println!("ERR {e:?}"),
    }
}

fn default_probe_path() -> String {
    let mut path: PathBuf = env::temp_dir();
    path.push("boogiebox-probe-init");
    path.to_string_lossy().into_owned()
}
