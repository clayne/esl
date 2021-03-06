#![deny(warnings)]
use std::env;
use std::fs::File;
use std::io::{Write, BufReader, BufRead};
use std::path::Path;
use std::str::FromStr;

#[allow(dead_code)]
#[allow(unused_macros)]
mod tag {
    include!("src/tag.rs");
}

use tag::*;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let src_path = Path::new("src/tags.list");
    let dest_path = Path::new(&out_dir).join("tags.rs");
    let mut dest = File::create(&dest_path).unwrap();
    let src = BufReader::new(File::open(&src_path).unwrap());
    let tags = src.lines().map(|s| Tag::from_str(&s.unwrap()).unwrap());
    for tag in tags  {
        writeln!(dest, "pub const {}: Tag = Tag::from({});", tag, tag.dword).unwrap();
    }
}
