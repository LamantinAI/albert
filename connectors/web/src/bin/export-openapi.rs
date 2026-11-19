use std::{
    io::{stdout, Error as IoError, Write},
    process::ExitCode,
};

use albert_connector_web::api::docs::schema;
use serde_json::{to_writer_pretty, Error as JsonError};
use thiserror::Error;

#[derive(Debug, Error)]
enum TaskError {
    #[error("write schema: {0}")]
    Io(#[from] IoError),
    #[error("serialize schema: {0}")]
    Json(#[from] JsonError),
}

fn export() -> Result<(), TaskError> {
    let mut output = stdout().lock();
    to_writer_pretty(&mut output, &schema())?;
    writeln!(output)?;
    Ok(())
}

fn main() -> ExitCode {
    match export() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
