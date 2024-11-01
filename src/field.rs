use crate::code_page::CodePage;
use crate::script_data::*;
use crate::strings::*;
use crate::serde_helpers::*;
use educe::Educe;
use either::{Either, Left, Right};
use enum_derive_2018::{EnumDisplay, EnumFromStr};
use enumn::N;
use macro_attr_2018::macro_attr;
use nameof::name_of;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::de::Error as de_Error;
use serde::de::{self, DeserializeSeed, Unexpected, VariantAccess};
use serde::ser::SerializeStruct;
use serde::ser::Error as ser_Error;
use serde_serialize_seed::{SerializeSeed, ValueWithSeed};
use std::convert::TryFrom;
use std::fmt::{self, Debug, Display, Formatter};
use std::mem::transmute;
use std::ops::{Index, IndexMut};
use std::str::FromStr;

pub use crate::tag::*;

include!(concat!(env!("OUT_DIR"), "/tags.rs"));

#[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub(crate) enum Newline {
    Unix,
    Dos
}

impl Newline {
    pub fn as_str(self) -> &'static str {
        if self == Newline::Unix { "\n" } else { "\r\n" }
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum FileType {
        ESP = 0,
        ESM = 1,
        ESS = 32
    }
}

enum_serde!(FileType, "file type", as u32, Unsigned, u64);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum DialogType {
        Topic = 0,
        Voice = 1,
        Greeting = 2,
        Persuasion = 3,
        Journal = 4
    }
}

enum_serde!(DialogType, "dialog type", as u8, Unsigned, u64);

mod dialog_type_u32 {
    use crate::field::DialogType;
    use serde::{Serializer, Deserializer, Serialize, Deserialize};
    use serde::de::Unexpected;
    use serde::de::Error as de_Error;
    use std::convert::TryInto;

    pub fn serialize<S>(&v: &DialogType, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if serializer.is_human_readable() {
            v.serialize(serializer)
        } else {
            (v as u32).serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DialogType, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            DialogType::deserialize(deserializer)
        } else {
            let d = u32::deserialize(deserializer)?;
            d.try_into().ok().and_then(DialogType::n).ok_or_else(|| D::Error::invalid_value(Unexpected::Unsigned(d as u64), &"dialog type"))
        }
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N)]
    #[repr(u32)]
    pub enum EffectRange {
        Self_ = 0,
        Touch = 1,
        Target = 2,
    }
}

impl Display for EffectRange {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            EffectRange::Self_ => write!(f, "Self"),
            EffectRange::Touch => write!(f, "Touch"),
            EffectRange::Target => write!(f, "Target"),
        }
    }
}

impl FromStr for EffectRange {
    type Err = ();
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Self" => Ok(EffectRange::Self_),
            "Touch" => Ok(EffectRange::Touch), 
            "Target" => Ok(EffectRange::Target),
            _ => Err(())
        }
    }
}

enum_serde!(EffectRange, "effect range", as u32, Unsigned, u64);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldType {
    U8List, U8ListZip,
    String(Option<u32>),
    StringZ, StringZList,
    Multiline(Newline),
    F32, I32, I16, I64, U8,
    MarkerU8(u8),
    Bool8, Bool32,
    Ingredient, ScriptMetadata, DialogMetadata, FileMetadata, Npc, NpcState, Effect, Spell,
    Ai, AiWander, AiTravel, AiTarget, AiActivate, NpcFlags, CreatureFlags, Book, ContainerFlags,
    Creature, Light, MiscItem, Apparatus, Weapon, Armor, BipedObject, BodyPart, Clothing, Enchantment,
    Tool, RepairItem, Pos, PosRot, PosRotOrCell, Grid, PathGrid, ScriptVars,
    I16List, I32List, F32List, Weather, Color, SoundChance, Potion, Class, Skill, EffectIndex,
    Item, Sound, EffectMetadata, Race, SoundGen, Info, Faction, SkillMetadata, Interior,
    CurrentTime, Time, EffectArg,
    Attributes, Skills, Tag,
    ScriptData
}
 
impl FieldType {
    pub fn from_tags(record_tag: Tag, prev_tag: Tag, field_tag: Tag, omwsave: bool) -> FieldType {
        match (record_tag, prev_tag, field_tag, omwsave) {
            (APPA, _, AADT, _) => FieldType::Apparatus,
            (INFO, _, ACDT, _) => FieldType::StringZ,
            (_, _, ACDT, _) => FieldType::U8ListZip,
            (_, _, ACID, _) => FieldType::I32,
            (_, _, ACSC, _) => FieldType::U8ListZip,
            (_, _, ACSL, _) => FieldType::U8ListZip,
            (_, _, ACT_, _) => FieldType::String(None),
            (_, _, ACTN, _) => FieldType::I32,
            (_, _, ACTV, _) => FieldType::MarkerU8(1),
            (_, _, AFLG, _) => FieldType::I32,
            (_, _, AI_A, _) => FieldType::AiActivate,
            (_, _, AI_E, _) => FieldType::AiTarget,
            (_, _, AI_F, _) => FieldType::AiTarget,
            (_, _, AI_T, _) => FieldType::AiTravel,
            (_, _, AI_W, _) => FieldType::AiWander,
            (_, _, AIDT, _) => FieldType::Ai,
            (_, _, AIPK, _) => FieldType::Tag,
            (_, _, AISE, _) => FieldType::Bool8,
            (ALCH, _, ALDT, _) => FieldType::Potion,
            (_, _, ALWY, _) => FieldType::Bool8,
            (CELL, _, AMBI, _) => FieldType::Interior,
            (FACT, _, ANAM, _) => FieldType::String(None),
            (_, _, ANAM, _) => FieldType::StringZ,
            (_, _, ANGL, _) => FieldType::F32,
            (_, _, ANIS, _) => FieldType::String(None),
            (ARMO, _, AODT, _) => FieldType::Armor,
            (REFR, _, APUD, _) => FieldType::String(None), // TODO
            (_, _, ARG_, _) => FieldType::EffectArg,
            (_, _, ASND, _) => FieldType::StringZ,
            (_, _, AVFX, _) => FieldType::StringZ,
            (_, _, BASE, _) => FieldType::F32,
            (BOOK, _, BKDT, _) => FieldType::Book,
            (ARMO, _, BNAM, _) => FieldType::String(None),
            (BODY, _, BNAM, _) => FieldType::String(None),
            (CLOT, _, BNAM, _) => FieldType::String(None),
            (INFO, _, BNAM, _) => FieldType::Multiline(Newline::Dos),
            (_, _, BNAM, _) => FieldType::StringZ,
            (_, _, BNDS, _) => FieldType::I32List,
            (_, _, BOUN, _) => FieldType::F32List,
            (_, _, BSND, _) => FieldType::StringZ,
            (_, _, BVFX, _) => FieldType::StringZ,
            (BODY, _, BYDT, _) => FieldType::BodyPart,
            (_, _, CAST, _) => FieldType::I32,
            (_, _, CFLG, _) => FieldType::I32,
            (_, _, CHRD, _) => FieldType::U8ListZip,
            (_, _, CIDX, _) => FieldType::I64,
            (CLAS, _, CLDT, _) => FieldType::Class,
            (_, _, CMND, _) => FieldType::Bool8,
            (ARMO, _, CNAM, _) => FieldType::String(None),
            (CLOT, _, CNAM, _) => FieldType::String(None),
            (KLST, _, CNAM, _) => FieldType::I32,
            (REGN, _, CNAM, _) => FieldType::Color,
            (_, _, CNAM, _) => FieldType::StringZ,
            (CELL, _, CNDT, _) => FieldType::Grid,
            (CONT, _, CNDT, _) => FieldType::F32,
            (_, TIME, COUN, _) => FieldType::I64,
            (_, ANIS, COUN, _) => FieldType::I64,
            (_, _, COUN, _) => FieldType::I32,
            (CELL, _, CRED, _) => FieldType::U8ListZip,
            (_, _, CREG, _) => FieldType::StringZ,
            (_, _, CRID, _) => FieldType::I32,
            (CELL, _, CSHN, _) => FieldType::StringZ,
            (_, _, CSND, _) => FieldType::StringZ,
            (CELL, _, CSTN, _) => FieldType::StringZ,
            (CLOT, _, CTDT, _) => FieldType::Clothing,
            (_, _, CURD, _) => FieldType::I32,
            (_, _, CVFX, _) => FieldType::StringZ,
            (_, _, CWTH, _) => FieldType::I32,
            (_, _, DANM, _) => FieldType::U8,
            (CELL, _, DATA, _) => FieldType::PosRotOrCell,
            (DIAL, _, DATA, _) => FieldType::DialogMetadata,
            (GMAP, _, DATA, _) => FieldType::U8ListZip,
            (INFO, _, DATA, _) => FieldType::Info,
            (LAND, _, DATA, _) => FieldType::I32,
            (LEVC, _, DATA, _) => FieldType::I32,
            (LEVI, _, DATA, _) => FieldType::I32,
            (LTEX, _, DATA, _) => FieldType::StringZ,
            (PGRD, _, DATA, _) => FieldType::PathGrid,
            (REFR, _, DATA, _) => FieldType::PosRot,
            (SNDG, _, DATA, _) => FieldType::SoundGen,
            (SOUN, _, DATA, _) => FieldType::Sound,
            (SSCR, _, DATA, _) => FieldType::String(None),
            (TES3, _, DATA, _) => FieldType::I64,
            (QUES, _, DATA, _) => FieldType::StringZ,
            (_, _, DATA, _) => FieldType::U8ListZip,
            (_, _, DELE, _) => FieldType::I32,
            (_, _, DEPE, _) => FieldType::String(None),
            (BSGN, _, DESC, _) => FieldType::StringZ,
            (_, _, DESC, _) => FieldType::String(None),
            (_, CAST, DISP, _) => FieldType::String(None),
            (_, _, DISP, _) => FieldType::I32,
            (_, _, DNAM, _) => FieldType::StringZ,
            (_, _, DODT, _) => FieldType::PosRot,
            (_, _, DRAW, _) => FieldType::I32,
            (_, _, DRTI, _) => FieldType::F32,
            (_, _, DTIM, _) => FieldType::Time,
            (_, _, DURA, _) => FieldType::F32,
            (_, _, EFID, _) => FieldType::EffectIndex,
            (_, _, EIND, _) => FieldType::I32,
            (_, _, ENAB, _) => FieldType::Bool8,
            (ALCH, _, ENAM, _) => FieldType::Effect,
            (ENCH, _, ENAM, _) => FieldType::Effect,
            (PCDT, _, ENAM, _) => FieldType::I64,
            (SPEL, _, ENAM, _) => FieldType::Effect,
            (_, _, ENAM, _) => FieldType::StringZ,
            (ENCH, _, ENDT, _) => FieldType::Enchantment,
            (_, _, EQIP, _) => FieldType::U8ListZip,
            (_, _, FACT, _) => FieldType::String(None),
            (FACT, _, FADT, _) => FieldType::Faction,
            (_, _, FALL, _) => FieldType::F32,
            (_, _, FARA, _) => FieldType::I32,
            (_, _, FARE, _) => FieldType::I32,
            (_, _, FAST, _) => FieldType::Bool8,
            (CELL, _, FGTN, _) => FieldType::StringZ,
            (CAM_, _, FIRS, _) => FieldType::Bool8,
            (CONT, _, FLAG, _) => FieldType::ContainerFlags,
            (CREA, _, FLAG, _) => FieldType::CreatureFlags,
            (NPC_, _, FLAG, _) => FieldType::NpcFlags,
            (_, _, FLAG, _) => FieldType::I32,
            (_, _, FLTV, _) => FieldType::F32,
            (GLOB, _, FNAM, _) => FieldType::String(None),
            (PCDT, _, FNAM, _) => FieldType::U8ListZip,
            (STLN, _, FNAM, _) => FieldType::String(None),
            (_, _, FNAM, _) => FieldType::StringZ,
            (_, _, FORM, _) => FieldType::I32,
            (CELL, _, FRMR, _) => FieldType::I32,
            (_, _, FRMR, _) => FieldType::I32List,
            (_, _, FTEX, _) => FieldType::U8ListZip,
            (_, _, GMDT, _) => FieldType::U8ListZip,
            (_, _, GOLD, _) => FieldType::I32,
            (_, _, GRAV, _) => FieldType::I32,
            (_, _, HCUS, _) => FieldType::MarkerU8(0),
            (TES3, _, HEDR, _) => FieldType::FileMetadata,
            (_, _, HFOW, _) => FieldType::Bool32,
            (_, _, HIDD, _) => FieldType::Bool8,
            (_, _, HLOC, _) => FieldType::MarkerU8(1),
            (_, _, HSND, _) => FieldType::StringZ,
            (_, _, HVFX, _) => FieldType::StringZ,
            (_, _, ICNT, _) => FieldType::I32,
            (_, _, ID__, _) => FieldType::String(None),
            (_, _, INAM, _) => FieldType::StringZ,
            (_, _, INCR, _) => FieldType::Attributes,
            (ARMO, _, INDX, _) => FieldType::BipedObject,
            (CLOT, _, INDX, _) => FieldType::BipedObject,
            (MGEF, _, INDX, _) => FieldType::EffectIndex,
            (SKIL, _, INDX, _) => FieldType::Skill,
            (_, _, INDX, _) => FieldType::I32,
            (CELL, _, INTV, _) => FieldType::F32,
            (LAND, _, INTV, _) => FieldType::Grid,
            (LEVC, _, INTV, _) => FieldType::I16,
            (LEVI, _, INTV, _) => FieldType::I16,
            (_, _, INTV, _) => FieldType::I32,
            (INGR, _, IRDT, _) => FieldType::Ingredient,
            (_, _, ITEM, _) => FieldType::I32List,
            (_, _, ITEX, _) => FieldType::StringZ,
            (_, _, JEDA, _) => FieldType::I32,
            (_, _, JEDM, _) => FieldType::I32,
            (_, _, JEMO, _) => FieldType::I32,
            (_, _, JETY, _) => FieldType::I32,
            (PCDT, _, KNAM, _) => FieldType::U8ListZip,
            (_, _, KNAM, _) => FieldType::StringZ,
            (_, _, LAST, _) => FieldType::I32,
            (_, _, LEFT, _) => FieldType::F32,
            (_, _, LEVL, _) => FieldType::I32,
            (ENAB, _, LEVT, _) => FieldType::Bool8,
            (_, _, LHAT, _) => FieldType::String(None),
            (LIGH, _, LHDT, _) => FieldType::Light,
            (_, _, LHIT, _) => FieldType::String(None),
            (PCDT, _, LNAM, _) => FieldType::I64,
            (LOCK, _, LKDT, _) => FieldType::Tool,
            (_, _, LKEP, _) => FieldType::Pos,
            (_, _, LOCA, _) => FieldType::String(None),
            (_, _, LPRO, _) => FieldType::I32,
            (CELL, _, LSHN, _) => FieldType::StringZ,
            (CELL, _, LSTN, _) => FieldType::StringZ,
            (_, _, LUAD, _) => FieldType::U8ListZip,
            (_, _, LUAS, _) => FieldType::String(None),
            (_, _, LUAW, _) => FieldType::I64,
            (_, _, LVCR, _) => FieldType::U8,
            (_, _, MAGN, _) => FieldType::F32,
            (FMAP, _, MAPD, _) => FieldType::U8ListZip,
            (FMAP, _, MAPH, _) => FieldType::I64,
            (_, _, MARK, _) => FieldType::PosRot,
            (TES3, _, MAST, _) => FieldType::StringZ,
            (MISC, _, MCDT, _) => FieldType::MiscItem,
            (MGEF, _, MEDT, _) => FieldType::EffectMetadata,
            (_, _, MGEF, _) => FieldType::EffectIndex,
            (PCDT, _, MNAM, _) => FieldType::StringZ,
            (CELL, _, MNAM, _) => FieldType::U8,
            (_, _, MODI, _) => FieldType::F32,
            (_, _, MODL, _) => FieldType::StringZ,
            (_, _, MOVE, _) => FieldType::I32,
            (_, _, MRK_, _) => FieldType::Grid,
            (CELL, _, MVRF, _) => FieldType::I32,
            (_, _, MVRF, _) => FieldType::I32List,
            (CELL, _, NAM0, _) => FieldType::I32,
            (PCDT, _, NAM0, _) => FieldType::StringZ,
            (SPLM, _, NAM0, _) => FieldType::U8,
            (PCDT, _, NAM1, _) => FieldType::StringZ,
            (PCDT, _, NAM2, _) => FieldType::StringZ,
            (PCDT, _, NAM3, _) => FieldType::StringZ,
            (CELL, _, NAM5, _) => FieldType::I32,
            (CELL, _, NAM8, _) => FieldType::U8ListZip,
            (CELL, _, NAM9, _) => FieldType::I32,
            (PCDT, _, NAM9, _) => FieldType::I32,
            (GMST, _, NAME, _) => FieldType::String(None),
            (GSCR, _, NAME, _) => FieldType::String(None),
            (INFO, _, NAME, _) => FieldType::String(None),
            (JOUR, _, NAME, _) => FieldType::Multiline(Newline::Unix),
            (SPLM, _, NAME, _) => FieldType::I32,
            (SSCR, _, NAME, _) => FieldType::String(None),
            (STLN, _, NAME, _) => FieldType::String(None),
            (_, _, NAME, _) => FieldType::StringZ,
            (_, _, ND3D, _) => FieldType::U8,
            (LEVC, _, NNAM, _) => FieldType::U8,
            (LEVI, _, NNAM, _) => FieldType::U8,
            (_, _, NNAM, _) => FieldType::StringZ,
            (_, _, NPCO, false) => FieldType::Item,
            (CREA, _, NPDT, _) => FieldType::Creature,
            (SPLM, _, NPDT, _) => FieldType::U8ListZip,
            (NPC_, _, NPDT, _) => FieldType::Npc,
            (NPCC, _, NPDT, _) => FieldType::NpcState,
            (_, _, NPCS, false) => FieldType::String(Some(32)),
            (_, _, NWTH, _) => FieldType::I32,
            (_, _, OBJE, _) => FieldType::Tag,
            (STLN, _, ONAM, _) => FieldType::String(None),
            (_, _, ONAM, _) => FieldType::StringZ,
            (_, _, PAYD, _) => FieldType::I32,
            (PROB, _, PBDT, _) => FieldType::Tool,
            (_, _, PGRC, _) => FieldType::U8ListZip,
            (_, _, PGRP, _) => FieldType::U8ListZip,
            (_, _, PLCE, _) => FieldType::String(None),
            (_, _, PLCN, _) => FieldType::String(None),
            (_, _, PLLE, _) => FieldType::I32,
            (_, _, PLNA, _) => FieldType::String(None),
            (PCDT, _, PNAM, _) => FieldType::U8ListZip,
            (_, _, PNAM, _) => FieldType::StringZ,
            (_, STAR, POS_, _) => FieldType::Pos,
            (_, _, POS_, _) => FieldType::PosRot,
            (_, _, PTEX, _) => FieldType::StringZ,
            (_, _, QFIN, _) => FieldType::Bool8,
            (_, _, QSTA, _) => FieldType::I32,
            (_, _, QWTH, _) => FieldType::I32,
            (RACE, _, RADT, _) => FieldType::Race,
            (_, _, REPT, _) => FieldType::MarkerU8(1),
            (_, _, REPU, _) => FieldType::I32,
            (_, _, RESP, false) => FieldType::Time,
            (_, _, RGNC, _) => FieldType::U8,
            (_, _, RGNN, _) => FieldType::StringZ,
            (_, _, RGNW, _) => FieldType::I32,
            (REPA, _, RIDT, _) => FieldType::RepairItem,
            (FACT, _, RNAM, _) => FieldType::String(Some(32)),
            (SCPT, _, RNAM, _) => FieldType::I32,
            (_, _, RNAM, _) => FieldType::StringZ,
            (_, _, RUN_, _) => FieldType::Bool32,
            (SCPT, _, SCDT, _) => FieldType::ScriptData,
            (SCPT, _, SCHD, _) => FieldType::ScriptMetadata,
            (TES3, _, SCRD, _) => FieldType::U8ListZip,
            (_, _, SCRI, _) => FieldType::StringZ,
            (_, _, SCRN, _) => FieldType::U8ListZip,
            (TES3, _, SCRS, _) => FieldType::U8ListZip,
            (_, _, SCTX, _) => FieldType::Multiline(Newline::Dos),
            (SCPT, _, SCVR, _) => FieldType::StringZList,
            (_, _, SCVR, _) => FieldType::String(None),
            (_, _, SELE, _) => FieldType::I32,
            (_, _, SIGN, _) => FieldType::String(None),
            (SKIL, _, SKDT, _) => FieldType::SkillMetadata,
            (_, _, SLCS, _) => FieldType::ScriptVars,
            (_, _, SLFD, _) => FieldType::F32List,
            (_, _, SLLD, _) => FieldType::I32List,
            (_, _, SLSD, _) => FieldType::I16List,
            (PCDT, _, SNAM, _) => FieldType::U8ListZip,
            (REGN, _, SNAM, _) => FieldType::SoundChance,
            (_, _, SNAM, _) => FieldType::StringZ,
            (_, _, SPAC, _) => FieldType::String(None),
            (_, _, SPAW, _) => FieldType::I32,
            (SPLM, _, SPDT, _) => FieldType::U8ListZip,
            (SPEL, _, SPDT, _) => FieldType::Spell,
            (_, _, SPEC, _) => FieldType::I32List,
            (_, _, SPEL, _) => FieldType::String(None),
            (_, _, STAR, _) => FieldType::Time,
            (_, _, STBA, _) => FieldType::F32,
            (_, _, STCU, _) => FieldType::F32,
            (_, _, STDF, _) => FieldType::F32,
            (_, _, STMO, _) => FieldType::F32,
            (CELL, _, STPR, _) => FieldType::U8ListZip,
            (REFR, _, STPR, _) => FieldType::U8ListZip,
            (_, _, STPR, _) => FieldType::F32,
            (_, _, STRV, _) => FieldType::String(None),
            (_, _, TAID, _) => FieldType::I32,
            (GSCR, _, TARG, _) => FieldType::String(None),
            (_, DATA, TARG, _) => FieldType::String(None),
            (_, _, TARG, _) => FieldType::I32,
            (ENAB, _, TELE, _) => FieldType::Bool8,
            (BOOK, _, TEXT, _) => FieldType::Multiline(Newline::Dos),
            (JOUR, _, TEXT, _) => FieldType::String(None),
            (_, _, TEXT, _) => FieldType::StringZ,
            (CSTA, ANIS, TIME, _) => FieldType::I32,
            (CSTA, _, TIME, _) => FieldType::I64,
            (_, _, TIME, _) => FieldType::Time,
            (_, _, TMPS, _) => FieldType::I32,
            (_, _, TNAM, _) => FieldType::StringZ,
            (_, _, TOPI, _) => FieldType::String(None),
            (_, _, TRFC, _) => FieldType::I32,
            (_, _, TSTM, _) => FieldType::CurrentTime,
            (_, _, TYPE, _) => FieldType::I32,
            (_, _, QID_, _) => FieldType::String(None),
            (INFO, _, QSTF, _) => FieldType::MarkerU8(1),
            (INFO, _, QSTN, _) => FieldType::MarkerU8(1),
            (INFO, _, QSTR, _) => FieldType::MarkerU8(1),
            (_, _, USED, _) => FieldType::String(None),
            (_, _, UNAM, _) => FieldType::MarkerU8(0),
            (_, _, VCLR, _) => FieldType::U8ListZip,
            (_, _, VHGT, _) => FieldType::U8ListZip,
            (_, _, VNML, _) => FieldType::U8ListZip,
            (_, _, VTEX, _) => FieldType::U8ListZip,
            (REGN, _, WEAT, _) => FieldType::Weather,
            (_, _, WHGT, _) => FieldType::F32,
            (_, _, WIDX, _) => FieldType::I64,
            (_, _, WLVL, _) => FieldType::F32,
            (_, _, WNAM, _) => FieldType::U8ListZip,
            (WEAP, _, WPDT, _) => FieldType::Weapon,
            (_, _, WUPD, _) => FieldType::I32,
            (_, _, WWAT, _) => FieldType::Attributes,
            (_, _, WWSK, _) => FieldType::Skills,
            (_, _, XCHG, _) => FieldType::F32,
            (_, _, XHLT, _) => FieldType::I32,
            (_, _, XIDX, _) => FieldType::I32,
            (REFR, _, XNAM, _) => FieldType::StringZ,
            (SPLM, _, XNAM, _) => FieldType::U8,
            (_, _, XSCL, _) => FieldType::F32,
            (_, _, XSOL, _) => FieldType::StringZ,
            (_, _, YEIN, _) => FieldType::String(None),
            (_, _, YETO, _) => FieldType::String(None),
            (REFR, _, YNAM, _) => FieldType::I32,
            (CELL, _, ZNAM, _) => FieldType::U8,
            _ => FieldType::U8List
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Educe)]
#[educe(PartialEq, Eq)]
pub struct Ingredient {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    #[serde(with="effect_index_option_i32")]
    pub effect_1_index: Either<Option<i32>, EffectIndex>,
    #[serde(with="effect_index_option_i32")]
    pub effect_2_index: Either<Option<i32>, EffectIndex>,
    #[serde(with="effect_index_option_i32")]
    pub effect_3_index: Either<Option<i32>, EffectIndex>,
    #[serde(with="effect_index_option_i32")]
    pub effect_4_index: Either<Option<i32>, EffectIndex>,
    #[serde(with="skill_option_i32")]
    pub effect_1_skill: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub effect_2_skill: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub effect_3_skill: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub effect_4_skill: Either<Option<i32>, Skill>,
    #[serde(with="attribute_option_i32")]
    pub effect_1_attribute: Either<Option<i32>, Attribute>,
    #[serde(with="attribute_option_i32")]
    pub effect_2_attribute: Either<Option<i32>, Attribute>,
    #[serde(with="attribute_option_i32")]
    pub effect_3_attribute: Either<Option<i32>, Attribute>,
    #[serde(with="attribute_option_i32")]
    pub effect_4_attribute: Either<Option<i32>, Attribute>,
}

pub(crate) fn eq_f32(a: &f32, b: &f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn eq_f32_list(a: &[f32], b: &[f32]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| eq_f32(x, y))
}

pub(crate) mod float_32 {
    use serde::{Serializer, Deserializer, Serialize};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use crate::serde_helpers::*;

    pub fn serialize<S>(v: &f32, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, F32AsIsSerde).serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<f32, D::Error> where D: Deserializer<'de> {
        F32AsIsSerde.deserialize(deserializer)
    }
}

mod option_i8 {
    use serde::{Serializer, Deserializer, Deserialize, Serialize};
    use serde::ser::Error as ser_Error;

    pub fn serialize<S>(&v: &Option<i8>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if serializer.is_human_readable() {
            v.serialize(serializer)
        } else {
            let v = if let Some(v) = v {
                if v == -1 { return Err(S::Error::custom("-1 is reserved")); }
                v
            } else {
                -1
            };
            v.serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i8>, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            <Option<i8>>::deserialize(deserializer)
        } else {
            let d = i8::deserialize(deserializer)?;
            if d == -1 {
                Ok(None)
            } else {
                Ok(Some(d))
            }
        }
    }
}

mod bool_u32 {
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use crate::serde_helpers::*;

    pub fn serialize<S>(v: &bool, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, BoolU32Serde).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error> where D: Deserializer<'de> {
        BoolU32Serde.deserialize(deserializer)
    }
}

mod bool_u8 {
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use crate::serde_helpers::*;

    pub fn serialize<S>(v: &bool, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, BoolU8Serde).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error> where D: Deserializer<'de> {
        BoolU8Serde.deserialize(deserializer)
    }
}

mod bool_either_i16 {
    use serde::{Serializer, Deserializer, Deserialize, Serialize};
    use serde::de::Error as de_Error;
    use either::{Either, Left, Right};
    use serde::de::Unexpected;

    pub fn serialize<S>(&v: &Either<bool, bool>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if serializer.is_human_readable() {
            v.serialize(serializer)
        } else {
            let v: i16 = match v {
                Left(false) => -2,
                Left(true) => -1,
                Right(false) => 0,
                Right(true) => 1
            };
            v.serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<bool, bool>, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            <Either<bool, bool>>::deserialize(deserializer)
        } else {
            let d = i16::deserialize(deserializer)?;
            match d {
                0 => Ok(Right(false)),
                1 => Ok(Right(true)),
                -1 => Ok(Left(true)),
                -2 => Ok(Left(false)),
                d => Err(D::Error::invalid_value(Unexpected::Signed(d as i64), &"-2, -1, 0, or 1"))
            }
        }
    }
}

#[derive(Educe, Debug, Clone, Serialize, Deserialize)]
#[educe(Eq, PartialEq)]
pub struct CurrentTime {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub hour: f32,
    pub day: u32,
    pub month: u32,
    pub year: u32
}

#[derive(Educe, Debug, Clone, Serialize, Deserialize)]
#[educe(Eq, PartialEq)]
pub struct Time {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub hour: f32,
    pub day: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ScriptVars {
    pub shorts: u32,
    pub longs: u32,
    pub floats: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ScriptMetadata {
    pub name: String,
    pub vars: ScriptVars,
    pub data_size: u32,
    pub var_table_size: u32
}

const SCRIPT_METADATA_NAME_FIELD: &str = name_of!(name in ScriptMetadata);
const SCRIPT_METADATA_VARS_FIELD: &str = name_of!(vars in ScriptMetadata);
const SCRIPT_METADATA_DATA_SIZE_FIELD: &str = name_of!(data_size in ScriptMetadata);
const SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD: &str = name_of!(var_table_size in ScriptMetadata);

const SCRIPT_METADATA_FIELDS: &[&str] = &[
    SCRIPT_METADATA_NAME_FIELD,
    SCRIPT_METADATA_VARS_FIELD,
    SCRIPT_METADATA_DATA_SIZE_FIELD,
    SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD,
];

#[derive(Clone)]
pub struct ScriptMetadataSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for ScriptMetadataSerde {
    type Value = ScriptMetadata;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type ScriptMetadata), 4)?;
        serializer.serialize_field(
            SCRIPT_METADATA_NAME_FIELD,
            &ValueWithSeed(value.name.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.serialize_field(SCRIPT_METADATA_VARS_FIELD, &value.vars)?;
        serializer.serialize_field(SCRIPT_METADATA_DATA_SIZE_FIELD, &value.data_size)?;
        serializer.serialize_field(SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD, &value.var_table_size)?;
        serializer.end()
    }
}

enum ScriptMetadataField {
    Name,
    Vars,
    DataSize,
    VarTableSize
}

struct ScriptMetadataFieldDeVisitor;

impl<'de> de::Visitor<'de> for ScriptMetadataFieldDeVisitor {
    type Value = ScriptMetadataField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "script metadata field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            SCRIPT_METADATA_NAME_FIELD => Ok(ScriptMetadataField::Name),
            SCRIPT_METADATA_VARS_FIELD => Ok(ScriptMetadataField::Vars),
            SCRIPT_METADATA_DATA_SIZE_FIELD => Ok(ScriptMetadataField::DataSize),
            SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD => Ok(ScriptMetadataField::VarTableSize),
            x => Err(E::unknown_field(x, SCRIPT_METADATA_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for ScriptMetadataField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(ScriptMetadataFieldDeVisitor)
    }
}

struct ScriptMetadataDeVisitor(ScriptMetadataSerde);

impl<'de> de::Visitor<'de> for ScriptMetadataDeVisitor {
    type Value = ScriptMetadata;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "script metadata")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut name = None;
        let mut vars = None;
        let mut data_size = None;
        let mut var_table_size = None;
        while let Some(field) = map.next_key()? {
            match field {
                ScriptMetadataField::Name =>
                    if name.replace(map.next_value_seed(StringSerde {
                        code_page: self.0.code_page, len: Some(32)
                    })?).is_some() {
                        return Err(A::Error::duplicate_field(SCRIPT_METADATA_NAME_FIELD));
                    },
                ScriptMetadataField::Vars => if vars.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(SCRIPT_METADATA_VARS_FIELD));
                },
                ScriptMetadataField::DataSize => if data_size.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(SCRIPT_METADATA_DATA_SIZE_FIELD));
                },
                ScriptMetadataField::VarTableSize => if var_table_size.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD));
                },
            }
        }
        let name = name.ok_or_else(|| A::Error::missing_field(SCRIPT_METADATA_NAME_FIELD))?;
        let vars = vars.ok_or_else(|| A::Error::missing_field(SCRIPT_METADATA_VARS_FIELD))?;
        let data_size = data_size.ok_or_else(|| A::Error::missing_field(SCRIPT_METADATA_DATA_SIZE_FIELD))?;
        let var_table_size = var_table_size.ok_or_else(|| A::Error::missing_field(SCRIPT_METADATA_VAR_TABLE_SIZE_FIELD))?;
        Ok(ScriptMetadata { name, vars, data_size, var_table_size })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let name = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let vars = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        let data_size = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(2, &self))?;
        let var_table_size = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(3, &self))?;
        Ok(ScriptMetadata { name, vars, data_size, var_table_size })
    }
}

impl<'de> DeserializeSeed<'de> for ScriptMetadataSerde {
    type Value = ScriptMetadata;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type ScriptMetadata), SCRIPT_METADATA_FIELDS, ScriptMetadataDeVisitor(self))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FileMetadata {
    pub version: u32,
    pub file_type: FileType,
    pub author: Either<u32, String>,
    pub description: Either<u32, Vec<String>>,
    pub records: u32
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
#[serde(rename="FileAuthorOption")]
enum FileAuthorOptionHRSurrogate {
    None(u32),
    Some(String)
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
#[serde(rename="FileDescriptionOption")]
enum FileDescriptionOptionHRSurrogate {
    None(u32),
    Some(Vec<String>)
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[serde(rename="FileMetadata")]
struct FileMetadataHRSurrogate {
    pub version: u32,
    pub file_type: FileType,
    pub author: FileAuthorOptionHRSurrogate,
    pub description: FileDescriptionOptionHRSurrogate,
    pub records: u32
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
struct FileMetadataNHRSurrogate20 {
    pub version: u32,
    pub file_type: FileType,
    pub author: u32,
    pub description: u32,
    pub records: u32
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct FileMetadataNHRSurrogate300 {
    pub version: u32,
    pub file_type: FileType,
    pub author: String,
    pub description: Vec<String>,
    pub records: u32
}

impl From<FileMetadata> for FileMetadataHRSurrogate {
    fn from(x: FileMetadata) -> FileMetadataHRSurrogate {
        FileMetadataHRSurrogate {
            version: x.version,
            file_type: x.file_type,
            author: match x.author {
                Left(x) => FileAuthorOptionHRSurrogate::None(x),
                Right(x) => FileAuthorOptionHRSurrogate::Some(x),
            },
            description: match x.description {
                Left(x) => FileDescriptionOptionHRSurrogate::None(x),
                Right(x) => FileDescriptionOptionHRSurrogate::Some(x),
            },
            records: x.records
        }
    }
}

impl From<FileMetadataHRSurrogate> for FileMetadata {
    fn from(x: FileMetadataHRSurrogate) -> FileMetadata {
        FileMetadata {
            version: x.version,
            file_type: x.file_type,
            author: match x.author {
                FileAuthorOptionHRSurrogate::None(x) => Left(x),
                FileAuthorOptionHRSurrogate::Some(x) => Right(x),
            },
            description: match x.description {
                FileDescriptionOptionHRSurrogate::None(x) => Left(x),
                FileDescriptionOptionHRSurrogate::Some(x) => Right(x),
            },
            records: x.records
        }
    }
}

impl From<FileMetadataNHRSurrogate20> for FileMetadata {
    fn from(x: FileMetadataNHRSurrogate20) -> FileMetadata {
        FileMetadata {
            version: x.version,
            file_type: x.file_type,
            author: Left(x.author),
            description: Left(x.description),
            records: x.records
        }
    }
}

impl From<FileMetadataNHRSurrogate300> for FileMetadata {
    fn from(x: FileMetadataNHRSurrogate300) -> FileMetadata {
        FileMetadata {
            version: x.version,
            file_type: x.file_type,
            author: Right(x.author),
            description: Right(x.description),
            records: x.records
        }
    }
}

impl TryFrom<FileMetadata> for Either<FileMetadataNHRSurrogate20, FileMetadataNHRSurrogate300> {
    type Error = ();

    fn try_from(x: FileMetadata) -> Result<Either<FileMetadataNHRSurrogate20, FileMetadataNHRSurrogate300>, Self::Error> {
        if x.author.is_left() && x.description.is_right() { return Err(()); }
        if x.author.is_right() && x.description.is_left() { return Err(()); }
        Ok(if x.author.is_left() {
            Left(FileMetadataNHRSurrogate20 {
                version: x.version,
                file_type: x.file_type,
                author: x.author.left().unwrap(),
                description: x.description.left().unwrap(),
                records: x.records
            })
        } else {
            Right(FileMetadataNHRSurrogate300 {
                version: x.version,
                file_type: x.file_type,
                author: x.author.right().unwrap(),
                description: x.description.right().unwrap(),
                records: x.records
            })
        })
    }
}

const FILE_METADATA_VERSION_FIELD: &str = name_of!(version in FileMetadata);
const FILE_METADATA_TYPE_FIELD: &str = "type";
const FILE_METADATA_AUTHOR_FIELD: &str = name_of!(author in FileMetadata);
const FILE_METADATA_DESCRIPTION_FIELD: &str = name_of!(description in FileMetadata);
const FILE_METADATA_RECORDS_FIELD: &str = name_of!(records in FileMetadata);

const FILE_METADATA_FIELDS: &[&str] = &[
    FILE_METADATA_VERSION_FIELD,
    FILE_METADATA_TYPE_FIELD,
    FILE_METADATA_AUTHOR_FIELD,
    FILE_METADATA_DESCRIPTION_FIELD,
    FILE_METADATA_RECORDS_FIELD,
];

#[derive(Clone)]
pub struct FileMetadataSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for FileMetadataSerde {
    type Value = FileMetadata;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            FileMetadataHRSurrogate::from(value.clone()).serialize(serializer)
        } else {
            let surrogate: Result<Either<FileMetadataNHRSurrogate20, FileMetadataNHRSurrogate300>, ()> = value.clone().try_into();
            let Ok(surrogate) = surrogate else { return Err(S::Error::custom("invalid file metadata")); };
            match surrogate {
                Left(fm20) => serializer.serialize_newtype_variant(
                    name_of!(type FileMetadata), 20, "FileMetadata20", &fm20
                ),
                Right(fm300) => serializer.serialize_newtype_variant(
                    name_of!(type FileMetadata), 300, "FileMetadata300",
                    &ValueWithSeed(&fm300, FileMetadataNHRSurrogate300Serde { code_page: self.code_page })
                ),
            }
        }
    }
}

#[derive(Clone)]
struct FileMetadataNHRSurrogate300Serde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for FileMetadataNHRSurrogate300Serde {
    type Value = FileMetadataNHRSurrogate300;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type FileMetadataNHRSurrogate300), 5)?;
        serializer.serialize_field(FILE_METADATA_VERSION_FIELD, &value.version)?;
        serializer.serialize_field(FILE_METADATA_TYPE_FIELD, &value.file_type)?;
        serializer.serialize_field(
            FILE_METADATA_AUTHOR_FIELD,
            &ValueWithSeed(value.author.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.serialize_field(
            FILE_METADATA_DESCRIPTION_FIELD,
            &ValueWithSeed(&value.description[..], StringListSerde {
                code_page: self.code_page, separator: Newline::Dos.as_str(), len: Some(256)
            })
        )?;
        serializer.serialize_field(FILE_METADATA_RECORDS_FIELD, &value.records)?;
        serializer.end()
    }
}

enum FileMetadataField {
    Version,
    Type,
    Author,
    Description,
    Records
}

struct FileMetadataFieldDeVisitor;

impl<'de> de::Visitor<'de> for FileMetadataFieldDeVisitor {
    type Value = FileMetadataField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "file metadata field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            FILE_METADATA_VERSION_FIELD => Ok(FileMetadataField::Version),
            FILE_METADATA_TYPE_FIELD => Ok(FileMetadataField::Type),
            FILE_METADATA_AUTHOR_FIELD => Ok(FileMetadataField::Author),
            FILE_METADATA_DESCRIPTION_FIELD => Ok(FileMetadataField::Description),
            FILE_METADATA_RECORDS_FIELD => Ok(FileMetadataField::Records),
            x => Err(E::unknown_field(x, FILE_METADATA_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for FileMetadataField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(FileMetadataFieldDeVisitor)
    }
}

struct FileMetadataNHRDeVisitor(FileMetadataSerde);

impl<'de> de::Visitor<'de> for FileMetadataNHRDeVisitor {
    type Value = FileMetadata;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "file metadata")
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error> where A: de::EnumAccess<'de> {
        let (variant_index, variant) = data.variant::<u32>()?;
        match variant_index {
            20 => Ok(variant.newtype_variant::<FileMetadataNHRSurrogate20>()?.into()),
            300 => Ok(variant.newtype_variant_seed(FileMetadataNHRSurrogate300Serde { code_page: self.0.code_page })?.into()),
            n => Err(A::Error::invalid_value(Unexpected::Unsigned(n as u64), &self))
        }
    }
}

impl<'de> DeserializeSeed<'de> for FileMetadataSerde {
    type Value = FileMetadata;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if deserializer.is_human_readable() {
            FileMetadataHRSurrogate::deserialize(deserializer).map(FileMetadata::from)
        } else {
            deserializer.deserialize_enum(
                name_of!(type FileMetadata),
                &["FileMetadata20", "FileMetadata300"],
                FileMetadataNHRDeVisitor(self)
            )
        }
    }
}

struct FileMetadataNHRSurrogate300DeVisitor(FileMetadataNHRSurrogate300Serde);

impl<'de> de::Visitor<'de> for FileMetadataNHRSurrogate300DeVisitor {
    type Value = FileMetadataNHRSurrogate300;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "file metadata")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut version = None;
        let mut file_type = None;
        let mut author = None;
        let mut description = None;
        let mut records = None;
        while let Some(field) = map.next_key()? {
            match field {
                FileMetadataField::Version => if version.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(FILE_METADATA_VERSION_FIELD));
                },
                FileMetadataField::Type => if file_type.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(FILE_METADATA_TYPE_FIELD));
                },
                FileMetadataField::Author => 
                    if author.replace(map.next_value_seed(StringSerde {
                        code_page: self.0.code_page, len: Some(32)
                    })?).is_some() {
                        return Err(A::Error::duplicate_field(FILE_METADATA_AUTHOR_FIELD));
                    },
                FileMetadataField::Description =>
                    if description.replace(
                        map.next_value_seed(StringListSerde {
                            code_page: self.0.code_page, separator: Newline::Dos.as_str(), len: Some(256)
                        })?
                    ).is_some() {
                        return Err(A::Error::duplicate_field(FILE_METADATA_DESCRIPTION_FIELD));
                    },
                FileMetadataField::Records => if records.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(FILE_METADATA_RECORDS_FIELD));
                },
            }
        }
        let version = version.ok_or_else(|| A::Error::missing_field(FILE_METADATA_VERSION_FIELD))?;
        let file_type = file_type.ok_or_else(|| A::Error::missing_field(FILE_METADATA_TYPE_FIELD))?;
        let author = author.ok_or_else(|| A::Error::missing_field(FILE_METADATA_AUTHOR_FIELD))?;
        let description = description.ok_or_else(|| A::Error::missing_field(FILE_METADATA_DESCRIPTION_FIELD))?;
        let records = records.ok_or_else(|| A::Error::missing_field(FILE_METADATA_RECORDS_FIELD))?;
        Ok(FileMetadataNHRSurrogate300 { version, file_type, author, description, records })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let version = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let file_type = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        let author = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(2, &self))?;
        let description = seq.next_element_seed(StringListSerde {
            code_page: self.0.code_page, separator: Newline::Dos.as_str(), len: Some(256)
        })?.ok_or_else(|| A::Error::invalid_length(3, &self))?;
        let records = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(4, &self))?;
        Ok(FileMetadataNHRSurrogate300 { version, file_type, author, description, records })
    }
}

impl<'de> DeserializeSeed<'de> for FileMetadataNHRSurrogate300Serde {
    type Value = FileMetadataNHRSurrogate300;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type FileMetadataNHRSurrogate300), FILE_METADATA_FIELDS, FileMetadataNHRSurrogate300DeVisitor(self))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Effect {
    #[serde(with="effect_index_option_i16")]
    pub index: Either<Option<i16>, EffectIndex>,
    #[serde(with="skill_option_i8")]
    pub skill: Either<Option<i8>, Skill>,
    #[serde(with="attribute_option_i8")]
    pub attribute: Either<Option<i8>, Attribute>,
    pub range: EffectRange,
    pub area: i32,
    pub duration: i32,
    pub magnitude_min: i32,
    pub magnitude_max: i32
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct NpcState {
    pub disposition: i16,
    pub reputation: i16,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct NpcStats {
    pub attributes: Attributes<u8>,
    pub skills: Skills<u8>,
    pub faction: u8,
    pub health: i16,
    pub magicka: i16,
    pub fatigue: i16,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
#[serde(rename="NpcStatsOption")]
enum NpcStatsOptionHRSurrogate {
    None(u16),
    Some(NpcStats)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Npc {
    pub level: u16,
    pub disposition: i8,
    pub reputation: i8,
    pub rank: i8,
    pub gold: i32,
    pub padding: u8,
    pub stats: Either<u16, NpcStats>
}

#[derive(Serialize, Deserialize)]
struct NpcHRSurrogate {
    pub level: u16,
    pub disposition: i8,
    pub reputation: i8,
    pub rank: i8,
    pub gold: i32,
    pub padding: u8,
    pub stats: NpcStatsOptionHRSurrogate
}

impl From<Npc> for NpcHRSurrogate {
    fn from(t: Npc) -> Self {
        NpcHRSurrogate {
            level: t.level, disposition: t.disposition, reputation:t.reputation,
            rank: t.rank, gold: t.gold, padding: t.padding,
            stats: t.stats.either(NpcStatsOptionHRSurrogate::None, NpcStatsOptionHRSurrogate::Some)
        }
    }
}

impl From<NpcHRSurrogate> for Npc {
    fn from(t: NpcHRSurrogate) -> Self {
        let stats = match t.stats {
            NpcStatsOptionHRSurrogate::None(x) => Left(x),
            NpcStatsOptionHRSurrogate::Some(x) => Right(x)
        };
        Npc {
            level: t.level, disposition: t.disposition, reputation:t.reputation,
            rank: t.rank, gold: t.gold, padding: t.padding,
            stats
        }
    }
}

impl Serialize for Npc {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if serializer.is_human_readable() {
            NpcHRSurrogate::from(self.clone()).serialize(serializer)
        } else {
            let surrogate: Either<NpcNHRSurrogate12, NpcNHRSurrogate52> = self.clone().into();
            match surrogate {
                Left(npc12) => serializer.serialize_newtype_variant(
                    name_of!(type Npc), 12, "Npc12", &npc12
                ),
                Right(npc52) => serializer.serialize_newtype_variant(
                    name_of!(type Npc), 52, "Npc52", &npc52
                ),
            }
        }
    }
}

struct NpcNHRDeserializer;

impl<'de> de::Visitor<'de> for NpcNHRDeserializer {
    type Value = Npc;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "NPC")
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error> where A: de::EnumAccess<'de> {
        let (variant_index, variant) = data.variant::<u32>()?;
        match variant_index {
            12 => Ok(variant.newtype_variant::<NpcNHRSurrogate12>()?.into()),
            52 => Ok(variant.newtype_variant::<NpcNHRSurrogate52>()?.into()),
            n => Err(A::Error::invalid_value(Unexpected::Unsigned(n as u64), &self))
        }
    }
}

impl<'de> Deserialize<'de> for Npc {
    fn deserialize<D>(deserializer: D) -> Result<Npc, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            NpcHRSurrogate::deserialize(deserializer).map(Npc::from)
        } else {
            deserializer.deserialize_enum(name_of!(type Npc), &["Npc12", "Npc52"], NpcNHRDeserializer)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename="Npc12")]
struct NpcNHRSurrogate12 {
    pub level: u16,
    pub disposition: i8,
    pub reputation: i8,
    pub rank: i8,
    pub padding_8: u8,
    pub padding_16: u16,
    pub gold: i32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename="Npc52")]
struct NpcNHRSurrogate52 {
    pub level: u16,
    pub stats: NpcStats,
    pub disposition: i8,
    pub reputation: i8,
    pub rank: i8,
    pub padding: u8,
    pub gold: i32,
}

impl From<Npc> for Either<NpcNHRSurrogate12, NpcNHRSurrogate52> {
    fn from(npc: Npc) -> Either<NpcNHRSurrogate12, NpcNHRSurrogate52> {
        match npc.stats {
            Right(stats) => Right(NpcNHRSurrogate52 {
                level: npc.level, disposition: npc.disposition,
                reputation: npc.reputation, rank: npc.rank,
                padding: npc.padding,
                gold: npc.gold,
                stats
            }),
            Left(padding_16) => Left(NpcNHRSurrogate12 {
                level: npc.level, disposition: npc.disposition,
                reputation: npc.reputation, rank: npc.rank,
                padding_8: npc.padding, padding_16,
                gold: npc.gold
            })
        }
    }
}

impl From<NpcNHRSurrogate52> for Npc {
    fn from(npc: NpcNHRSurrogate52) -> Npc {
        Npc {
            level: npc.level, disposition: npc.disposition, reputation: npc.reputation,
            rank: npc.rank, gold: npc.gold, padding: npc.padding,
            stats: Right(npc.stats)
        }
    }
}

impl From<NpcNHRSurrogate12> for Npc {
    fn from(npc: NpcNHRSurrogate12) -> Npc {
        Npc {
            level: npc.level, disposition: npc.disposition, reputation: npc.reputation,
            rank: npc.rank, gold: npc.gold, padding: npc.padding_8,
            stats: Left(npc.padding_16)
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Item {
    pub count: i32,
    pub item_id: String,
}

const ITEM_COUNT_FIELD: &str = name_of!(count in Item);
const ITEM_ITEM_ID_FIELD: &str = name_of!(item_id in Item);

const ITEM_FIELDS: &[&str] = &[
    ITEM_COUNT_FIELD,
    ITEM_ITEM_ID_FIELD,
];

#[derive(Clone)]
pub struct ItemSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for ItemSerde {
    type Value = Item;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type Item), 2)?;
        serializer.serialize_field(ITEM_COUNT_FIELD, &value.count)?;
        serializer.serialize_field(
            ITEM_ITEM_ID_FIELD,
            &ValueWithSeed(value.item_id.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.end()
    }
}

enum ItemField {
    Count,
    ItemId,
}

struct ItemFieldDeVisitor;

impl<'de> de::Visitor<'de> for ItemFieldDeVisitor {
    type Value = ItemField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "item field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            ITEM_COUNT_FIELD => Ok(ItemField::Count),
            ITEM_ITEM_ID_FIELD => Ok(ItemField::ItemId),
            x => Err(E::unknown_field(x, ITEM_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for ItemField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(ItemFieldDeVisitor)
    }
}

struct ItemDeVisitor(ItemSerde);

impl<'de> de::Visitor<'de> for ItemDeVisitor {
    type Value = Item;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "item")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut count = None;
        let mut item_id = None;
        while let Some(field) = map.next_key()? {
            match field {
                ItemField::Count => if count.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(ITEM_COUNT_FIELD));
                },
                ItemField::ItemId => 
                    if item_id.replace(map.next_value_seed(StringSerde {
                        code_page: self.0.code_page, len: Some(32)
                    })?).is_some() {
                        return Err(A::Error::duplicate_field(ITEM_ITEM_ID_FIELD));
                    },
            }
        }
        let count = count.ok_or_else(|| A::Error::missing_field(ITEM_COUNT_FIELD))?;
        let item_id = item_id.ok_or_else(|| A::Error::missing_field(ITEM_ITEM_ID_FIELD))?;
        Ok(Item { count, item_id })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let count = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let item_id = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        Ok(Item { count, item_id })
    }
}

impl<'de> DeserializeSeed<'de> for ItemSerde {
    type Value = Item;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type Item), ITEM_FIELDS, ItemDeVisitor(self))
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum SpellType {
        Spell = 0,
        Ability = 1,
        Blight = 2,
        Disease = 3,
        Curse = 4,
        Power = 5
    }
}

enum_serde!(SpellType, "spell type", as u32, Unsigned, u64);

bitflags_ext! {
    pub struct SpellFlags: u32 {
        AUTO_CALCULATE_COST = 1,
        PC_START = 2,
        ALWAYS_SUCCEEDS = 4,
    }
}

enum_serde!(SpellFlags, "spell flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Spell {
    #[serde(rename="type")]
    pub spell_type: SpellType,
    pub cost: u32,
    pub flags: SpellFlags
}

bitflags_ext! {
    pub struct Services: u32 {
        WEAPON = 0x00000001,
        ARMOR = 0x00000002,
        CLOTHING = 0x00000004,
        BOOKS = 0x00000008,
        INGREDIENTS = 0x00000010,
        PICKS = 0x00000020,
        PROBES = 0x00000040,
        LIGHTS = 0x00000080,
        APPARATUS = 0x00000100,
        REPAIR_ITEMS = 0x00000200,
        MISCELLANEOUS  = 0x00000400,
        SPELLS = 0x00000800,
        MAGIC_ITEMS = 0x00001000,
        POTIONS = 0x00002000,
        TRAINING = 0x00004000,
        SPELLMAKING = 0x00008000,
        ENCHANTING = 0x00010000,
        REPAIR = 0x00020000,
        _40000 = 0x40000,
        _80000 = 0x00080000,
        _100000 = 0x100000,
        _200000 = 0x00200000,
        _400000 = 0x00400000,
        _800000 = 0x00800000,
        _1000000 = 0x01000000,
        _2000000 = 0x2000000,
        _4000000 = 0x4000000,
        _8000000 = 0x8000000,
        _10000000 = 0x10000000,
        _20000000 = 0x20000000,
        _40000000 = 0x40000000,
        _80000000 = 0x80000000,
    }
}

enum_serde!(Services, "services", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Ai {
    pub hello: u16,
    pub fight: u8,
    pub flee: u8,
    pub alarm: u8,
    pub padding_8: u8,
    pub padding_16: u16,
    pub services: Services
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct AiWander {
    pub distance: u16,
    pub duration: u16,
    pub time_of_day: u8,
    pub idle: [u8; 8],
    #[serde(with="bool_u8")]
    pub repeat: bool,
}

bitflags_ext! {
    pub struct AiTravelFlags: u32 {
        RESET = 0x000100,
        _1 = 0x000001,
        _800 = 0x000800,
        _1000 = 0x001000,
        _4000 = 0x004000,
        _10000 = 0x010000,
        _20000 = 0x020000,
        _40000 = 0x040000,
        _400000 = 0x400000
    }
}

enum_serde!(AiTravelFlags, "AI travel flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct AiTravel {
    pub pos: Pos,
    pub flags: AiTravelFlags
}

bitflags_ext! {
    pub struct AiTargetFlags: u8 {
        _1 = 0x01,
        _2 = 0x02,
        _4 = 0x04,
        _8 = 0x08
    }
}

enum_serde!(AiTargetFlags, "AI target flags", u8, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Educe)]
#[educe(Eq, PartialEq)]
pub struct AiTarget {
    pub pos: Pos,
    pub duration: u16,
    pub actor_id: String,
    pub reset: bool,
    pub flags: AiTargetFlags
}

const AI_TARGET_POS_FIELD: &str = name_of!(pos in AiTarget);
const AI_TARGET_DURATION_FIELD: &str = name_of!(duration in AiTarget);
const AI_TARGET_ACTOR_ID_FIELD: &str = name_of!(actor_id in AiTarget);
const AI_TARGET_RESET_FIELD: &str = name_of!(reset in AiTarget);
const AI_TARGET_FLAGS_FIELD: &str = name_of!(flags in AiTarget);

const AI_TARGET_FIELDS: &[&str] = &[
    AI_TARGET_POS_FIELD,
    AI_TARGET_DURATION_FIELD,
    AI_TARGET_ACTOR_ID_FIELD,
    AI_TARGET_RESET_FIELD,
    AI_TARGET_FLAGS_FIELD,
];

#[derive(Clone)]
pub struct AiTargetSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for AiTargetSerde {
    type Value = AiTarget;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type AiTarget), 5)?;
        serializer.serialize_field(AI_TARGET_POS_FIELD, &value.pos)?;
        serializer.serialize_field(AI_TARGET_DURATION_FIELD, &value.duration)?;
        serializer.serialize_field(
            AI_TARGET_ACTOR_ID_FIELD,
            &ValueWithSeed(value.actor_id.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.serialize_field(AI_TARGET_RESET_FIELD, &ValueWithSeed(&value.reset, BoolU8Serde))?;
        serializer.serialize_field(AI_TARGET_FLAGS_FIELD, &value.flags)?;
        serializer.end()
    }
}

enum AiTargetField {
    Pos,
    Duration,
    ActorId,
    Reset,
    Flags
}

struct AiTargetFieldDeVisitor;

impl<'de> de::Visitor<'de> for AiTargetFieldDeVisitor {
    type Value = AiTargetField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AI target field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            AI_TARGET_POS_FIELD => Ok(AiTargetField::Pos),
            AI_TARGET_DURATION_FIELD => Ok(AiTargetField::Duration),
            AI_TARGET_ACTOR_ID_FIELD => Ok(AiTargetField::ActorId),
            AI_TARGET_RESET_FIELD => Ok(AiTargetField::Reset),
            AI_TARGET_FLAGS_FIELD => Ok(AiTargetField::Flags),
            x => Err(E::unknown_field(x, AI_TARGET_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for AiTargetField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(AiTargetFieldDeVisitor)
    }
}

struct AiTargetDeVisitor(AiTargetSerde);

impl<'de> de::Visitor<'de> for AiTargetDeVisitor {
    type Value = AiTarget;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AI target")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut pos = None;
        let mut duration = None;
        let mut actor_id = None;
        let mut reset = None;
        let mut flags = None;
        while let Some(field) = map.next_key()? {
            match field {
                AiTargetField::Pos => if pos.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(AI_TARGET_POS_FIELD));
                },
                AiTargetField::Duration => if duration.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(AI_TARGET_DURATION_FIELD));
                },
                AiTargetField::ActorId =>
                    if actor_id.replace(map.next_value_seed(
                        StringSerde { code_page: self.0.code_page, len: Some(32) }
                    )?).is_some() {
                        return Err(A::Error::duplicate_field(AI_TARGET_ACTOR_ID_FIELD));
                    },
                AiTargetField::Reset => if reset.replace(map.next_value_seed(BoolU8Serde)?).is_some() {
                    return Err(A::Error::duplicate_field(AI_TARGET_RESET_FIELD));
                },
                AiTargetField::Flags => if flags.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(AI_TARGET_FLAGS_FIELD));
                },
            }
        }
        let pos = pos.ok_or_else(|| A::Error::missing_field(AI_TARGET_POS_FIELD))?;
        let duration = duration.ok_or_else(|| A::Error::missing_field(AI_TARGET_DURATION_FIELD))?;
        let actor_id = actor_id.ok_or_else(|| A::Error::missing_field(AI_TARGET_ACTOR_ID_FIELD))?;
        let reset = reset.ok_or_else(|| A::Error::missing_field(AI_TARGET_RESET_FIELD))?;
        let flags = flags.ok_or_else(|| A::Error::missing_field(AI_TARGET_FLAGS_FIELD))?;
        Ok(AiTarget { pos, duration, actor_id, reset, flags })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let pos = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let duration = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        let actor_id = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(2, &self))?;
        let reset = seq.next_element_seed(BoolU8Serde)?
            .ok_or_else(|| A::Error::invalid_length(3, &self))?;
        let flags = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(4, &self))?;
        Ok(AiTarget { pos, duration, actor_id, reset, flags })
    }
}

impl<'de> DeserializeSeed<'de> for AiTargetSerde {
    type Value = AiTarget;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type AiTarget), AI_TARGET_FIELDS, AiTargetDeVisitor(self))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiActivate {
    pub object_id: String,
    pub reset: bool
}

const AI_ACTIVATE_OBJECT_ID_FIELD: &str = name_of!(object_id in AiActivate);
const AI_ACTIVATE_RESET_FIELD: &str = name_of!(reset in AiActivate);

const AI_ACTIVATE_FIELDS: &[&str] = &[
    AI_ACTIVATE_OBJECT_ID_FIELD,
    AI_ACTIVATE_RESET_FIELD,
];

#[derive(Clone)]
pub struct AiActivateSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for AiActivateSerde {
    type Value = AiActivate;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type AiActivate), 2)?;
        serializer.serialize_field(
            AI_ACTIVATE_OBJECT_ID_FIELD,
            &ValueWithSeed(value.object_id.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.serialize_field(AI_ACTIVATE_RESET_FIELD, &ValueWithSeed(&value.reset, BoolU8Serde))?;
        serializer.end()
    }
}

enum AiActivateField {
    ObjectId,
    Reset,
}

struct AiActivateFieldDeVisitor;

impl<'de> de::Visitor<'de> for AiActivateFieldDeVisitor {
    type Value = AiActivateField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AI activate field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            AI_ACTIVATE_OBJECT_ID_FIELD => Ok(AiActivateField::ObjectId),
            AI_ACTIVATE_RESET_FIELD => Ok(AiActivateField::Reset),
            x => Err(E::unknown_field(x, AI_ACTIVATE_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for AiActivateField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(AiActivateFieldDeVisitor)
    }
}

struct AiActivateDeVisitor(AiActivateSerde);

impl<'de> de::Visitor<'de> for AiActivateDeVisitor {
    type Value = AiActivate;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AI activate")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut object_id = None;
        let mut reset = None;
        while let Some(field) = map.next_key()? {
            match field {
                AiActivateField::ObjectId =>
                    if object_id.replace(map.next_value_seed(
                        StringSerde { code_page: self.0.code_page, len: Some(32) }
                    )?).is_some() {
                        return Err(A::Error::duplicate_field(AI_ACTIVATE_OBJECT_ID_FIELD));
                    },
                AiActivateField::Reset => if reset.replace(map.next_value_seed(BoolU8Serde)?).is_some() {
                    return Err(A::Error::duplicate_field(AI_ACTIVATE_RESET_FIELD));
                },
            }
        }
        let object_id = object_id.ok_or_else(|| A::Error::missing_field(AI_ACTIVATE_OBJECT_ID_FIELD))?;
        let reset = reset.ok_or_else(|| A::Error::missing_field(AI_ACTIVATE_RESET_FIELD))?;
        Ok(AiActivate { object_id, reset })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let object_id = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let reset = seq.next_element_seed(BoolU8Serde)?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        Ok(AiActivate { object_id, reset })
    }
}

impl<'de> DeserializeSeed<'de> for AiActivateSerde {
    type Value = AiActivate;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type AiActivate), AI_ACTIVATE_FIELDS, AiActivateDeVisitor(self))
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum Blood {
        Default = 0,
        Skeleton = 4,
        MetalSparks = 8,
        _12 = 12,
    }
}

enum_serde!(Blood, "blood", as u8, Unsigned, u64);

bitflags_ext! {
    pub struct NpcFlags: u8 {
        FEMALE = 0x01,
        ESSENTIAL = 0x02,
        RESPAWN = 0x04,
        _8 = 0x08,
        AUTO_CALCULATE_STATS = 0x10
    }
}

enum_serde!(NpcFlags, "NPC flags", u8, bits(), try from_bits, Unsigned, u64, ^0x08);

bitflags_ext! {
    pub struct CreatureFlags: u8 {
        BIPED = 0x01,
        RESPAWN = 0x02,
        WEAPON_AND_SHIELD = 0x04,
        SWIMS = 0x10,
        FLIES = 0x20,
        WALKS = 0x40,
        ESSENTIAL = 0x80
    }
}

enum_serde!(CreatureFlags, "creature flags", u8, bits(), try from_bits, Unsigned, u64, ^0x08);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct FlagsAndBlood<Flags> {
    pub flags: Flags,
    pub blood: Blood,
    pub padding: u16,
}

bitflags_ext! {
    pub struct BookFlags: u32 {
        SCROLL = 0x01,
        _10 = 0x10
    }
}

enum_serde!(BookFlags, "book flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Book {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    pub flags: BookFlags,
    #[serde(with="skill_option_i32")]
    pub skill: Either<Option<i32>, Skill>,
    pub enchantment: u32
}

bitflags_ext! {
    pub struct ContainerFlags: u32 {
        ORGANIC = 0x01,
        RESPAWN = 0x02
    }
}

enum_serde!(ContainerFlags, "container flags", u32, bits(), try from_bits, Unsigned, u64, ^0x08);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum CreatureType {
        Creature = 0,
        Daedra  = 1,
        Undead = 2,
        Humanoid = 3
    }
}

enum_serde!(CreatureType, "creature type", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Creature {
    #[serde(rename="type")]
    pub creature_type: CreatureType,
    pub level: u32,
    pub attributes: Attributes<u32>,
    pub health: u32,
    pub magicka: u32,
    pub fatigue: u32,
    pub soul: u32,
    pub combat: u32,
    pub magic: u32,
    pub stealth: u32,
    pub attack_1_min: u32,
    pub attack_1_max: u32,
    pub attack_2_min: u32,
    pub attack_2_max: u32,
    pub attack_3_min: u32,
    pub attack_3_max: u32,
    pub gold: u32,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum Attribute {
        Strength = 0,
        Intelligence = 1,
        Willpower = 2,
        Agility = 3,
        Speed = 4,
        Endurance = 5,
        Personality = 6,
        Luck = 7,
    }
}

enum_serde!(Attribute, "attribute", as u32, Unsigned, u64);

mod attribute_option_i8 {
    use either::{Either};
    use crate::field::Attribute;
    use crate::serde_helpers::*;
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i8>, Attribute>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "attribute",
            none: -1,
            from: |x| x.try_into().ok().and_then(Attribute::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i8>, Attribute>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "attribute",
            none: -1,
            from: |x| x.try_into().ok().and_then(Attribute::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

mod attribute_option_i32 {
    use either::{Either};
    use crate::field::Attribute;
    use crate::serde_helpers::*;
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i32>, Attribute>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "attribute",
            none: -1,
            from: |x| x.try_into().ok().and_then(Attribute::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i32>, Attribute>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "attribute",
            none: -1,
            from: |x| x.try_into().ok().and_then(Attribute::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Attributes<T> {
    pub strength: T,
    pub intelligence: T,
    pub willpower: T,
    pub agility: T,
    pub speed: T,
    pub endurance: T,
    pub personality: T,
    pub luck: T,
}

impl<T> Index<Attribute> for Attributes<T> {
    type Output = T;

    fn index(&self, index: Attribute) -> &Self::Output {
        match index {
            Attribute::Strength => &self.strength,
            Attribute::Intelligence => &self.intelligence,
            Attribute::Willpower => &self.willpower,
            Attribute::Agility => &self.agility,
            Attribute::Speed => &self.speed,
            Attribute::Endurance => &self.endurance,
            Attribute::Personality => &self.personality,
            Attribute::Luck => &self.luck,
        }
    }
}

impl<T> IndexMut<Attribute> for Attributes<T> {
    fn index_mut(&mut self, index: Attribute) -> &mut Self::Output {
        match index {
            Attribute::Strength => &mut self.strength,
            Attribute::Intelligence => &mut self.intelligence,
            Attribute::Willpower => &mut self.willpower,
            Attribute::Agility => &mut self.agility,
            Attribute::Speed => &mut self.speed,
            Attribute::Endurance => &mut self.endurance,
            Attribute::Personality => &mut self.personality,
            Attribute::Luck => &mut self.luck,
        }
    }
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
pub struct EffectArg {
    pub dword: u32
}

impl EffectArg {
    pub const STRENGTH_BLOCK_00000000: EffectArg = EffectArg { dword: 0x00000000 };
    pub const ARMORER_INTELLIGENCE_00000001: EffectArg = EffectArg { dword: 0x00000001 };
    pub const MEDIUM_ARMOR_WILLPOWER_00000002: EffectArg = EffectArg { dword: 0x00000002 };
    pub const HEAVY_ARMOR_AGILITY_00000003: EffectArg = EffectArg { dword: 0x00000003 };
    pub const BLUNT_WEAPON_SPEED_00000004: EffectArg = EffectArg { dword: 0x00000004 };
    pub const LONG_BLADE_ENDURANCE_00000005: EffectArg = EffectArg { dword: 0x00000005 };
    pub const AXE_PERSONALITY_00000006: EffectArg = EffectArg { dword: 0x00000006 };
    pub const SPEAR_LUCK_00000007: EffectArg = EffectArg { dword: 0x00000007 };
    pub const ATHLETICS_00000008: EffectArg = EffectArg { dword: 0x00000008 };
    pub const ENCHANT_00000009: EffectArg = EffectArg { dword: 0x00000009 };
    pub const DESTRUCTION_0000000A: EffectArg = EffectArg { dword: 0x0000000A };
    pub const ALTERATION_0000000B: EffectArg = EffectArg { dword: 0x0000000B };
    pub const ILLUSION_0000000C: EffectArg = EffectArg { dword: 0x0000000C };
    pub const CONJURATION_0000000D: EffectArg = EffectArg { dword: 0x0000000D };
    pub const MYSTICISM_0000000E: EffectArg = EffectArg { dword: 0x0000000E };
    pub const RESTORATION_0000000F: EffectArg = EffectArg { dword: 0x0000000F };
    pub const ALCHEMY_00000010: EffectArg = EffectArg { dword: 0x00000010 };
    pub const UNARMORED_00000011: EffectArg = EffectArg { dword: 0x00000011 };
    pub const SECURITY_00000012: EffectArg = EffectArg { dword: 0x00000012 };
    pub const SNEAK_00000013: EffectArg = EffectArg { dword: 0x00000013 };
    pub const ACROBATICS_00000014: EffectArg = EffectArg { dword: 0x00000014 };
    pub const LIGHT_ARMOR_00000015: EffectArg = EffectArg { dword: 0x00000015 };
    pub const SHORT_BLADE_00000016: EffectArg = EffectArg { dword: 0x00000016 };
    pub const MARKSMAN_00000017: EffectArg = EffectArg { dword: 0x00000017 };
    pub const MERCANTILE_00000018: EffectArg = EffectArg { dword: 0x00000018 };
    pub const SPEECHCRAFT_00000019: EffectArg = EffectArg { dword: 0x00000019 };
    pub const HAND_TO_HAND_0000001A: EffectArg = EffectArg { dword: 0x0000001A };
}

impl From<u32> for EffectArg {
    fn from(dword: u32) -> EffectArg {
        EffectArg { dword }
    }
}

impl Debug for EffectArg {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl Display for EffectArg {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self {
            EffectArg::STRENGTH_BLOCK_00000000 => write!(f, "STRENGTH_BLOCK_00000000"),
            EffectArg::ARMORER_INTELLIGENCE_00000001 => write!(f, "ARMORER_INTELLIGENCE_00000001"),
            EffectArg::MEDIUM_ARMOR_WILLPOWER_00000002 => write!(f, "MEDIUM_ARMOR_WILLPOWER_00000002"),
            EffectArg::HEAVY_ARMOR_AGILITY_00000003 => write!(f, "HEAVY_ARMOR_AGILITY_00000003"),
            EffectArg::BLUNT_WEAPON_SPEED_00000004 => write!(f, "BLUNT_WEAPON_SPEED_00000004"),
            EffectArg::LONG_BLADE_ENDURANCE_00000005 => write!(f, "LONG_BLADE_ENDURANCE_00000005"),
            EffectArg::AXE_PERSONALITY_00000006 => write!(f, "AXE_PERSONALITY_00000006"),
            EffectArg::SPEAR_LUCK_00000007 => write!(f, "SPEAR_LUCK_00000007"),
            EffectArg::ATHLETICS_00000008 => write!(f, "ATHLETICS_00000008"),
            EffectArg::ENCHANT_00000009 => write!(f, "ENCHANT_00000009"),
            EffectArg::DESTRUCTION_0000000A => write!(f, "DESTRUCTION_0000000A"),
            EffectArg::ALTERATION_0000000B => write!(f, "ALTERATION_0000000B"),
            EffectArg::ILLUSION_0000000C => write!(f, "ILLUSION_0000000C"),
            EffectArg::CONJURATION_0000000D => write!(f, "CONJURATION_0000000D"),
            EffectArg::MYSTICISM_0000000E => write!(f, "MYSTICISM_0000000E"),
            EffectArg::RESTORATION_0000000F => write!(f, "RESTORATION_0000000F"),
            EffectArg::ALCHEMY_00000010 => write!(f, "ALCHEMY_00000010"),
            EffectArg::UNARMORED_00000011 => write!(f, "UNARMORED_00000011"),
            EffectArg::SECURITY_00000012 => write!(f, "SECURITY_00000012"),
            EffectArg::SNEAK_00000013 => write!(f, "SNEAK_00000013"),
            EffectArg::ACROBATICS_00000014 => write!(f, "ACROBATICS_00000014"),
            EffectArg::LIGHT_ARMOR_00000015 => write!(f, "LIGHT_ARMOR_00000015"),
            EffectArg::SHORT_BLADE_00000016 => write!(f, "SHORT_BLADE_00000016"),
            EffectArg::MARKSMAN_00000017 => write!(f, "MARKSMAN_00000017"),
            EffectArg::MERCANTILE_00000018 => write!(f, "MERCANTILE_00000018"),
            EffectArg::SPEECHCRAFT_00000019 => write!(f, "SPEECHCRAFT_00000019"),
            EffectArg::HAND_TO_HAND_0000001A => write!(f, "HAND_TO_HAND_0000001A"),
            EffectArg { dword } => write!(f, "{dword:08X}"),
         }
    }
}

impl FromStr for EffectArg {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "STRENGTH_BLOCK_00000000" => EffectArg::STRENGTH_BLOCK_00000000,
            "STRENGTH" => EffectArg::STRENGTH_BLOCK_00000000,
            "BLOCK" => EffectArg::STRENGTH_BLOCK_00000000,
            "ARMORER_INTELLIGENCE_00000001" => EffectArg::ARMORER_INTELLIGENCE_00000001,
            "ARMORER" => EffectArg::ARMORER_INTELLIGENCE_00000001,
            "INTELLIGENCE" => EffectArg::ARMORER_INTELLIGENCE_00000001,
            "MEDIUM_ARMOR_WILLPOWER_00000002" => EffectArg::MEDIUM_ARMOR_WILLPOWER_00000002,
            "MEDIUM_ARMOR" => EffectArg::MEDIUM_ARMOR_WILLPOWER_00000002,
            "WILLPOWER" => EffectArg::MEDIUM_ARMOR_WILLPOWER_00000002,
            "HEAVY_ARMOR_AGILITY_00000003" => EffectArg::HEAVY_ARMOR_AGILITY_00000003,
            "HEAVY_ARMOR" => EffectArg::HEAVY_ARMOR_AGILITY_00000003,
            "AGILITY" => EffectArg::HEAVY_ARMOR_AGILITY_00000003,
            "BLUNT_WEAPON_SPEED_00000004" => EffectArg::BLUNT_WEAPON_SPEED_00000004,
            "BLUNT_WEAPON" => EffectArg::BLUNT_WEAPON_SPEED_00000004,
            "SPEED" => EffectArg::BLUNT_WEAPON_SPEED_00000004,
            "LONG_BLADE_ENDURANCE_00000005" => EffectArg::LONG_BLADE_ENDURANCE_00000005,
            "LONG_BLADE" => EffectArg::LONG_BLADE_ENDURANCE_00000005,
            "ENDURANCE" => EffectArg::LONG_BLADE_ENDURANCE_00000005,
            "AXE_PERSONALITY_00000006" => EffectArg::AXE_PERSONALITY_00000006,
            "AXE" => EffectArg::AXE_PERSONALITY_00000006,
            "PERSONALITY" => EffectArg::AXE_PERSONALITY_00000006,
            "SPEAR_LUCK_00000007" => EffectArg::SPEAR_LUCK_00000007,
            "SPEAR" => EffectArg::SPEAR_LUCK_00000007,
            "LUCK" => EffectArg::SPEAR_LUCK_00000007,
            "ATHLETICS_00000008" => EffectArg::ATHLETICS_00000008,
            "ATHLETICS" => EffectArg::ATHLETICS_00000008,
            "ENCHANT_00000009" => EffectArg::ENCHANT_00000009,
            "ENCHANT" => EffectArg::ENCHANT_00000009,
            "DESTRUCTION_0000000A" => EffectArg::DESTRUCTION_0000000A,
            "DESTRUCTION" => EffectArg::DESTRUCTION_0000000A,
            "ALTERATION_0000000B" => EffectArg::ALTERATION_0000000B,
            "ALTERATION" => EffectArg::ALTERATION_0000000B,
            "ILLUSION_0000000C" => EffectArg::ILLUSION_0000000C,
            "ILLUSION" => EffectArg::ILLUSION_0000000C,
            "CONJURATION_0000000D" => EffectArg::CONJURATION_0000000D,
            "CONJURATION" => EffectArg::CONJURATION_0000000D,
            "MYSTICISM_0000000E" => EffectArg::MYSTICISM_0000000E,
            "MYSTICISM" => EffectArg::MYSTICISM_0000000E,
            "RESTORATION_0000000F" => EffectArg::RESTORATION_0000000F,
            "RESTORATION" => EffectArg::RESTORATION_0000000F,
            "ALCHEMY_00000010" => EffectArg::ALCHEMY_00000010,
            "ALCHEMY" => EffectArg::ALCHEMY_00000010,
            "UNARMORED_00000011" => EffectArg::UNARMORED_00000011,
            "UNARMORED" => EffectArg::UNARMORED_00000011,
            "SECURITY_00000012" => EffectArg::SECURITY_00000012,
            "SECURITY" => EffectArg::SECURITY_00000012,
            "SNEAK_00000013" => EffectArg::SNEAK_00000013,
            "SNEAK" => EffectArg::SNEAK_00000013,
            "ACROBATICS_00000014" => EffectArg::ACROBATICS_00000014,
            "ACROBATICS" => EffectArg::ACROBATICS_00000014,
            "LIGHT_ARMOR_00000015" => EffectArg::LIGHT_ARMOR_00000015,
            "LIGHT_ARMOR" => EffectArg::LIGHT_ARMOR_00000015,
            "SHORT_BLADE_00000016" => EffectArg::SHORT_BLADE_00000016,
            "SHORT_BLADE" => EffectArg::SHORT_BLADE_00000016,
            "MARKSMAN_00000017" => EffectArg::MARKSMAN_00000017,
            "MARKSMAN" => EffectArg::MARKSMAN_00000017,
            "MERCANTILE_00000018" => EffectArg::MERCANTILE_00000018,
            "MERCANTILE" => EffectArg::MERCANTILE_00000018,
            "SPEECHCRAFT_00000019" => EffectArg::SPEECHCRAFT_00000019,
            "SPEECHCRAFT" => EffectArg::SPEECHCRAFT_00000019,
            "HAND_TO_HAND_0000001A" => EffectArg::HAND_TO_HAND_0000001A,
            "HAND_TO_HAND" => EffectArg::HAND_TO_HAND_0000001A,
            s => EffectArg { dword: u32::from_str_radix(s, 16).map_err(|_| ())? },
        })
    }
}

enum_serde!(EffectArg, "effect arg", u32, dword, from);

impl From<Skill> for EffectArg {
    fn from(skill: Skill) -> EffectArg {
        EffectArg::from(skill as u32)
    }
}

impl From<Attribute> for EffectArg {
    fn from(attribute: Attribute) -> EffectArg {
        EffectArg::from(attribute as u32)
    }
}

impl TryFrom<EffectArg> for Skill {
    type Error = ();

    fn try_from(arg: EffectArg) -> Result<Skill, ()> {
        Skill::n(arg.dword).ok_or(())
    }
}

impl TryFrom<EffectArg> for Attribute {
    type Error = ();

    fn try_from(arg: EffectArg) -> Result<Attribute, ()> {
        Attribute::n(arg.dword).ok_or(())
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum Skill {
        Block = 0,
        Armorer = 1,
        MediumArmor = 2,
        HeavyArmor = 3,
        BluntWeapon = 4,
        LongBlade = 5,
        Axe = 6,
        Spear = 7,
        Athletics = 8,
        Enchant = 9,
        Destruction = 10,
        Alteration = 11,
        Illusion = 12,
        Conjuration = 13,
        Mysticism = 14,
        Restoration = 15,
        Alchemy = 16,
        Unarmored = 17,
        Security = 18,
        Sneak = 19,
        Acrobatics = 20,
        LightArmor = 21,
        ShortBlade = 22,
        Marksman = 23,
        Mercantile = 24,
        Speechcraft = 25,
        HandToHand = 26
    }
}

enum_serde!(Skill, "skill", as u32, Unsigned, u64);

mod skill_option_i32 {
    use crate::field::Skill;
    use crate::serde_helpers::*;
    use either::{Either};
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i32>, Skill>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "skill",
            none:-1,
            from: |x| x.try_into().ok().and_then(Skill::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i32>, Skill>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "skill",
            none:-1,
            from: |x| x.try_into().ok().and_then(Skill::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

mod skill_option_i8 {
    use crate::field::Skill;
    use crate::serde_helpers::*;
    use either::{Either};
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i8>, Skill>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "skill",
            none: -1,
            from: |x| x.try_into().ok().and_then(Skill::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i8>, Skill>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "skill",
            none: -1,
            from: |x| x.try_into().ok().and_then(Skill::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Skills<T> {
    pub block: T,
    pub armorer: T,
    pub medium_armor: T,
    pub heavy_armor: T,
    pub blunt_weapon: T,
    pub long_blade: T,
    pub axe: T,
    pub spear: T,
    pub athletics: T,
    pub enchant: T,
    pub destruction: T,
    pub alteration: T,
    pub illusion: T,
    pub conjuration: T,
    pub mysticism: T,
    pub restoration: T,
    pub alchemy: T,
    pub unarmored: T,
    pub security: T,
    pub sneak: T,
    pub acrobatics: T,
    pub light_armor: T,
    pub short_blade: T,
    pub marksman: T,
    pub mercantile: T,
    pub speechcraft: T,
    pub hand_to_hand: T,
}

impl<T> Index<Skill> for Skills<T> {
    type Output = T;

    fn index(&self, index: Skill) -> &Self::Output {
        match index {
            Skill::Block => &self.block,
            Skill::Armorer => &self.armorer,
            Skill::MediumArmor => &self.medium_armor,
            Skill::HeavyArmor => &self.heavy_armor,
            Skill::BluntWeapon => &self.blunt_weapon,
            Skill::LongBlade => &self.long_blade,
            Skill::Axe => &self.axe,
            Skill::Spear => &self.spear,
            Skill::Athletics => &self.athletics,
            Skill::Enchant => &self.enchant,
            Skill::Destruction => &self.destruction,
            Skill::Alteration => &self.alteration,
            Skill::Illusion => &self.illusion,
            Skill::Conjuration => &self.conjuration,
            Skill::Mysticism => &self.mysticism,
            Skill::Restoration => &self.restoration,
            Skill::Alchemy => &self.alchemy,
            Skill::Unarmored => &self.unarmored,
            Skill::Security => &self.security,
            Skill::Sneak => &self.sneak,
            Skill::Acrobatics => &self.acrobatics,
            Skill::LightArmor => &self.light_armor,
            Skill::ShortBlade => &self.short_blade,
            Skill::Marksman => &self.marksman,
            Skill::Mercantile => &self.mercantile,
            Skill::Speechcraft => &self.speechcraft,
            Skill::HandToHand => &self.hand_to_hand,
        }
    }
}

impl<T> IndexMut<Skill> for Skills<T> {
    fn index_mut(&mut self, index: Skill) -> &mut Self::Output {
        match index {
            Skill::Block => &mut self.block,
            Skill::Armorer => &mut self.armorer,
            Skill::MediumArmor => &mut self.medium_armor,
            Skill::HeavyArmor => &mut self.heavy_armor,
            Skill::BluntWeapon => &mut self.blunt_weapon,
            Skill::LongBlade => &mut self.long_blade,
            Skill::Axe => &mut self.axe,
            Skill::Spear => &mut self.spear,
            Skill::Athletics => &mut self.athletics,
            Skill::Enchant => &mut self.enchant,
            Skill::Destruction => &mut self.destruction,
            Skill::Alteration => &mut self.alteration,
            Skill::Illusion => &mut self.illusion,
            Skill::Conjuration => &mut self.conjuration,
            Skill::Mysticism => &mut self.mysticism,
            Skill::Restoration => &mut self.restoration,
            Skill::Alchemy => &mut self.alchemy,
            Skill::Unarmored => &mut self.unarmored,
            Skill::Security => &mut self.security,
            Skill::Sneak => &mut self.sneak,
            Skill::Acrobatics => &mut self.acrobatics,
            Skill::LightArmor => &mut self.light_armor,
            Skill::ShortBlade => &mut self.short_blade,
            Skill::Marksman => &mut self.marksman,
            Skill::Mercantile => &mut self.mercantile,
            Skill::Speechcraft => &mut self.speechcraft,
            Skill::HandToHand => &mut self.hand_to_hand,
        }
    }
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum School {
        Alteration = 0,
        Conjuration = 1,
        Illusion = 2,
        Destruction = 3,
        Mysticism = 4,
        Restoration = 5,
    }
}

enum_serde!(School, "school", as u32, Unsigned, u64);

impl From<School> for Skill {
    fn from(s: School) -> Skill {
        match s {
            School::Alteration => Skill::Alteration,
            School::Conjuration => Skill::Conjuration,
            School::Illusion => Skill::Illusion,
            School::Destruction => Skill::Destruction,
            School::Mysticism => Skill::Mysticism,
            School::Restoration => Skill::Restoration
        }
    }
}

impl TryFrom<Skill> for School {
    type Error = ();
    
    fn try_from(s: Skill) -> Result<School, ()> {
        match s {
            Skill::Alteration => Ok(School::Alteration),
            Skill::Conjuration => Ok(School::Conjuration),
            Skill::Illusion => Ok(School::Illusion),
            Skill::Destruction => Ok(School::Destruction),
            Skill::Mysticism => Ok(School::Mysticism),
            Skill::Restoration => Ok(School::Restoration),
            _ => Err(())
        }
    }
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
pub enum EffectArgType {
    Attribute, Skill
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum EffectIndex {
        WaterBreathing = 0, SwiftSwim = 1, WaterWalking = 2, Shield = 3, FireShield = 4, LightningShield = 5,
        FrostShield = 6, Burden = 7, Feather = 8, Jump = 9, Levitate = 10, SlowFall = 11, Lock = 12, Open = 13,
        FireDamage = 14, ShockDamage = 15, FrostDamage = 16, DrainAttribute = 17, DrainHealth = 18,
        DrainSpellpoints = 19, DrainFatigue = 20, DrainSkill = 21, DamageAttribute = 22, DamageHealth = 23,
        DamageMagicka = 24, DamageFatigue = 25, DamageSkill = 26, Poison = 27, WeaknessToFire = 28,
        WeaknessToFrost = 29, WeaknessToShock = 30, WeaknessToMagicka = 31, WeaknessToCommonDisease = 32,
        WeaknessToBlightDisease = 33, WeaknessToCorprusDisease = 34, WeaknessToPoison = 35,
        WeaknessToNormalWeapons = 36, DisintegrateWeapon = 37, DisintegrateArmor = 38, Invisibility = 39,
        Chameleon = 40, Light = 41, Sanctuary = 42, NightEye = 43, Charm = 44, Paralyze = 45, Silence = 46,
        Blind = 47, Sound = 48, CalmHumanoid = 49, CalmCreature = 50, FrenzyHumanoid = 51, FrenzyCreature = 52,
        DemoralizeHumanoid = 53, DemoralizeCreature = 54, RallyHumanoid = 55, RallyCreature = 56, Dispel = 57,
        Soultrap = 58, Telekinesis = 59, Mark = 60, Recall = 61, DivineIntervention = 62, AlmsiviIntervention = 63,
        DetectAnimal = 64, DetectEnchantment = 65, DetectKey = 66, SpellAbsorption = 67, Reflect = 68,
        CureCommonDisease = 69, CureBlightDisease = 70, CureCorprusDisease = 71, CurePoison = 72,
        CureParalyzation = 73, RestoreAttribute = 74, RestoreHealth = 75, RestoreSpellPoints = 76,
        RestoreFatigue = 77, RestoreSkill = 78, FortifyAttribute = 79, FortifyHealth = 80, FortifySpellpoints = 81,
        FortifyFatigue = 82, FortifySkill = 83, FortifyMagickaMultiplier = 84, AbsorbAttribute = 85,
        AbsorbHealth = 86, AbsorbSpellPoints = 87, AbsorbFatigue = 88, AbsorbSkill = 89, ResistFire = 90,
        ResistFrost = 91, ResistShock = 92, ResistMagicka = 93, ResistCommonDisease = 94,
        ResistBlightDisease = 95, ResistCorprusDisease = 96, ResistPoison = 97, ResistNormalWeapons = 98,
        ResistParalysis = 99, RemoveCurse = 100, TurnUndead = 101, SummonScamp = 102, SummonClannfear = 103,
        SummonDaedroth = 104, SummonDremora = 105, SummonAncestralGhost = 106, SummonSkeletalMinion = 107,
        SummonLeastBonewalker = 108, SummonGreaterBonewalker = 109, SummonBonelord = 110,
        SummonWingedTwilight = 111, SummonHunger = 112, SummonGoldensaint = 113, SummonFlameAtronach = 114,
        SummonFrostAtronach = 115, SummonStormAtronach = 116, FortifyAttackBonus = 117, CommandCreatures = 118,
        CommandHumanoids = 119, BoundDagger = 120, BoundLongsword = 121, BoundMace = 122, BoundBattleAxe = 123,
        BoundSpear = 124, BoundLongbow = 125, ExtraSpell = 126, BoundCuirass = 127, BoundHelm = 128,
        BoundBoots = 129, BoundShield = 130, BoundGloves = 131, Corpus = 132, Vampirism = 133,
        SummonCenturionSphere = 134, SunDamage = 135, StuntedMagicka = 136, SummonFabricant = 137,
        SummonCreature01 = 138, SummonCreature02 = 139, SummonCreature03 = 140, SummonCreature04 = 141,
        SummonCreature05 = 142
    }
}

impl EffectIndex {
    pub fn arg_type(self) -> Option<EffectArgType> {
        match self {
            EffectIndex::AbsorbAttribute |
            EffectIndex::DamageAttribute |
            EffectIndex::DrainAttribute |
            EffectIndex::FortifyAttribute |
            EffectIndex::RestoreAttribute =>
                Some(EffectArgType::Attribute),
            EffectIndex::AbsorbSkill |
            EffectIndex::DamageSkill |
            EffectIndex::DrainSkill |
            EffectIndex::FortifySkill |
            EffectIndex::RestoreSkill =>
                Some(EffectArgType::Skill),
            _ => None
         }
    }
}

enum_serde!(EffectIndex, "effect index", as u32, Unsigned, u64);

mod effect_index_option_i32 {
    use crate::field::EffectIndex;
    use crate::serde_helpers::*;
    use either::{Either};
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i32>, EffectIndex>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "effect index",
            none: -1,
            from: |x| x.try_into().ok().and_then(EffectIndex::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i32>, EffectIndex>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "effect index",
            none: -1,
            from: |x| x.try_into().ok().and_then(EffectIndex::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

mod effect_index_option_i16 {
    use crate::field::EffectIndex;
    use crate::serde_helpers::*;
    use either::{Either};
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i16>, EffectIndex>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "effect index",
            none: -1,
            from: |x| x.try_into().ok().and_then(EffectIndex::n),
            into: |x| (x as u32).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i16>, EffectIndex>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "effect index",
            none: -1,
            from: |x| x.try_into().ok().and_then(EffectIndex::n),
            into: |x| (x as u32).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

bitflags_ext! {
    pub struct EffectFlags: u32 {
        SPELLMAKING = 0x200,
        ENCHANTING = 0x400,
        LIGHT_NEGATIVE = 0x800
    }
}

enum_serde!(EffectFlags, "effect flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct EffectMetadata {
    pub school: School,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub base_cost: f32,
    pub flags: EffectFlags,
    #[serde(with="color_components")]
    pub color: Color,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub size_factor: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub speed: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub size_cap: f32,
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Display for Color {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl FromStr for Color {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 7{ return Err(()); }
        if &s[0..1] != "#" || &s[1..2] == "+" { return Err(()); }
        let r = u8::from_str_radix(&s[1..3], 16).map_err(|_| ())?;
        let g = u8::from_str_radix(&s[3..5], 16).map_err(|_| ())?;
        let b = u8::from_str_radix(&s[5..7], 16).map_err(|_| ())?;
        Ok(Color { r, g, b })
    }
}

impl Color {
    pub fn to_u32(self) -> u32 {
        (self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16)
    }
    
    pub fn try_from_u32(u: u32) -> Option<Color> {
        if u & 0xFF000000 != 0 { 
            None
        } else {
            let r = (u & 0xFF) as u8;
            let g = ((u >> 8) & 0xFF) as u8;
            let b = ((u >> 16) & 0xFF) as u8;
            Some(Color { r, g, b })
        }
    }
}

enum_serde!(Color, "RGB color", u32, to_u32(), try try_from_u32, Unsigned, u64);

mod color_components {
    use std::convert::TryInto;
    use serde::{Serializer, Deserializer, Serialize, Deserialize};
    use crate::field::Color;
    use serde::de::Unexpected;
    use serde::de::Error as de_Error;
    use serde::ser::SerializeTuple;

    pub fn serialize<S>(&c: &Color, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        if serializer.is_human_readable() {
            c.serialize(serializer)
        } else {
            let mut serializer = serializer.serialize_tuple(3)?;
            serializer.serialize_element(&(c.r as u32))?;
            serializer.serialize_element(&(c.g as u32))?;
            serializer.serialize_element(&(c.b as u32))?;
            serializer.end()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Color, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            Color::deserialize(deserializer)
        } else {
            let (r, g, b) = <(u32, u32, u32)>::deserialize(deserializer)?;
            let r = r.try_into().map_err(|_| D::Error::invalid_value(Unexpected::Unsigned(r as u64), &"0 .. 255"))?;
            let g = g.try_into().map_err(|_| D::Error::invalid_value(Unexpected::Unsigned(g as u64), &"0 .. 255"))?;
            let b = b.try_into().map_err(|_| D::Error::invalid_value(Unexpected::Unsigned(b as u64), &"0 .. 255"))?;
            Ok(Color { r, g, b })
        }
    }
}

bitflags_ext! {
    pub struct LightFlags: u32 {
        DYNAMIC = 0x0001,
        CAN_CARRY = 0x0002,
        NEGATIVE = 0x0004,
        FLICKER = 0x0008,
        FIRE = 0x0010,
        OFF_BY_DEFAULT = 0x0020,
        FLICKER_SLOW = 0x0040,
        PULSE = 0x0080,
        PULSE_SLOW = 0x0100
    }
}

enum_serde!(LightFlags, "light flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Light {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    pub time: i32,
    pub radius: u32,
    pub color: Color,
    pub flags: LightFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct MiscItem {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    #[serde(with="bool_u32")]
    pub is_key: bool,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum ApparatusType {
        MortarPestle = 0,
        Alembic  = 1,
        Calcinator = 2,
        Retort = 3
    }
}

enum_serde!(ApparatusType, "apparatus type", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Apparatus {
    #[serde(rename="type")]
    pub apparatus_type: ApparatusType,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub quality: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum ArmorType {
        Helmet = 0,
        Cuirass = 1,
        LeftPauldron = 2,
        RightPauldron = 3,
        Greaves = 4,
        Boots = 5,
        LeftGauntlet = 6,
        RightGauntlet = 7,
        Shield = 8,
        LeftBracer = 9,
        RightBracer = 10
    }
}

enum_serde!(ArmorType, "armor type", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Armor {
    #[serde(rename="type")]
    pub armor_type: ArmorType,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    pub health: u32,
    pub enchantment: u32,
    pub armor: u32,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u16)]
    pub enum WeaponType {
        ShortBladeOneHand = 0,
        LongBladeOneHand = 1,
        LongBladeTwoClose = 2,
        BluntOneHand = 3,
        BluntTwoClose = 4,
        BluntTwoWide = 5,
        SpearTwoWide = 6,
        AxeOneHand = 7,
        AxeTwoClose = 8,
        MarksmanBow = 9,
        MarksmanCrossbow = 10,
        MarksmanThrown = 11,
        Arrow = 12,
        Bolt = 13
    }
}

enum_serde!(WeaponType, "weapon type", as u16, Unsigned, u64);

bitflags_ext! {
    pub struct WeaponFlags: u32 {
        MAGICAL = 0x01,
        SILVER = 0x02
    }
}

enum_serde!(WeaponFlags, "weapon flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Weapon {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    #[serde(rename="type")]
    pub weapon_type: WeaponType,
    pub health: u16,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub speed: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub reach: f32,
    pub enchantment: u16,
    pub chop_min: u8,
    pub chop_max: u8,
    pub slash_min: u8,
    pub slash_max: u8,
    pub thrust_min: u8,
    pub thrust_max: u8,
    pub flags: WeaponFlags,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum BodyPartKind {
        Head = 0,
        Hair = 1,
        Neck = 2,
        Chest = 3,
        Groin = 4,
        Hand = 5,
        Wrist = 6,
        Forearm = 7,
        UpperArm = 8,
        Foot = 9,
        Ankle = 10,
        Knee = 11,
        UpperLeg = 12,
        Clavicle = 13,
        Tail = 14,
    }
}

enum_serde!(BodyPartKind, "body part kind", as u8, Unsigned, u64);

bitflags_ext! {
    pub struct BodyPartFlags: u8 {
        FEMALE = 0x01,
        NON_PLAYABLE = 0x02
    }
}

enum_serde!(BodyPartFlags, "body part flags", u8, bits(), try from_bits, Unsigned, u64);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum BodyPartType {
        Skin = 0,
        Clothing = 1,
        Armor = 2
    }
}

enum_serde!(BodyPartType, "body part type", as u8, Unsigned, u64);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum BipedObject {
        Head = 0,
        Hair = 1,
        Neck = 2,
        Cuirass = 3,
        Groin = 4,
        Skirt = 5,
        RightHand = 6,
        LeftHand = 7,
        RightWrist = 8,
        LeftWrist = 9,
        Shield = 10,
        RightForearm = 11,
        LeftForearm = 12,
        RightUpperArm = 13,
        LeftUpperArm = 14,
        RightFoot = 15,
        LeftFoot = 16,
        RightAnkle = 17,
        LeftAnkle = 18,
        RightKnee = 19,
        LeftKnee = 20,
        RightUpperLeg = 21,
        LeftUpperLeg = 22,
        RightPauldron = 23,
        LeftPauldron = 24,
        Weapon = 25,
        Tail = 26,
    }
}

enum_serde!(BipedObject, "biped object", as u8, Unsigned, u64);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum ClothingType {
        Pants = 0,
        Shoes = 1,
        Shirt = 2,
        Belt = 3,
        Robe = 4,
        RightGlove = 5,
        LeftGlove = 6,
        Skirt = 7,
        Ring = 8,
        Amulet = 9
    }
}

enum_serde!(ClothingType, "clothing type", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct BodyPart {
    pub kind: BodyPartKind,
    #[serde(with="bool_u8")]
    pub vampire: bool,
    pub flags: BodyPartFlags,
    #[serde(rename="type")]
    pub body_part_type: BodyPartType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Clothing {
    #[serde(rename="type")]
    pub clothing_type: ClothingType,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u16,
    pub enchantment: u16,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum EnchantmentType {
        CastOnce = 0,
        WhenStrikes = 1,
        WhenUsed = 2,
        ConstantEffect = 3
    }
}

enum_serde!(EnchantmentType, "enchantment type", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Enchantment {
    #[serde(rename="type")]
    pub enchantment_type: EnchantmentType,
    pub cost: u32,
    pub charge_amount: u32,
    #[serde(with="bool_either_i16")]
    pub auto_calculate: Either<bool, bool>,
    pub padding: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Tool {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub quality: f32,
    pub uses: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub(crate) struct RepairItem {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    pub uses: u32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub quality: f32,
}

impl From<RepairItem> for Tool {
    fn from(t: RepairItem)-> Tool {
        Tool { weight: t.weight, value: t.value, quality: t.quality, uses: t.uses }
    }
}

impl From<Tool> for RepairItem {
    fn from(t: Tool)-> RepairItem {
        RepairItem { weight: t.weight, value: t.value, quality: t.quality, uses: t.uses }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Pos {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub x: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub y: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Rot {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub x: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub y: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PosRot {
    pub pos: Pos,
    pub rot: Rot,
}

bitflags_ext! {
    pub struct CellFlags: u32 {
        INTERIOR = 0x01,
        HAS_WATER = 0x02,
        ILLEGAL_TO_SLEEP = 0x04,
        BEHAVE_LIKE_EXTERIOR = 0x80,
        _8 = 0x08,
        _10 = 0x10,
        _20 = 0x20,
        _40 = 0x40
    }
}

enum_serde!(CellFlags, "cell flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Cell {
    pub flags: CellFlags,
    pub position: CellPosition,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename="Cell")]
struct CellHRSurrogate {
    pub flags: CellFlags,
    pub position: CellPosition,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename="Cell")]
struct CellNHRSurrogate {
    pub flags: CellFlags,
    pub position: (i32, i32),
}

impl From<CellHRSurrogate> for Cell {
    fn from(cell: CellHRSurrogate) -> Cell {
        Cell {
            flags: cell.flags,
            position: cell.position,
        }
    }
}

impl From<CellNHRSurrogate> for Cell {
    fn from(cell: CellNHRSurrogate) -> Cell {
        let position = CellPosition::from_exterior(
            cell.flags.contains(CellFlags::INTERIOR),
            cell.position
        );
        Cell { flags: cell.flags, position }
    }
}

impl Serialize for Cell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let is_interior = self.flags.contains(CellFlags::INTERIOR);
        let position_is_interior = match self.position {
            CellPosition::Interior { .. } => true,
            CellPosition::Exterior { .. } => false,
        };
        if is_interior != position_is_interior {
            return Err(S::Error::custom("cell INTERIOR flag and position type do not match"));
        }
        if serializer.is_human_readable() {
            CellHRSurrogate {
                flags: self.flags,
                position: self.position.clone()
            }.serialize(serializer)
        } else {
            CellNHRSurrogate {
                flags: self.flags,
                position: self.position.to_exterior()
            }.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D>(deserializer: D) -> Result<Cell, D::Error> where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            CellHRSurrogate::deserialize(deserializer).map(Cell::from)
        } else {
            CellNHRSurrogate::deserialize(deserializer).map(Cell::from)
        }
    }
}

#[derive(Debug, Clone, Educe, Serialize, Deserialize)]
#[educe(Eq, PartialEq)]
#[serde(tag="type")]
pub enum CellPosition {
    Interior {
        #[educe(PartialEq(method="eq_f32"))]
        #[serde(with="float_32")]
        x: f32,
        #[educe(PartialEq(method="eq_f32"))]
        #[serde(with="float_32")]
        y: f32
    },
    Exterior { x: i32, y: i32 }
}

impl CellPosition {
    fn from_exterior(is_interior: bool, position: (i32, i32)) -> Self {
        if is_interior {
            CellPosition::Interior {
                x: unsafe { transmute::<i32, f32>(position.0) },
                y: unsafe { transmute::<i32, f32>(position.1) },
            }
        } else {
            CellPosition::Exterior {
                x: position.0,
                y: position.1
            }
        }
    }

    fn to_exterior(&self) -> (i32, i32) {
        match self {
            &CellPosition::Exterior { x, y } => (x, y),
            &CellPosition::Interior { x, y } => (
                unsafe { transmute::<f32, i32>(x) },
                unsafe { transmute::<f32, i32>(y) }
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Interior {
    pub ambient: Color,
    pub sunlight: Color,
    pub fog: Color,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub fog_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Grid {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PathGrid {
    pub grid: Grid,
    pub flags: u16,
    pub points: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Weather {
    pub clear: u8,
    pub cloudy: u8,
    pub foggy: u8,
    pub overcast: u8,
    pub rain: u8,
    pub thunder: u8,
    pub ash: u8,
    pub blight: u8,
    pub ex: Option<WeatherEx>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct WeatherEx {
    pub snow: u8,
    pub blizzard: u8,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SoundChance {
    pub sound_id: String,
    pub chance: u8,
}

const SOUND_CHANCE_SOUND_ID_FIELD: &str = name_of!(sound_id in SoundChance);
const SOUND_CHANCE_CHANCE_FIELD: &str = name_of!(chance in SoundChance);

const SOUND_CHANCE_FIELDS: &[&str] = &[
    SOUND_CHANCE_SOUND_ID_FIELD,
    SOUND_CHANCE_CHANCE_FIELD,
];

#[derive(Clone)]
pub struct SoundChanceSerde {
    pub code_page: Option<CodePage>
}

impl SerializeSeed for SoundChanceSerde {
    type Value = SoundChance;

    fn serialize<S: Serializer>(&self, value: &Self::Value, serializer: S) -> Result<S::Ok, S::Error> {
        let mut serializer = serializer.serialize_struct(name_of!(type SoundChance), 2)?;
        serializer.serialize_field(
            SOUND_CHANCE_SOUND_ID_FIELD,
            &ValueWithSeed(value.sound_id.as_str(), StringSerde { code_page: self.code_page, len: Some(32) })
        )?;
        serializer.serialize_field(SOUND_CHANCE_CHANCE_FIELD, &value.chance)?;
        serializer.end()
    }
}

enum SoundChanceField {
    SoundId,
    Chance,
}

struct SoundChanceFieldDeVisitor;

impl<'de> de::Visitor<'de> for SoundChanceFieldDeVisitor {
    type Value = SoundChanceField;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "sound chance field")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
            SOUND_CHANCE_SOUND_ID_FIELD => Ok(SoundChanceField::SoundId),
            SOUND_CHANCE_CHANCE_FIELD => Ok(SoundChanceField::Chance),
            x => Err(E::unknown_field(x, SOUND_CHANCE_FIELDS)),
        }
    }
}

impl<'de> de::Deserialize<'de> for SoundChanceField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(SoundChanceFieldDeVisitor)
    }
}

struct SoundChanceDeVisitor(SoundChanceSerde);

impl<'de> de::Visitor<'de> for SoundChanceDeVisitor {
    type Value = SoundChance;

    fn expecting(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "AI activate")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut sound_id = None;
        let mut chance = None;
        while let Some(field) = map.next_key()? {
            match field {
                SoundChanceField::SoundId =>
                    if sound_id.replace(map.next_value_seed(
                        StringSerde { code_page: self.0.code_page, len: Some(32) }
                    )?).is_some() {
                        return Err(A::Error::duplicate_field(SOUND_CHANCE_SOUND_ID_FIELD));
                    },
                SoundChanceField::Chance => if chance.replace(map.next_value()?).is_some() {
                    return Err(A::Error::duplicate_field(SOUND_CHANCE_CHANCE_FIELD));
                },
            }
        }
        let sound_id = sound_id.ok_or_else(|| A::Error::missing_field(SOUND_CHANCE_SOUND_ID_FIELD))?;
        let chance = chance.ok_or_else(|| A::Error::missing_field(SOUND_CHANCE_CHANCE_FIELD))?;
        Ok(SoundChance { sound_id, chance })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error> where A: de::SeqAccess<'de> {
        let sound_id = seq.next_element_seed(StringSerde { code_page: self.0.code_page, len: Some(32) })?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let chance = seq.next_element()?
            .ok_or_else(|| A::Error::invalid_length(1, &self))?;
        Ok(SoundChance { sound_id, chance })
    }
}

impl<'de> DeserializeSeed<'de> for SoundChanceSerde {
    type Value = SoundChance;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_struct(name_of!(type SoundChance), SOUND_CHANCE_FIELDS, SoundChanceDeVisitor(self))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct Potion {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub weight: f32,
    pub value: u32,
    #[serde(with="bool_u32")]
    pub auto_calculate_value: bool,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum Specialization {
        Combat = 0,
        Magic = 1,
        Stealth = 2,
    }
}

enum_serde!(Specialization, "specialization", as u32, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Class {
    pub primary_attribute_1: Attribute,
    pub primary_attribute_2: Attribute,
    pub specialization: Specialization,
    pub minor_skill_1: Skill,
    pub major_skill_1: Skill,
    pub minor_skill_2: Skill,
    pub major_skill_2: Skill,
    pub minor_skill_3: Skill,
    pub major_skill_3: Skill,
    pub minor_skill_4: Skill,
    pub major_skill_4: Skill,
    pub minor_skill_5: Skill,
    pub major_skill_5: Skill,
    #[serde(with="bool_u32")]
    pub playable: bool,
    pub auto_calc_services: Services,
}

bitflags_ext! {
    pub struct RaceFlags: u32 {
        PLAYABLE = 0x01,
        BEAST_RACE = 0x02
    }
}

enum_serde!(RaceFlags, "race flags", u32, bits(), try from_bits, Unsigned, u64);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct RaceAttribute {
    pub male: u32,
    pub female: u32,
}

impl Index<Sex> for RaceAttribute {
    type Output = u32;

    fn index(&self, index: Sex) -> &Self::Output {
        match index {
            Sex::Male => &self.male,
            Sex::Female => &self.female,
        }
    }
}

impl IndexMut<Sex> for RaceAttribute {
    fn index_mut(&mut self, index: Sex) -> &mut Self::Output {
        match index {
            Sex::Male => &mut self.male,
            Sex::Female => &mut self.female,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct RaceParameter {
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub male: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub female: f32,
}

impl Index<Sex> for RaceParameter {
    type Output = f32;

    fn index(&self, index: Sex) -> &Self::Output {
        match index {
            Sex::Male => &self.male,
            Sex::Female => &self.female,
        }
    }
}

impl IndexMut<Sex> for RaceParameter {
    fn index_mut(&mut self, index: Sex) -> &mut Self::Output {
        match index {
            Sex::Male => &mut self.male,
            Sex::Female => &mut self.female,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Race {
    #[serde(with="skill_option_i32")]
    pub skill_1: Either<Option<i32>, Skill>,
    pub skill_1_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_2: Either<Option<i32>, Skill>,
    pub skill_2_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_3: Either<Option<i32>, Skill>,
    pub skill_3_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_4: Either<Option<i32>, Skill>,
    pub skill_4_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_5: Either<Option<i32>, Skill>,
    pub skill_5_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_6: Either<Option<i32>, Skill>,
    pub skill_6_bonus: u32,
    #[serde(with="skill_option_i32")]
    pub skill_7: Either<Option<i32>, Skill>,
    pub skill_7_bonus: u32,
    pub attributes: Attributes<RaceAttribute>,
    pub height: RaceParameter,
    pub weight: RaceParameter,
    pub flags: RaceFlags
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Sound {
    pub volume: u8,
    pub range_min: u8,
    pub range_max: u8,
}

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u32)]
    pub enum SoundGen {
        Left = 0,
        Right = 1,
        SwimLeft = 2,
        SwimRight = 3,
        Moan = 4,
        Roar = 5,
        Scream = 6,
        Land = 7
    }
}

enum_serde!(SoundGen, "sound gen", as u32, Unsigned, u64);

macro_attr! {
    #[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Copy, Clone)]
    #[derive(Debug, N, EnumDisplay!, EnumFromStr!)]
    #[repr(u8)]
    pub enum Sex {
        Male = 0,
        Female = 1
    }
}

enum_serde!(Sex, "sex", as u8, Unsigned, u64);

mod sex_option_i8 {
    use crate::field::Sex;
    use crate::serde_helpers::*;
    use either::{Either};
    use serde::{Deserializer, Serialize, Serializer};
    use serde::de::DeserializeSeed;
    use serde_serialize_seed::ValueWithSeed;
    use std::convert::TryInto;

    pub fn serialize<S>(v: &Either<Option<i8>, Sex>, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        ValueWithSeed(v, OptionIndexSerde {
            name: "sex",
            none: -1,
            from: |x| x.try_into().ok().and_then(Sex::n),
            into: |x| (x as u8).try_into().unwrap()
        }).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Either<Option<i8>, Sex>, D::Error> where D: Deserializer<'de> {
        OptionIndexSerde {
            name: "sex",
            none: -1,
            from: |x| x.try_into().ok().and_then(Sex::n),
            into: |x| (x as u8).try_into().unwrap()
        }.deserialize(deserializer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Info {
    #[serde(with="dialog_type_u32")]
    pub dialog_type: DialogType,
    pub disp_index: u32,
    #[serde(with="option_i8")]
    pub rank: Option<i8>,
    #[serde(with="sex_option_i8")]
    pub sex: Either<Option<i8>, Sex>,
    #[serde(with="option_i8")]
    pub pc_rank: Option<i8>,
    pub padding: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Rank {
    pub attribute_1: u32,
    pub attribute_2: u32,
    pub primary_skill: u32,
    pub favored_skill: u32,
    pub reputation: u32
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Faction {
    pub favored_attribute_1: Attribute,
    pub favored_attribute_2: Attribute,
    pub ranks: [Rank; 10],
    #[serde(with="skill_option_i32")]
    pub favored_skill_1: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_2: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_3: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_4: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_5: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_6: Either<Option<i32>, Skill>,
    #[serde(with="skill_option_i32")]
    pub favored_skill_7: Either<Option<i32>, Skill>,
    #[serde(with="bool_u32")]
    pub hidden_from_pc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Educe)]
#[educe(Eq, PartialEq)]
pub struct SkillMetadata {
    pub governing_attribute: Attribute,
    pub specialization: Specialization,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub use_value_1: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub use_value_2: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub use_value_3: f32,
    #[educe(PartialEq(method="eq_f32"))]
    #[serde(with="float_32")]
    pub use_value_4: f32,
}

macro_rules! define_field {
    ($($variant:ident($(#[educe(PartialEq(method=$a:literal))])? $from:ty),)*) => {
        #[derive(Debug, Clone)]
        #[derive(Educe)]
        #[educe(PartialEq, Eq)]
        pub enum Field {
            None,
            $($variant($(#[educe(PartialEq(method=$a))])? $from)),*
        }
        
        $(
        impl From<$from> for Field {
            fn from(v: $from) -> Self { Field::$variant(v) }
        }
        )*
    }
}

define_field!(
    Ai(Ai),
    AiActivate(AiActivate),
    AiTarget(AiTarget),
    AiTravel(AiTravel),
    AiWander(AiWander),
    Apparatus(Apparatus),
    Armor(Armor),
    BipedObject(BipedObject),
    BodyPart(BodyPart),
    Book(Book),
    Cell(Cell),
    Class(Class),
    Clothing(Clothing),
    Color(Color),
    ContainerFlags(ContainerFlags),
    Creature(Creature),
    CreatureFlags(FlagsAndBlood<CreatureFlags>),
    DialogType(DialogType),
    Effect(Effect),
    EffectIndex(EffectIndex),
    Tag(Tag),
    EffectMetadata(EffectMetadata),
    Enchantment(Enchantment),
    F32(#[educe(PartialEq(method="eq_f32"))] f32),
    F32List(#[educe(PartialEq(method="eq_f32_list"))] Vec<f32>),
    Faction(Faction),
    FileMetadata(FileMetadata),
    Grid(Grid),
    I16(i16),
    I16List(Vec<i16>),
    I32(i32),
    I32List(Vec<i32>),
    I64(i64),
    Info(Info),
    Ingredient(Ingredient),
    Interior(Interior),
    Item(Item),
    Light(Light),
    MiscItem(MiscItem),
    Npc(Npc),
    NpcFlags(FlagsAndBlood<NpcFlags>),
    NpcState(NpcState),
    PathGrid(PathGrid),
    Pos(Pos),
    PosRot(PosRot),
    Potion(Potion),
    Race(Race),
    ScriptMetadata(ScriptMetadata),
    ScriptVars(ScriptVars),
    Skill(Skill),
    SkillMetadata(SkillMetadata),
    Sound(Sound),
    SoundChance(SoundChance),
    SoundGen(SoundGen),
    Spell(Spell),
    String(String),
    StringList(Vec<String>),
    StringZ(StringZ),
    StringZList(StringZList),
    Tool(Tool),
    U8(u8),
    Bool(bool),
    U8List(Vec<u8>),
    Weapon(Weapon),
    Weather(Weather),
    CurrentTime(CurrentTime),
    Time(Time),
    EffectArg(EffectArg),
    Attributes(Attributes<u32>),
    Skills(Skills<u32>),
    ScriptData(ScriptData),
);

impl From<()> for Field {
    fn from(_: ()) -> Self { Field::None }
}

fn allow_fit(record_tag: Tag, field_tag: Tag) -> bool {
    matches!((record_tag, field_tag)
        , (_, AI_A)
        | (_, AI_E)
        | (_, AI_F)
        | (ARMO, BNAM)
        | (BODY, BNAM)
        | (CLOT, BNAM)
        | (INFO, BNAM)
        | (ARMO, CNAM)
        | (SSCR, DATA)
        | (BSGN, DESC)
        | (ACTI, FNAM)
        | (TES3, HEDR)
        | (CELL, NAME)
        | (JOUR, NAME)
        | (SSCR, NAME)
        | (INFO, NNAM)
        | (INFO, PNAM)
        | (FACT, RNAM)
        | (_, SCTX)
        | (REGN, SNAM)
        | (BOOK, TEXT)
    )
}

impl Field {
    pub fn fit(&mut self, record_tag: Tag, prev_tag: Tag, field_tag: Tag, omwsave: bool) {
        if !allow_fit(record_tag, field_tag) { return; }
        match FieldType::from_tags(record_tag, prev_tag, field_tag, omwsave) {
            FieldType::FileMetadata => {
                if let Field::FileMetadata(v) = self {
                    v.author.as_mut().right().map(|a| a.find('\0').map(|i| a.truncate(i)));
                    if let Right(description) = v.description.as_mut() {
                        let mut d = description.join(Newline::Dos.as_str());
                        d.find('\0').map(|i| d.truncate(i));
                        *description = d.split(Newline::Dos.as_str()).map(String::from).collect();
                    }
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::String(_) => {
                if let Field::String(v) = self {
                    v.find('\0').map(|i| v.truncate(i));
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::SoundChance => {
                if let Field::SoundChance(v) = self {
                    v.sound_id.find('\0').map(|i| v.sound_id.truncate(i));
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::Multiline(newline) => {
                if let Field::StringList(v) = self {
                    let mut s = v.join(newline.as_str());
                    s.find('\0').map(|i| s.truncate(i));
                    *v = s.split(newline.as_str()).map(String::from).collect();
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::StringZ => {
                if let Field::StringZ(v) = self {
                    v.string.find('\0').map(|i| v.string.truncate(i));
                    v.has_tail_zero = true;
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::StringZList => {
                if let Field::StringZList(v) = self {
                    v.has_tail_zero = true;
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::AiTarget => {
                if let Field::AiTarget(v) = self {
                    v.actor_id.find('\0').map(|i| v.actor_id.truncate(i));
                } else {
                    panic!("invalid field type")
                }
            },
            FieldType::AiActivate => {
                if let Field::AiActivate(v) = self {
                    v.object_id.find('\0').map(|i| v.object_id.truncate(i));
                } else {
                    panic!("invalid field type")
                }
            },
            _ => ()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use quickcheck_macros::quickcheck;
    use std::str::FromStr;

    #[test]
    fn debug_and_display_tag() {
        assert_eq!("TES3", format!("{}", TES3));
        assert_eq!("TES3", format!("{:?}", TES3));
        assert_eq!(Ok(SCPT), Tag::from_str("SCPT"));
    }

    #[test]
    fn test_file_type() {
        assert_eq!("ESM", format!("{}", FileType::ESM));
        assert_eq!("ESS", format!("{:?}", FileType::ESS));
        assert_eq!(Some(FileType::ESP), FileType::n(0));
        assert_eq!(None, FileType::n(2));
        assert_eq!(32, FileType::ESS as u32);
        assert_eq!(Ok(FileType::ESP), FileType::from_str("ESP"));
    }
    
    #[test]
    fn light_flags_from_str() {
        assert_eq!(
            LightFlags::from_str("DYNAMIC CAN_CARRY FIRE FLICKER_SLOW"),
            Ok(LightFlags::DYNAMIC | LightFlags::CAN_CARRY | LightFlags::FIRE | LightFlags::FLICKER_SLOW)
        );
    }

    #[quickcheck]
    fn effect_arg_from_str_is_display_inversion(dword: u32) -> bool {
        EffectArg::from_str(&EffectArg { dword }.to_string()) == Ok(EffectArg { dword })
    }

    #[quickcheck]
    fn effect_arg_from_dword_str_eq_effect_arg_from_dword(dword: u32) -> bool {
        EffectArg::from_str(&format!("{dword:04X}")) == Ok(EffectArg { dword })
    }
}
