#![feature(globs, macro_rules)]
extern crate flate2;
extern crate xml;
extern crate serialize;

use std::io::{BufReader, IoError, EndOfFile};
use std::from_str::FromStr;
use std::collections::HashMap;
use xml::reader::EventReader;
use xml::common::Attribute;
use xml::reader::events::*;
use serialize::base64::{FromBase64, FromBase64Error};
use flate2::reader::ZlibDecoder;

macro_rules! get_attrs {
    ($attrs:expr, optionals: [$(($oName:pat, $oVar:ident, $oT:ty, $oMethod:expr)),*], 
     required: [$(($name:pat, $var:ident, $t:ty, $method:expr)),*], $err:expr) => {
        {
            $(let mut $oVar: Option<$oT> = None;)*
            $(let mut $var: Option<$t> = None;)*
            for attr in $attrs.iter() {
                match attr.name.local_name[] {
                    $($oName => $oVar = $oMethod(attr.value.clone()),)*
                    $($name => $var = $method(attr.value.clone()),)*
                    _ => {}
                }
            }
            if !(true $(&& $var.is_some())*) {
                return Err($err);
            }
            (($($oVar),*), ($($var.unwrap()),*))
        }
    }
}

macro_rules! parse_tag {
    ($parser:expr, $close_tag:expr, $($open_tag:expr => $open_method:expr),*) => {
        loop {
            match $parser.next() {
                StartElement {name, attributes, ..} => {
                    if false {}
                    $(else if name.local_name[] == $open_tag {
                        match $open_method(attributes) {
                            Ok(()) => {},
                            Err(e) => return Err(e)
                        };
                    })*
                }
                EndElement {name, ..} => {
                    if name.local_name[] == $close_tag {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
}

#[deriving(Show)]
pub enum TiledError {
    MissingAttributes(String),
    DecompressingError(IoError),
    DecodingError(FromBase64Error),
    Other(String)
}

pub type Properties = HashMap<String, String>;

fn parse_properties<B: Buffer>(parser: &mut EventReader<B>) -> Result<Properties, TiledError> {
    let mut p = HashMap::new();
    parse_tag!(parser, "properties",
               "property" => |attrs:Vec<Attribute>| {
                    let ((), (k, v)) = get_attrs!(
                        attrs,
                        optionals: [],
                        required: [("name", key, String, |v| Some(v)),
                                   ("value", value, String, |v| Some(v))],
                        MissingAttributes("property must have a name and a value".to_string()));
                    p.insert(k, v);
                    Ok(())
               });
    Ok(p)
}

#[deriving(Show)]
pub struct Map {
    version: String,
    orientation: Orientation,
    width: int,
    height: int,
    tile_width: int,
    tile_height: int,
    tilesets: Vec<Tileset>,
    layers: Vec<Layer>,
    properties: Properties
}

impl Map {
    pub fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<Attribute>) -> Result<Map, TiledError>  {
        let ((), (v, o, w, h, tw, th)) = get_attrs!(
            attrs, 
            optionals: [], 
            required: [("version", version, String, |v| Some(v)),
                       ("orientation", orientation, Orientation, |v:String| from_str(v[])),
                       ("width", width, int, |v:String| from_str(v[])),
                       ("height", height, int, |v:String| from_str(v[])),
                       ("tilewidth", tile_width, int, |v:String| from_str(v[])),
                       ("tileheight", tile_height, int, |v:String| from_str(v[]))],
            MissingAttributes("map must have a version, width and height with correct types".to_string()));

        let mut tilesets = Vec::new();
        let mut layers = Vec::new();
        let mut properties = HashMap::new();
        parse_tag!(parser, "map", 
                   "tileset" => |attrs| {
                        tilesets.push(try!(Tileset::new(parser, attrs)));
                        Ok(())
                   },
                   "layer" => |attrs| {
                        layers.push(try!(Layer::new(parser, attrs, w as uint)));
                        Ok(())
                   },
                   "properties" => |_| {
                        properties = try!(parse_properties(parser));
                        Ok(())
                   });
        Ok(Map {version: v, orientation: o,
                width: w, height: h, 
                tile_width: tw, tile_height: th,
                tilesets: tilesets, layers: layers,
                properties: properties})
    }

    pub fn get_tileset_by_gid(&self, gid: uint) -> Option<&Tileset> {
        let mut maximum_gid: int = -1;
        let mut maximum_ts = None;
        for tileset in self.tilesets.iter() {
            if tileset.first_gid as int > maximum_gid && tileset.first_gid < gid {
                maximum_gid = tileset.first_gid as int;
                maximum_ts = Some(tileset);
            }
        }
        maximum_ts
    }
}

#[deriving(Show)]
pub enum Orientation {
    Orthogonal,
    Isometric,
    Staggered
}

impl FromStr for Orientation {
    fn from_str(s: &str) -> Option<Orientation> {
        match s {
            "orthogonal" => Some(Orthogonal),
            "isometric" => Some(Isometric),
            "Staggered" => Some(Staggered),
            _ => None
        }
    }
}

#[deriving(Show)]
pub struct Tileset {
    pub first_gid: uint,
    pub name: String,
    pub images: Vec<Image>
}

impl Tileset {
    pub fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<Attribute>) -> Result<Tileset, TiledError> {
        let ((), (g, n)) = get_attrs!(
           attrs,
           optionals: [],
           required: [("firstgid", first_gid, uint, |v:String| from_str(v[])),
                      ("name", name, String, |v| Some(v))],
           MissingAttributes("tileset must have a firstgid and name with correct types".to_string()));

        let mut images = Vec::new();
        parse_tag!(parser, "tileset",
                   "image" => |attrs| {
                        images.push(try!(Image::new(parser, attrs)));
                        Ok(())
                   });
        Ok(Tileset {first_gid: g, name: n, images: images})
   }
}

#[deriving(Show)]
pub struct Image {
    pub source: String,
    pub width: int,
    pub height: int
}

impl Image {
    pub fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<Attribute>) -> Result<Image, TiledError> {
        let ((), (s, w, h)) = get_attrs!(
            attrs,
            optionals: [],
            required: [("source", source, String, |v| Some(v)),
                       ("width", width, int, |v:String| from_str(v[])),
                       ("height", height, int, |v:String| from_str(v[]))],
            MissingAttributes("image must have a source, width and height with correct types".to_string()));
        
        parse_tag!(parser, "image", "" => |_| Ok(()));
        Ok(Image {source: s, width: w, height: h})
    }
}

#[deriving(Show)]
pub struct Layer {
    pub name: String,
    pub opacity: f32,
    pub visible: bool,
    pub tiles: Vec<Vec<u32>>,
    pub properties: Properties
}

impl Layer {
    pub fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<Attribute>, width: uint) -> Result<Layer, TiledError> {
        let ((o, v), n) = get_attrs!(
            attrs,
            optionals: [("opacity", opacity, f32, |v:String| from_str(v[])),
                        ("visible", visible, bool, |v:String| from_str(v[]).map(|x:int| x == 1))],
            required: [("name", name, String, |v| Some(v))],
            MissingAttributes("layer must have a name".to_string()));
        let mut tiles = Vec::new();
        let mut properties = HashMap::new();
        parse_tag!(parser, "layer",
                   "data" => |attrs| {
                        tiles = try!(parse_data(parser, attrs, width));
                        Ok(())
                   },
                   "properties" => |_| {
                        properties = try!(parse_properties(parser));
                        Ok(())
                   });
        Ok(Layer {name: n, opacity: o.unwrap_or(1.0), visible: v.unwrap_or(true), tiles: tiles,
                  properties: properties})
    }
}

fn parse_data<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<Attribute>, width: uint) -> Result<Vec<Vec<u32>>, TiledError> {
    let ((), (e, c)) = get_attrs!(
        attrs,
        optionals: [],
        required: [("encoding", encoding, String, |v| Some(v)),
                   ("compression", compression, String, |v| Some(v))],
        MissingAttributes("data must have an encoding and a compression".to_string()));
    if !(e[] == "base64" && c[] == "zlib") {
        return Err(Other("Only base64 and zlib allowed for the moment".to_string()));
    }
    loop {
        match parser.next() {
            Characters(s) => {
                match s[].trim().from_base64() {
                    Ok(v) => {
                        let mut zd = ZlibDecoder::new(BufReader::new(v[]));
                        let mut data = Vec::new();
                        let mut row = Vec::new();
                        loop {
                            match zd.read_le_u32() {
                                Ok(v) => row.push(v),
                                Err(IoError{kind, ..}) if kind == EndOfFile => return Ok(data),
                                Err(e) => return Err(DecompressingError(e))
                            }
                            if row.len() == width {
                                data.push(row);
                                row = Vec::new();
                            }
                        }
                    }
                    Err(e) => return Err(DecodingError(e))
                }
            }
            EndElement {name, ..} => {
                if name.local_name[] == "data" {
                    return Ok(Vec::new());
                }
            }
            _ => {}
        }
    }
}

pub fn parse<B: Buffer>(parser: &mut EventReader<B>) -> Result<Map, TiledError>{
    loop {
        match parser.next() {
            StartElement {name, attributes, ..}  => {
                if name.local_name[] == "map" {
                    return Map::new(parser, attributes);
                }
            }
            _ => {}
        }
    }
}
