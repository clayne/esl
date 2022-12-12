use serde::{Serializer, Serialize};
use std::mem::{replace};
use std::fmt::{self, Display, Debug, Formatter};
use serde::ser::{self, Impossible, SerializeSeq, SerializeTuple, SerializeTupleStruct};
use serde::ser::{SerializeStruct, SerializeTupleVariant, SerializeStructVariant, SerializeMap};
use serde::serde_if_integer128;
use std::io::{self, Write};
use byteorder::{WriteBytesExt, LittleEndian};

use crate::code::code_page::*;

#[derive(Debug)]
pub enum Error {
    Custom(String),
    LargeObject(usize),
    InvalidString(String),
    UnrepresentableChar(char, CodePage),
    ZeroSizedLastSequenceElement,
    VariantIndexMismatch { variant_index: u32, variant_size: u32 },
    ZeroSizedOptional,
    ShortStringNotFit { max_len: usize, len: usize },
    ShortStringTailZero,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Error::Custom(s) => Display::fmt(s, f),
            Error::LargeObject(size) => write!(f, "object has too large size ({size} B)"),
            Error::InvalidString(s) =>
                write!(f, "the '{s}' string does not correspond to any source byte sequence"),
            Error::UnrepresentableChar(c, p) =>
                write!(f, "the '{c}' char is not representable in {p:?} code page"),
            Error::ZeroSizedLastSequenceElement => write!(f, "last element in sequence or map cannot have zero size"),
            Error::VariantIndexMismatch { variant_index, variant_size } =>
                write!(f, "variant index ({variant_index}) should be equal to variant size ({variant_size})"),
            Error::ZeroSizedOptional => write!(f, "optional element cannot have zero size"),
            Error::ShortStringNotFit { max_len, len } =>
                write!(f, "short string (len = {len}) does not fit (max len = {max_len})"),
            Error::ShortStringTailZero => write!(f, "short string value has tail zero"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { None }
}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Self { Error::Custom(format!("{msg}")) }
}

#[derive(Debug)]
pub enum IoError {
    Io(io::Error),
    Other(Error),
}

impl Display for IoError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            IoError::Io(e) => Display::fmt(e, f),
            IoError::Other(e) => Display::fmt(e, f),
        }
    }
}

impl std::error::Error for IoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IoError::Io(e) => Some(e),
            IoError::Other(e) => Some(e),
        }
    }
}

impl ser::Error for IoError {
    fn custom<T: Display>(msg: T) -> Self { IoError::Other(Error::custom(msg)) }
}

impl From<Error> for IoError {
    fn from(e: Error) -> IoError { IoError::Other(e) }
}

impl From<io::Error> for IoError {
    fn from(e: io::Error) -> IoError { IoError::Io(e) }
}

const SIZE_STUB: u32 = 0x1375F17B;

fn size(len: usize) -> Result<u32, Error> {
    if len > u32::max_value() as usize {
        Err(Error::LargeObject(len))
    } else {
        Ok(len as u32)
    }
}

pub(crate) trait Writer: Write {
    type Buf: Debug;
    
    fn pos(&self) -> usize;
    fn begin_isolate(&mut self) -> io::Result<Self::Buf>;
    fn end_isolate(&mut self, buf: Self::Buf, variadic_part_pos: usize) -> Result<(), IoError>;
}

impl Writer for Vec<u8> {
    type Buf = usize;

    fn pos(&self) -> usize { self.len() }

    fn begin_isolate(&mut self) -> io::Result<Self::Buf> {
        let value_size_stub_offset = self.len();
        self.write_u32::<LittleEndian>(SIZE_STUB).unwrap();
        Ok(value_size_stub_offset)
    }

    fn end_isolate(&mut self, value_size_stub_offset: usize, value_offset: usize) -> Result<(), IoError> {
        let value_size = size(self.len() - value_offset)?;
        (&mut self[value_size_stub_offset..value_size_stub_offset + 4]).write_u32::<LittleEndian>(value_size).unwrap();
        Ok(())
    }
}

pub(crate) struct SliceWriter<'a> {
    slice: &'a mut [u8],
    pos: usize
}

impl<'a> SliceWriter<'a> {
    pub fn new(slice: &'a mut [u8]) -> Self {
        SliceWriter {
            slice,
            pos: 0
        }
    }
    
    pub fn written(&self) -> usize { self.pos }
}

impl<'a> Write for SliceWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = (&mut self.slice[self.pos..]).write(buf)?;
        self.pos += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> Writer for SliceWriter<'a> {
    type Buf = usize;

    fn pos(&self) -> usize { self.pos }

    fn begin_isolate(&mut self) -> io::Result<Self::Buf> {
        let value_size_stub_offset = self.pos;
        self.write_u32::<LittleEndian>(SIZE_STUB)?;
        Ok(value_size_stub_offset)
    }

    fn end_isolate(&mut self, value_size_stub_offset: usize, value_offset: usize) -> Result<(), IoError> {
        let value_size = size(self.pos - value_offset)?;
        (&mut self.slice[value_size_stub_offset..value_size_stub_offset + 4]).write_u32::<LittleEndian>(value_size).unwrap();
        Ok(())
    }
}

pub(crate) struct GenericWriter<'a, W: Write + ?Sized> {
    writer: &'a mut W,
    write_buf: Option<Vec<u8>>,
    pos: usize,
}

impl<'a, W: Write + ?Sized> GenericWriter<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        GenericWriter {
            writer,
            write_buf: None,
            pos: 0
        }
    }
}

impl<'a, W: Write + ?Sized> Write for GenericWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(write_buf) = &mut self.write_buf {
            write_buf.extend_from_slice(buf);
            self.pos += buf.len();
            Ok(buf.len())
        } else {
            let written = self.writer.write(buf)?;
            self.pos += written;
            Ok(written)
        }
    }
    
    fn flush(&mut self) -> io::Result<()> {
        if self.write_buf.is_none() {
            self.writer.flush()
        } else {
            Ok(())
        }
    }
}

impl<'a, W: Write + ?Sized> Writer for GenericWriter<'a, W> {
    type Buf = Option<usize>;

    fn pos(&self) -> usize { self.pos }

    fn begin_isolate(&mut self) -> io::Result<Self::Buf> {
        if let Some(write_buf) = &mut self.write_buf {
            let value_size_stub_offset = write_buf.len();
            write_buf.write_u32::<LittleEndian>(SIZE_STUB).unwrap();
            self.pos += 4;
            Ok(Some(value_size_stub_offset))
        } else {
            self.pos += 4;
            self.write_buf = Some(Vec::new());
            Ok(None)
        }
    }

    fn end_isolate(&mut self, value_size_stub_offset: Option<usize>, value_pos: usize) -> Result<(), IoError> {
        let value_size = size(self.pos - value_pos)?;
        if let Some(value_size_stub_offset) = value_size_stub_offset {
            let write_buf = self.write_buf.as_mut().unwrap();
            (&mut write_buf[value_size_stub_offset..value_size_stub_offset + 4]).write_u32::<LittleEndian>(value_size).unwrap();
        } else {
            let write_buf = self.write_buf.take().unwrap();
            self.writer.write_u32::<LittleEndian>(value_size)?;
            self.writer.write_all(&write_buf[..])?;
        }
        Ok(())
    }
}

pub(crate) struct Size(pub usize);

impl Write for Size {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Writer for Size {
    type Buf = ();

    fn pos(&self) -> usize { self.0 }

    fn begin_isolate(&mut self) -> io::Result<Self::Buf> { Ok(()) }

    fn end_isolate(&mut self, _: (), _: usize) -> Result<(), IoError> {
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct ShortStringSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    size: usize,
}

#[derive(Debug)]
pub(crate) struct ShortStringZeroesSerializer<'a> {
    size: &'a mut usize,
}

#[derive(Debug)]
pub(crate) struct ShortStringZeroSerializer { }

#[derive(Debug)]
pub(crate) struct EslSerializer<'r, 'q, 'a, W: Writer> {
    isolated: bool,
    code_page: CodePage,
    writer: &'a mut W,
    map_entry_value_buf: Option<&'r mut Option<W::Buf>>,
    is_short_string_serializer: Option<&'q mut bool>,
}

impl<'r, 'q, 'a, W: Writer> EslSerializer<'r, 'q, 'a, W> {
    pub fn new(isolated: bool, code_page: CodePage, writer: &'a mut W) -> Self {
        EslSerializer { isolated, code_page, writer, map_entry_value_buf: None, is_short_string_serializer: None }
    }
}

#[derive(Debug)]
pub(crate) struct SeqSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    last_element_has_zero_size: bool,
    buf_and_start_pos: Option<(W::Buf, usize)>,
}

#[derive(Debug)]
pub(crate) struct MapSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    value_buf_and_pos: Option<(W::Buf, usize)>,
    is_short_string_serializer: bool,
}

#[derive(Debug, Copy, Clone)]
enum ShortStringField { Zeroes, String(usize) }

#[derive(Debug)]
pub(crate) struct StructSerializer<'r, 'a, W: Writer> {
    short_string_field: Option<Option<ShortStringField>>,
    code_page: CodePage,
    writer: &'a mut W,
    len: Option<usize>,
    start_pos_and_variant_index: Option<(usize, u32)>,
    value_buf: Option<&'r mut Option<W::Buf>>,
}

impl<'a, W: Writer> SerializeSeq for SeqSerializer<'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        let element_pos = self.writer.pos();
        v.serialize(EslSerializer {
            isolated: false,
            writer: self.writer, code_page: self.code_page,
            map_entry_value_buf: None,
            is_short_string_serializer: None,
        })?;
        self.last_element_has_zero_size = self.writer.pos() == element_pos;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.last_element_has_zero_size {
            return Err(Error::ZeroSizedLastSequenceElement.into());
        }
        if let Some((buf, start_pos)) = self.buf_and_start_pos {
            self.writer.end_isolate(buf, start_pos)?;
        }
        Ok(())
    }
}

impl<'a, W: Writer> SerializeMap for MapSerializer<'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        let mut value_buf = None;
        let mut is_short_string_serializer = false;
        key.serialize(EslSerializer {
            writer: self.writer,
            code_page: self.code_page,
            map_entry_value_buf: Some(&mut value_buf),
            isolated: false,
            is_short_string_serializer: Some(&mut is_short_string_serializer),
        })?;
        self.is_short_string_serializer = is_short_string_serializer;
        if !self.is_short_string_serializer {
            let b = replace(&mut self.value_buf_and_pos, Some((if let Some(value_buf) = value_buf {
                value_buf
            } else {
                self.writer.begin_isolate()?
            }, self.writer.pos())));
            assert!(b.is_none());
        }
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        v.serialize(EslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page,
            map_entry_value_buf: None,
            is_short_string_serializer: None
        })?;
        if !self.is_short_string_serializer {
            if let Some((value_buf, value_pos)) = self.value_buf_and_pos.take() {
                self.writer.end_isolate(value_buf, value_pos)?;
            }
        }
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'r, 'a, W: Writer> StructSerializer<'r, 'a, W> {
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), IoError> {
        if let Some(len) = self.len.as_mut() {
            if *len == 0 { panic!() }
            *len -= 1;
        }
        if let Some(short_string_field) = self.short_string_field {
            let Some(short_string_field) = short_string_field else {
                panic!("invalid short string serializer usage");
            };
            match short_string_field {
                ShortStringField::Zeroes => {
                    let mut size = 0;
                    v.serialize(ShortStringZeroesSerializer { size: &mut size })?;
                    self.short_string_field = Some(Some(ShortStringField::String(size)));
                },
                ShortStringField::String(size) => {
                    v.serialize(ShortStringSerializer { size, code_page: self.code_page, writer: self.writer })?;
                    self.short_string_field = Some(None);
                },
            }
        } else {
            v.serialize(EslSerializer {
                isolated: self.len.map_or(false, |len| len == 0),
                writer: self.writer, code_page: self.code_page,
                map_entry_value_buf: None,
                is_short_string_serializer: None,
            })?;
            if let &mut Some(ref mut value_buf) = &mut self.value_buf {
                if value_buf.is_none() {
                    **value_buf = Some(self.writer.begin_isolate()?);
                }
            }
        }
        Ok(())
    }

    fn end(self) -> Result<(), IoError> {
        if let Some((start_pos, variant_index)) = self.start_pos_and_variant_index {
            let variant_size = size(self.writer.pos() - start_pos)?;
            if variant_index != variant_size {
                return Err(Error::VariantIndexMismatch { variant_index, variant_size }.into());
            }
        }
        Ok(())
    }
}

impl<'r, 'a, W: Writer> SerializeTuple for StructSerializer<'r, 'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.end()
    }
}

impl<'r, 'a, W: Writer> SerializeTupleStruct for StructSerializer<'r, 'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.end()
    }
}

impl<'r, 'a, W: Writer> SerializeStruct for StructSerializer<'r, 'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.end()
    }
}

impl<'r, 'a, W: Writer> SerializeTupleVariant for StructSerializer<'r, 'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.end()
    }
}

impl<'r, 'a, W: Writer> SerializeStructVariant for StructSerializer<'r, 'a, W> {
    type Ok = ();
    type Error = IoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.end()
    }
}

impl<'r, 'q, 'a, W: Writer> Serializer for EslSerializer<'r, 'q, 'a, W> {
    type Ok = ();
    type Error = IoError;
    type SerializeSeq = SeqSerializer<'a, W>;
    type SerializeTuple = StructSerializer<'r, 'a, W>;
    type SerializeTupleStruct = StructSerializer<'r, 'a, W>;
    type SerializeTupleVariant = StructSerializer<'r, 'a, W>;
    type SerializeStruct = StructSerializer<'r, 'a, W>;
    type SerializeStructVariant = StructSerializer<'r, 'a, W>;
    type SerializeMap = MapSerializer<'a, W>;

    fn is_human_readable(&self) -> bool { false }
    
    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(v.into())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(size(v.len())?)?;
        }
        self.writer.write_all(v)?;
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u8(v)?;
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.writer.write_i8(v)?;
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u16::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.writer.write_i16::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u32::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.writer.write_i32::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.writer.write_f32::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u64::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.writer.write_i64::<LittleEndian>(v)?;
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.writer.write_f64::<LittleEndian>(v)?;
        Ok(())
    }

    serde_if_integer128! {
        fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
            self.writer.write_u128::<LittleEndian>(v)?;
            Ok(())
        }

        fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
            self.writer.write_i128::<LittleEndian>(v)?;
            Ok(())
        }
    }

    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        panic!("serialize_char not supported");
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let bytes = self.code_page.encode(v).map_err(|e|
            e.map_or_else(|| Error::InvalidString(v.to_string()), |c| Error::UnrepresentableChar(c, self.code_page))
        )?;
        self.serialize_bytes(&bytes)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(self, _: &'static str, variant_index: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        if self.map_entry_value_buf.is_some() {
            panic!("invalid short string serializer usage");
        }
        if variant_index != 0 {
            return Err(Error::VariantIndexMismatch { variant_index, variant_size: 0 }.into());
        }
        if !self.isolated {
            self.serialize_u32(0)
        } else {
            Ok(())
        }
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        v.serialize(EslSerializer { map_entry_value_buf: None, ..self })
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(0)?;
        }
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let buf = if !self.isolated {
            Some(self.writer.begin_isolate()?)
        } else {
            None
        };
        let value_pos = self.writer.pos();
        value.serialize(EslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page,
            map_entry_value_buf: None,
            is_short_string_serializer: None,
        })?;
        if self.writer.pos() == value_pos {
            return Err(Error::ZeroSizedOptional.into());
        }
        if let Some(buf) = buf {
            self.writer.end_isolate(buf, value_pos)?;
        }
        Ok(())
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, variant_index: u32, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        if self.map_entry_value_buf.is_some() {
            panic!("invalid short string serializer usage");
        }
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(variant_index)?;
        }
        let value_pos = self.writer.pos();
        v.serialize(EslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page,
            map_entry_value_buf: None,
            is_short_string_serializer: None,
        })?;
        let variant_size = size(self.writer.pos() - value_pos)?;
        if variant_index != variant_size {
            return Err(Error::VariantIndexMismatch { variant_index, variant_size }.into());
        }
        Ok(())
    }

    fn serialize_tuple_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        let (short_string_field, size) = if self.map_entry_value_buf.is_some() {
            *self.is_short_string_serializer.unwrap() = true;
            if len != 2 || variant_index != SHORT_STRING_VARIANT_INDEX {
                panic!("invalid short string variant index");
            }
            (Some(Some(ShortStringField::Zeroes)), 0)
        } else {
            if !self.isolated {
                self.writer.write_u32::<LittleEndian>(variant_index)?;
            }
            (None, variant_index)
        };
        Ok(StructSerializer {
            short_string_field,
            len: Some(len),
            start_pos_and_variant_index: if self.map_entry_value_buf.is_some() {
                None
            } else {
                Some((self.writer.pos(), size))
            },
            writer: self.writer, code_page: self.code_page,
            value_buf: None
        })
    }

    fn serialize_struct_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        if self.map_entry_value_buf.is_some() {
            panic!("invalid short string serializer usage");
        }
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(variant_index)?;
        }
        Ok(StructSerializer {
            short_string_field: None,
            len: Some(len),
            start_pos_and_variant_index: Some((self.writer.pos(), variant_index)),
            writer: self.writer, code_page: self.code_page,
            value_buf: None
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(StructSerializer {
            short_string_field: None,
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            start_pos_and_variant_index: None,
            value_buf: self.map_entry_value_buf
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(StructSerializer {
            short_string_field: None,
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            start_pos_and_variant_index: None,
            value_buf: None
        })
    }

    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(StructSerializer {
            short_string_field: None,
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            start_pos_and_variant_index: None,
            value_buf: None
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let buf = if !self.isolated { Some((self.writer.begin_isolate()?, self.writer.pos())) } else { None };
        Ok(SeqSerializer {
            writer: self.writer,
            code_page: self.code_page,
            last_element_has_zero_size: false,
            buf_and_start_pos: buf
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        assert_eq!(len, Some(1), "Maps with length different from 1 not supported.");
        Ok(MapSerializer {
            value_buf_and_pos: None,
            writer: self.writer, code_page: self.code_page,
            is_short_string_serializer: false,
        })
    }
}

impl Serializer for ShortStringZeroSerializer {
    type Ok = ();
    type Error = IoError;
    type SerializeSeq = Impossible<(), Self::Error>;
    type SerializeTuple = Impossible<(), Self::Error>;
    type SerializeTupleStruct = Impossible<(), Self::Error>;
    type SerializeTupleVariant = Impossible<(), Self::Error>;
    type SerializeStruct = Impossible<(), Self::Error>;
    type SerializeStructVariant = Impossible<(), Self::Error>;
    type SerializeMap = Impossible<(), Self::Error>;

    fn is_human_readable(&self) -> bool { false }
    
    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        if v != 0 {
            panic!("invalid short string serializer usage");
        }
        Ok(())
    }

    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    serde_if_integer128! {
        fn serialize_u128(self, _: u128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }

        fn serialize_i128(self, _: i128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }
    }

    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_str(self, _: &str) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        panic!("invalid short string serializer usage");
    }
}

impl<'a> Serializer for ShortStringZeroesSerializer<'a> {
    type Ok = ();
    type Error = IoError;
    type SerializeSeq = Impossible<(), Self::Error>;
    type SerializeTuple = ShortStringZeroesSerializer<'a>;
    type SerializeTupleStruct = Impossible<(), Self::Error>;
    type SerializeTupleVariant = Impossible<(), Self::Error>;
    type SerializeStruct = Impossible<(), Self::Error>;
    type SerializeStructVariant = Impossible<(), Self::Error>;
    type SerializeMap = Impossible<(), Self::Error>;

    fn is_human_readable(&self) -> bool { false }
    
    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    serde_if_integer128! {
        fn serialize_u128(self, _: u128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }

        fn serialize_i128(self, _: i128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }
    }

    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_str(self, _: &str) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        *self.size = len;
        Ok(self)
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        panic!("invalid short string serializer usage");
    }
}

impl<'a> SerializeTuple for ShortStringZeroesSerializer<'a> {
    type Ok = ();
    type Error = IoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        v.serialize(ShortStringZeroSerializer { })
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl<'a, W: Writer> Serializer for ShortStringSerializer<'a, W> {
    type Ok = ();
    type Error = IoError;
    type SerializeSeq = Impossible<(), Self::Error>;
    type SerializeTuple = Impossible<(), Self::Error>;
    type SerializeTupleStruct = Impossible<(), Self::Error>;
    type SerializeTupleVariant = Impossible<(), Self::Error>;
    type SerializeStruct = Impossible<(), Self::Error>;
    type SerializeStructVariant = Impossible<(), Self::Error>;
    type SerializeMap = Impossible<(), Self::Error>;

    fn is_human_readable(&self) -> bool { false }
    
    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    serde_if_integer128! {
        fn serialize_u128(self, _: u128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }

        fn serialize_i128(self, _: i128) -> Result<Self::Ok, Self::Error> {
            panic!("invalid short string serializer usage");
        }
    }

    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let mut bytes = self.code_page.encode(v).map_err(|e|
            e.map_or_else(|| Error::InvalidString(v.to_string()), |c| Error::UnrepresentableChar(c, self.code_page))
        )?;
        if bytes.last() == Some(&0) {
            return Err(Error::ShortStringTailZero.into());
        }
        if bytes.len() > self.size {
            return Err(Error::ShortStringNotFit { max_len: self.size, len: bytes.len() }.into());
        }
        bytes.resize(self.size, 0);
        self.writer.write_all(&bytes)?;
        Ok(())
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T)
        -> Result<Self::Ok, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        panic!("invalid short string serializer usage");
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        panic!("invalid short string serializer usage");
    }
}

#[cfg(test)]
mod tests {
    use crate::code::ser::*;
    use serde::{Serialize, Serializer};
    use std::collections::HashMap;

    #[derive(Serialize)]
    struct Abcd {
        a: i16,
        c: u32,
        d: String
    }

    #[test]
    fn vec_serialize_struct() {
        let s = Abcd { a: 5, c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut v)).unwrap();
        assert_eq!(v, [5, 0, 90, 0, 0, 0, 83]);
    }

    #[test]
    fn vec_serialize_struct_not_isolated() {
        let s = Abcd { a: 5, c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(EslSerializer::new(false, CodePage::Russian, &mut v)).unwrap();
        assert_eq!(v, [5, 0, 90, 0, 0, 0, 1, 0, 0, 0, 83]);
    }

    #[derive(Hash, Eq, PartialEq)]
    enum Variant { Variant1, Variant2 }

    impl Serialize for Variant {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
            serializer.serialize_u8(match self {
                Variant::Variant1 => 1,
                Variant::Variant2 => 2
            })
        }
    }

    #[derive(Serialize, Hash, Eq, PartialEq)]
    struct Key {
        variant: Variant,
        s: String
    }

    #[derive(Serialize)]
    struct Map {
        map: HashMap<Key, String>,
        unit: (),
        i: i8
    }

    #[test]
    fn vec_serialize_map() {
        let mut s = Map {
            map: HashMap::new(),
            unit: (),
            i: -3
        };
        s.map.insert(Key { variant: Variant::Variant2, s: "str".into() }, "value".into());
        let mut v = Vec::new();
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut v)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            5, 0, 0, 0, 118, 97, 108, 117, 101,
            253
        ]);
    }

    #[test]
    fn vec_serialize_tuple_key() {
        let mut s: HashMap<(Key, Key), u64> = HashMap::new();
        s.insert((
            Key { variant: Variant::Variant2, s: "str".into() },
            Key { variant: Variant::Variant1, s: "стр".into() }
        ), 22);
        let mut v = Vec::new();
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut v)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            8, 0, 0, 0,
            1, 3, 0, 0, 0, 241, 242, 240,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }

    #[derive(Serialize, Hash, Eq, PartialEq)]
    struct Key2((Key, Key));

    #[test]
    fn vec_serialize_newtype_key() {
        let mut s: HashMap<Key2, u64> = HashMap::new();
        s.insert(Key2((
            Key { variant: Variant::Variant2, s: "str".into() },
            Key { variant: Variant::Variant1, s: "стр".into() }
        )), 22);
        let mut v = Vec::new();
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut v)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            1, 3, 0, 0, 0, 241, 242, 240,
            8, 0, 0, 0,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }

    #[test]
    fn serialize_struct() {
        let s = Abcd { a: 5, c: 90, d: "S".into() };
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut w)).unwrap();
        assert_eq!(v, [5, 0, 90, 0, 0, 0, 83]);
    }

    #[test]
    fn serialize_struct_not_isolated() {
        let s = Abcd { a: 5, c: 90, d: "S".into() };
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        s.serialize(EslSerializer::new(false, CodePage::Russian, &mut w)).unwrap();
        assert_eq!(v, [5, 0, 90, 0, 0, 0, 1, 0, 0, 0, 83]);
    }

    #[test]
    fn serialize_map() {
        let mut s = Map {
            map: HashMap::new(),
            unit: (),
            i: -3
        };
        s.map.insert(Key { variant: Variant::Variant2, s: "str".into() }, "value".into());
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut w)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            5, 0, 0, 0, 118, 97, 108, 117, 101,
            253
        ]);
    }

    #[test]
    fn serialize_tuple_key() {
        let mut s: HashMap<(Key, Key), u64> = HashMap::new();
        s.insert((
            Key { variant: Variant::Variant2, s: "str".into() },
            Key { variant: Variant::Variant1, s: "стр".into() }
        ), 22);
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut w)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            8, 0, 0, 0,
            1, 3, 0, 0, 0, 241, 242, 240,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }

    #[test]
    fn serialize_newtype_key() {
        let mut s: HashMap<Key2, u64> = HashMap::new();
        s.insert(Key2((
            Key { variant: Variant::Variant2, s: "str".into() },
            Key { variant: Variant::Variant1, s: "стр".into() }
        )), 22);
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        s.serialize(EslSerializer::new(true, CodePage::Russian, &mut w)).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            1, 3, 0, 0, 0, 241, 242, 240,
            8, 0, 0, 0,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }

    #[derive(Debug, Eq, PartialEq, Hash)]
    struct String32(String);

    impl Serialize for String32 {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut serializer = serializer.serialize_tuple_variant("", SHORT_STRING_VARIANT_INDEX, "", 2)?;
            serializer.serialize_field(&[0u8; 32])?;
            serializer.serialize_field(&self.0)?;
            serializer.end()
        }
    }

    #[test]
    fn serialize_short_string() {
        let mut v = Vec::new();
        let mut w = GenericWriter { write_buf: None, writer: &mut v, pos: 0 };
        let s = EslSerializer::new(true, CodePage::Russian, &mut w);
        let mut d: HashMap<String32, ()> = HashMap::new();
        d.insert(String32("AbcdEfgh".into()), ());
        d.serialize(s).unwrap();
        assert_eq!(v, vec![
            65, 98, 99, 100, 69, 102, 103, 104,
            0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0
        ]);
    }
}
