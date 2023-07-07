use std::io::{ErrorKind, Read};
use std::io;

use byteorder::{BigEndian, ByteOrder};
use ::bytes::BytesMut;
use combine::{many, parser, ParseResult, Parser, EasyParser};
use combine::parser::function::FnParser;
use combine::parser::byte::byte;
use combine::parser::byte::bytes;
use combine::parser::range::take;
use combine::RangeStream;
use flate2::read::ZlibDecoder;
use tokio_util::codec::Decoder;



macro_rules! parser {
    ($name: ident, $return_type: ty, $e:expr) => {
        fn $name<'a, I>() -> FnParser<I, fn(I) -> ParseResult<$return_type, I>>
            where I: RangeStream
            {
                fn _event<'a, I>(input: I) -> ParseResult<$return_type, I>
                    where I: RangeStream
                    {
                        $e.parse_stream(input)
                    }
                parser(_event)
            }

    }
}

const CODE_JSON_EVENT: u8 = b'J';
const CODE_COMPRESSED: u8 = b'C';
const CODE_WINDOW_SIZE: u8 = b'W';
const PROTO_VERSION: u8 = b'2';

#[derive(Debug)]
pub struct Event {
    pub sequence: usize,
    pub raw: String,
}

impl Event {
    pub fn new(seq: usize, raw: &[u8]) -> Self {
        Event {
            sequence: seq,
            raw: String::from_utf8_lossy(raw).into_owned(),
        }
    }
}

// pub fn read_batch(data: &[u8]) -> Result<Vec<Event>, io::Error> {
//     let any_num2 = take(4).map(BigEndian::read_u32).map(|x| x as usize);
//     let compressed_block = byte(PROTO_VERSION).with(byte(CODE_COMPRESSED)).with(any_num()).then(take).and_then(extract);
//
//
//     byte(PROTO_VERSION)
//         .with(byte(CODE_WINDOW_SIZE))
//         .with(any_num2)
//         .with(compressed_block)
//         .parse(data)
//         .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse batch"))
//         .and_then(|(e, _)| {
//             many(event_block())
//                 .parse(e.as_slice())
//                 .map(|v| v.0)
//                 .map_err(|_| {
//                     io::Error::new(io::ErrorKind::InvalidData, "Failed to parse event block")
//                 })
//         })
// }
//
//
// fn any_num2<I: RangeStream, E>() -> FnParser<I, fn(I) -> ParseResult<usize, E>> {
//
// }
//
// parser! {
//     any_num, usize,
//     take(4).map(BigEndian::read_u32).map(|x| x as usize)
// }
//
// parser! {
//     event_block, Event,
//     byte(PROTO_VERSION).with(byte(CODE_JSON_EVENT)).with((any_num(), any_num().then(take)))
//         .map(|(seq, raw)| Event::new(seq, raw))
// }
//
// parser! {
//     compressed_block, Vec<u8>,
//     byte(PROTO_VERSION).with(byte(CODE_COMPRESSED)).with(any_num()).then(take).and_then(extract)
// }

fn extract(input: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut buf = Vec::new();
    let mut d = ZlibDecoder::new(input);
    d.read_to_end(&mut buf)?;
    Ok(buf)
}

#[derive(Debug)]
pub struct Request {
    pub events: Vec<Event>,
}

pub struct LumberjackCodec { }


impl Decoder for LumberjackCodec {
    type Item = Request;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let len = src.len();

        if len == 0 {
            return Ok(None)
        }

        let compressed_block = byte(PROTO_VERSION)
            .with(byte(CODE_COMPRESSED))
            .with(take(4).map(BigEndian::read_u32).map(|x| x as usize))
            .then(take)
            .and_then(extract);

        let mut lj_parser = byte(PROTO_VERSION)
            .with(byte(CODE_WINDOW_SIZE))
            .with(take(4).map(BigEndian::read_u32).map(|x| x as usize))
            .with(compressed_block);

        let res: Result<Vec<Event>, io::Error> = lj_parser.easy_parse(src.as_ref())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Failed to parse batch: {:?}", e).as_str()))
            .and_then(|(e, x)| {
                let mut event_parser = many(byte(PROTO_VERSION)
                         .with(byte(CODE_JSON_EVENT))
                         .with((take(4).map(BigEndian::read_u32).map(|x| x as usize), take(4).map(BigEndian::read_u32).map(|x| x as usize).then(take)))
                        .map(|(seq, raw)| Event::new(seq, raw)));

                event_parser.easy_parse(e.as_slice())
                    .map(|v| v.0)
                    .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse event block"))

            });

        src.split_to(len);

        return Ok(Some(Request { events: res.unwrap() }))
    }
}


#[cfg(test)]
mod lumberjack_decoder {
    use std::fs::File;
    use std::io::Read;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;
    use crate::lumberjack_decoder::LumberjackCodec;

    #[test]
    fn decode_test() {
        let mut binary = File::open("/home/wspeirs/src/log-ship/log-ship/audit_beat.bin").expect("Error opening binary file");
        let mut buff = Vec::new();

        binary.read_to_end(&mut buff).expect("Error reading binary");

        let mut bytes = BytesMut::from(buff.as_slice());
        let mut codec = LumberjackCodec { };

        let res = codec.decode(&mut bytes);
        assert!(res.is_ok());

        let res = res.unwrap();
        assert!(res.is_some());

        let res = res.unwrap();

        for event in res.events {
            println!("EVENT: {:?}", event);
        }
    }
}
