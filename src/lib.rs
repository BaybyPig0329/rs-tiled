#![allow(unstable)]
#![feature(slicing_syntax)]
extern crate flate2;
extern crate xml;
extern crate serialize;

use std::io::{BufReader, IoError, EndOfFile};
use std::str::FromStr;
use std::collections::HashMap;
use std::fmt;
use xml::reader::EventReader;
use xml::reader::events::XmlEvent::*;
use xml::attribute::OwnedAttribute;
use serialize::base64::{FromBase64, FromBase64Error};
use flate2::reader::ZlibDecoder;
use std::num::from_str_radix;

// Loops through the attributes once and pulls out the ones we ask it to. It
// will check that the required ones are there. This could have been done with
// attrs.find but that would be inefficient.
//
// This is probably a really terrible way to do this. It does cut down on lines
// though which is nice.
macro_rules! get_attrs {
    ($attrs:expr, optionals: [$(($oName:pat, $oVar:ident, $oMethod:expr)),*], 
     required: [$(($name:pat, $var:ident, $method:expr)),*], $err:expr) => {
        {
            $(let mut $oVar = None;)*
            $(let mut $var = None;)*
            for attr in $attrs.iter() {
                match attr.name.local_name.as_slice() {
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

// Goes through the children of the tag and will call the correct function for
// that child. Closes the tag
//
// Not quite as bad.
macro_rules! parse_tag {
    ($parser:expr, $close_tag:expr, $($open_tag:expr => $open_method:expr),*) => {
        loop {
            match $parser.next() {
                StartElement {name, attributes, ..} => {
                    if false {}
                    $(else if name.local_name == $open_tag {
                        match $open_method(attributes) {
                            Ok(()) => {},
                            Err(e) => return Err(e)
                        };
                    })*
                }
                EndElement {name, ..} => {
                    if name.local_name == $close_tag {
                        break;
                    }
                }
                EndDocument => return Err(TiledError::PrematureEnd("Document ended before we expected.".to_string())),
                _ => {}
            }
        }
    }
}

#[derive(Show)]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8
}

impl FromStr for Colour {
    fn from_str(s: &str) -> Option<Colour> {
        let s = if s.starts_with("#") {
            &s[1..]
        } else { 
            s 
        };
        if s.len() != 6 {
            return None;
        }
        let r = from_str_radix(&s[0..2], 16);
        let g = from_str_radix(&s[2..4], 16);
        let b = from_str_radix(&s[4..6], 16);
        if r.is_some() && g.is_some() && b.is_some() {
            return Some(Colour {red: r.unwrap(), green: g.unwrap(), blue: b.unwrap()})
        }
        None
    }
}

/// Errors which occured when parsing the file
#[derive(Show)]
pub enum TiledError {
    /// A attribute was missing, had the wrong type of wasn't formated
    /// correctly.
    MalformedAttributes(String),
    /// An error occured when decompressing using the 
    /// [flate2](https://github.com/alexcrichton/flate2-rs) crate.
    DecompressingError(IoError),
    DecodingError(FromBase64Error),
    PrematureEnd(String),
    Other(String)
}

impl fmt::Display for TiledError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            TiledError::MalformedAttributes(ref s) => write!(fmt, "{}", s),
            TiledError::DecompressingError(ref e) => write!(fmt, "{}", e),
            TiledError::DecodingError(ref e) => write!(fmt, "{}", e),
            TiledError::PrematureEnd(ref e) => write!(fmt, "{}", e),
            TiledError::Other(ref s) => write!(fmt, "{}", s),
        }
    }
}

pub type Properties = HashMap<String, String>;

fn parse_properties<B: Buffer>(parser: &mut EventReader<B>) -> Result<Properties, TiledError> {
    let mut p = HashMap::new();
    parse_tag!(parser, "properties",
               "property" => |&mut:attrs:Vec<OwnedAttribute>| {
                    let ((), (k, v)) = get_attrs!(
                        attrs,
                        optionals: [],
                        required: [("name", key, |&:v| Some(v)),
                                   ("value", value, |&:v| Some(v))],
                        TiledError::MalformedAttributes("property must have a name and a value".to_string()));
                    p.insert(k, v);
                    Ok(())
               });
    Ok(p)
}

/// All Tiled files will be parsed i32o this. Holds all the layers and tilesets
#[derive(Show)]
pub struct Map {
    pub version: String,
    pub orientation: Orientation,
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub tilesets: Vec<Tileset>,
    pub layers: Vec<Layer>,
    pub object_groups: Vec<ObjectGroup>,
    pub properties: Properties,
    pub background_colour: Option<Colour>,
}

impl Map {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>) -> Result<Map, TiledError>  {
        let (c, (v, o, w, h, tw, th)) = get_attrs!(
            attrs, 
            optionals: [("backgroundcolor", colour, |&:v:String| v.parse())], 
            required: [("version", version, |&:v| Some(v)),
                       ("orientation", orientation, |&:v:String| v.parse()),
                       ("width", width, |&:v:String| v.parse()),
                       ("height", height, |&:v:String| v.parse()),
                       ("tilewidth", tile_width, |&:v:String| v.parse()),
                       ("tileheight", tile_height, |&:v:String| v.parse())],
            TiledError::MalformedAttributes("map must have a version, width and height with correct types".to_string()));

        let mut tilesets = Vec::new();
        let mut layers = Vec::new();
        let mut properties = HashMap::new();
        let mut object_groups = Vec::new();
        parse_tag!(parser, "map", 
                   "tileset" => |&mut: attrs| {
                        tilesets.push(try!(Tileset::new(parser, attrs)));
                        Ok(())
                   },
                   "layer" => |&mut:attrs| {
                        layers.push(try!(Layer::new(parser, attrs, w )));
                        Ok(())
                   },
                   "properties" => |&mut:_| {
                        properties = try!(parse_properties(parser));
                        Ok(())
                   },
                   "objectgroup" => |&mut:attrs| {
                       object_groups.push(try!(ObjectGroup::new(parser, attrs)));
                       Ok(())
                   });
        Ok(Map {version: v, orientation: o,
                width: w, height: h, 
                tile_width: tw, tile_height: th,
                tilesets: tilesets, layers: layers, object_groups: object_groups,
                properties: properties,
                background_colour: c,})
    }

    /// This function will return the correct Tileset given a GID.
    pub fn get_tileset_by_gid(&self, gid: u32) -> Option<&Tileset> {
        let mut maximum_gid: i32 = -1;
        let mut maximum_ts = None;
        for tileset in self.tilesets.iter() {
            if tileset.first_gid as i32 > maximum_gid && tileset.first_gid < gid {
                maximum_gid = tileset.first_gid as i32;
                maximum_ts = Some(tileset);
            }
        }
        maximum_ts
    }
}

#[derive(Show)]
pub enum Orientation {
    Orthogonal,
    Isometric,
    Staggered
}

impl FromStr for Orientation {
    fn from_str(s: &str) -> Option<Orientation> {
        match s {
            "orthogonal" => Some(Orientation::Orthogonal),
            "isometric" => Some(Orientation::Isometric),
            "Staggered" => Some(Orientation::Staggered),
            _ => None
        }
    }
}

/// A tileset, usually the tilesheet image.
#[derive(Show)]
pub struct Tileset {
    /// The GID of the first tile stored
    pub first_gid: u32,
    pub name: String,
    pub tile_width: u32,
    pub tile_height: u32,
    pub spacing: u32,
    pub margin: u32,
    /// The Tiled spec says that a tileset can have mutliple images so a `Vec` 
    /// is used. Usually you will only use one.
    pub images: Vec<Image>
}

impl Tileset {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>) -> Result<Tileset, TiledError> {
        let ((s, m), (g, n, w, h)) = get_attrs!(
           attrs,
           optionals: [("spacing", spacing, |&:v:String| v.parse()),
                       ("margin", margin, |&:v:String| v.parse())],
           required: [("firstgid", first_gid, |&:v:String| v.parse()),
                      ("name", name, |&:v| Some(v)),
                      ("tilewidth", width, |&:v:String| v.parse()),
                      ("tileheight", height, |&:v:String| v.parse())],
           TiledError::MalformedAttributes("tileset must have a firstgid, name tile width and height with correct types".to_string()));

        let mut images = Vec::new();
        parse_tag!(parser, "tileset",
                   "image" => |&mut:attrs| {
                        images.push(try!(Image::new(parser, attrs)));
                        Ok(())
                   });
        Ok(Tileset {first_gid: g, 
                    name: n, 
                    tile_width: w, tile_height: h, 
                    spacing: s.unwrap_or(0),
                    margin: m.unwrap_or(0),
                    images: images})
   }
}

#[derive(Show)]
pub struct Image {
    /// The filepath of the image
    pub source: String,
    pub width: i32,
    pub height: i32,
    pub transparent_colour: Option<Colour>,
}

impl Image {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>) -> Result<Image, TiledError> {
        let (c, (s, w, h)) = get_attrs!(
            attrs,
            optionals: [("trans", trans, |&:v:String| v.parse())],
            required: [("source", source, |&:v| Some(v)),
                       ("width", width, |&:v:String| v.parse()),
                       ("height", height, |&:v:String| v.parse())],
            TiledError::MalformedAttributes("image must have a source, width and height with correct types".to_string()));
        
        parse_tag!(parser, "image", "" => |&:_| Ok(()));
        Ok(Image {source: s, width: w, height: h, transparent_colour: c})
    }
}

#[derive(Show)]
pub struct Layer {
    pub name: String,
    pub opacity: f32,
    pub visible: bool,
    /// The tiles are arranged in rows. Each tile is a number which can be used
    ///  to find which tileset it belongs to and can then be rendered.
    pub tiles: Vec<Vec<u32>>,
    pub properties: Properties
}

impl Layer {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>, width: u32) -> Result<Layer, TiledError> {
        let ((o, v), n) = get_attrs!(
            attrs,
            optionals: [("opacity", opacity, |&:v:String| v.parse()),
                        ("visible", visible, |&:v:String| v.parse().map(|x:i32| x == 1))],
            required: [("name", name, |&:v| Some(v))],
            TiledError::MalformedAttributes("layer must have a name".to_string()));
        let mut tiles = Vec::new();
        let mut properties = HashMap::new();
        parse_tag!(parser, "layer",
                   "data" => |&mut:attrs| {
                        tiles = try!(parse_data(parser, attrs, width));
                        Ok(())
                   },
                   "properties" => |&mut:_| {
                        properties = try!(parse_properties(parser));
                        Ok(())
                   });
        Ok(Layer {name: n, opacity: o.unwrap_or(1.0), visible: v.unwrap_or(true), tiles: tiles,
                  properties: properties})
    }
}

#[derive(Show)]
pub struct ObjectGroup {
    pub name: String,
    pub opacity: f32,
    pub visible: bool,
    pub objects: Vec<Object>,
    pub colour: Option<Colour>,
}

impl ObjectGroup {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>) -> Result<ObjectGroup, TiledError> {
        let ((o, v, c), n) = get_attrs!(
            attrs,
            optionals: [("opacity", opacity, |&:v:String| v.parse()),
                        ("visible", visible, |&:v:String| v.parse().map(|x:i32| x == 1)),
                        ("color", colour, |&:v:String| v.parse())],
            required: [("name", name, |&:v| Some(v))],
            TiledError::MalformedAttributes("object groups must have a name".to_string()));
        let mut objects = Vec::new();
        parse_tag!(parser, "objectgroup",
                   "object" => |&mut:attrs| {
                        objects.push(try!(Object::new(parser, attrs)));
                        Ok(())
                   });
        Ok(ObjectGroup {name: n, 
                        opacity: o.unwrap_or(1.0), visible: v.unwrap_or(true), 
                        objects: objects,
                        colour: c})
    }
}

#[derive(Show)]
pub enum Object {
     Rect { x: i32,  y: i32,  width: u32,  height: u32,  visible: bool},
     Ellipse { x: i32,  y: i32,  width: u32,  height: u32,  visible: bool},
     Polyline { x: i32,  y: i32,  points: Vec<(i32, i32)>,  visible: bool},
     Polygon { x: i32,  y: i32,  points: Vec<(i32, i32)>,  visible: bool}
}

impl Object {
    fn new<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>) -> Result<Object, TiledError> {
        let ((w, h, v), (x, y)) = get_attrs!(
            attrs,
            optionals: [("width", width, |&:v:String| v.parse()),
                        ("height", height, |&:v:String| v.parse()),
                        ("visible", visible, |&:v:String| v.parse())],
            required: [("x", x, |&:v:String| v.parse()),
                       ("y", y, |&:v:String| v.parse())],
            TiledError::MalformedAttributes("objects must have an x and a y number".to_string()));
        let mut obj = None;
        let v = v.unwrap_or(true);
        parse_tag!(parser, "object",
                   "ellipse" => |&mut:_| {
                        if w.is_none() || h.is_none() {
                            return Err(TiledError::MalformedAttributes("An ellipse must have a width and height".to_string()));
                        }
                        let (w, h) = (w.unwrap(), h.unwrap());
                        obj = Some(Object::Ellipse {x: x, y: y, 
                                            width: w , height: h ,
                                            visible: v});
                        Ok(())
                    },
                    "polyline" => |&mut:attrs| {
                        obj = Some(try!(Object::new_polyline(x, y, v, attrs)));
                        Ok(())
                    },
                    "polygon" => |&mut:attrs| {
                        obj = Some(try!(Object::new_polygon(x, y, v, attrs)));
                        Ok(())
                    });
        if obj.is_some() {
            Ok(obj.unwrap())
        } else if w.is_some() && h.is_some() {
            let w = w.unwrap();
            let h = h.unwrap();
            Ok(Object::Rect {x: x, y: y, width: w, height: h, visible: v})
        } else {
            Err(TiledError::MalformedAttributes("A rect must have a width and a height".to_string()))
        }
    }

    fn new_polyline(x: i32, y: i32, v: bool, attrs: Vec<OwnedAttribute>) -> Result<Object, TiledError> {
        let ((), s) = get_attrs!(
            attrs,
            optionals: [],
            required: [("points", points, |&:v| Some(v))],
            TiledError::MalformedAttributes("A polyline must have points".to_string()));
       let points = try!(Object::parse_points(s));
       Ok(Object::Polyline {x: x, y: y, points: points, visible: v})
    }

    fn new_polygon(x: i32, y: i32, v: bool, attrs: Vec<OwnedAttribute>) -> Result<Object, TiledError> {
        let ((), s) = get_attrs!(
            attrs,
            optionals: [],
            required: [("points", points, |&:v| Some(v))],
            TiledError::MalformedAttributes("A polygon must have points".to_string()));
       let points = try!(Object::parse_points(s));
       Ok(Object::Polygon {x: x, y: y, points: points, visible: v})
    }

    fn parse_points(s: String) -> Result<Vec<(i32, i32)>, TiledError> {
        let pairs = s.split(' ');
        let mut points = Vec::new();
        for v in pairs.map(|&:p| p.splitn(1, ',')) {
            let v: Vec<&str> = v.clone().collect();
            if v.len() != 2 {
                return Err(TiledError::MalformedAttributes("one of a polyline's points does not have an x and y coordinate".to_string()));
            }
            let (x, y) = (v[0].parse(), v[1].parse());
            if x.is_none() || y.is_none() {
                return Err(TiledError::MalformedAttributes("one of polyline's points does not have i32eger coordinates".to_string()));
            }
            points.push((x.unwrap(), y.unwrap()));
        }
        Ok(points)
    }
}

fn parse_data<B: Buffer>(parser: &mut EventReader<B>, attrs: Vec<OwnedAttribute>, width: u32) -> Result<Vec<Vec<u32>>, TiledError> {
    let ((), (e, c)) = get_attrs!(
        attrs,
        optionals: [],
        required: [("encoding", encoding, |&:v| Some(v)),
                   ("compression", compression, |&:v| Some(v))],
        TiledError::MalformedAttributes("data must have an encoding and a compression".to_string()));
    if !(e == "base64" && c == "zlib") {
        return Err(TiledError::Other("Only base64 and zlib allowed for the moment".to_string()));
    }
    loop {
        match parser.next() {
            Characters(s) => {
                match s.trim().from_base64() {
                    Ok(v) => {
                        let mut zd = ZlibDecoder::new(BufReader::new(v.as_slice()));
                        let mut data = Vec::new();
                        let mut row = Vec::new();
                        loop {
                            match zd.read_le_u32() {
                                Ok(v) => row.push(v),
                                Err(IoError{kind, ..}) if kind == EndOfFile => return Ok(data),
                                Err(e) => return Err(TiledError::DecompressingError(e))
                            }
                            if row.len() as u32 == width {
                                data.push(row);
                                row = Vec::new();
                            }
                        }
                    }
                    Err(e) => return Err(TiledError::DecodingError(e))
                }
            }
            EndElement {name, ..} => {
                if name.local_name == "data" {
                    return Ok(Vec::new());
                }
            }
            _ => {}
        }
    }
}

/// Parse a buffer hopefully containing the contents of a Tiled file and try to
/// parse it.
pub fn parse<B: Buffer>(reader: B) -> Result<Map, TiledError> {
    let mut parser = EventReader::new(reader);
    loop {
        match parser.next() {
            StartElement {name, attributes, ..}  => {
                if name.local_name == "map" {
                    return Map::new(&mut parser, attributes);
                }
            }
            EndDocument => return Err(TiledError::PrematureEnd("Document ended before map was parsed".to_string())),
            _ => {}
        }
    }
}
