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
//! fields
//!
//! # Introduction
//!
//! NDE ([Nullsoft Database Engine]((http://wiki.winamp.com/wiki/Nullsoft_Database_Engine))) field
//! types. I've only implemented the fields needed to read my muisc library.
//! [Contact](mailto:sp1ff@pobox.com) me if you're using this & have fields in your library that
//! aren't handled here.
//!
//! # Discussion
//!
//! As mentioned in the [`xnde`] docs, an NDE table consists of a collection of records serialized
//! very compactly. An index is a list of offsets into the data (the `.dat` file on disk) at each
//! each row _begins_, in the order corresponding to the index. E.g. in my database, index 255
//! is the primary index and just gives the record offsets in sequential order, while index 0
//! lists the exact same set of records, but in lexicographical order of the "filename" field.
//!
//! [`xnde`]: ../index.html
//!
//! In my database, the first record contains the columns for the table, and the second the
//! indicies.
//!
//! ## Record Formats
//!
//! Each record consists of a sequence of fields. Each field has the form:
//!
//! ```ignore
//!     +----+------+----------+------+------+------+
//!     | ID | type | max_size | prev | next | data |
//!     +----+------+----------+------+------+------+
//! ```
//!
//! ID is a 8-bit unsigned int; this appears to be unique to each column, and serves to map a field
//! to a column in other records. Types as an 8-bit unsigned int describing the field type. In
//! the NDE implementation, these map to a set of contants:
//!
//! ```c
//! #define FIELD_UNKNOWN   255
//! enum
//! {
//!     FIELD_COLUMN     =  0,
//!     FIELD_INDEX      =  1,
//!     FIELD_REDIRECTOR =  2,
//!     FIELD_STRING     =  3,
//!     FIELD_INTEGER    =  4,
//!     FIELD_BOOLEAN    =  5,
//!     FIELD_BINARY     =  6, // max size 65536
//!     FIELD_GUID       =  7,
//!     FIELD_PRIVATE    =  8,
//!     FIELD_BITMAP     =  6,
//!     FIELD_FLOAT      =  9,
//!     FIELD_DATETIME   = 10,
//!     FIELD_LENGTH     = 11,
//!     FIELD_FILENAME   = 12,
//!     FIELD_INT64      = 13,
//!     FIELD_BINARY32   = 14, // binary field, but 32bit sizes instead of 16bit
//!     FIELD_INT128     = 15, // mainly for storing MD5 hashes
//! };
//! ```
//!
//! In this crate, they are represented by the enum [`FieldType`].
//!
//! `max_size` is a 32-bit little-endian unsigned int containing the size of the `data` field
//! (i.e. the field-specific blob after the common header). If fields were guaranteed to be
//! sequential within a record, this quantity could be computed from `next`, so I can only
//! assume that is not the case generally, even though it was for my database.
//!
//! Also, note that this is the serialized size on disk; I have seen fields where the field-
//! specific data took up _less_ than this. I suspect, but haven't verified, that this allows
//! the write implementation to update a field whose new serialized representation happens to
//! be smaller by just writing it in place and not updating the rest of the file to "pack"
//! it more tightly.
//!
//! `prev` is a 32-bit little-endian unsigned int containing the offset of the previous field in
//! this record, and `next` is the same giving the offset of the next. Not all columns need appear
//! in each record, and the record length is nowhere written down; the reader must simply read one
//! field after another until encountering one whose `next` field is zero. As an aside, this
//! strongly suggests a buffered read implementation, which the NDE uses.
//!
//! ## Field Formats
//!
//! The following diagrams display field layouts _after_ the common field header.
//!
//! ### Column
//!
//! ```ignore
//!     +----------+------------------+--------------+--------------------+
//!     | type: u8 | unique index: i8 | name len: u8 | name: ASCII string |
//!     +----------+------------------+--------------+--------------------+
//! ```
//!
//! The column name is a length-prefixed ASCII string (i.e. no trailing nil).
//!
//! ### Filename & String
//!
//! ```ignore
//!     +----+------+
//!     | cb | text |
//!     +----+------+
//! ```
//!
//! `cb` is a sixteen-bit, little-endian unsigned integer containing the number of bytes in the
//! filename or string. The text _may_ be UTF-16 encoded; in that case we expect a BOM. Else the
//! reference implementation simply copies the bytes; this implementation assumes UTF-8. Note that
//! the string is not null-terminated.
//!
//! ### Index
//!
//! ```ignore
//!     +-----+------+----+------+
//!     | pos | type | cb | name |
//!     +-----+------+----+------+
//! ```
//!
//! `pos` is a 32-bit, little endian signed int. I never figured out what it does. `type' is
//! also 32-bit, LE, signed int, and refers to the type of field found in this column. `cb` is a
//! 32-bit, LE unsigned int describing the length of the `name` field, which is the ASCII text
//! of the filter name.
//!
//! ### Int64
//!
//! ```ignore
//!     +-----+
//!     | val |
//!     +-----+
//! ```
//!
//! A 64-bit, little-endian, signed integer.
//!
//! ### Datetime, Integer, Length
//!
//! ```ignore
//!     +-------+
//!     | value |
//!     +-------+
//! ```
//!
//! `value` is a 32-bit little-endian integer. I still haven't unravelled how to interpret it in all
//! cases, but it _is_ a signed integer (i.e. not a simple Unix-style "seconds-since-epoch" value
//! for time, or seconds for length).
//!

use parse_display::Display;

use serde::{Deserialize, Serialize};

use std::io::Read;

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                           error type                                           //
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Display)]
pub enum Cause {
    /// An error in another crate or  module-- cf. source.
    #[display("An error in another crate or  module-- cf. source.")]
    Other,
    /// Unknown field type
    #[display("Uknown field type {}")]
    BadFieldType(u8),
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
    #[display("fields error caused by {:#?}.")]
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
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

impl std::convert::From<std::string::FromUtf16Error> for Error {
    fn from(err: std::string::FromUtf16Error) -> Self {
        Error {
            cause: Cause::Other,
            source: Some(Box::new(err)),
            trace: Some(backtrace::Backtrace::new()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                     Basic NDE Field Types                                      //
////////////////////////////////////////////////////////////////////////////////////////////////////

/// NDE field types, maintaining the associated C numeric constants
#[derive(Debug, Deserialize, Display, PartialEq, Serialize)]
pub enum FieldType {
    #[display("COLUMN")]
    Column = 0,
    #[display("INDEX")]
    Index = 1,
    #[display("REDIRECTOR")]
    Redirector = 2,
    #[display("STRING")]
    String = 3,
    #[display("INTEGER")]
    Integer = 4,
    #[display("BOOLEAN")]
    Boolean = 5,
    #[display("BINARY")]
    Binary = 6, // max size 65536
    #[display("GUID")]
    Guid = 7,
    #[display("PRIVATE")]
    Private = 8,
    // TODO(sp1ff): how to handle this?
    // #[display("BITMAP")]
    // Bitmap = 6,
    #[display("FLOAT")]
    Float = 9,
    #[display("DATETIME")]
    Datetime = 10,
    #[display("LENGTH")]
    Length = 11,
    #[display("FILENAME")]
    Filename = 12,
    #[display("INT64")]
    Int64 = 13,
    #[display("BINARY32")]
    Binary32 = 14, //  binary field, but 32bit sizes instead of 16bit
    #[display("INT128")]
    Int128 = 15, //  mainly for storing MD5 hashes
}

// TODO(sp1ff): TryFrom instead?
impl FieldType {
    pub fn from(i: u8) -> Result<FieldType> {
        match i {
            0 => Ok(FieldType::Column),
            1 => Ok(FieldType::Index),
            2 => Ok(FieldType::Redirector),
            3 => Ok(FieldType::String),
            4 => Ok(FieldType::Integer),
            5 => Ok(FieldType::Boolean),
            6 => Ok(FieldType::Binary),
            7 => Ok(FieldType::Guid),
            8 => Ok(FieldType::Private),
            9 => Ok(FieldType::Float),
            10 => Ok(FieldType::Datetime),
            11 => Ok(FieldType::Length),
            12 => Ok(FieldType::Filename),
            13 => Ok(FieldType::Int64),
            14 => Ok(FieldType::Binary32),
            15 => Ok(FieldType::Int128),
            _ => Err(Error::new(Cause::BadFieldType(i))),
        }
    }
}

#[derive(Debug, Serialize)]
pub enum FieldValue {
    Unknown,
    Column((i32, String)),
    Index((i32, i32)),
    String(String),
    Integer(i32),
    Boolean(bool),
    Float(f64),
    Datetime(i32),
    Length(i32),
    Filename(std::path::PathBuf),
    Int64(i64),
}

/// Common NDE Field behavior
// This annotation is from the `tyeptag' crate; it marks the Trait NdeField as having only
// implementors who themselves implement Deserialize & Serialize. It also allows the serde
// library to operate on things of type `&dyn NdeField' (I believe).
#[typetag::serde(tag = "type")]
pub trait NdeField: std::fmt::Display {
    fn id(&self) -> i32;
    fn type_id(&self) -> Option<FieldType>;
    fn prev_field_pos(&self) -> u64;
    fn next_field_pos(&self) -> u64;
    fn value(&self) -> FieldValue;
}

#[derive(Debug, Deserialize, Display, Serialize)]
/// Common NDE Field attributes: id, next-field, prev-field
#[display(
    "ID {id}, size: {max_size_on_disk}, prev: {prev_field_pos:#06x}, next: {next_field_pos:#06x}"
)]
pub struct NdeFieldBase {
    /// Field "identifier"-- no idea what this is used for, yet.
    id: i32,
    /// Maximum size occupied by this field
    max_size_on_disk: usize,
    /// File offset of the previous field
    prev_field_pos: u64,
    /// File offset of the next field
    next_field_pos: u64,
}

impl NdeFieldBase {
    /// Read from disk-- the caller is assumed to already have the id, since it would have been
    /// parsed as a result of following redirects
    fn new<R: Read>(rdr: &mut R, id: i32) -> Result<NdeFieldBase> {
        let mut buf: [u8; 4] = [0; 4];
        rdr.read_exact(&mut buf)?;
        let max_size_on_disk = u32::from_le_bytes(buf) as usize;
        rdr.read_exact(&mut buf)?;
        let next_field_pos = u32::from_le_bytes(buf) as u64;
        rdr.read_exact(&mut buf)?;
        let prev_field_pos = u32::from_le_bytes(buf) as u64;
        Ok(NdeFieldBase {
            id: id,
            max_size_on_disk: max_size_on_disk,
            prev_field_pos: prev_field_pos,
            next_field_pos: next_field_pos,
        })
    }
    #[allow(dead_code)]
    fn id(&self) -> i32 {
        self.id
    }
    fn max_size_on_disk(&self) -> usize {
        self.max_size_on_disk
    }
    #[allow(dead_code)]
    fn next(&self) -> u64 {
        self.next_field_pos
    }
}

#[cfg(test)]
mod nde_field_base_tests {

    /// Trivial test case
    #[test]
    fn smoke() -> std::result::Result<(), String> {
        use super::*;
        let bytes: [u8; 12] = [
            0x10, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let b = match NdeFieldBase::new(&mut bytes.as_ref(), 11) {
            Ok(x) => x,
            Err(e) => {
                return Err(format!("{}", e));
            }
        };
        assert_eq!(b.id(), 11);
        assert_eq!(b.next(), 20);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                    concrete NDE field types                                    //
////////////////////////////////////////////////////////////////////////////////////////////////////

/// An NDE Field which we do not know how to parse
#[derive(Debug, Deserialize, Display, Serialize)]
// TODO(sp1ff): format the raw bytes more nicely (without newlines)
#[display("Unk {field_type}: {base} data: {bytes:#?}")]
pub struct UnsupportedNdeField {
    base: NdeFieldBase,
    field_type: FieldType,
    bytes: Vec<u8>,
}

impl UnsupportedNdeField {
    pub fn new<R: Read>(rdr: &mut R, id: i32, ft: FieldType) -> Result<UnsupportedNdeField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: Vec<u8> = Vec::with_capacity(base.max_size_on_disk());
        buf.resize(base.max_size_on_disk(), 0);
        rdr.read_exact(buf.as_mut_slice())?;
        Ok(UnsupportedNdeField {
            base: base,
            field_type: ft,
            bytes: buf,
        })
    }
}

#[typetag::serde]
impl NdeField for UnsupportedNdeField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        None
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Unknown
    }
}

/// NDE FIELD_COLUMN
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("Column: {base}, {col_type}, {name}")]
pub struct ColumnField {
    base: NdeFieldBase,
    col_type: FieldType,
    index_unique: bool,
    name: String,
}

impl ColumnField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<ColumnField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 3] = [0; 3];
        rdr.read_exact(&mut buf)?;

        let col_type = FieldType::from(buf[0])?;
        let index_unique = buf[1] != 0;
        let cb = buf[2] as usize;

        let mut buf: Vec<u8> = Vec::with_capacity(cb);
        buf.resize(cb, 0);
        rdr.read_exact(buf.as_mut_slice())?;

        let name = String::from_utf8(buf)?;

        Ok(ColumnField {
            base: base,
            col_type: col_type,
            index_unique: index_unique,
            name: name,
        })
    }
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[typetag::serde]
impl NdeField for ColumnField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Filename)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Column((self.id(), self.name.clone()))
    }
}

/// NDE FIELD_DATETIME
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {data}")]
pub struct DatetimeField {
    base: NdeFieldBase,
    data: i32,
}

impl DatetimeField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<DatetimeField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 4] = [0; 4];
        rdr.read_exact(&mut buf)?;
        let data = i32::from_le_bytes(buf);
        Ok(DatetimeField {
            base: base,
            data: data,
        })
    }
}

#[typetag::serde]
impl NdeField for DatetimeField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Datetime)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Datetime(self.data)
    }
}

/// NDE FIELD_FILENAME
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {path:#?}")]
pub struct FilenameField {
    base: StringField,
    path: std::path::PathBuf,
}

impl FilenameField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<FilenameField> {
        let base = StringField::new(rdr, id)?;
        let path = std::path::PathBuf::from(base.text());
        Ok(FilenameField {
            base: base,
            path: path,
        })
    }
}

#[typetag::serde]
impl NdeField for FilenameField {
    fn id(&self) -> i32 {
        self.base.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Filename)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Filename(self.path.clone())
    }
}

/// NDE FIELD_INDEX
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base}, pos: {pos}, type: {ftype}, name: {name}")]
pub struct IndexField {
    base: NdeFieldBase,
    pos: u64,
    ftype: i32,
    name: String,
}

impl IndexField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<IndexField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 4] = [0; 4];
        rdr.read_exact(&mut buf)?;
        let pos = u32::from_le_bytes(buf) as u64;
        rdr.read_exact(&mut buf)?;
        let ftype = i32::from_le_bytes(buf);
        let mut buf: [u8; 1] = [0; 1];
        rdr.read_exact(&mut buf)?;
        let cb = buf[0] as usize;
        let mut buf: Vec<u8> = Vec::with_capacity(cb);
        buf.resize(cb, 0);
        rdr.read_exact(buf.as_mut_slice())?;
        let name = String::from_utf8(buf)?;
        Ok(IndexField {
            base: base,
            pos: pos,
            ftype: ftype,
            name: name,
        })
    }
}

#[typetag::serde]
impl NdeField for IndexField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Index)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Index((self.id(), self.ftype))
    }
}

/// NDE FIELD_INT64
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {data}")]
pub struct Int64Field {
    base: NdeFieldBase,
    data: i64,
}

impl Int64Field {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<Int64Field> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 8] = [0; 8];
        rdr.read_exact(&mut buf)?;
        let data = i64::from_le_bytes(buf);
        Ok(Int64Field {
            base: base,
            data: data,
        })
    }
}

#[typetag::serde]
impl NdeField for Int64Field {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Int64)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Int64(self.data)
    }
}

/// NDE FIELD_INTEGER
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {data}")]
pub struct IntegerField {
    base: NdeFieldBase,
    data: i32,
}

impl IntegerField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<IntegerField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 4] = [0; 4];
        rdr.read_exact(&mut buf)?;
        let data = i32::from_le_bytes(buf);
        Ok(IntegerField {
            base: base,
            data: data,
        })
    }
}

#[typetag::serde]
impl NdeField for IntegerField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Integer)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Integer(self.data)
    }
}

/// NDE FIELD_LENGTH
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {data}")]
pub struct LengthField {
    base: NdeFieldBase,
    data: i32,
}

impl LengthField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<LengthField> {
        let base = NdeFieldBase::new(rdr, id)?;
        let mut buf: [u8; 4] = [0; 4];
        rdr.read_exact(&mut buf)?;
        let data = i32::from_le_bytes(buf);
        Ok(LengthField {
            base: base,
            data: data,
        })
    }
}

#[typetag::serde]
impl NdeField for LengthField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::Length)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::Length(self.data)
    }
}

/// NDE FIELD_STRING
#[derive(Debug, Deserialize, Display, Serialize)]
#[display("{base} {text}")]
pub struct StringField {
    base: NdeFieldBase,
    text: String,
}

impl StringField {
    pub fn new<R: Read>(rdr: &mut R, id: i32) -> Result<StringField> {
        let base = NdeFieldBase::new(rdr, id)?;

        // Next up: a u16 containing the string length
        let mut buf: [u8; 2] = [0; 2];
        rdr.read_exact(&mut buf)?;
        let cb = u16::from_le_bytes(buf) as usize;

        if cb == 0 {
            return Ok(StringField {
                base: base,
                text: String::new(),
            });
        }

        let mut buf: Vec<u8> = Vec::with_capacity(cb);
        buf.resize(cb, 0);
        rdr.read_exact(buf.as_mut_slice())?;

        // the text *may* be UTF-16 encoded; from reading the NDE source code, it appears we can
        // depend on a BOM being present if so.
        let text = if cb >= 2 && cb % 2 == 0 && buf[0] == 0xff && buf[1] == 0xfe {
            // the rest of `buf' are little-endian u16-s giving a utf-16 encoding
            let mut buf16: Vec<u16> = Vec::with_capacity(cb - 2);
            for i in (2..cb).step_by(2) {
                // TODO(sp1ff): there must be a better way
                let tmp = [buf[i], buf[i + 1]];
                buf16.push(u16::from_le_bytes(tmp));
            }
            String::from_utf16(&buf16)?
        } else if cb >= 2 && cb % 2 == 0 && buf[0] == 0xfe && buf[1] == 0xff {
            // the rest of `buf' are big-endian u16-s giving a utf-16 encoding
            let mut buf16: Vec<u16> = Vec::with_capacity(cb - 2);
            for i in (2..cb).step_by(2) {
                // TODO(sp1ff): there must be a better way
                let tmp = [buf[i], buf[i + 1]];
                buf16.push(u16::from_be_bytes(tmp));
            }
            String::from_utf16(&buf16)?
        } else {
            // `buf' contains a utf-8 string
            String::from_utf8(buf)?
        };

        Ok(StringField {
            base: base,
            text: text,
        })
    }
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

#[typetag::serde]
impl NdeField for StringField {
    fn id(&self) -> i32 {
        self.base.id
    }
    fn type_id(&self) -> Option<FieldType> {
        Some(FieldType::String)
    }
    fn prev_field_pos(&self) -> u64 {
        self.base.prev_field_pos
    }
    fn next_field_pos(&self) -> u64 {
        self.base.next_field_pos
    }
    fn value(&self) -> FieldValue {
        FieldValue::String(self.text.clone())
    }
}

#[cfg(test)]
mod string_field_tests {

    #[test]
    /// StringField smoke tests
    fn string_field_smoke() -> Result<(), String> {
        use super::*;
        let bytes: [u8; 32] = [
            0x14, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00,
            0xff, 0xfe, 0x43, 0x00, 0x3a, 0x00, 0x5c, 0x00, 0x61, 0x00, 0x2e, 0x00, 0x6d, 0x00,
            0x70, 0x00, 0x33, 0x00,
        ];
        let s = match StringField::new(&mut bytes.as_ref(), 1) {
            Ok(s) => s,
            Err(err) => {
                return Err(format!("{}", err));
            }
        };
        let t = match s.value() {
            FieldValue::String(t) => t,
            _ => {
                return Err(String::from("bad field value"));
            }
        };
        eprintln!("t is {}", t);
        assert_eq!(t, "C:\\a.mp3");
        Ok(())
    }
}

pub fn field_factory<R: Read>(rdr: &mut R, id: i32, ft: FieldType) -> Result<Box<dyn NdeField>> {
    match ft {
        FieldType::Column => Ok(Box::new(ColumnField::new(rdr, id)?)),
        FieldType::Datetime => Ok(Box::new(DatetimeField::new(rdr, id)?)),
        FieldType::Filename => Ok(Box::new(FilenameField::new(rdr, id)?)),
        FieldType::Index => Ok(Box::new(IndexField::new(rdr, id)?)),
        FieldType::Integer => Ok(Box::new(IntegerField::new(rdr, id)?)),
        FieldType::Int64 => Ok(Box::new(Int64Field::new(rdr, id)?)),
        FieldType::Length => Ok(Box::new(LengthField::new(rdr, id)?)),
        FieldType::String => Ok(Box::new(StringField::new(rdr, id)?)),
        _ => Ok(Box::new(UnsupportedNdeField::new(rdr, id, ft)?)),
    }
}
