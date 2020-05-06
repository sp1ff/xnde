// Copyright (C) 2020 Michael Herstine <sp1ff@pobox.com>
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
//! Track type
//!
//! # Introduction
//!
//! This module introduces the [`Track`] struct, which represents a single track in your Winamp
//! Music Library. The idea is to map each record in the NDE "main" table to a [`Track`] instance.
//! [`Track`] derives the [`Serialize`] [`Serde`] trait, making it easy to write to file.
//!
//! [`Track`]: struct.Track.html
//! [`Serialize`]: https://docs.serde.rs/serde/trait.Serialize.html
//! [`Serde`]: https://docs.serde.rs
//!
//! # Discussion
//!
//! This is probably the module with which I am the least satisfied. The design problem is:
//!
//! 1. the attributes of a track shall be derived at runtime from the columns in the Music
//!    Library Database main table rather than fixed at compile time.
//!
//! 2. even at runtime, within the table, not every column is guaranteed to appear in any
//!    given record
//!
//! I solved this by first building a mapping from column ID to track attributes (when reading the
//! first record of the table). For each subsequent record, that lets me map each field (for which I
//! have a column ID) to the corresponding track attribute. I use a per-record map of
//! [`TrackAttribute`] to [`FieldValue`] to keep track of what I've seen so far (since I don't want
//! to count even on the fields appearing in the same order). Finally, once I've parsed the entire
//! record, I read the elements out & into the new [`Track`] instance.
//!
//! The basic design isn't awful, but the implementation code is prolix & inelegant. Suggestions
//! [welcome](mailto:sp1ff@pobox.com).
//!
//! [`TrackAttribute`]: enum.TrackAttribute.html
//! [`FieldValue`]: enum.FieldValue.html
//! [`Track`]: struct.Track.html

use crate::fields::{ColumnField, FieldValue, NdeField};

use log::error;
use parse_display::Display;
use serde::Serialize;

use std::collections::HashMap;

////////////////////////////////////////////////////////////////////////////////////////////////////
//                                           error type                                           //
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Display)]
pub enum Cause {
    /// An error in another crate or  module-- cf. source.
    #[display("An error in another crate or  module-- cf. source.")]
    Other,
    /// No filename field found
    #[display("No filename field found.")]
    NoFilename,
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

pub type Result<T> = std::result::Result<T, Error>;

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Enumerated set of attributes which Track may include
#[derive(Debug, Eq, Hash, PartialEq)]
pub enum TrackAttrs {
    Filename,
    Artist,
    Title,
    Album,
    Year,
    Genre,
    Comment,
    TrackNo,
    Length,
    Type,
    LastUpd,
    LastPlay,
    Rating,
    Tuid2,
    PlayCount,
    Filetime,
    Filesize,
    Bitrate,
    Disc,
    Albumartist,
    ReplaygainAlbumGain,
    ReplaygainTrackGain,
    Publisher,
    Composer,
    Bpm,
    Discs,
    Tracks,
    IsPodcast,
    PodcastChannel,
    PodcastPubdate,
    GracenoteFileId,
    GracenoteExtData,
    Lossless,
    Category,
    Codec,
    Director,
    Producer,
    Width,
    Height,
    MimeType,
    DateAdded,
}

/// Map NDE table columns (discovered at runtime) to Track attributes (fixed at compile-time)
pub type ColumnMap = HashMap<i32, TrackAttrs>;

/// Build a ColumnMap from the columns in a table's first record
pub fn new_column_map<'a, CI>(cols: CI) -> ColumnMap
where
    CI: Iterator<Item = &'a ColumnField>,
{
    let mut col_map: HashMap<i32, TrackAttrs> = HashMap::new();
    for col in cols {
        let id = col.id();
        match col.name().as_str() {
            "filename" => {
                col_map.insert(id, TrackAttrs::Filename);
            }
            "artist" => {
                col_map.insert(id, TrackAttrs::Artist);
            }
            "title" => {
                col_map.insert(id, TrackAttrs::Title);
            }
            "album" => {
                col_map.insert(id, TrackAttrs::Album);
            }
            "year" => {
                col_map.insert(id, TrackAttrs::Year);
            }
            "genre" => {
                col_map.insert(id, TrackAttrs::Genre);
            }
            "comment" => {
                col_map.insert(id, TrackAttrs::Comment);
            }
            "trackno" => {
                col_map.insert(id, TrackAttrs::TrackNo);
            }
            "length" => {
                col_map.insert(id, TrackAttrs::Length);
            }
            "type" => {
                col_map.insert(id, TrackAttrs::Type);
            }
            "lastupd" => {
                col_map.insert(id, TrackAttrs::LastUpd);
            }
            "lastplay" => {
                col_map.insert(id, TrackAttrs::LastPlay);
            }
            "rating" => {
                col_map.insert(id, TrackAttrs::Rating);
            }
            "tuid2" => {
                col_map.insert(id, TrackAttrs::Tuid2);
            }
            "playcount" => {
                col_map.insert(id, TrackAttrs::PlayCount);
            }
            "filetime" => {
                col_map.insert(id, TrackAttrs::Filetime);
            }
            "filesize" => {
                col_map.insert(id, TrackAttrs::Filesize);
            }
            "bitrate" => {
                col_map.insert(id, TrackAttrs::Bitrate);
            }
            "disc" => {
                col_map.insert(id, TrackAttrs::Disc);
            }
            "albumartist" => {
                col_map.insert(id, TrackAttrs::Albumartist);
            }
            "replaygain_album_gain" => {
                col_map.insert(id, TrackAttrs::ReplaygainAlbumGain);
            }
            "replaygain_track_gain" => {
                col_map.insert(id, TrackAttrs::ReplaygainTrackGain);
            }
            "publisher" => {
                col_map.insert(id, TrackAttrs::Publisher);
            }
            "composer" => {
                col_map.insert(id, TrackAttrs::Composer);
            }
            "bpm" => {
                col_map.insert(id, TrackAttrs::Bpm);
            }
            "discs" => {
                col_map.insert(id, TrackAttrs::Discs);
            }
            "tracks" => {
                col_map.insert(id, TrackAttrs::Tracks);
            }
            "ispodcast" => {
                col_map.insert(id, TrackAttrs::IsPodcast);
            }
            "podcastchannel" => {
                col_map.insert(id, TrackAttrs::PodcastChannel);
            }
            "podcastpubdate" => {
                col_map.insert(id, TrackAttrs::PodcastPubdate);
            }
            "GracenoteFileID" => {
                col_map.insert(id, TrackAttrs::GracenoteFileId);
            }
            "GracenoteExtData" => {
                col_map.insert(id, TrackAttrs::GracenoteExtData);
            }
            "lossless" => {
                col_map.insert(id, TrackAttrs::Lossless);
            }
            "category" => {
                col_map.insert(id, TrackAttrs::Category);
            }
            "codec" => {
                col_map.insert(id, TrackAttrs::Codec);
            }
            "director" => {
                col_map.insert(id, TrackAttrs::Director);
            }
            "producer" => {
                col_map.insert(id, TrackAttrs::Producer);
            }
            "width" => {
                col_map.insert(id, TrackAttrs::Width);
            }
            "height" => {
                col_map.insert(id, TrackAttrs::Height);
            }
            "mimetype" => {
                col_map.insert(id, TrackAttrs::MimeType);
            }
            "dateadded" => {
                col_map.insert(id, TrackAttrs::DateAdded);
            }
            _ => (),
        }
    }
    col_map
}

/// Winamp Music Library track
#[derive(Debug, Serialize)]
pub struct Track {
    filename: std::path::PathBuf,
    artist: Option<String>,
    title: Option<String>,
    album: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
    comment: Option<String>,
    trackno: Option<i32>,
    length: Option<i32>,
    ttype: Option<i32>,
    lastupd: Option<i32>,
    lastplay: Option<i32>,
    rating: Option<i32>,
    tuid2: Option<String>,
    play_count: Option<i32>,
    filetime: Option<i32>,
    filesize: Option<i64>,
    bitrate: Option<i32>,
    disc: Option<i32>,
    albumartist: Option<String>,
    replaygain_album_gain: Option<String>,
    replaygain_track_gain: Option<String>,
    publisher: Option<String>,
    composer: Option<String>,
    bpm: Option<i32>,
    discs: Option<i32>,
    tracks: Option<i32>,
    is_podcast: Option<i32>,
    podcast_channel: Option<String>,
    podcast_pubdate: Option<i32>,
    gracenote_file_id: Option<String>,
    gracenote_ext_data: Option<String>,
    lossless: Option<i32>,
    category: Option<String>,
    codec: Option<String>,
    director: Option<String>,
    producer: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    mimetype: Option<String>,
    date_added: Option<i32>,
}

impl Track {
    pub fn new<'a, FI>(col_map: &ColumnMap, fields: FI) -> Result<Track>
    where
        FI: Iterator<Item = &'a Box<dyn NdeField>>,
    {
        // build a map `attrs_map' from TrackAttrs to fields
        let mut attrs_map: HashMap<TrackAttrs, crate::fields::FieldValue> = HashMap::new();

        for field in fields {
            match col_map.get(&field.id()) {
                Some(attr) => match (attr, field.value()) {
                    (TrackAttrs::Filename, FieldValue::Filename(x)) => {
                        attrs_map.insert(TrackAttrs::Filename, FieldValue::Filename(x));
                    }
                    (TrackAttrs::Artist, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Artist, FieldValue::String(x));
                    }
                    (TrackAttrs::Title, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Title, FieldValue::String(x));
                    }
                    (TrackAttrs::Album, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Album, FieldValue::String(x));
                    }
                    (TrackAttrs::Year, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Year, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Genre, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Genre, FieldValue::String(x));
                    }
                    (TrackAttrs::Comment, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Comment, FieldValue::String(x));
                    }
                    (TrackAttrs::TrackNo, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::TrackNo, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Length, FieldValue::Length(x)) => {
                        attrs_map.insert(TrackAttrs::Length, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Type, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Type, FieldValue::Integer(x));
                    }
                    (TrackAttrs::LastUpd, FieldValue::Datetime(x)) => {
                        attrs_map.insert(TrackAttrs::LastUpd, FieldValue::Datetime(x));
                    }
                    (TrackAttrs::LastPlay, FieldValue::Datetime(x)) => {
                        attrs_map.insert(TrackAttrs::LastPlay, FieldValue::Datetime(x));
                    }
                    (TrackAttrs::Rating, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Rating, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Tuid2, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Tuid2, FieldValue::String(x));
                    }
                    (TrackAttrs::PlayCount, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::PlayCount, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Filetime, FieldValue::Datetime(x)) => {
                        attrs_map.insert(TrackAttrs::Filetime, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Filesize, FieldValue::Int64(x)) => {
                        attrs_map.insert(TrackAttrs::Filesize, FieldValue::Int64(x));
                    }
                    (TrackAttrs::Bitrate, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Bitrate, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Disc, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Disc, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Albumartist, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Albumartist, FieldValue::String(x));
                    }
                    (TrackAttrs::ReplaygainAlbumGain, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::ReplaygainAlbumGain, FieldValue::String(x));
                    }
                    (TrackAttrs::ReplaygainTrackGain, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::ReplaygainTrackGain, FieldValue::String(x));
                    }
                    (TrackAttrs::Publisher, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Publisher, FieldValue::String(x));
                    }
                    (TrackAttrs::Composer, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Composer, FieldValue::String(x));
                    }
                    (TrackAttrs::Bpm, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Bpm, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Discs, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Discs, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Tracks, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Tracks, FieldValue::Integer(x));
                    }
                    (TrackAttrs::IsPodcast, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::IsPodcast, FieldValue::Integer(x));
                    }
                    (TrackAttrs::PodcastChannel, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::PodcastChannel, FieldValue::String(x));
                    }
                    (TrackAttrs::PodcastPubdate, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::PodcastPubdate, FieldValue::Integer(x));
                    }
                    (TrackAttrs::GracenoteFileId, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::GracenoteFileId, FieldValue::String(x));
                    }
                    (TrackAttrs::GracenoteExtData, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::GracenoteExtData, FieldValue::String(x));
                    }
                    (TrackAttrs::Lossless, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Lossless, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Category, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Category, FieldValue::String(x));
                    }
                    (TrackAttrs::Codec, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Codec, FieldValue::String(x));
                    }
                    (TrackAttrs::Director, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Director, FieldValue::String(x));
                    }
                    (TrackAttrs::Producer, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::Producer, FieldValue::String(x));
                    }
                    (TrackAttrs::Width, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Width, FieldValue::Integer(x));
                    }
                    (TrackAttrs::Height, FieldValue::Integer(x)) => {
                        attrs_map.insert(TrackAttrs::Height, FieldValue::Integer(x));
                    }
                    (TrackAttrs::MimeType, FieldValue::String(x)) => {
                        attrs_map.insert(TrackAttrs::MimeType, FieldValue::String(x));
                    }
                    (TrackAttrs::DateAdded, FieldValue::Datetime(x)) => {
                        attrs_map.insert(TrackAttrs::DateAdded, FieldValue::Datetime(x));
                    }
                    _ => {
                        error!("failed to match: ({:#?}, {:#?})!", attr, field.value());
                    }
                },
                None => {
                    error!("failed to match: {}", field.id());
                }
            }
        }

        // TODO(sp1ff): This seems awful to me. I don't know if this is Rusty (Rustaceous?)
        // build the track instance thus:
        let filename = match attrs_map.get(&TrackAttrs::Filename) {
            Some(FieldValue::Filename(x)) => x.clone(),
            _ => {
                return Err(Error::new(Cause::NoFilename));
            }
        };
        // TODO(sp1ff): return an error if there is a field with the correct column id, but the
        // wrong type!
        let artist = match attrs_map.get(&TrackAttrs::Artist) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let title = match attrs_map.get(&TrackAttrs::Title) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let album = match attrs_map.get(&TrackAttrs::Album) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let year = match attrs_map.get(&TrackAttrs::Year) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let genre = match attrs_map.get(&TrackAttrs::Genre) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let comment = match attrs_map.get(&TrackAttrs::Comment) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let trackno = match attrs_map.get(&TrackAttrs::TrackNo) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let length = match attrs_map.get(&TrackAttrs::Length) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let ttype = match attrs_map.get(&TrackAttrs::Type) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let lastupd = match attrs_map.get(&TrackAttrs::LastUpd) {
            Some(FieldValue::Datetime(x)) => Some(*x),
            _ => None,
        };
        let lastplay = match attrs_map.get(&TrackAttrs::LastPlay) {
            Some(FieldValue::Datetime(x)) => Some(*x),
            _ => None,
        };
        let rating = match attrs_map.get(&TrackAttrs::Rating) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let tuid2 = match attrs_map.get(&TrackAttrs::Tuid2) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let play_count = match attrs_map.get(&TrackAttrs::PlayCount) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };

        let filetime = match attrs_map.get(&TrackAttrs::Filetime) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let filesize = match attrs_map.get(&TrackAttrs::Filesize) {
            Some(FieldValue::Int64(x)) => Some(*x),
            _ => None,
        };
        let bitrate = match attrs_map.get(&TrackAttrs::Bitrate) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let disc = match attrs_map.get(&TrackAttrs::Disc) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let albumartist = match attrs_map.get(&TrackAttrs::Albumartist) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let replaygain_album_gain = match attrs_map.get(&TrackAttrs::ReplaygainAlbumGain) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let replaygain_track_gain = match attrs_map.get(&TrackAttrs::ReplaygainTrackGain) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let publisher = match attrs_map.get(&TrackAttrs::Publisher) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let composer = match attrs_map.get(&TrackAttrs::Composer) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let bpm = match attrs_map.get(&TrackAttrs::Bpm) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let discs = match attrs_map.get(&TrackAttrs::Discs) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let tracks = match attrs_map.get(&TrackAttrs::Tracks) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let ispodcast = match attrs_map.get(&TrackAttrs::IsPodcast) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let podcastchannel = match attrs_map.get(&TrackAttrs::PodcastChannel) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let podcastpubdate = match attrs_map.get(&TrackAttrs::PodcastPubdate) {
            Some(FieldValue::Datetime(x)) => Some(*x),
            _ => None,
        };
        let gracenote_file_id = match attrs_map.get(&TrackAttrs::GracenoteFileId) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let gracenote_ext_data = match attrs_map.get(&TrackAttrs::GracenoteExtData) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let lossless = match attrs_map.get(&TrackAttrs::Lossless) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let category = match attrs_map.get(&TrackAttrs::Category) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let codec = match attrs_map.get(&TrackAttrs::Codec) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let director = match attrs_map.get(&TrackAttrs::Director) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let producer = match attrs_map.get(&TrackAttrs::Producer) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let width = match attrs_map.get(&TrackAttrs::Width) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let height = match attrs_map.get(&TrackAttrs::Height) {
            Some(FieldValue::Integer(x)) => Some(*x),
            _ => None,
        };
        let mimetype = match attrs_map.get(&TrackAttrs::MimeType) {
            Some(FieldValue::String(x)) => Some(x.clone()),
            _ => None,
        };
        let dateadded = match attrs_map.get(&TrackAttrs::DateAdded) {
            Some(FieldValue::Datetime(x)) => Some(*x),
            _ => None,
        };

        Ok(Track {
            filename: filename,
            artist: artist,
            title: title,
            album: album,
            year: year,
            genre: genre,
            comment: comment,
            trackno: trackno,
            length: length,
            ttype: ttype,
            lastupd: lastupd,
            lastplay: lastplay,
            rating: rating,
            tuid2: tuid2,
            play_count: play_count,
            filetime: filetime,
            filesize: filesize,
            bitrate: bitrate,
            disc: disc,
            albumartist: albumartist,
            replaygain_album_gain: replaygain_album_gain,
            replaygain_track_gain: replaygain_track_gain,
            publisher: publisher,
            composer: composer,
            bpm: bpm,
            discs: discs,
            tracks: tracks,
            is_podcast: ispodcast,
            podcast_channel: podcastchannel,
            podcast_pubdate: podcastpubdate,
            gracenote_file_id: gracenote_file_id,
            gracenote_ext_data: gracenote_ext_data,
            lossless: lossless,
            category: category,
            codec: codec,
            director: director,
            producer: producer,
            width: width,
            height: height,
            mimetype: mimetype,
            date_added: dateadded,
        })
    }
}
