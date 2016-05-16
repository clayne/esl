module Data.Tes3.Native where

#include <haskell>

data T3Mark
  = TES3 | HEDR | MAST | DATA | GLOB | NAME | FNAM | FORM
  | GMST | GMDT | ACTI | ALCH | APPA | ARMO | BODY | BOOK
  | BSGN | CELL | CLAS | CLOT | CNTC | CONT | CREA | CREC
  | DIAL | DOOR | ENCH | FACT | INFO | INGR | LAND | LEVC
  | LEVI | LIGH | LOCK | LTEX | MGEF | MISC | NPC_ | NPCC
  | PGRD | PROB | RACE | REGN | REPA | SCPT | SKIL | SNDG
  | SOUN | SPEL | SSCR | STAT | WEAP | SAVE | JOUR | QUES
  | GSCR | PLAY | CSTA | GMAP | DIAS | WTHR | KEYS | DYNA
  | ASPL | ACTC | MPRJ | PROJ | DCOU | MARK | FILT | DBGP
  | STRV | INTV | FLTV | SCHD | SCVR | SCDT | SCTX | MODL
  | IRDT | SCRI | ITEX | CNDT | FLAG | MCDT | NPCO | AADT
  | CTDT | RNAM | INDX | CNAM | ANAM | BNAM | KNAM | NPDT
  | AIDT | DODT | AI_W | CLDT | BYDT | RGNN | AODT | NAM5
  | DESC | WHGT | FADT | AMBI | FRMR | RADT | NAM0 | NPCS
  | DNAM | XSCL | SKDT | DELE | MEDT | PTEX | CVFX | BVFX
  | HVFX | AVFX | BSND | CSND | HSND | ASND | WEAT | SNAM
  deriving (Eq, Ord, Enum, Bounded, Show)

data T3Sign = T3Mark T3Mark | T3Sign Word32 deriving Eq

instance Show T3Sign where
  show (T3Mark m) = show m
  show (T3Sign s) =
    let (a, b, c, d) = toBytes s in
    [chr $ fromIntegral a, chr $ fromIntegral b, chr $ fromIntegral c, chr $ fromIntegral d]

instance Read T3Sign where
  readPrec = do
    a <- get
    b <- get
    c <- get
    d <- get
    return $ t3SignNew $ fromBytes (fromIntegral $ ord a) (fromIntegral $ ord b) (fromIntegral $ ord c) (fromIntegral $ ord d)

fromBytes :: Word8 -> Word8 -> Word8 -> Word8 -> Word32
fromBytes a b c d
  =  (fromIntegral a)
  .|. (shift (fromIntegral b) 8)
  .|. (shift (fromIntegral c) 16)
  .|. (shift (fromIntegral d) 24)
  
toBytes :: Word32 -> (Word8, Word8, Word8, Word8)
toBytes w =
  ( fromIntegral $ w .&. 0xFF
  , fromIntegral $ (.&. 0xFF) $ shift w (-8)
  , fromIntegral $ (.&. 0xFF) $ shift w (-16)
  , fromIntegral $ (.&. 0xFF) $ shift w (-24)
  )

t3MarkValueSlow :: T3Mark -> Word32
t3MarkValueSlow mark =
  calc (show mark)
  where
    calc [a, b, c, d] = fromBytes (fromIntegral $ ord a) (fromIntegral $ ord b) (fromIntegral $ ord c) (fromIntegral $ ord d)
    calc _ = 0x0BAD0BAD

t3MarkValues :: S.Map T3Mark Word32
t3MarkValues = SM.fromList [(m, t3MarkValueSlow m) | m <- [minBound .. maxBound]]

t3MarkValue :: T3Mark -> Word32
t3MarkValue m = fromMaybe 0x0BAD0BAD $ SM.lookup m t3MarkValues

t3SignValue :: T3Sign -> Word32
t3SignValue (T3Mark m) = t3MarkValue m
t3SignValue (T3Sign s) = s

t3MarkNews :: S.Map Word32 T3Mark
t3MarkNews = SM.fromList [(t3MarkValue m, m) | m <- [minBound .. maxBound]]

t3MarkNew :: Word32 -> Maybe T3Mark
t3MarkNew w = SM.lookup w t3MarkNews

t3SignNew :: Word32 -> T3Sign
t3SignNew w = fromMaybe (T3Sign w) $ T3Mark <$> t3MarkNew w

data KnownT3FileType = ESP | ESM | ESS deriving (Eq, Enum, Show)
data T3FileType = KnownT3FileType KnownT3FileType | UnknownT3FileType Word32 deriving (Eq)

instance Show T3FileType where
  show (KnownT3FileType t) = show t
  show (UnknownT3FileType w) = showHex w "h"

t3FileTypeNew :: Word32 -> T3FileType
t3FileTypeNew 0 = KnownT3FileType ESP
t3FileTypeNew 1 = KnownT3FileType ESM
t3FileTypeNew 32 = KnownT3FileType ESS
t3FileTypeNew a = UnknownT3FileType a

data T3Field = T3BinaryField T3Sign ByteString deriving (Eq, Show)
data T3Record = T3Record T3Sign [T3Field] deriving (Eq, Show)
data T3FileRef = T3FileRef S.ByteString Word64 deriving (Eq, Show)
data T3Header = T3Header Word32 T3FileType S.ByteString S.ByteString Word32 [T3FileRef] deriving (Eq, Show)
data T3File = T3File T3Header [T3Record] deriving (Eq, Show)