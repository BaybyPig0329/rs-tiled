#![feature(globs)]

extern crate serialize;
extern crate xml;
extern crate tiled;

use std::io::File;
use std::io::BufferedReader;
use xml::reader::EventReader;
use tiled::parse;

fn main() {
    let file = File::open(&Path::new("assets/tiled_base64_zlib.tmx")).unwrap();
    println!("Opened file");
    let reader = BufferedReader::new(file);
    let mut parser = EventReader::new(reader);
    println!("{}", parse(&mut parser));
}
