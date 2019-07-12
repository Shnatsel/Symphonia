// Sonata
// Copyright (c) 2019 The Sonata Project Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
use sonata_core::errors::{Result, unsupported_error, decode_error};
use sonata_core::io::{Bytestream, BufStream, FiniteStream};
use sonata_core::tags::{StandardTagKey, Tag};

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::str;

use lazy_static::lazy_static;
use encoding_rs::UTF_16BE;

use super::unsync::{decode_unsynchronisation, read_syncsafe_leq32};

// The following is a list of all standardized ID3v2.x frames for all ID3v2 major versions and their implementation
// status ("S" column) in Sonata.
//
// ID3v2.2 uses 3 character frame identifiers as opposed to the 4 character identifiers used in subsequent versions. 
// This table may be used to map equivalent frames between the two versions.
//
// All ID3v2.3 frames are officially part of ID3v2.4 with the exception of those marked "n/a". However, it is likely 
// that ID3v2.3-only frames appear in some real-world ID3v2.4 tags.
//
//   -   ----   ----    ----    ----------------    ------------------------------------------------
//   S   v2.2   v2.3    v2.4    Std. Key            Description
//   -   ----   ----    ----    ----------------    ------------------------------------------------
//       CRA    AENC                                Audio encryption
//       CRM                                        Encrypted meta frame
//       PIC    APIC                                Attached picture
//                      ASPI                        Audio seek point index
//       COM    COMM             Comment            Comments
//              COMR                                Commercial frame
//              ENCR                                Encryption method registration
//       EQU    EQUA                                Equalisation
//                      EQU2                        Equalisation (2)
//       ETC    ETCO                                Event timing codes
//       GEO    GEOB                                General encapsulated object
//              GRID                                Group identification registration
//       IPL    IPLS    TIPL     Engineer           Involved people list
//       LNK    LINK                                Linked information
//       MCI    MCDI                                Music CD identifier
//       MLL    MLLT                                MPEG location lookup table
//              OWNE                                Ownership frame
//   x          PRIV                                Private frame
//       CNT    PCNT                                Play counter
//       POP    POPM                                Popularimeter
//              POSS                                Position synchronisation frame
//       BUF    RBUF                                Recommended buffer size
//       RVA    RVAD                                Relative volume adjustment
//                      RVA2                        Relative volume adjustment (2)
//       REV    RVRB                                Reverb
//                      SEEK                        Seek frame
//                      SIGN                        Signature frame
//       SLT    SYLT                                Synchronized lyric/text
//       STC    SYTC                                Synchronized tempo codes
//   x   TAL    TALB             Album              Album/Movie/Show title
//   x   TBP    TBPM             Bpm                BPM (beats per minute)
//   x   TCM    TCOM             Composer           Composer
//   x   TCO    TCON             Genre              Content type
//   x   TCR    TCOP             Copyright          Copyright message
//   x   TDA    TDAT             Date               Date
//   x                  TDEN                        Encoding time
//   x   TDY    TDLY                                Playlist delay
//   x                  TDOR                        Original release time
//   x                  TDRC     Date               Recording time
//   x                  TDRL                        Release time
//   x                  TDTG                        Tagging time
//   x   TEN    TENC             EncodedBy          Encoded by
//   x   TXT    TEXT             Writer             Lyricist/Text writer
//   x   TFT    TFLT                                File type
//   x   TIM    TIME     n/a                        Time
//   x   TT1    TIT1             ContentGroup       Content group description
//   x   TT2    TIT2             TrackTitle         Title/songname/content description
//   x   TT3    TIT3             TrackSubtitle      Subtitle/Description refinement
//   x   TKE    TKEY                                Initial key
//   x   TLA    TLAN             Language           Language(s)
//   x   TLE    TLEN                                Length
//   x                  TMCL                        Musician credits list
//   x   TMT    TMED             MediaFormat        Media type
//   x                  TMOO     Mood               Mood
//   x   TOT    TOAL                                Original album/movie/show title
//   x   TOF    TOFN                                Original filename
//   x   TOL    TOLY                                Original lyricist(s)/text writer(s)
//   x   TOA    TOPE                                Original artist(s)/performer(s)
//   x   TOR    TORY    n/a      OriginalDate       Original release year
//   x          TOWN                                File owner/licensee
//   x   TP1    TPE1             Artist             Lead performer(s)/Soloist(s)
//   x   TP2    TPE2             AlbumArtist        Band/orchestra/accompaniment
//   x   TP3    TPE3             Performer          Conductor/performer refinement
//   x   TP4    TPE4             Remixer            Interpreted, remixed, or otherwise modified by
//   x   TPA    TPOS             TrackNumber        Part of a set
//   x                  TPRO                        Produced notice
//   x   TPB    TPUB             Label              Publisher
//   x   TRK    TRCK             TrackNumber        Track number/Position in set
//   x   TRD    TRDA    n/a                         Recording dates
//   x          TRSN                                Internet radio station name
//   x          TRSO                                Internet radio station owner
//   x                  TSOA     SortAlbumn         Album sort order
//   x                  TSOP     SortArtist         Performer sort order
//   x                  TSOT     SortTrackTitle     Title sort order
//   x   TSI    TSIZ    n/a                         Size
//   x   TRC    TSRC             IdentIsrc          ISRC (international standard recording code)
//   x   TSS    TSSE             Encoder            Software/Hardware and settings used for encoding
//   x   TYE    TYER    n/a      Date               Year
//   x   TXX    TXXX                                User defined text information frame
//       UFI    UFID                                Unique file identifier
//              USER                                Terms of use
//       ULT    USLT                                Unsychronized lyric/text transcription
//   x   WCM    WCOM                                Commercial information
//   x   WCP    WCOP                                Copyright/Legal information
//   x   WAF    WOAF                                Official audio file webpage
//   x   WAR    WOAR                                Official artist/performer webpage
//   x   WAS    WOAS                                Official audio source webpage
//   x          WORS                                Official internet radio station homepage
//   x          WPAY                                Payment
//   x   WPB    WPUB                                Publishers official webpage
//   x   WXX    WXXX                                User defined URL link frame
//
// Information on these frames can be found at:
//
//     ID3v2.2: http://id3.org/id3v2-00
//     ID3v2.3: http://id3.org/d3v2.3.0
//     ID3v2.4: http://id3.org/id3v2.4.0-frames

/// The result of parsing a frame.
pub enum FrameResult {
    /// Padding was encountered instead of a frame.
    Padding,
    /// An unknown frame was found and its body skipped.
    UnsupportedFrame,
    /// A frame was parsed and yielded a single `Tag`.
    Tag(Tag),
    // A frame was parsed and yielded many `Tag`s.
    MultipleTags(Vec<Tag>)
}

type FrameParser = fn(&mut BufStream, Option<StandardTagKey>, &str) -> Result<FrameResult>;

lazy_static! {
    static ref LEGACY_FRAME_MAP: 
        HashMap<&'static [u8; 3], &'static [u8; 4]> = {
            let mut m = HashMap::new();
            m.insert(b"CRA", b"AENC");
            m.insert(b"PIC", b"APIC");
            m.insert(b"COM", b"COMM");
            m.insert(b"EQU", b"EQUA");
            m.insert(b"ETC", b"ETCO");
            m.insert(b"GEO", b"GEOB");
            m.insert(b"IPL", b"IPLS");
            m.insert(b"LNK", b"LINK");
            m.insert(b"MCI", b"MCDI");
            m.insert(b"MLL", b"MLLT");
            m.insert(b"CNT", b"PCNT");
            m.insert(b"POP", b"POPM");
            m.insert(b"BUF", b"RBUF");
            m.insert(b"RVA", b"RVAD");
            m.insert(b"REV", b"RVRB");
            m.insert(b"SLT", b"SYLT");
            m.insert(b"STC", b"SYTC");
            m.insert(b"TAL", b"TALB");
            m.insert(b"TBP", b"TBPM");
            m.insert(b"TCM", b"TCOM");
            m.insert(b"TCO", b"TCON");
            m.insert(b"TCR", b"TCOP");
            m.insert(b"TDA", b"TDAT");
            m.insert(b"TDY", b"TDLY");
            m.insert(b"TEN", b"TENC");
            m.insert(b"TXT", b"TEXT");
            m.insert(b"TFT", b"TFLT");
            m.insert(b"TIM", b"TIME");
            m.insert(b"TT1", b"TIT1");
            m.insert(b"TT2", b"TIT2");
            m.insert(b"TT3", b"TIT3");
            m.insert(b"TKE", b"TKEY");
            m.insert(b"TLA", b"TLAN");
            m.insert(b"TLE", b"TLEN");
            m.insert(b"TMT", b"TMED");
            m.insert(b"TOT", b"TOAL");
            m.insert(b"TOF", b"TOFN");
            m.insert(b"TOL", b"TOLY");
            m.insert(b"TOA", b"TOPE");
            m.insert(b"TOR", b"TORY");
            m.insert(b"TP1", b"TPE1");
            m.insert(b"TP2", b"TPE2");
            m.insert(b"TP3", b"TPE3");
            m.insert(b"TP4", b"TPE4");
            m.insert(b"TPA", b"TPOS");
            m.insert(b"TPB", b"TPUB");
            m.insert(b"TRK", b"TRCK");
            m.insert(b"TRD", b"TRDA");
            m.insert(b"TSI", b"TSIZ");
            m.insert(b"TRC", b"TSRC");
            m.insert(b"TSS", b"TSSE");
            m.insert(b"TYE", b"TYER");
            m.insert(b"TXX", b"TXXX");
            m.insert(b"UFI", b"UFID");
            m.insert(b"ULT", b"USLT");
            m.insert(b"WCM", b"WCOM");
            m.insert(b"WCP", b"WCOP");
            m.insert(b"WAF", b"WOAF");
            m.insert(b"WAR", b"WOAR");
            m.insert(b"WAS", b"WOAS");
            m.insert(b"WPB", b"WPUB");
            m.insert(b"WXX", b"WXXX");
            m
        };
}

lazy_static! {
    static ref FRAME_PARSERS: 
        HashMap<&'static [u8; 4], (FrameParser, Option<StandardTagKey>)> = {
            let mut m = HashMap::new();
            // m.insert(b"AENC", read_null_frame);
            // m.insert(b"APIC", read_null_frame);
            // m.insert(b"ASPI", read_null_frame);
            // m.insert(b"COMM", read_null_frame);
            // m.insert(b"COMR", read_null_frame);
            // m.insert(b"ENCR", read_null_frame);
            // m.insert(b"EQU2", read_null_frame);
            // m.insert(b"EQUA", read_null_frame);
            // m.insert(b"ETCO", read_null_frame);
            // m.insert(b"GEOB", read_null_frame);
            // m.insert(b"GRID", read_null_frame);
            // m.insert(b"IPLS", read_null_frame);
            // m.insert(b"LINK", read_null_frame);
            // m.insert(b"MCDI", read_null_frame);
            // m.insert(b"MLLT", read_null_frame);
            // m.insert(b"OWNE", read_null_frame);
            // m.insert(b"PCNT", read_null_frame);
            // m.insert(b"POPM", read_null_frame);
            // m.insert(b"POSS", read_null_frame);
            m.insert(b"PRIV", (read_priv_frame as FrameParser, None));
            // m.insert(b"RBUF", read_null_frame);
            // m.insert(b"RVA2", read_null_frame);
            // m.insert(b"RVAD", read_null_frame);
            // m.insert(b"RVRB", read_null_frame);
            // m.insert(b"SEEK", read_null_frame);
            // m.insert(b"SIGN", read_null_frame);
            // m.insert(b"SYLT", read_null_frame);
            // m.insert(b"SYTC", read_null_frame);
            m.insert(b"TALB", (read_text_frame, Some(StandardTagKey::Album)));
            m.insert(b"TBPM", (read_text_frame, Some(StandardTagKey::Bpm)));
            m.insert(b"TCOM", (read_text_frame, Some(StandardTagKey::Composer)));
            m.insert(b"TCON", (read_text_frame, Some(StandardTagKey::Genre)));
            m.insert(b"TCOP", (read_text_frame, Some(StandardTagKey::Copyright)));
            m.insert(b"TDAT", (read_text_frame, Some(StandardTagKey::Date)));
            m.insert(b"TDEN", (read_text_frame, None));
            m.insert(b"TDLY", (read_text_frame, None));
            m.insert(b"TDOR", (read_text_frame, None));
            m.insert(b"TDRC", (read_text_frame, Some(StandardTagKey::Date)));
            m.insert(b"TDRL", (read_text_frame, None));
            m.insert(b"TDTG", (read_text_frame, None));
            m.insert(b"TENC", (read_text_frame, Some(StandardTagKey::EncodedBy)));
            // Also Writer?
            m.insert(b"TEXT", (read_text_frame, Some(StandardTagKey::Writer)));
            m.insert(b"TFLT", (read_text_frame, None));
            m.insert(b"TIME", (read_text_frame, None));
            m.insert(b"TIPL", (read_text_frame, Some(StandardTagKey::Engineer)));
            m.insert(b"TIT1", (read_text_frame, Some(StandardTagKey::ContentGroup)));
            m.insert(b"TIT2", (read_text_frame, Some(StandardTagKey::TrackTitle)));
            m.insert(b"TIT3", (read_text_frame, Some(StandardTagKey::TrackSubtitle)));
            m.insert(b"TKEY", (read_text_frame, None));
            m.insert(b"TLAN", (read_text_frame, Some(StandardTagKey::Language)));
            m.insert(b"TLEN", (read_text_frame, None));
            m.insert(b"TMCL", (read_text_frame, None));
            m.insert(b"TMED", (read_text_frame, Some(StandardTagKey::MediaFormat)));
            m.insert(b"TMOO", (read_text_frame, Some(StandardTagKey::Mood)));
            m.insert(b"TOAL", (read_text_frame, None));
            m.insert(b"TOFN", (read_text_frame, None));
            m.insert(b"TOLY", (read_text_frame, None));
            m.insert(b"TOPE", (read_text_frame, None));
            m.insert(b"TORY", (read_text_frame, Some(StandardTagKey::OriginalDate)));
            m.insert(b"TOWN", (read_text_frame, None));
            m.insert(b"TPE1", (read_text_frame, Some(StandardTagKey::Artist)));
            m.insert(b"TPE2", (read_text_frame, Some(StandardTagKey::AlbumArtist)));
            m.insert(b"TPE3", (read_text_frame, Some(StandardTagKey::Conductor)));
            m.insert(b"TPE4", (read_text_frame, Some(StandardTagKey::Remixer)));
            // May be "track number / total tracks"
            m.insert(b"TPOS", (read_text_frame, Some(StandardTagKey::TrackNumber)));
            m.insert(b"TPRO", (read_text_frame, None));
            m.insert(b"TPUB", (read_text_frame, Some(StandardTagKey::Label)));
            // May be "track number / total tracks"
            m.insert(b"TRCK", (read_text_frame, Some(StandardTagKey::TrackNumber)));
            m.insert(b"TRDA", (read_text_frame, None));
            m.insert(b"TRSN", (read_text_frame, None));
            m.insert(b"TRSO", (read_text_frame, None));
            m.insert(b"TSIZ", (read_text_frame, None));
            m.insert(b"TSOA", (read_text_frame, Some(StandardTagKey::SortAlbum)));
            m.insert(b"TSOP", (read_text_frame, Some(StandardTagKey::SortArtist)));
            m.insert(b"TSOT", (read_text_frame, Some(StandardTagKey::SortTrackTitle)));
            m.insert(b"TSRC", (read_text_frame, Some(StandardTagKey::IdentIsrc)));
            m.insert(b"TSSE", (read_text_frame, Some(StandardTagKey::Encoder)));
            m.insert(b"TXXX", (read_txxx_frame, None));
            m.insert(b"TYER", (read_text_frame, Some(StandardTagKey::Date)));
            // m.insert(b"UFID", read_null_frame);
            // m.insert(b"USER", read_null_frame);
            // m.insert(b"USLT", read_null_frame);
            m.insert(b"WCOM", (read_url_frame, None));
            m.insert(b"WCOP", (read_url_frame, None));
            m.insert(b"WOAF", (read_url_frame, None));
            m.insert(b"WOAR", (read_url_frame, None));
            m.insert(b"WOAS", (read_url_frame, None));
            m.insert(b"WORS", (read_url_frame, None));
            m.insert(b"WPAY", (read_url_frame, None));
            m.insert(b"WPUB", (read_url_frame, None));
            m.insert(b"WXXX", (read_wxxx_frame, None));
            m
        };
}

/// Validates that a frame id only contains the uppercase letters A-Z, and digits 0-9.
fn validate_frame_id(id: &[u8]) -> bool {
    // Only frame IDs with 3 or 4 characters are valid.
    if id.len() != 4 && id.len() != 3 {
        return false;
    }

    // Character:   '/'   [ '0'  ...  '9' ]  ':'  ...  '@'  [ 'A'  ...  'Z' ]   '['
    // ASCII Code:  0x2f  [ 0x30 ... 0x39 ]  0x3a ... 0x40  [ 0x41 ... 0x5a ]  0x5b
    id.iter().filter(|&b| !((*b > 0x2f && *b < 0x3a) || (*b > 0x40 && *b < 0x5b))).count() == 0
}

/// Finds a frame parser for "modern" ID3v2.3 or ID2v2.4 tags.
fn find_parser(id: &[u8; 4]) -> Option<&(FrameParser, Option<StandardTagKey>)> {
    FRAME_PARSERS.get(id)
}

/// Finds a frame parser for a "legacy" ID3v2.2 tag by finding an equivalent "modern" ID3v2.3+ frame parser.
fn find_parser_legacy(id: &[u8; 3]) -> Option<&(FrameParser, Option<StandardTagKey>)> {
     match LEGACY_FRAME_MAP.get(id) {
        Some(id) => find_parser(id),
        _        => None
    }
}

/// Read an ID3v2.2 frame.
pub fn read_id3v2p2_frame<B: Bytestream>(reader: &mut B) -> Result<FrameResult> {
    let id = reader.read_triple_bytes()?;

    // Check if padding (all 0s) was reached.
    if id == [0, 0, 0] {
        return Ok(FrameResult::Padding);
    }

    // Validate that the frame ID is legal.
    if !validate_frame_id(&id) {
        return decode_error("Invalid frame ID.");
    }

    let size = reader.read_be_u24()? as u64;

    // Find a parser for the frame. If there is none, skip over the remainder of the frame as it cannot be parsed.
    let parser = match find_parser_legacy(&id) {
        Some(p) => p,
        None => {
            reader.ignore_bytes(size)?;
            return Ok(FrameResult::UnsupportedFrame);
        }
    };

    // A frame must be atleast 1 byte as per the specification.
    if size == 0 {
        return decode_error("Frame has a size of 0.");
    }

    let data = reader.read_boxed_slice_bytes(size as usize)?;

    parser.0(&mut BufStream::new(&data), parser.1, str::from_utf8(&id).unwrap())
}

/// Read an ID3v2.3 frame.
pub fn read_id3v2p3_frame<B: Bytestream>(reader: &mut B) -> Result<FrameResult> {
    let id = reader.read_quad_bytes()?;

    // Check if padding (all 0s) was reached.
    if id == [0, 0, 0, 0] {
        return Ok(FrameResult::Padding);
    }

    // Validate that the frame ID is legal.
    if !validate_frame_id(&id) {
        return decode_error("Invalid frame ID.");
    }

    let mut size = reader.read_be_u32()? as u64;
    let flags = reader.read_be_u16()?;

    // Unused flag bits must be cleared.
    if flags & 0x1f1f != 0x0 {
        return decode_error("Unused flag bits are not cleared.");
    }

    // Find a parser for the frame. If there is none, skip over the remainder of the frame as it cannot be parsed.
    let parser = match find_parser(&id) {
        Some(p) => p,
        None => {
            reader.ignore_bytes(size)?;
            return Ok(FrameResult::UnsupportedFrame);
        }
    };

    // Frame zlib DEFLATE compression usage flag.
    // TODO: Implement decompression if it is actually used in the real world.
    if flags & 0x80 != 0x0 {
        reader.ignore_bytes(size)?;
        return unsupported_error("Compressed frames are not supported.");
    }

    // Frame encryption usage flag. This will likely never be supported since encryption methods are vendor-specific.
    if flags & 0x4 != 0x0 {
        reader.ignore_bytes(size)?;
        return unsupported_error("Encrypted frames are not supported.");
    }

    // Frame group identifier byte. Used to group a set of frames. There is no analogue in Sonata.
    if size >= 1 && (flags & 0x20) != 0x0 {
        reader.read_byte()?;
        size -= 1;
    }

    // A frame must be atleast 1 byte as per the specification.
    if size == 0 {
        return decode_error("Frame has a size of 0.");
    }

    let data = reader.read_boxed_slice_bytes(size as usize)?;

    parser.0(&mut BufStream::new(&data), parser.1, str::from_utf8(&id).unwrap())
}

/// Read an ID3v2.4 frame.
pub fn read_id3v2p4_frame<B: Bytestream + FiniteStream>(reader: &mut B) -> Result<FrameResult> {
    let id = reader.read_quad_bytes()?;

    // Check if padding (all 0s) was reached.
    if id == [0, 0, 0, 0] {
        return Ok(FrameResult::Padding);
    }

    // Validate that the frame ID is legal.
    if !validate_frame_id(&id) {
        return decode_error("Invalid frame ID.");
    }

    let mut size = read_syncsafe_leq32(reader, 28)? as u64;
    let flags = reader.read_be_u16()?;

    // Unused flag bits must be cleared.
    if flags & 0x8fb0 != 0x0 {
        return decode_error("Unused flag bits are not cleared.");
    }

    // Find a parser for the frame. If there is none, skip over the remainder of the frame as it cannot be parsed.
    let parser = match find_parser(&id) {
        Some(p) => p,
        None => {
            reader.ignore_bytes(size)?;
            return Ok(FrameResult::UnsupportedFrame);
        }
    };

    // Frame zlib DEFLATE compression usage flag.
    // TODO: Implement decompression if it is actually used in the real world.
    if flags & 0x8 != 0x0 {
        reader.ignore_bytes(size)?;
        return unsupported_error("Compressed frames are not supported.");
    }

    // Frame encryption usage flag. This will likely never be supported since encryption methods are vendor-specific.
    if flags & 0x4 != 0x0 {
        reader.ignore_bytes(size)?;
        return unsupported_error("Encrypted frames are not supported.");
    }

    // Frame group identifier byte. Used to group a set of frames. There is no analogue in Sonata.
    if size >= 1 && (flags & 0x40) != 0x0 {
        reader.read_byte()?;
        size -= 1;
    }

    // The data length indicator is optional in the frame header. This field indicates the original size of the frame
    // body before compression, encryption, and/or unsynchronisation. It is mandatory if encryption or compression are 
    // used, but only encouraged for unsynchronisation. It's not that helpful, so we just ignore it.
    if size >= 4 && (flags & 0x1) != 0x0 { 
        read_syncsafe_leq32(reader, 28)?;
        size -= 4;
    }

    // A frame must be atleast 1 byte as per the specification.
    if size == 0 {
        return decode_error("Frame has a size of 0.");
    }

    // Read the frame body into a new buffer. This is, unfortunate. The original plan was to use an UnsyncStream to
    // transparently decode the unsynchronisation stream, however, the format does not make this easy. For one, the 
    // decoded data length field is optional. This is fine.. sometimes. For example, text frames should have their 
    // text field terminated by 0x00 or 0x0000, so it /should/ be possible to scan for the termination. However, 
    // despite being mandatory per the specification, not all tags have terminated text fields. It gets even worse 
    // when your text field is actually a list. The condition to continue scanning for terminations is if there is 
    // more data left in the frame body. However, the frame body length is the unsynchronised length, not the decoded 
    // length (that part is optional). If we scan for a termination, we know the length of the /decoded/ data, not how 
    // much data we actually consumed to obtain that decoded data. Therefore we exceed the bounds of the frame. 
    // With this in mind, the easiest thing to do is just load frame body into memory, subject to a memory limit, and 
    // decode it before passing it to a parser. Therefore we always know the decoded data length and the typical 
    // algorithms work. It should be noted this isn't necessarily worse. Scanning for a termination still would've 
    // required a buffer to scan into with the UnsyncStream, whereas we can just get references to the decoded data 
    // buffer we create here. You win some, you lose some. :)
    let mut raw_data = reader.read_boxed_slice_bytes(size as usize)?;

    // The frame body is unsynchronised. Decode the unsynchronised data back to it's original form in-place before 
    // wrapping the decoded data in a BufStream for the frame parsers.
    if flags & 0x2 != 0x0 {
        let unsync_data = decode_unsynchronisation(&mut raw_data);

        parser.0(&mut BufStream::new(&unsync_data), parser.1, str::from_utf8(&id).unwrap())
    }
    // The frame body has not been unsynchronised. Wrap the raw data buffer in BufStream without any additional 
    // decoding.
    else {
        parser.0(&mut BufStream::new(&raw_data), parser.1, str::from_utf8(&id).unwrap())
    }
}

/// Reads all text frames frame except for `TXXX`.
fn read_text_frame(reader: &mut BufStream, std_key: Option<StandardTagKey>, id: &str) -> Result<FrameResult> {
    // The first byte of the frame is the encoding.
    let encoding = match Encoding::parse(reader.read_byte()?) {
        Some(encoding) => encoding,
        _              => return decode_error("Invalid text encoding.")
    };

    let mut tags = Vec::<Tag>::new();

    // The remainder of the frame is one or more null-terminated strings.
    loop {
        let len = reader.bytes_available() as usize;

        if len > 0 {
            // Scan for the appropriate null terminator based on the encoding. If a null terminator is not found, then 
            // scan_bytes() will return the remainder of the BufStream. This should handle the case where the text field
            // is not properly terminated.
            let data = scan_text(reader, encoding, len)?;
            // Decode the encoded text and build the tag.
            tags.push(Tag::new(std_key, id, &decode_text(encoding, data)));
        }
        else {
            break;
        }
    }

    Ok(FrameResult::MultipleTags(tags))
}

/// Reads a `TXXX` (user defined) text frame.
fn read_txxx_frame(reader: &mut BufStream, _: Option<StandardTagKey>, _: &str) -> Result<FrameResult> {
    // The first byte of the frame is the encoding.
    let encoding = match Encoding::parse(reader.read_byte()?) {
        Some(encoding) => encoding,
        _              => return decode_error("Invalid TXXX text encoding.")
    };

    // Read the description (key) string.
    let desc = {
        let desc_buf = scan_text(reader, encoding, reader.bytes_available() as usize)?;
        let mut desc = "TXXX:".to_string();
        desc.push_str(&decode_text(encoding, desc_buf));
        desc
    };

    // Since a TXXX frame can have a null-terminated list of values, and Sonata allows multiple tags with the same
    // key, create one Tag per listed value.
    let mut tags = Vec::<Tag>::new();

    loop {
        let len = reader.bytes_available() as usize;

        if len > 0 {
            let val_buf = scan_text(reader, encoding, len)?;
            tags.push(Tag::new(None, &desc, &decode_text(encoding, val_buf)));
        }
        else {
            break;
        }
    }

    Ok(FrameResult::MultipleTags(tags))
}

/// Reads all URL frames except for `WXXX`.
fn read_url_frame(reader: &mut BufStream, std_key: Option<StandardTagKey>, id: &str) -> Result<FrameResult> {
    // Scan for ISO-8859-1 text (all URLs are encoded in this format).
    let url_buf = scan_text(reader, Encoding::Iso8859_1, reader.bytes_available() as usize)?;
    // Create a Tag.
    let tag = Tag::new(std_key, id, &decode_text(Encoding::Iso8859_1, url_buf));

    Ok(FrameResult::Tag(tag))
}

/// Reads a `WXXX` (user defined) URL frame.
fn read_wxxx_frame(reader: &mut BufStream, std_key: Option<StandardTagKey>, _: &str) -> Result<FrameResult> {
    // The first byte of the WXXX frame is the encoding of the description.
    let encoding = match Encoding::parse(reader.read_byte()?) {
        Some(encoding) => encoding,
        _              => return decode_error("Invalid WXXX URL description encoding.")
    };

    // Read the description (key) string.
    let desc = {
        let desc_buf = scan_text(reader, encoding, reader.bytes_available() as usize)?;
        let mut desc = "WXXX:".to_string();
        desc.push_str(&decode_text(encoding, desc_buf));
        desc
    };

    // Scan for ISO-8859-1 text (all URLs are encoded in this format).
    let url_buf = scan_text(reader, Encoding::Iso8859_1, reader.bytes_available() as usize)?;
    // Create a Tag.
    let tag = Tag::new(std_key, &desc, &decode_text(Encoding::Iso8859_1, url_buf));

    Ok(FrameResult::Tag(tag))
}

/// Reads a `PRIV` (private) frame.
fn read_priv_frame(reader: &mut BufStream, std_key: Option<StandardTagKey>, _: &str) -> Result<FrameResult> {
    // Scan for ISO-8859-1 text (all URLs are encoded in this format).
    let ident = {
        let ident_buf = scan_text(reader, Encoding::Iso8859_1, reader.bytes_available() as usize)?;
        let mut ident = "PRIV:".to_string();
        ident.push_str(&decode_text(Encoding::Iso8859_1, ident_buf));
        ident
    };

    let data_buf = reader.read_buf_bytes_ref(reader.bytes_available() as usize)?;

    // Quick and dirty buffer to hex-string serialization.
    // TODO: Either allow Tags to hold binary data, OR encode as base64.
    let mut data = String::new();

    for ch in data_buf {
        let u = (ch & 0xf0) >> 4;
        let l = ch & 0x0f;
        data.push_str("\\0x");
        data.push(if u < 10 { (b'0' + u) as char } else { (b'a' + u - 10) as char});
        data.push(if l < 10 { (b'0' + l) as char } else { (b'a' + l - 10) as char});
    }
    
    // Create a Tag.
    let tag = Tag::new(std_key, &ident, &data);

    Ok(FrameResult::Tag(tag))
}

/// Enumeration of valid encodings for text fields in ID3v2 tags
#[derive(Copy, Clone, Debug)]
enum Encoding {
    /// ISO-8859-1 (aka Latin-1) characters in the range 0x20-0xFF.
    Iso8859_1,
    /// UTF-16 (or UCS-2) with a byte-order-mark (BOM). If the BOM is missing, big-endian encoding is assumed.
    Utf16Bom,
    /// UTF-16 big-endian without a byte-order-mark (BOM). 
    Utf16Be,
    /// UTF-8.
    Utf8,
}

impl Encoding {
    fn parse(encoding: u8) -> Option<Encoding> {
        match encoding {
            // ISO-8859-1 terminated with 0x00.
            0 => Some(Encoding::Iso8859_1),
            // UTF-16 with byte order marker (BOM), terminated with 0x00 0x00.
            1 => Some(Encoding::Utf16Bom),
            // UTF-16BE without byte order marker (BOM), terminated with 0x00 0x00.
            2 => Some(Encoding::Utf16Be),
            // UTF-8 terminated with 0x00.
            3 => Some(Encoding::Utf8),
            // Invalid encoding.
            _ => None
        }
    }
}

/// Scans the provided `BufStream` for the appropriate null terminator for the given encoding, and returns a slice 
/// of all the bytes upto and including the null terminator or an error. If there is no match, upto `max_len` bytes 
/// will be returned.
fn scan_text<'a>(reader: &'a mut BufStream, encoding: Encoding, max_len: usize) -> io::Result<&'a [u8]> {
    match encoding {
        Encoding::Iso8859_1 | Encoding::Utf8    => reader.scan_bytes_aligned_ref(&[0x00], 1, max_len),
        Encoding::Utf16Bom  | Encoding::Utf16Be => reader.scan_bytes_aligned_ref(&[0x00, 0x00], 2, max_len),
    }
}

/// Decodes a slice of bytes containing encoded text into a UTF-8 `str`. Trailing null terminators are removed, and any
/// invalid characters are replaced with the [U+FFFD REPLACEMENT CHARACTER].
fn decode_text(encoding: Encoding, data: &[u8]) -> Cow<'_, str> {
    let mut end = data.len();

    match encoding {
        Encoding::Iso8859_1 => {
            // The ID3v2 specification says that only ISO-8859-1 characters between 0x20 to 0xFF, inclusive, 
            // are considered valid. Any null terminator(s) (trailing 0x00 byte for ISO-8859-1) will also be
            // removed.
            //
            // TODO: Improve this conversion by returning a copy-on-write str sliced from data if all characters
            // are > 0x1F and < 0x80. Fallback to the iterator approach otherwise.
            data.iter().filter(|&b| *b > 0x1f).map(|&b| b as char).collect()
        },
        Encoding::Utf8 => {
            // Remove any null terminator(s) (trailing 0x00 byte for UTF-8).
            while end > 0 {
                if data[end-1] != 0 { break; }
                end -= 1;
            }
            String::from_utf8_lossy(&data[..end])
        },
        Encoding::Utf16Bom  | Encoding::Utf16Be => {
            // Remove any null terminator(s) (trailing [0x00, 0x00] bytes for UTF-16 variants).
            while end > 1 {
                if data[end-2] != 0x0 || data[end-1] != 0x0 { break; }
                end -= 2;
            }
            // Decode UTF-16 to UTF-8. If a byte-order-mark is present, UTF_16BE.decode() will use the indicated
            // endianness. Otherwise, big endian is assumed.
            UTF_16BE.decode(&data[..end]).0
        }
    }
}