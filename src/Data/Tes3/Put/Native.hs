module Data.Tes3.Put.Native where

#include <haskell>
import Data.Tes3

sign :: T3Sign -> ByteString
sign = runPut . putWord32le . t3SignValue

size :: ByteString -> ByteString
size = runPut . putWord32le . fromIntegral . B.length

flags :: T3Flags -> ByteString
flags = runPut . putWord64le . t3FlagsValue

tail :: ByteString -> Word32 -> ByteString
tail b n = runPut (foldl (>>) (return ()) $ replicate (fromIntegral $ n - fromIntegral (B.length b)) (putWord8 0))

w8 :: Word8 -> ByteString
w8 = runPut . putWord8

w32 :: Word32 -> ByteString
w32 = runPut . putWord32le

f32 :: Float -> ByteString
f32 v = runPut (putWord32le $ if isNaN v then 0xFFFFFFFF else floatToWord v)

i32 :: Int32 -> ByteString
i32 = BB.toLazyByteString . BB.int32LE

putT3Field :: T3Sign -> T3Field -> ByteString
putT3Field _ (T3BinaryField s b) = sign s <> size b <> b
putT3Field record_sign (T3StringField s t) =
  let b = t3StringValue t in
  case t3FieldType record_sign s of
    T3FixedString n -> sign s <> w32 n <> b <> tail b n
    T3String _ -> sign s <> size b <> b
    _ -> error "putT3Field T3StringField"
putT3Field record_sign (T3MultilineField s t) =
  let
    delimiter = case t3FieldType record_sign s of
      T3Multiline use_unix_newlines _ -> if use_unix_newlines then "\n" else "\r\n"
      _ -> error "putT3Field T3MultilineField"
    in
  let b = t3StringValue $ T.intercalate delimiter t in
  sign s <> size b <> b
putT3Field _ (T3MultiStringField s t) =
  let b = t3StringValue $ T.intercalate "\0" t in
  sign s <> size b <> b
putT3Field _ (T3RefField s n t) =
  let b = t3StringValue t in
  sign s <> w32 36 <> i32 n <> b <> tail b 32
putT3Field _ (T3FloatField s v) = sign s <> w32 4 <> either w32 f32 v
putT3Field _ (T3IntField s v) = sign s <> w32 4 <> i32 v
putT3Field _ (T3ShortField s v) = sign s <> w32 2 <> BB.toLazyByteString (BB.int16LE v)
putT3Field _ (T3LongField s v) = sign s <> w32 8 <> BB.toLazyByteString (BB.int64LE v)
putT3Field _ (T3ByteField s v) = sign s <> w32 1 <> runPut (putWord8 v)
putT3Field _ (T3CompressedField s b) =
  let u = GZip.decompress b in
  sign s <> size u <> u
putT3Field _
  ( T3IngredientField s
    ( T3IngredientData weight value
      (T3IngredientEffects e1 e2 e3 e4)
      (T3IngredientSkills s1 s2 s3 s4)
      (T3IngredientAttributes a1 a2 a3 a4)
    )
  ) =
  sign s <> w32 56 <> f32 weight <> w32 value
    <> i32 e1 <> i32 e2 <> i32 e3 <> i32 e4
    <> i32 s1 <> i32 s2 <> i32 s3 <> i32 s4
    <> i32 a1 <> i32 a2 <> i32 a3 <> i32 a4
putT3Field _
  ( T3ScriptField s
    ( T3ScriptHeader name
      shorts longs floats
      data_size var_table_size
    )
  ) =
  let b = t3StringValue name in
  sign s <> w32 52 <> b <> tail b 32
    <> w32 shorts <> w32 longs <> w32 floats
    <> w32 data_size <> w32 var_table_size
putT3Field _ (T3DialField s v) = sign s <> either ((w32 4 <>) . w32) ((w32 1 <>) . w8 . t3DialTypeValue) v
putT3Field _ (T3NoneField s) = sign s <> w32 4 <> w32 0
putT3Field _ (T3HeaderField s (T3FileHeader version file_type author descr)) =
  let v = runPut $ putWord32le version in
  let f = runPut $ putWord32le $ t3FileTypeValue file_type in
  let a = t3StringValue author in
  let d = t3StringValue $ T.intercalate "\r\n" descr in
  let items_count_placeholder = w32 0 in
  sign s <> runPut (putWord32le 300) <> v <> f <> a <> tail a 32 <> d <> tail d 256 <> items_count_placeholder

putT3Record :: T3Record -> ByteString
putT3Record (T3Record s g fields) =
  let b = foldl (<>) B.empty [putT3Field s f | f <- fields] in
  sign s <> size b <> flags g <> b
