module Data.Tes3.Get.Native where

#include <haskell>
import Data.Tes3

expect :: (Show a, Eq a) => a -> Get e a -> Get (Either String e) ()
expect expected getter = do
  offset <- totalBytesRead
  actual <- onError Right getter
  if actual /= expected
    then failG $ Left $ concat [showHex offset $ "h: ", shows expected " expected, but ", shows actual " provided."]
    else return ()

sign :: Get () T3Sign
sign = t3SignNew <$> getWord32le

size :: Get () Word32
size = getWord32le

gap :: Get () Word64
gap = getWord64le

binaryField :: Get e ByteString
binaryField = getRemainingLazyByteString

stringField :: Get e Text
stringField = t3StringNew <$> getRemainingLazyByteString

multilineField :: Get e [Text]
multilineField = T.splitOn "\r\n" <$> t3StringNew <$> getRemainingLazyByteString

adjustedMultilineField :: Get e [Text]
adjustedMultilineField = T.splitOn "\r\n" <$> T.dropWhileEnd (== '\0') <$> t3StringNew <$> getRemainingLazyByteString

multiStringField :: Get e [Text]
multiStringField = T.splitOn "\0" <$> t3StringNew <$> getRemainingLazyByteString

refField :: Get () (Word32, Text)
refField = do
  z <- getWord32le
  n <- getLazyByteString 32
  return (z, T.dropWhileEnd (== '\0') $ t3StringNew n)

fixedStringField :: Word32 -> Get () Text
fixedStringField z = T.dropWhileEnd (== '\0') <$> t3StringNew <$> getLazyByteString (fromIntegral z)

floatField :: Get () Float
floatField = wordToFloat <$> getWord32le

compressedField :: Get e ByteString
compressedField = GZip.compress <$> getRemainingLazyByteString

ingredientField :: Get () T3IngredientData
ingredientField = do
  weight <- wordToFloat <$> getWord32le
  value <- getWord32le
  e1 <- getInt32le
  e2 <- getInt32le
  e3 <- getInt32le
  e4 <- getInt32le
  s1 <- getInt32le
  s2 <- getInt32le
  s3 <- getInt32le
  s4 <- getInt32le
  a1 <- getInt32le
  a2 <- getInt32le
  a3 <- getInt32le
  a4 <- getInt32le
  return $ T3IngredientData weight value
    (T3IngredientEffects e1 e2 e3 e4)
    (T3IngredientSkills s1 s2 s3 s4)
    (T3IngredientAttributes a1 a2 a3 a4)

scriptField :: Get () T3ScriptHeader
scriptField = do
  name <- T.dropWhileEnd (== '\0') <$> t3StringNew <$> getLazyByteString 32
  shorts <- getWord32le
  longs <- getWord32le
  floats <- getWord32le
  data_size <- getWord32le
  var_table_size <- getWord32le
  return $ T3ScriptHeader name shorts longs floats data_size var_table_size

fieldBody :: Bool -> T3Sign -> T3Sign -> Get () T3Field
fieldBody adjust record_sign s =
  f (t3FieldType record_sign s)
  where
    f (T3FixedString z) = T3FixedStringField s <$> fixedStringField z
    f T3String = T3StringField s <$> stringField
    f (T3AdjustableString a) = T3StringField s <$> (if adjust then a else id) <$> stringField
    f T3Multiline = T3MultilineField s <$> multilineField
    f T3AdjustableMultiline = T3MultilineField s <$> if adjust then adjustedMultilineField else multilineField
    f T3MultiString = T3MultiStringField s <$> multiStringField
    f T3Ref = (\(z, n) -> T3RefField s z n) <$> refField
    f T3Binary = T3BinaryField s <$> binaryField
    f T3Float = T3FloatField s <$> floatField
    f T3Int = T3IntField s <$> getInt32le
    f T3Short = T3ShortField s <$> getInt16le
    f T3Long = T3LongField s <$> getInt64le
    f T3Byte = T3ByteField s <$> getWord8
    f T3Compressed = T3CompressedField s <$> compressedField
    f T3Ingredient = T3IngredientField s <$> ingredientField
    f T3Script = T3ScriptField s <$> scriptField

field :: Bool -> T3Sign -> Get String T3Field
field adjust record_sign = do
  s <- sign `withError` "{0}: unexpected end of field"
  z <- size `withError` "{0}: unexpected end of field"
  let body = fieldBody adjust record_sign s `withError` "{0}: unexpected end of field"
  isolate (fromIntegral z) body $ \c -> "{0}: field size mismatch: " ++ show z ++ " expected, but " ++ show c ++ " consumed."

recordBody :: Bool -> T3Sign -> Get String [T3Field]
recordBody adjust s = whileM (not <$> isEmpty) $ field adjust s

recordTail :: Bool -> T3Sign -> Get (Either String ()) (Word64, [T3Field])
recordTail adjust s = do
  z <- size `withError` Right ()
  g <- gap `withError` Right ()
  f <- onError Left $ isolate (fromIntegral z) (recordBody adjust s) $ \c -> "{0}: record size mismatch: " ++ show z ++ " expected, but " ++ show c ++ " consumed."
  return (g, f)

getT3Record :: Bool -> Get String T3Record
getT3Record adjust = do
  s <- sign `withError` "{0}: unexpected end of record"
  (g, f) <- onError (either id (const "{0}: unexpected end of record")) $ recordTail adjust s
  return $ T3Record s g f

getT3FileSignature :: Get String ()
getT3FileSignature = onError (const "File format not recognized.") $ expect (T3Mark TES3) sign

fileRef :: Get (Either String ()) T3FileRef
fileRef = do
  expect (T3Mark MAST) sign
  m <- size `withError` Right ()
  name <- t3StringNew <$> B.fromStrict <$> getByteString (fromIntegral m) `withError` Right ()
  expect (T3Mark DATA) sign
  expect 8 size
  z <- getWord64le `withError` Right ()
  return $ T3FileRef name z

fileHeaderData :: Get (Either String ()) (T3FileHeader, Word32)
fileHeaderData = do
  expect (T3Mark HEDR) sign
  expect 300 size
  version <- getWord32le `withError` Right ()
  file_type_value <- getWord32le `withError` Right ()
  file_type <- case t3FileTypeNew file_type_value of
    Nothing -> failG $ Left $ "Unknown file type: " ++ show file_type_value
    Just x -> return x
  author <- T.dropWhileEnd (== '\0') <$> t3StringNew <$> B.fromStrict <$> getByteString 32 `withError` Right ()
  description <- T.splitOn "\r\n" <$> T.dropWhileEnd (== '\0') <$> t3StringNew <$> B.fromStrict <$> getByteString 256 `withError` Right ()
  items_count <- getWord32le `withError` Right ()
  refs <- whileM (not <$> isEmpty) fileRef
  return (T3FileHeader version file_type author description refs, items_count)

fileHeader :: Get (Either String ()) (T3FileHeader, Word32)
fileHeader = do
  z <- size `withError` Right ()
  expect 0 gap
  let t = onError (either id (const "{0}: unexpected end of header")) fileHeaderData
  onError Left $ isolate (fromIntegral z) t $ \c -> "{0}: header size mismatch: " ++ show z ++ " expected, but " ++ show c ++ " consumed."

getT3FileHeader :: Get String (T3FileHeader, Word32)
getT3FileHeader = onError (either id (const "{0}: unexpected end of file")) fileHeader
