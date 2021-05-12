use color_eyre::eyre::Result;
use std::{ffi::OsString, io::Write};

mod age;
mod agenix;
mod cli;
mod util;

#[allow(clippy::missing_errors_doc)]
#[allow(clippy::missing_panics_doc)]
pub fn agenix<I, T>(itr: I, mut writer: impl Write, mut writer_err: impl Write) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    agenix::run(itr, &mut writer, &mut writer_err)?;

    Ok(())
}
