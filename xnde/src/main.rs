// Copyright (C) 2020-2023 Michael Herstine <sp1ff@pobox.com>
//
// This file is part of xnde.
//
// xnde is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// xnde is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with xnde.  If not, see <http://www.gnu.org/licenses/>. *

// If I try to document this file at this level, it collides with the corresponding docs in lib.rs
// when I run `cargo doc`. This is a known [issue](https://github.com/rust-lang/cargo/issues/6313),
// as is the entire "src/{main,lib}.rs"
// [pattern](https://github.com/rust-lang/api-guidelines/issues/167). Oh, well. I suppose a CLI
// should be self-documenting, anyway.

mod vars;

use env_logger::Env;
use xnde::{dump, export, DumpFormat, ExportFormat};

use clap::{value_parser, Arg, Command};

// There are many crates for deriving a Display implementation; I tried
// [withoutboats](https://boats.gitlab.io/blog/)'s
// [display_derive](https://github.com/withoutboats/display_derive), but it panicked and I couldn't
// immediately see why (it doesn't seem to be actively maintained). The next most popular crate on
// crates.io was [parse_display](https://docs.rs/parse-display/0.1.1/parse_display/).
use parse_display::Display;

use std::convert::TryFrom;
use std::path::{Path, PathBuf};

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                         app error type                                         //
////////////////////////////////////////////////////////////////////////////////////////////////////

// I'm still not sure how I want Rust error-handling to work. It seems that
// [others](https://lukaskalbertodt.github.io/2019/11/14/thoughts-on-error-handling-in-rust.html)
// [are](https://www.ncameron.org/blog/rust-in-2020-one-more-thing/) in the same position.
//
// I want my Error types to have a programmatic description; textual descriptions are fine for
// log and error messages, but they should be synthesized at the time of generation. That is
// why I am intrigued by the idea of deriving an Error implementation from an enum (like
// [snafu](https://docs.rs/snafu/0.6.6/snafu/)).
//
// I also want my Error types to support, but not require, chaining compliant with the std
// Error source. They should also support backtraces, perhaps gated by a compile flag of some
// kind.
//
// They should *not* require call-time annotation, again like
// [snafu](https://docs.rs/snafu/0.6.6/snafu/), although attaching context in the _callee_, along
// the lines of [context-attribute](https://github.com/yoshuawuyts/context-attribute) seems
// reasonable.
//
// I proceed by hand-coding an Error implementation that satisfies all these conditions, with an
// eye toward automating the process via Rust macros.

#[derive(Debug, Display)]
enum Cause {
    /// An error in another crate or module took place during this module's operation
    #[display("Another crate's or module's error-- cf. source.")]
    Other,
    /// Some sort of internal logic error has occurred
    #[display(
        "An internal error has occurred; please consider filing a bug report to sp1ff@pobox.com."
    )]
    Internal,
    /// No sub-command specified
    #[display("No sub-command given.")]
    NoSubCommand,
}

#[derive(Debug, Display)]
#[display("{cause} Source (if any): {source} Stack trace (if any): {trace}")]
struct Error {
    /// Enumerated status code-- perhaps this is a holdover from my C++ days, but I've found that
    /// programmatic error-handling is facilitated by status codes, not text. Textual messages
    /// should be synthesized from other information only when it is time to present the error to a
    /// human (in a log file, say).
    #[display("XNDE error {}.")]
    cause: Cause,
    // This is an Option that may contain a Box containing something that implements
    // std::error::Error.  It is still unclear to me how this satisfies the lifetime bound in
    // std::error::Error::source, which additionally mandates that the boxed thing have 'static
    // lifetime. There is a discussion of this at
    // <https://users.rust-lang.org/t/what-does-it-mean-to-return-dyn-error-static/37619/6>,
    // but at the time of this writing, i cannot follow it.
    // TODO(sp1ff): figure out how to format `source'
    #[display("XNDE error caused by {:#?}.")]
    source: Option<Box<dyn std::error::Error>>,
    /// Optional backtrace
    // TODO(sp1ff): figure out how to format `source'
    #[display("backtrace: {:#?}.")]
    trace: Option<backtrace::Backtrace>,
}

impl Error {
    fn new(cause: Cause) -> Error {
        // TODO(sp1ff): can I trim this frame off the stack trace?
        Error {
            cause: cause,
            source: None,
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::error::Error for Error {
    /// The lower-level source of this error, if any.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.source {
            // This is an Option that may contain a reference to something that implements
            // std::error::Error and has lifetime 'static. I'm still not sure what 'static means,
            // exactly, but at the time of this writing, I take it to mean a thing which can, if
            // needed, last for the program lifetime (e.g. it contains no references to anything
            // that itself does not have 'static lifetime)
            Some(bx) => Some(bx.as_ref()),
            None => None,
        }
    }
}

// TODO(sp1ff): there *must* be a way to do this generically, but doing it for all T conflicts
// with the std implementation of std::convert::From<T> for T. Maybe this could be my first macro?
impl std::convert::From<xnde::Error> for Error {
    fn from(err: xnde::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<log::SetLoggerError> for Error {
    fn from(err: log::SetLoggerError) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                          The Big Tuna                                          //
////////////////////////////////////////////////////////////////////////////////////////////////////

fn main() -> Result<(), Error> {
    use vars::{AUTHOR, VERSION};
    let matches = Command::new("xnde")
        .version(VERSION)
        .author(AUTHOR)
        .about("xnde -- eXtricate your music library from the Nullsoft Database Engine")
        .long_about(
            "This is a little command-line tool for reading Winamp Music Library databases
and exporting the data into other formats. The Nullsoft Database Engine (NDE) was developed
against the Win32 API and (seemingly) ported to MacOS, but never Linux.",
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Produce more copious output.")
                .required(false)
                .num_args(0),
        )
        .subcommand(
            Command::new("dump")
                .about("dump the contents of a Winamp Music Library")
                .long_about(
                    "Walk the contents of a single NDE table ('main', presumably) & dump its
contents to stdout. Useful for exploring & trouble-shooting.",
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .short('f')
                        .help("Format in which your Muic Library shall be printed")
                        .num_args(1)
                        .default_value("display"),
                )
                .arg(
                    Arg::new("index")
                        .help("NDE index file (`main.idx', e.g.)")
                        .index(1)
                        .requires("data")
                        .required(true)
                        .value_parser(value_parser!(std::path::PathBuf)),
                )
                .arg(
                    Arg::new("data")
                        .help("corresponding NDE data file (`main.dat', e.g.)")
                        .index(2)
                        .required(true)
                        .value_parser(value_parser!(std::path::PathBuf)),
                ),
        )
        .subcommand(
            Command::new("export")
                .about("export the contents of a Winamp Music Library")
                .long_about(
                    "Walk the contents of the NDE 'main' table. For each record therein, transform
it in-memory into a struct representing a single Music Library track (along with all its
associated metadata: playcount, rating, last played, &c). Serialize the entire collection to
one of a few formats for subsequent use.",
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .help(
                            "file to which the serlialized form of your Winamp Music Library shall
be written",
                        )
                        .num_args(1)
                        .default_value("main.out")
                        .value_parser(value_parser!(PathBuf)), // .value_name("FILE"),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .short('f')
                        .help("Format to which your Music Library shall be serialized")
                        .num_args(1)
                        // TODO(sp1ff): add more output formats
                        .default_value("sexp"), // .value_name("FORMAT")
                )
                .arg(
                    Arg::new("index")
                        .help("NDE index file (`main.idx', e.g.)")
                        .index(1)
                        .requires("data")
                        .required(true)
                        .value_parser(value_parser!(std::path::PathBuf)),
                )
                .arg(
                    Arg::new("data")
                        .help("corresponding NDE data file (`main.dat', e.g.)")
                        .index(2)
                        .required(true)
                        .value_parser(value_parser!(std::path::PathBuf)),
                ),
        )
        .get_matches();

    env_logger::init_from_env(Env::default().filter_or(
        "RUST_LOG",
        if matches.get_flag("verbose") {
            "debug"
        } else {
            "info"
        },
    ));

    if let Some(subm) = matches.subcommand_matches("dump") {
        let format = subm
            .get_one::<String>("format")
            .ok_or(Error::new(Cause::Internal))?;
        // We marked both of these arguments as "required" above, so `clap' *should* have checked
        // for their presence. That said, I can't bring myself to call `unwrap'.
        let idx = subm
            .get_one::<PathBuf>("index")
            .ok_or(Error::new(Cause::Internal))?;
        let dat = subm
            .get_one::<PathBuf>("data")
            .ok_or(Error::new(Cause::Internal))?;
        return Ok(dump(
            Path::new(idx),
            Path::new(dat),
            DumpFormat::try_from(format.as_str())?,
        )?);
    } else if let Some(subm) = matches.subcommand_matches("export") {
        // We marked both of these as having default values, so `value_of` should never return
        // Err, here. That said, I can't bring myself to call `unwrap'.
        let format = subm
            .get_one::<String>("format")
            .ok_or(Error::new(Cause::Internal))?;
        let output = subm
            .get_one::<PathBuf>("output")
            .ok_or(Error::new(Cause::Internal))?;
        // We marked both of these arguments as "required" above... yadayadayada.
        let idx = subm
            .get_one::<PathBuf>("index")
            .ok_or(Error::new(Cause::Internal))?;
        let dat = subm
            .get_one::<PathBuf>("data")
            .ok_or(Error::new(Cause::Internal))?;
        return Ok(export(
            Path::new(idx),
            Path::new(dat),
            ExportFormat::try_from(format.as_str())?,
            Path::new(output),
        )?);
    } else {
        // TODO(sp1ff): exit with status 2 here
        Err(Error::new(Cause::NoSubCommand))
    }
}
