use serde::{Serializer, Serialize};
use std::mem::{transmute, replace};
use std::fmt::{self, Display, Debug};
use encoding::{EncoderTrap};
use serde::ser::{self, SerializeSeq, SerializeTuple, SerializeTupleStruct, SerializeStruct, SerializeTupleVariant, SerializeStructVariant, SerializeMap};
use std::io::{self, Write};
use either::{Either, Left, Right};
use byteorder::{WriteBytesExt, LittleEndian};

use crate::code::code_page::*;

#[derive(Debug)]
pub enum SerError {
    Custom(String),
    LargeObject(usize),
    UnrepresentableChar(char, CodePage),
    ZeroSizedLastSequenceElement,
    VariantIndexMismatch { variant_index: u32, variant_size: u32 },
    ZeroSizedOptional,
}

impl Display for SerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerError::Custom(s) => Display::fmt(s, f),
            SerError::LargeObject(size) => write!(f, "object has too large size ({} B)", size),
            SerError::UnrepresentableChar(c, p) => write!(f, "the '{}' char is not representable in {:?} code page", c, p),
            SerError::ZeroSizedLastSequenceElement => write!(f, "last element in sequence or map cannot have zero size"),
            SerError::VariantIndexMismatch { variant_index, variant_size } =>
                write!(f, "variant index ({}) should be equal to variant size ({})", variant_index, variant_size),
            SerError::ZeroSizedOptional => write!(f, "optional element cannot have zero size"),
        }
    }
}

impl std::error::Error for SerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl ser::Error for SerError {
    fn custom<T: Display>(msg: T) -> Self { SerError::Custom(format!("{}", msg)) }
}

#[derive(Debug)]
pub enum SerOrIoError {
    Io(io::Error),
    Ser(SerError),
}

impl Display for SerOrIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerOrIoError::Io(e) => Display::fmt(e, f),
            SerOrIoError::Ser(e) => Display::fmt(e, f),
        }
    }
}

impl std::error::Error for SerOrIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SerOrIoError::Io(e) => Some(e),
            SerOrIoError::Ser(e) => Some(e),
        }
    }
}

impl ser::Error for SerOrIoError {
    fn custom<T: Display>(msg: T) -> Self { SerOrIoError::Ser(SerError::custom(msg)) }
}

impl From<SerError> for SerOrIoError {
    fn from(e: SerError) -> SerOrIoError { SerOrIoError::Ser(e) }
}

impl From<io::Error> for SerOrIoError {
    fn from(e: io::Error) -> SerOrIoError { SerOrIoError::Io(e) }
}

pub trait Writer: Write {
    type Buf: Debug;
    
    fn pos(&self) -> usize;
    fn begin_isolate(&mut self) -> Self::Buf;
    fn end_isolate(&mut self, buf: Self::Buf, variadic_part_pos: usize) -> Result<(), SerOrIoError>;
}

impl Writer for Vec<u8> {
    type Buf = usize;

    fn pos(&self) -> usize { self.len() }

    fn begin_isolate(&mut self) -> Self::Buf {
        let value_size_stub_offset = self.len();
        self.write_u32::<LittleEndian>(SIZE_STUB).unwrap();
        value_size_stub_offset
    }

    fn end_isolate(&mut self, value_size_stub_offset: usize, value_offset: usize) -> Result<(), SerOrIoError> {
        let value_size = size(self.len() - value_offset)?;
        write_u32(&mut self[value_size_stub_offset..value_size_stub_offset + 4], value_size);
        Ok(())
    }
}

pub struct GenericWriter<'a, W: Write + ?Sized> {
    writer: &'a mut W,
    write_buf: Option<Vec<u8>>,
    pos: usize,
}

impl<'a, W: Write + ?Sized> Write for GenericWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pos += buf.len();
        if let Some(write_buf) = &mut self.write_buf {
            write_buf.extend_from_slice(buf);
            Ok(buf.len())
        } else {
            self.writer.write(buf)
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

    fn begin_isolate(&mut self) -> Self::Buf {
         if let Some(write_buf) = &self.write_buf {
             let value_size_stub_offset = write_buf.len();
             self.write_u32::<LittleEndian>(SIZE_STUB).unwrap();
             Some(value_size_stub_offset)
         } else {
             self.pos += 4;
             self.write_buf = Some(Vec::new());
             None
         }
    }

    fn end_isolate(&mut self, value_size_stub_offset: Option<usize>, value_pos: usize) -> Result<(), SerOrIoError> {
        let value_size = size(self.pos - value_pos)?;
        if let Some(value_size_stub_offset) = value_size_stub_offset {
            let write_buf = self.write_buf.as_mut().unwrap();
            write_u32(&mut write_buf[value_size_stub_offset..value_size_stub_offset + 4], value_size);
        } else {
            let write_buf = self.write_buf.take().unwrap();
            self.writer.write_u32::<LittleEndian>(value_size).unwrap();
            self.writer.write_all(&write_buf[..])?;
        }
        Ok(())
    }
}
    
#[derive(Debug)]
struct VecEslSerializer<'a, W: Writer> {
    isolated: bool,
    code_page: CodePage,
    writer: &'a mut W
}

#[derive(Debug)]
struct VecBaseSeqSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    last_element_has_zero_size: bool,
    size: usize,
    buf: Option<(W::Buf, usize)>,
}

#[derive(Debug)]
struct VecSeqSerializer<'a, W: Writer>(VecBaseSeqSerializer<'a, W>);

#[derive(Debug)]
struct VecStructSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    len: Option<usize>,
    size: usize
}

#[derive(Debug)]
struct VecKeySeqSerializer<'a, W: Writer>(VecBaseSeqSerializer<'a, W>);

#[derive(Debug)]
struct VecBaseMapSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    value_buf: Option<(W::Buf, usize)>,
    size: usize
}

#[derive(Debug)]
struct VecMapSerializer<'a, W: Writer>(VecBaseMapSerializer<'a, W>);

#[derive(Debug)]
struct VecKeyMapSerializer<'a, W: Writer>(VecBaseMapSerializer<'a, W>);

#[derive(Debug)]
struct VecKeyStructSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    size: usize
}

#[derive(Debug)]
struct VecKeyTupleSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    value_buf: Option<W::Buf>,
    size: usize
}

#[derive(Debug)]
struct VecBaseStructVariantSerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
    variant_index: u32,
    size: usize
}

#[derive(Debug)]
struct VecStructVariantSerializer<'a, W: Writer> {
    base: VecBaseStructVariantSerializer<'a, W>,
    len: Option<usize>,
}

#[derive(Debug)]
struct VecKeyStructVariantSerializer<'a, W: Writer>(VecBaseStructVariantSerializer<'a, W>);

#[derive(Debug)]
struct VecKeySerializer<'a, W: Writer> {
    code_page: CodePage,
    writer: &'a mut W,
}

fn write_u32(writer: &mut [u8], v: u32) {
    writer[0] = (v & 0xFF) as u8;
    writer[1] = ((v >> 8) & 0xFF) as u8;
    writer[2] = ((v >> 16) & 0xFF) as u8;
    writer[3] = (v >> 24) as u8;
}

fn size(len: usize) -> Result<u32, SerError> {
    if len > u32::max_value() as usize {
        Err(SerError::LargeObject(len))
    } else {
        Ok(len as u32)
    }
}

impl<'a, W: Writer> VecBaseSeqSerializer<'a, W> {
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), SerOrIoError> {
        let size = v.serialize(VecEslSerializer {
            isolated: false,
            writer: self.writer, code_page: self.code_page,
        })?;
        self.last_element_has_zero_size = size == 0;
        self.size += size;
        Ok(())
    }

    fn end(self) -> Result<usize, SerOrIoError> {
        if self.last_element_has_zero_size {
            return Err(SerError::ZeroSizedLastSequenceElement.into());
        }
        if let Some((buf, start_pos)) = self.buf {
            self.writer.end_isolate(buf, start_pos)?;
            Ok(4 + self.size)
        } else {
            Ok(self.size)
        }
    }
}

impl<'a, W: Writer> SerializeSeq for VecSeqSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.0.end()
    }
}

impl<'a, W: Writer> SerializeSeq for VecKeySeqSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_element(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.end()?))
    }
}

const SIZE_STUB: u32 = 0x1375F17B;

impl<'a, W: Writer> VecBaseMapSerializer<'a, W> {
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SerOrIoError> {
        let (value_buf, size) = key.serialize(VecKeySerializer {
            writer: self.writer,
            code_page: self.code_page
        })?;
        self.size += size;
        let value_buf = replace(&mut self.value_buf, Some(if let Some(value_buf) = value_buf {
            value_buf
        } else {
            self.size += 4;
            (self.writer.begin_isolate(), self.writer.pos())
        }));
        assert!(value_buf.is_none());
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), SerOrIoError> {
        let value_size = v.serialize(VecEslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        let (value_buf, value_pos) = self.value_buf.take().unwrap();
        self.writer.end_isolate(value_buf, value_pos)?;
        self.size += value_size;
        Ok(())
    }
}

impl<'a, W: Writer> SerializeMap for VecMapSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.0.serialize_key(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_value(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.0.size)
    }
}

impl<'a, W: Writer> SerializeMap for VecKeyMapSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.0.serialize_key(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_value(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.size))
    }
}

fn vec_serialize_field<T: Serialize + ?Sized>(writer: &mut impl Writer, code_page: CodePage, len: &mut Option<usize>, v: &T)
    -> Result<usize, SerOrIoError> {

    len.as_mut().map(|len| {
        if *len == 0 { panic!() }
        *len -= 1;
    });
    v.serialize(VecEslSerializer {
        isolated: len.map_or(false, |len| len == 0),
        writer, code_page
    })
}

fn vec_serialize_key_field<T: Serialize + ?Sized>(writer: &mut impl Writer, code_page: CodePage, v: &T)
    -> Result<usize, SerOrIoError> {

    v.serialize(VecEslSerializer {
        isolated: false,
        writer, code_page
    })
}

impl<'a, W: Writer> SerializeTuple for VecStructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += vec_serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Writer> SerializeTupleStruct for VecStructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += vec_serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Writer> SerializeStruct for VecStructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.size += vec_serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Writer> SerializeTuple for VecKeyTupleSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += if self.value_buf.is_some() {
            vec_serialize_key_field(self.writer, self.code_page, v)?
        } else {
            let key_size = vec_serialize_key_field(self.writer, self.code_page, v)?;
            self.value_buf = Some(self.writer.begin_isolate());
            key_size + 4
        };
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let size = self.size;
        let writer = self.writer;
        Ok((self.value_buf.map(|x| (x, writer.pos())), size))
    }
}

impl<'a, W: Writer> SerializeStruct for VecKeyStructSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.size += vec_serialize_key_field(self.writer, self.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.size))
    }
}

impl<'a, W: Writer> SerializeTupleStruct for VecKeyStructSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += vec_serialize_key_field(self.writer, self.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.size))
    }
}

impl<'a, W: Writer> VecBaseStructVariantSerializer<'a, W> {
    fn end(self) -> Result<usize, SerOrIoError> {
        let variant_size = size(self.size)?;
        if self.variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index: self.variant_index, variant_size }.into());
        }
        Ok(self.size)
    }
}

impl<'a, W: Writer> SerializeTupleVariant for VecStructVariantSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.base.size += vec_serialize_field(self.base.writer, self.base.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.base.end()
    }
}

impl<'a, W: Writer> SerializeStructVariant for VecStructVariantSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.base.size += vec_serialize_field(self.base.writer, self.base.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.base.end()
    }
}

impl<'a, W: Writer> SerializeTupleVariant for VecKeyStructVariantSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.size += vec_serialize_key_field(self.0.writer, self.0.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.end()?))
    }
}

impl<'a, W: Writer> SerializeStructVariant for VecKeyStructVariantSerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.0.size += vec_serialize_key_field(self.0.writer, self.0.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.end()?))
    }
}

fn bool_byte(v: bool) -> u8 {
    if v { 1 } else { 0 }
}

fn char_byte(code_page: CodePage, v: char) -> Result<u8, SerError> {
    let v = code_page.encoding()
        .encode(&v.to_string(), EncoderTrap::Strict)
        .map_err(|_| SerError::UnrepresentableChar(v, code_page))?;
    debug_assert_eq!(v.len(), 1);
    Ok(v[0])
}

fn str_bytes(code_page: CodePage, v: &str) -> Result<Vec<u8>, SerError> {
    code_page.encoding()
        .encode(v, EncoderTrap::Strict)
        .map_err(|s| SerError::UnrepresentableChar(s.chars().nth(0).unwrap(), code_page))
}

impl<'a, W: Writer> Serializer for VecEslSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;
    type SerializeSeq = VecSeqSerializer<'a, W>;
    type SerializeTuple = VecStructSerializer<'a, W>;
    type SerializeTupleStruct = VecStructSerializer<'a, W>;
    type SerializeTupleVariant = VecStructVariantSerializer<'a, W>;
    type SerializeStruct = VecStructSerializer<'a, W>;
    type SerializeStructVariant = VecStructVariantSerializer<'a, W>;
    type SerializeMap = VecMapSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(bool_byte(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(size(v.len())?)?;
        }
        self.writer.write(v)?;
        Ok(if self.isolated { 0 } else { 4 } + v.len())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u8(v)?;
        Ok(1)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(unsafe { transmute(v) })
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u16::<LittleEndian>(v)?;
        Ok(2)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u16(unsafe { transmute(v) })
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u32::<LittleEndian>(v)?;
        Ok(4)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u64::<LittleEndian>(v)?;
        Ok(8)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    serde_if_integer128! {
        fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
            self.writer.write_u128::<LittleEndian>(v)?;
            Ok(16)
        }

        fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
            self.serialize_u128(unsafe { transmute(v) })
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let byte = char_byte(self.code_page, v)?;
        self.serialize_u8(byte)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let bytes = str_bytes(self.code_page, v)?;
        self.serialize_bytes(&bytes)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(0)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(0)
    }

    fn serialize_unit_variant(self, _: &'static str, variant_index: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        if variant_index != 0 {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size: 0 }.into());
        }
        if !self.isolated {
            self.serialize_u32(0)
        } else {
            Ok(0)
        }
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        v.serialize(self)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(0)?;
        }
        Ok(if self.isolated { 0 } else { 4 })
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let buf = if !self.isolated {
            Some((self.writer.begin_isolate(), self.writer.pos()))
        } else {
            None
        };
        let value_size = value.serialize(VecEslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        if value_size == 0 {
            return Err(SerError::ZeroSizedOptional.into());
        }
        buf.map_or(Ok(()), |(buf, pos)| self.writer.end_isolate(buf, pos))?;
        Ok(if self.isolated { 0 } else { 4 } + value_size)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, variant_index: u32, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(variant_index)?;
        }
        let v_size = v.serialize(VecEslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        let variant_size = size(v_size)?;
        if variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size }.into());
        }
        Ok(if self.isolated { 0 } else { 4 } + v_size)
    }

    fn serialize_tuple_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(variant_index)?;
        }
        Ok(VecStructVariantSerializer {
            len: if self.isolated { Some(len) } else { None },
            base: VecBaseStructVariantSerializer {
                variant_index,
                writer: self.writer, code_page: self.code_page,
                size: 0
            }
        })
    }

    fn serialize_struct_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        if !self.isolated {
            self.writer.write_u32::<LittleEndian>(variant_index)?;
        }
        Ok(VecStructVariantSerializer {
            len: if self.isolated { Some(len) } else { None },
            base: VecBaseStructVariantSerializer {
                variant_index,
                writer: self.writer, code_page: self.code_page,
                size: 0
            }
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(VecStructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(VecStructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(VecStructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let buf = if !self.isolated { Some((self.writer.begin_isolate(), self.writer.pos())) } else { None };
        Ok(VecSeqSerializer(VecBaseSeqSerializer {
            writer: self.writer,
            code_page: self.code_page,
            last_element_has_zero_size: false,
            size: 0,
            buf
        }))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(VecMapSerializer(VecBaseMapSerializer {
            value_buf: None,
            writer: self.writer, code_page: self.code_page,
            size: 0
        }))
    }
}

impl<'a, W: Writer> Serializer for VecKeySerializer<'a, W> {
    type Ok = (Option<(W::Buf, usize)>, usize);
    type Error = SerOrIoError;
    type SerializeSeq = VecKeySeqSerializer<'a, W>;
    type SerializeTuple = VecKeyTupleSerializer<'a, W>;
    type SerializeTupleStruct = VecKeyStructSerializer<'a, W>;
    type SerializeTupleVariant = VecKeyStructVariantSerializer<'a, W>;
    type SerializeStruct = VecKeyStructSerializer<'a, W>;
    type SerializeStructVariant = VecKeyStructVariantSerializer<'a, W>;
    type SerializeMap = VecKeyMapSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(bool_byte(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u32::<LittleEndian>(size(v.len())?)?;
        self.writer.write(v)?;
        Ok((None, 4 + v.len()))
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u8(v)?;
        Ok((None, 1))
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(unsafe { transmute(v) })
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u16::<LittleEndian>(v)?;
        Ok((None, 2))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u16(unsafe { transmute(v) })
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u32::<LittleEndian>(v)?;
        Ok((None, 4))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.writer.write_u64::<LittleEndian>(v)?;
        Ok((None, 8))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    serde_if_integer128! {
        fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
            self.writer.write_u128::<LittleEndian>(v)?;
            Ok((None, 16))
        }

        fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
            self.serialize_u128(unsafe { transmute(v) })
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let byte = char_byte(self.code_page, v)?;
        self.serialize_u8(byte)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let bytes = str_bytes(self.code_page, v)?;
        self.serialize_bytes(&bytes)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, 0))
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok((None, 0))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        let size = v.serialize(VecEslSerializer {
            isolated: false,
            writer: self.writer, code_page: self.code_page
        })?;
        Ok((None, size))
    }

    fn serialize_unit_variant(self, _: &'static str, variant_index: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        if variant_index != 0 {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size: 0 }.into());
        }
        self.serialize_u32(0)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(0)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let (value_buf, value_pos) = (self.writer.begin_isolate(), self.writer.pos());
        let value_size = value.serialize(VecEslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        if value_size == 0 {
            return Err(SerError::ZeroSizedOptional.into());
        }
        self.writer.end_isolate(value_buf, value_pos)?;
        Ok((None, 4 + value_size))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, variant_index: u32, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        self.writer.write_u32::<LittleEndian>(variant_index)?;
        let v_size = v.serialize(VecEslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        let variant_size = size(v_size)?;
        if variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size }.into());
        }
        Ok((None, 4 + v_size))
    }

    fn serialize_tuple_variant(self, _: &'static str, variant_index: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        self.writer.write_u32::<LittleEndian>(variant_index)?;
        Ok(VecKeyStructVariantSerializer(VecBaseStructVariantSerializer { writer: self.writer, code_page: self.code_page, variant_index, size: 0}))
    }

    fn serialize_struct_variant(self, _: &'static str, variant_index: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        self.writer.write_u32::<LittleEndian>(variant_index)?;
        Ok(VecKeyStructVariantSerializer(VecBaseStructVariantSerializer { writer: self.writer, code_page: self.code_page, variant_index, size: 0}))
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(VecKeyTupleSerializer {
            writer: self.writer, code_page: self.code_page,
            value_buf: None,
            size: 0
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(VecKeyStructSerializer {
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(VecKeyStructSerializer {
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let buf = (self.writer.begin_isolate(), self.writer.pos());
        Ok(VecKeySeqSerializer(VecBaseSeqSerializer {
            last_element_has_zero_size: false,
            writer: self.writer, code_page: self.code_page,
            size: 0,
            buf: Some(buf)
        }))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(VecKeyMapSerializer(VecBaseMapSerializer {
            value_buf: None,
            writer: self.writer, code_page: self.code_page,
            size: 0
        }))
    }
}

#[derive(Debug)]
struct EslSerializer<'a, W: Write + ?Sized> {
    isolated: bool,
    code_page: CodePage,
    writer: &'a mut W
}

#[derive(Debug)]
struct SeqSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    last_element_has_zero_size: bool,
    buf: Either<usize, Vec<u8>>,
}

#[derive(Debug)]
struct StructSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    len: Option<usize>,
    size: usize
}

#[derive(Debug)]
struct KeySeqSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    last_element_has_zero_size: bool,
    buf: Vec<u8>,
}

#[derive(Debug)]
struct BaseMapSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    key_tail: Option<Vec<u8>>,
    size: usize
}

#[derive(Debug)]
struct MapSerializer<'a, W: Write + ?Sized>(BaseMapSerializer<'a, W>);

#[derive(Debug)]
struct KeyMapSerializer<'a, W: Write + ?Sized>(BaseMapSerializer<'a, W>);

#[derive(Debug)]
struct KeyStructSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    size: usize
}

#[derive(Debug)]
struct KeyTupleSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    tail: Option<Vec<u8>>,
    size: usize
}

#[derive(Debug)]
struct BaseStructVariantSerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
    variant_index: u32,
    size: usize
}

#[derive(Debug)]
struct StructVariantSerializer<'a, W: Write + ?Sized> {
    base: BaseStructVariantSerializer<'a, W>,
    len: Option<usize>,
}

#[derive(Debug)]
struct KeyStructVariantSerializer<'a, W: Write + ?Sized>(BaseStructVariantSerializer<'a, W>);

#[derive(Debug)]
struct KeySerializer<'a, W: Write + ?Sized> {
    code_page: CodePage,
    writer: &'a mut W,
}

fn serialize_u8(writer: &mut (impl Write + ?Sized), v: u8) -> Result<(), SerOrIoError> {
    writer.write_all(&[v]).map_err(SerOrIoError::Io)
}

fn serialize_u32(writer: &mut (impl Write + ?Sized), v: u32) -> Result<(), SerOrIoError> {
    writer.write_all(&[
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
        (v >> 24) as u8
    ]).map_err(SerOrIoError::Io)
}

fn serialize_bytes(writer: &mut (impl Write + ?Sized), v: &[u8]) -> Result<(), SerOrIoError> {
    writer.write_all(v).map_err(SerOrIoError::Io)
}

impl<'a, W: Write + ?Sized> SerializeSeq for SeqSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;
    
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        let size = match self.buf.as_mut() {
            Left(self_size) => {
                let size = v.serialize(EslSerializer {
                    isolated: false,
                    writer: self.writer, code_page: self.code_page,
                })?;
                *self_size += size;
                size
            },
            Right(buf) => {
                v.serialize(VecEslSerializer {
                    isolated: false,
                    writer: buf,
                    code_page: self.code_page
                })?
            }
        };
        self.last_element_has_zero_size = size == 0;
        Ok(())
    }
    
    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.last_element_has_zero_size {
            return Err(SerError::ZeroSizedLastSequenceElement.into());
        }
        match self.buf.as_ref() {
            Left(&size) => Ok(size),
            Right(buf) => {
                serialize_u32(self.writer, size(buf.len())?)?;
                serialize_bytes(self.writer, &buf)?;
                Ok(4 + buf.len())
            }
        }
    }
}

impl<'a, W: Write + ?Sized> SerializeSeq for KeySeqSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.last_element_has_zero_size = v.serialize(VecEslSerializer {
            isolated: false,
            writer: &mut self.buf,
            code_page: self.code_page
        })? == 0;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.last_element_has_zero_size {
            return Err(SerError::ZeroSizedLastSequenceElement.into());
        }
        serialize_u32(self.writer, size(self.buf.len())?)?;
        serialize_bytes(self.writer, &self.buf)?;
        Ok((None, 4 + self.buf.len()))
    }
}

impl<'a, W: Write + ?Sized> BaseMapSerializer<'a, W> {
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SerOrIoError> {
        let (buf, size) = key.serialize(KeySerializer { 
            writer: self.writer,
            code_page: self.code_page
        })?;
        self.size += size;
        self.key_tail = buf;
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), SerOrIoError> {
        let mut value = Vec::new();
        v.serialize(VecEslSerializer {
            isolated: true,
            writer: &mut value,
            code_page: self.code_page
        })?;
        serialize_u32(self.writer, size(value.len())?)?;
        if let Some(key_tail) = &self.key_tail {
            serialize_bytes(self.writer, key_tail)?;
        }
        serialize_bytes(self.writer, &value)?;
        self.size += 4 + value.len();
        Ok(())
    }
}

impl<'a, W: Write + ?Sized> SerializeMap for MapSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.0.serialize_key(key)
    }
    
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_value(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.0.size)
    }
}

impl<'a, W: Write + ?Sized> SerializeMap for KeyMapSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.0.serialize_key(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.serialize_value(v)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.size))
    }
}

fn serialize_field<T: Serialize + ?Sized>(writer: &mut (impl Write + ?Sized), code_page: CodePage, len: &mut Option<usize>, v: &T)
    -> Result<usize, SerOrIoError> {
    
    len.as_mut().map(|len| {
        if *len == 0 { panic!() }
        *len -= 1;
    });
    v.serialize(EslSerializer {
        isolated: len.map_or(false, |len| len == 0),
        writer, code_page
    })
}

fn serialize_key_field<T: Serialize + ?Sized>(writer: &mut (impl Write + ?Sized), code_page: CodePage, v: &T)
    -> Result<usize, SerOrIoError> {

    v.serialize(EslSerializer {
        isolated: false,
        writer, code_page
    })
}

impl<'a, W: Write + ?Sized> SerializeTuple for StructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Write + ?Sized> SerializeTupleStruct for StructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Write + ?Sized> SerializeStruct for StructSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.size += serialize_field(self.writer, self.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.size)
    }
}

impl<'a, W: Write + ?Sized> SerializeTuple for KeyTupleSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += if let Some(tail) = self.tail.as_mut() {
            vec_serialize_key_field(tail, self.code_page, v)?
        } else {
            self.tail = Some(Vec::new());
            serialize_key_field(self.writer, self.code_page, v)?
        };
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((self.tail, self.size))
    }
}

impl<'a, W: Write + ?Sized> SerializeStruct for KeyStructSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.size += serialize_key_field(self.writer, self.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.size))
    }
}

impl<'a, W: Write + ?Sized> SerializeTupleStruct for KeyStructSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.size += serialize_key_field(self.writer, self.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.size))
    }
}

impl<'a, W: Write + ?Sized> BaseStructVariantSerializer<'a, W> {
    fn end(self) -> Result<usize, SerOrIoError> {
        let variant_size = size(self.size)?;
        if self.variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index: self.variant_index, variant_size }.into());
        }
        Ok(self.size)
    }
}

impl<'a, W: Write + ?Sized> SerializeTupleVariant for StructVariantSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.base.size += serialize_field(self.base.writer, self.base.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.base.end()
    }
}

impl<'a, W: Write + ?Sized> SerializeStructVariant for StructVariantSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.base.size += serialize_field(self.base.writer, self.base.code_page, &mut self.len, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.base.end()
    }
}

impl<'a, W: Write + ?Sized> SerializeTupleVariant for KeyStructVariantSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.0.size += serialize_key_field(self.0.writer, self.0.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.end()?))
    }
}

impl<'a, W: Write + ?Sized> SerializeStructVariant for KeyStructVariantSerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Result<(), Self::Error> {
        self.0.size += serialize_key_field(self.0.writer, self.0.code_page, v)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, self.0.end()?))
    }
}

fn serialize_u16(writer: &mut (impl Write + ?Sized), v: u16) -> Result<(), SerOrIoError> {
    writer.write_all(&[(v & 0xFF) as u8, (v >> 8) as u8]).map_err(SerOrIoError::Io)
}

fn serialize_u64(writer: &mut (impl Write + ?Sized), v: u64) -> Result<(), SerOrIoError> {
    writer.write_all(&[
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
        ((v >> 24) & 0xFF) as u8,
        ((v >> 32) & 0xFF) as u8,
        ((v >> 40) & 0xFF) as u8,
        ((v >> 48) & 0xFF) as u8,
        (v >> 56) as u8
    ]).map_err(SerOrIoError::Io)
}

serde_if_integer128! {
    fn serialize_u128(writer: &mut (impl Write + ?Sized), v: u128) -> Result<(), SerOrIoError> {
        writer.write_all(&[
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 24) & 0xFF) as u8,
            ((v >> 32) & 0xFF) as u8,
            ((v >> 40) & 0xFF) as u8,
            ((v >> 48) & 0xFF) as u8,
            ((v >> 56) & 0xFF) as u8,
            ((v >> 64) & 0xFF) as u8,
            ((v >> 72) & 0xFF) as u8,
            ((v >> 80) & 0xFF) as u8,
            ((v >> 88) & 0xFF) as u8,
            ((v >> 96) & 0xFF) as u8,
            ((v >> 104) & 0xFF) as u8,
            ((v >> 112) & 0xFF) as u8,
            (v >> 120) as u8
        ]).map_err(SerOrIoError::Io)
    }
}

impl<'a, W: Write + ?Sized> Serializer for EslSerializer<'a, W> {
    type Ok = usize;
    type Error = SerOrIoError;
    type SerializeSeq = SeqSerializer<'a, W>;
    type SerializeTuple = StructSerializer<'a, W>;
    type SerializeTupleStruct = StructSerializer<'a, W>;
    type SerializeTupleVariant = StructVariantSerializer<'a, W>;
    type SerializeStruct = StructSerializer<'a, W>;
    type SerializeStructVariant = StructVariantSerializer<'a, W>;
    type SerializeMap = MapSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(bool_byte(v))
    }
    
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            serialize_u32(self.writer, size(v.len())?)?;
        }
        serialize_bytes(self.writer, v)?;
        Ok(if self.isolated { 0 } else { 4 } + v.len())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        serialize_u8(self.writer, v)?;
        Ok(1)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(unsafe { transmute(v) })
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        serialize_u16(self.writer, v)?;
        Ok(2)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u16(unsafe { transmute(v) })
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        serialize_u32(self.writer, v)?;
        Ok(4)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        serialize_u64(self.writer, v)?;
        Ok(8)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    serde_if_integer128! {
        fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
            serialize_u128(self.writer, v)?;
            Ok(16)
        }

        fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
            self.serialize_u128(unsafe { transmute(v) })
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let byte = char_byte(self.code_page, v)?;
        self.serialize_u8(byte)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let bytes = str_bytes(self.code_page, v)?;
        self.serialize_bytes(&bytes)
    }
    
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(0)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(0)
    }

    fn serialize_unit_variant(self, _: &'static str, variant_index: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        if variant_index != 0 { 
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size: 0 }.into());
        }
        if !self.isolated {
            self.serialize_u32(0)
        } else {
            Ok(0)
        }
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        v.serialize(self)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        if !self.isolated {
            serialize_u32(self.writer, 0)?;
        }
        Ok(if self.isolated { 0 } else { 4 })
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let mut bytes = Vec::new(); 
        if value.serialize(VecEslSerializer {
            isolated: true,
            writer: &mut bytes,
            code_page: self.code_page
        })? == 0 {
            return Err(SerError::ZeroSizedOptional.into());
        }
        self.serialize_bytes(&bytes)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, variant_index: u32, _: &'static str, v: &T) 
        -> Result<Self::Ok, Self::Error> {

        if !self.isolated {
            serialize_u32(self.writer, variant_index)?;
        }
        let v_size = v.serialize(EslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        let variant_size = size(v_size)?;
        if variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size }.into());
        }
        Ok(if self.isolated { 0 } else { 4 } + v_size)
    }

    fn serialize_tuple_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize) 
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        if !self.isolated {
            serialize_u32(self.writer, variant_index)?;
        }
        Ok(StructVariantSerializer {
            len: if self.isolated { Some(len) } else { None },
            base: BaseStructVariantSerializer {
                variant_index,
                writer: self.writer, code_page: self.code_page,
                size: 0
            }
        })
    }

    fn serialize_struct_variant(self, _: &'static str, variant_index: u32, _: &'static str, len: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        if !self.isolated {
            serialize_u32(self.writer, variant_index)?;
        }
        Ok(StructVariantSerializer {
            len: if self.isolated { Some(len) } else { None },
            base: BaseStructVariantSerializer {
                variant_index,
                writer: self.writer, code_page: self.code_page,
                size: 0
            }
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(StructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(StructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(StructSerializer {
            len: if self.isolated { Some(len) } else { None },
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(SeqSerializer { 
            last_element_has_zero_size: false,
            buf: if self.isolated { Left(0) } else { Right(Vec::new()) },
            writer: self.writer, code_page: self.code_page,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(MapSerializer(BaseMapSerializer {
            key_tail: None,
            writer: self.writer, code_page: self.code_page,
            size: 0
        }))
    }
}

impl<'a, W: Write + ?Sized> Serializer for KeySerializer<'a, W> {
    type Ok = (Option<Vec<u8>>, usize);
    type Error = SerOrIoError;
    type SerializeSeq = KeySeqSerializer<'a, W>;
    type SerializeTuple = KeyTupleSerializer<'a, W>;
    type SerializeTupleStruct = KeyStructSerializer<'a, W>;
    type SerializeTupleVariant = KeyStructVariantSerializer<'a, W>;
    type SerializeStruct = KeyStructSerializer<'a, W>;
    type SerializeStructVariant = KeyStructVariantSerializer<'a, W>;
    type SerializeMap = KeyMapSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(bool_byte(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        serialize_u32(self.writer, size(v.len())?)?;
        serialize_bytes(self.writer, v)?;
        Ok((None, 4 + v.len()))
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        serialize_u8(self.writer, v)?;
        Ok((None, 1))
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u8(unsafe { transmute(v) })
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        serialize_u16(self.writer, v)?;
        Ok((None, 2))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u16(unsafe { transmute(v) })
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        serialize_u32(self.writer, v)?;
        Ok((None, 4))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(unsafe { transmute(v) })
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        serialize_u64(self.writer, v)?;
        Ok((None, 8))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(unsafe { transmute(v) })
    }

    serde_if_integer128! {
        fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
            serialize_u128(self.writer, v)?;
            Ok((None, 16))
        }

        fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
            self.serialize_u128(unsafe { transmute(v) })
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let byte = char_byte(self.code_page, v)?;
        self.serialize_u8(byte)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        let bytes = str_bytes(self.code_page, v)?;
        self.serialize_bytes(&bytes)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok((None, 0))
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok((None, 0))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        let size = v.serialize(EslSerializer {
            isolated: false,
            writer: self.writer, code_page: self.code_page
        })?;
        Ok((None, size))
    }

    fn serialize_unit_variant(self, _: &'static str, variant_index: u32, _: &'static str)
        -> Result<Self::Ok, Self::Error> {

        if variant_index != 0 {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size: 0 }.into());
        }
        self.serialize_u32(0)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_u32(0)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let mut bytes = Vec::new();
        if value.serialize(VecEslSerializer {
            isolated: true,
            writer: &mut bytes,
            code_page: self.code_page
        })? == 0 {
            return Err(SerError::ZeroSizedOptional.into());
        }
        self.serialize_bytes(&bytes)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, variant_index: u32, _: &'static str, v: &T)
        -> Result<Self::Ok, Self::Error> {

        serialize_u32(self.writer, variant_index)?;
        let v_size = v.serialize(EslSerializer {
            isolated: true,
            writer: self.writer,
            code_page: self.code_page
        })?;
        let variant_size = size(v_size)?;
        if variant_index != variant_size {
            return Err(SerError::VariantIndexMismatch { variant_index, variant_size }.into());
        }
        Ok((None, 4 + v_size))
    }

    fn serialize_tuple_variant(self, _: &'static str, variant_index: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeTupleVariant, Self::Error> {

        serialize_u32(self.writer, variant_index)?;
        Ok(KeyStructVariantSerializer(BaseStructVariantSerializer { writer: self.writer, code_page: self.code_page, variant_index, size: 0}))
    }

    fn serialize_struct_variant(self, _: &'static str, variant_index: u32, _: &'static str, _: usize)
        -> Result<Self::SerializeStructVariant, Self::Error> {

        serialize_u32(self.writer, variant_index)?;
        Ok(KeyStructVariantSerializer(BaseStructVariantSerializer { writer: self.writer, code_page: self.code_page, variant_index, size: 0}))
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(KeyTupleSerializer {
            writer: self.writer, code_page: self.code_page,
            tail: None,
            size: 0
        })
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(KeyStructSerializer {
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(KeyStructSerializer {
            writer: self.writer, code_page: self.code_page,
            size: 0
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(KeySeqSerializer {
            last_element_has_zero_size: false,
            buf: Vec::new(),
            writer: self.writer, code_page: self.code_page,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(KeyMapSerializer(BaseMapSerializer {
            key_tail: None,
            writer: self.writer, code_page: self.code_page,
            size: 0
        }))
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
        b: char,
        c: u32,
        d: String
    }

    #[test]
    fn vec_serialize_struct() {
        let s = Abcd { a: 5, b: 'Ы', c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(VecEslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, [5, 0, 219, 90, 0, 0, 0, 83]);
    }

    #[test]
    fn vec_serialize_struct_not_isolated() {
        let s = Abcd { a: 5, b: 'Ы', c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(VecEslSerializer {
            isolated: false,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, [5, 0, 219, 90, 0, 0, 0, 1, 0, 0, 0, 83]);
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
        s.serialize(VecEslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
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
        s.serialize(VecEslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
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
        s.serialize(VecEslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            1, 3, 0, 0, 0, 241, 242, 240,
            8, 0, 0, 0,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }

    #[test]
    fn serialize_struct() {
        let s = Abcd { a: 5, b: 'Ы', c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(EslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, [5, 0, 219, 90, 0, 0, 0, 83]);
    }

    #[test]
    fn serialize_struct_not_isolated() {
        let s = Abcd { a: 5, b: 'Ы', c: 90, d: "S".into() };
        let mut v = Vec::new();
        s.serialize(EslSerializer {
            isolated: false,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, [5, 0, 219, 90, 0, 0, 0, 1, 0, 0, 0, 83]);
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
        s.serialize(EslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
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
        s.serialize(EslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
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
        s.serialize(EslSerializer {
            isolated: true,
            code_page: CodePage::Russian,
            writer: &mut v
        }).unwrap();
        assert_eq!(v, vec![
            2, 3, 0, 0, 0, 115, 116, 114,
            1, 3, 0, 0, 0, 241, 242, 240,
            8, 0, 0, 0,
            22, 0, 0, 0, 0, 0, 0, 0
        ]);
    }
}
