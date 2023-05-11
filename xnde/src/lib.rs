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
//! xnde
//!
//! # Introduction
//!
//! Ex-NDE (Nullsoft Database Engine); Extract-NDE. Extricate your data from your Winamp Music
//! Library [database](http://wiki.winamp.com/wiki/Nullsoft_Database_Engine).
//!
//! This (binary) crate includes logic as well as a small program for reading your Winamp Music
//! Library database & dumping the contents to other (hopefully more convenient) formats. It does
//! *not* attempt a full re-implementation of the Nullsoft Database Engine (see
//! [here](https://github.com/sp1ff/xnde/README.org) for thoughts on that); it just reads
//! the main table of the Music Library database & dumps the contents.
//!
//! Not only is this is my first Rust crate, but the implementation is incomplete (see [`fields`],
//! e.g.), and the code is generally littered with assorted TODO comments marking things I'd like to
//! do better, or more elegantly, or at all. That said, the point to this crate is to get my data
//! out of an archaic format & into one I can use more readily. I've accomplished that, so I don't
//! know how much time I want to spend polishing the implementation. PRs, suggestions & complaints
//! [welcome](mailto:sp1ff@pobox.com)!
//!
//! [`fields`]: fields/index.html
//!
//! # Discussion
//!
//! ## Indicies and Tables
//!
//! Despite its name, the
//! [Nullsoft Database Engine](http://wiki.winamp.com/wiki/Nullsoft_Database_Engine) is not a
//! full-featured RDMS. True to the coding ethos that characterized [Winamp](https://winamp.com)
//! generally, it was a tightly-coded, purpose-built implementation that produced extremely
//! compact representations on disk.
//!
//! A "table" in NDE parlance is a collection of records. On disk, it is described by two files:
//!
//!    1. the "data" file (`.dat`) in which the table's records are serialized in a very space
//!       efficient manner
//!
//!    2. the "index" file (`.idx`) which describes how to traverse the data file in various
//!       orders
//!
//! The index file is described below. See [`fields`] for details on the data file.
//!
//! ## File Formats
//!
//! ### Index File Format
//!
//! The top-level structure of the index file is:
//!
//! ```ignore
//!     +------------+-------------+-------------+-------------+
//!     | "NDEINDEX" | no. records | primary idx | aux idx...  |
//!     +------------+-------------+-------------+-------------+
//! ```
//!
//! The first element is a signature marking the file's purpose in the form of the string "NDEINDEX"
//! in ASCII/UTF-8 format. The second is the number of records in the table (and hence the number of
//! elements in each index), expressed as a 32-bit little-endian unsigned integer.
//!
//! Each index has the following format:
//!
//! ```ignore
//!     +--------+--------------------+
//!     | idx ID | record location... |
//!     +--------+--------------------+
//! ```
//!
//! The index ID is a 32-bit little-endian unsigned integer. The only significance to the ID I
//! could find in the code is that the required primary index has an ID of 255 (or `PRIMARY_INDEX`).
//!
//! There is no "end-of-record" marker; since each record location is a constant size (eight bytes),
//! one simply reads the expected number of bytes seqeuntially after reading the number of records.
//! There is also no indication of how many indicies the file contains; one simply reads chunks of
//! `no. records * 8` bytes until EoF is reached.
//!
//! Each records is two 32-bit integers:
//!
//! ```ignore
//!     +--------+------+
//!     | offset | ???? |
//!     +--------+------+
//! ```
//!
//! The first element is a 32-bit little-endian unsigned int giving the offset into the datafile of
//! a record.  The question marks for the second element are because I never figured out what this
//! was for.

pub mod fields;
pub mod tracks;

use fields::{field_factory, FieldType, NdeField};
use tracks::{new_column_map, Track};

use parse_display::Display;

use log::{debug, info};

use std::{
    convert::TryFrom,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                       module error type                                        //
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Display)]
pub enum Cause {
    /// An error in another crate or module's error took place during this module's operation
    #[display("An error in another crate or module-- cf. source.")]
    Other,
    /// No signature in the index or data file
    #[display("No signature in the index or data file")]
    NoSig,
    /// No indicies found in the index file
    #[display("No indicies found in the index file")]
    NoIndicies,
    /// Failed to read a UTF-8 string
    #[display("Failed to read a UTF-8 string")]
    NotUtf8,
    /// Failed to read a UTF-16 string
    #[display("Failed to read a UTF-16 string")]
    NotUtf16,
    /// A non-column field appeared in the first record
    #[display("While parsing first record, got field of type {}")]
    NonColumnField(FieldType),
    /// Bad format specification
    #[display("Couldn't interepret {} as a format")]
    BadFormat(String),
}

#[derive(Debug, Display)]
#[display("{cause} Source (if any): {source} Stack trace (if any): {trace}")]
pub struct Error {
    /// Enumerated status code
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

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<std::string::FromUtf8Error> for Error {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Error {
            cause: Cause::NotUtf8,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<std::string::FromUtf16Error> for Error {
    fn from(err: std::string::FromUtf16Error) -> Self {
        Error {
            cause: Cause::NotUtf16,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<fields::Error> for Error {
    fn from(err: fields::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<serde_lexpr::error::Error> for Error {
    fn from(err: serde_lexpr::error::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<crate::tracks::Error> for Error {
    fn from(err: crate::tracks::Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                           NDE Index                                            //
////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct NdeIndex {
    _id: i32,
    table: Vec<(u64, i32)>,
}

impl NdeIndex {
    fn from_reader<R: std::io::Read>(r: &mut R, nrec: usize) -> Result<Option<NdeIndex>> {
        // We expect to be looking at this index's ID...
        let mut buf: [u8; 4] = [0; 4];
        // if this file is exhausted, we are looking at EOF. Check for that in particular:
        match r.read_exact(&mut buf) {
            Err(err) => {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    return Ok(None);
                } else {
                    return Err(Error::from(err));
                }
            }
            _ => (),
        }
        let id = i32::from_le_bytes(buf);

        let mut table: Vec<(u64, i32)> = Vec::with_capacity(nrec);
        for _i in 0..nrec {
            r.read_exact(&mut buf)?;
            let off = u32::from_le_bytes(buf);
            r.read_exact(&mut buf)?;
            let collab = i32::from_le_bytes(buf);
            table.push((off as u64, collab));
        }
        Ok(Some(NdeIndex {
            _id: id,
            table: table,
        }))
    }
    /// Retrieve the offset for record i in this index
    fn off(&self, i: usize) -> u64 {
        self.table[i].0
    }
    fn len(&self) -> usize {
        self.table.len()
    }
}

/// Read all indicies out of an index file; rdr is assumed to be pointing at the signature (i.e.
/// byte zero if we're reading a .idx file)
pub fn read_indicies<R: Read + Seek>(rdr: &mut R) -> Result<Vec<NdeIndex>> {
    let mut buf: [u8; 8] = [0; 8];
    rdr.read_exact(&mut buf)?;
    if b"NDEINDEX" != &buf {
        return Err(Error::new(Cause::NoSig));
    }

    let mut buf: [u8; 4] = [0; 4];
    rdr.read_exact(&mut buf)?;
    let nrecs = u32::from_le_bytes(buf) as usize;

    // Read {id, nrec*(u32,i32)} until EOF
    let mut idxes: Vec<NdeIndex> = Vec::new();
    let mut next = NdeIndex::from_reader(rdr, nrecs)?;
    while let Some(index) = next {
        idxes.push(index);
        next = NdeIndex::from_reader(rdr, nrecs)?;
    }

    Ok(idxes)
}

#[cfg(test)]
mod index_tests {

    /// Trivial test-- an index with two records
    #[test]
    fn smoke() -> Result<(), String> {
        use super::*;
        let bytes: [u8; 20] = [
            0xff, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        ];
        let idx = match NdeIndex::from_reader(&mut bytes.as_ref(), 2) {
            Ok(opt) => match opt {
                Some(x) => x,
                None => {
                    return Err(String::from("premature EOF"));
                }
            },
            Err(err) => {
                return Err(format!("{}", err));
            }
        };
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.off(0), 8);
        assert_eq!(idx.off(1), 32);
        Ok(())
    }

    /// Test a malformed index
    #[test]
    fn negative() -> Result<(), String> {
        use super::*;
        let bytes: [u8; 12] = [
            0xff, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let _idx = match NdeIndex::from_reader(&mut bytes.as_ref(), 2) {
            Ok(_) => {
                return Err(String::from("construction should have failed"));
            }
            Err(_) => (),
        };
        Ok(())
    }

    /// Test reading a full .idx file
    #[test]
    fn idx() -> Result<(), String> {
        use super::*;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"NDEINDEX");
        buf.extend_from_slice(&2i32.to_le_bytes()); // each index has 2 records
        buf.extend_from_slice(&0xffi32.to_le_bytes()); // ID -1 (PRIMARY_INDEX)
        buf.extend_from_slice(&8i32.to_le_bytes()); // offset 0x08
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0x20i32.to_le_bytes()); // offset 0x020
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0x00i32.to_le_bytes()); // ID 0
        buf.extend_from_slice(&0x20i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0x08i32.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes());

        let mut cur = std::io::Cursor::new(buf);
        match read_indicies(&mut cur) {
            Ok(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].len(), 2);
                assert_eq!(v[0].off(0), 8);
                assert_eq!(v[0].off(1), 32);
                assert_eq!(v[1].len(), 2);
                assert_eq!(v[1].off(1), 8);
                assert_eq!(v[1].off(0), 32);
            }
            Err(err) => {
                return Err(format!("{}", err));
            }
        }

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                       redirect handling                                        //
////////////////////////////////////////////////////////////////////////////////////////////////////

fn follow_redirects<R: Read + Seek>(rdr: &mut R) -> Result<(u8, FieldType)> {
    let mut id: u8 = 0;
    let mut ftype = FieldType::Redirector;
    while ftype == FieldType::Redirector {
        // read two chars: ID & type
        let mut buf: [u8; 2] = [0; 2];
        rdr.read_exact(&mut buf)?;

        id = buf[0];
        ftype = FieldType::from(buf[1])?;
        if ftype == FieldType::Redirector {
            let mut buf: [u8; 4] = [0; 4];
            rdr.read_exact(&mut buf)?;
            let at = u32::from_le_bytes(buf) as u64;
            rdr.seek(SeekFrom::Start(at))?;
            debug!("found redirect, jumping to {:#04x}", at);
        }
    }

    Ok((id, ftype))
}

#[cfg(test)]
mod redirect_tests {

    /// Trivial tests-- tough to test since my databases have no redirects
    #[test]
    fn smoke() -> Result<(), String> {
        use super::*;
        let bytes: [u8; 2] = [0x01, 0x00];
        let mut rdr = std::io::Cursor::new(bytes);
        match follow_redirects(&mut rdr) {
            Ok((id, ft)) => {
                assert_eq!(id, 1);
                assert_eq!(ft, FieldType::Column);
            }
            Err(err) => {
                return Err(format!("{}", err));
            }
        }

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                         dumping logic                                          //
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub enum DumpFormat {
    Display,
    Sexp,
    Json,
}

impl TryFrom<&str> for DumpFormat {
    type Error = Error;
    fn try_from(x: &str) -> std::result::Result<DumpFormat, Error> {
        match x {
            "display" => Ok(DumpFormat::Display),
            "sexp" => Ok(DumpFormat::Sexp),
            "json" => Ok(DumpFormat::Json),
            _ => Err(Error::new(Cause::BadFormat(String::from(x)))),
        }
    }
}

// TODO(sp1ff): re-write to take readers; write unit tests
/// Dump the contents of a Winamp Music Library to stdout
pub fn dump(idx: &Path, dat: &Path, format: DumpFormat) -> Result<()> {
    let fdidx = File::open(idx)?;
    let mut bufidx = BufReader::new(fdidx);
    let idxes = read_indicies(&mut bufidx)?;
    info!("There are {} indicies.", idxes.len());

    if idxes.len() == 0 {
        return Err(Error::new(Cause::NoIndicies));
    }
    let nrecs = idxes[0].len();
    info!("Each index has {} records.", nrecs);

    // Alright: if we've made it this far, we've parsed the index file. Now use the primary
    // index to walk the data file.
    let mut fddat = File::open(dat)?;

    let mut buf: [u8; 8] = [0; 8];
    fddat.read_exact(&mut buf)?;
    if b"NDETABLE" != &buf {
        return Err(Error::new(Cause::NoSig));
    }

    for i in 0..nrecs {
        let at = idxes[0].off(i);
        debug!("Parsing record {} at {:#04x}.", i, at);
        fddat.seek(SeekFrom::Start(at))?;

        // we now walk the fields in record `i':
        let mut next_field_pos: u64 = at;

        while next_field_pos != 0 {
            let (id, ftype) = follow_redirects(&mut fddat)?;
            // field-specific data follows..
            match field_factory(&mut fddat, id as i32, ftype) {
                Ok(x) => {
                    // Display x:
                    match format {
                        DumpFormat::Display => info!("{}", x),
                        DumpFormat::Sexp => info!("{}", serde_lexpr::to_string(&x)?),
                        DumpFormat::Json => info!("{}", serde_json::to_string(&x)?),
                    }
                    next_field_pos = x.next_field_pos();
                }
                Err(err) => {
                    return Err(Error::from(err));
                }
            }

            if next_field_pos != 0 {
                fddat.seek(SeekFrom::Start(next_field_pos))?;
            }
        }
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                          export logic                                          //
////////////////////////////////////////////////////////////////////////////////////////////////////

pub enum ExportFormat {
    Json,
    Sexp,
}

impl TryFrom<&str> for ExportFormat {
    type Error = Error;
    fn try_from(x: &str) -> std::result::Result<Self, Error> {
        match x {
            "sexp" => Ok(ExportFormat::Sexp),
            "json" => Ok(ExportFormat::Json),
            _ => Err(Error::new(Cause::BadFormat(String::from(x)))),
        }
    }
}

// TODO(sp1ff): re-write to take readers; write unit tests
/// transform your Winamp music library into an in-memory datastructure and serialize it
/// to any variety of formats via Serde.
pub fn export(idx: &Path, dat: &Path, format: ExportFormat, out: &Path) -> Result<()> {
    let fdidx = File::open(idx)?;
    let mut bufidx = BufReader::new(fdidx);
    let idxes = read_indicies(&mut bufidx)?;
    debug!("There are {} indicies.", idxes.len());

    if idxes.len() == 0 {
        return Err(Error::new(Cause::NoIndicies));
    }
    let nrecs = idxes[0].len();
    debug!("Each index has {} records.", nrecs);

    // Alright: if we've made it this far, we've parsed the index file. Now use the primary
    // index to walk the data file.
    let mut fddat = File::open(dat)?;

    let mut buf: [u8; 8] = [0; 8];
    fddat.read_exact(&mut buf)?;
    if b"NDETABLE" != &buf {
        return Err(Error::new(Cause::NoSig));
    }

    // The first record should list the columns in this table.
    let at = idxes[0].off(0);
    fddat.seek(SeekFrom::Start(at))?;

    let mut cols: Vec<fields::ColumnField> = Vec::new();
    let mut next_field_pos: u64 = at;
    while next_field_pos != 0 {
        let (id, ftype) = follow_redirects(&mut fddat)?;
        if ftype != FieldType::Column {
            return Err(Error::new(Cause::NonColumnField(ftype)));
        }
        let x = fields::ColumnField::new(&mut fddat, id as i32)?;
        next_field_pos = x.next_field_pos();
        cols.push(x);
        if next_field_pos != 0 {
            fddat.seek(SeekFrom::Start(next_field_pos))?;
        }
    }

    debug!("There are {} columns.", cols.len());

    let col_map = new_column_map(cols.iter());
    debug!("column map: {:#?}", col_map);

    // The second record should contain the indicies defined on this table; we're only making
    // use of the primary, so skip this.
    let mut trks: Vec<tracks::Track> = Vec::with_capacity(nrecs);
    info!("Creating {} Tracks...", nrecs - 2);
    for i in 2..nrecs {
        let at = idxes[0].off(i);
        fddat.seek(SeekFrom::Start(at))?;

        // we now walk the fields in record `i':
        let mut rec: Vec<Box<dyn fields::NdeField>> = Vec::with_capacity(cols.len());
        let mut next_field_pos: u64 = at;

        while next_field_pos != 0 {
            let (id, ftype) = follow_redirects(&mut fddat)?;
            // field-specific data follows..
            match field_factory(&mut fddat, id as i32, ftype) {
                Ok(x) => {
                    next_field_pos = x.next_field_pos();
                    rec.push(x);
                }
                Err(err) => {
                    return Err(Error::from(err));
                }
            }

            if next_field_pos != 0 {
                fddat.seek(SeekFrom::Start(next_field_pos))?;
            }
        }

        // Between `cols' & `rec', we have enough to create a Track
        let t = Track::new(&col_map, rec.iter())?;
        trks.push(t);
    }
    info!("Creating {} Tracks...done.", nrecs - 2);

    info!("Writing {}...", out.display());
    let f = File::create(out)?;
    match format {
        ExportFormat::Sexp => serde_lexpr::to_writer(f, &trks)?,
        ExportFormat::Json => serde_json::to_writer(f, &trks)?,
    }
    info!("Writing {}...done.", out.display());

    Ok(())
}
