use std::io::BufRead;
use std::str;

use nom::eol;

/// http://paulbourke.net/dataformats/mtl/
///
use super::common::*;

#[derive(PartialEq, Debug)]
pub enum MtlLine {
    Comment(String),
    NewMtl(String),
    AmbientMap(String),
    DiffuseMap(String),
    SpecularMap(String),
    BumpMap(String),

    AmbientColor(f32, f32, f32),
    DiffuseColor(f32, f32, f32),
    SpecularColor(f32, f32, f32),
    KeColor(f32, f32, f32), // unknown, but blender writes it

    TransmissionFilter(f32, f32, f32),

    OpticalDensity(f32),
    SpecularExponent(f32),
    TransparencyD(f32),
    TransparencyTr(f32),
    IlluminationModel(u32),
    Sharpness(u32),
    Blank,
}

def_string_line!(newmtl_line, "newmtl", MtlLine, NewMtl);
def_string_line!(ambient_map_line, "map_Ka", MtlLine, AmbientMap);
def_string_line!(diffuse_map_line, "map_Kd", MtlLine, DiffuseMap);
def_string_line!(specular_map_line, "map_Ks", MtlLine, SpecularMap);
def_string_line!(bump_map_line, "map_bump", MtlLine, BumpMap);

named!(pub ka_ambient_line< &[u8], MtlLine >, map!(
    delimited!(tag!("Ka"), float_triple, end_of_line), |(r,g,b)| MtlLine::AmbientColor(r,g,b)
));

named!(pub transmission_filter_line< &[u8], MtlLine >, map!(
    delimited!(tag!("Tf"), float_triple, end_of_line), |(r,g,b)| MtlLine::TransmissionFilter(r,g,b)
));

named!(pub kd_diffuse_line< &[u8], MtlLine >, map!(
    delimited!(tag!("Kd"), float_triple, end_of_line), |(r,g,b)| MtlLine::DiffuseColor(r,g,b)
));

named!(pub ks_specular_line< &[u8], MtlLine >, map!(
    delimited!(tag!("Ks"), float_triple, end_of_line), |(r,g,b)| MtlLine::SpecularColor(r,g,b)
));

named!(pub ke_line< &[u8], MtlLine >, map!(
    delimited!(tag!("Ke"), float_triple, end_of_line), |(r,g,b)| MtlLine::KeColor(r,g,b)
));

named!(pub transparency_line_d< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("d"), float, end_of_line)), MtlLine::TransparencyD
));

named!(pub transparency_line_tr< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("Tr"), float, end_of_line)), MtlLine::TransparencyTr
));

named!(pub optical_density_line< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("Ni"), float, end_of_line)),  MtlLine::OpticalDensity
));

named!(pub illum_line< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("illum"), uint, end_of_line)), MtlLine::IlluminationModel
));

named!(pub sharpness_line< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("sharpness"), uint, end_of_line)), MtlLine::Sharpness
));

named!(pub specular_exponent_line< &[u8], MtlLine >, map!(
    sp!(delimited!(tag!("Ns"), float, end_of_line)), MtlLine::SpecularExponent
));

named!(
    comment_line<MtlLine>,
    map!(sp!(comment), |s| MtlLine::Comment(
        str::from_utf8(s).unwrap().trim().to_string()
    ))
);

named!(blank_line<MtlLine>, map!(sp!(eol), |_| MtlLine::Blank));

named!(
    parse_mtl_line<MtlLine>,
    alt!(
        newmtl_line
            | ambient_map_line
            | diffuse_map_line
            | specular_map_line
            | bump_map_line
            | ka_ambient_line
            | kd_diffuse_line
            | ks_specular_line
            | ke_line
            | transparency_line_d
            | transparency_line_tr
            | optical_density_line
            | illum_line
            | sharpness_line
            | specular_exponent_line
            | comment_line
            | blank_line
    )
);

pub struct MtlParser<R> {
    reader: R,
}

impl<R> MtlParser<R>
where
    R: BufRead,
{
    pub fn new(reader: R) -> Self {
        MtlParser { reader }
    }
}

impl<R> Iterator for MtlParser<R>
where
    R: BufRead,
{
    type Item = MtlLine;

    fn next(&mut self) -> Option<Self::Item> {
        use nom::IResult;
        let mut line = String::new();
        let read_result = self.reader.read_line(&mut line);
        match read_result {
            Ok(len) => {
                if len > 0 {
                    let result = parse_mtl_line(line.as_bytes());
                    match result {
                        IResult::Done(_, o) => Some(o),
                        IResult::Error(_e) => None,
                        IResult::Incomplete(_) => self.next(),
                    }
                } else {
                    None
                }
            }
            Err(_o) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs::File;
    use std::io::BufReader;

    use super::*;
    pub fn read_file(filename: &str) -> Result<MtlParser<BufReader<File>>, Box<dyn Error>> {
        let file = File::open(filename)?;
        let reader = BufReader::new(file);
        Ok(MtlParser { reader })
    }

    #[test]
    fn mtl_parser_can_load_from_file() -> Result<(), Box<dyn Error>> {
        let parser = read_file("assets/transparent_blue_cube.mtl")?;
        let parsed_lines = parser.collect::<Vec<_>>();
        println!("{:?}", parsed_lines);
        assert_eq!(parsed_lines.len(), 12);
        Ok(())
    }

    #[test]
    fn can_parse_newmtl_line() {
        let (_, b) = newmtl_line("newmtl material/name\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::NewMtl("material/name".to_string()));
    }

    #[test]
    fn can_parse_ambient_map_line() {
        let (_, b) = ambient_map_line("map_Ka sometexture.png\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::AmbientMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_diffuse_map_line() {
        let (_, b) = diffuse_map_line("map_Kd sometexture.png\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::DiffuseMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_specular_map_line() {
        let (_, b) = specular_map_line("map_Ks sometexture.png\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::SpecularMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_transparency_d_line() {
        let (_, b) = transparency_line_d("d 0.5\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::TransparencyD(0.5));
    }

    #[test]
    fn can_parse_transparency_tr_line() {
        let (_, b) = transparency_line_tr("Tr 0.5\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::TransparencyTr(0.5));
    }

    #[test]
    fn can_parse_illumination_model_line() {
        let (_, b) = illum_line("illum 2\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::IlluminationModel(2));
    }

    #[test]
    fn can_parse_specular_exponent_line() {
        let (_, b) = specular_exponent_line("Ns 2\n".as_bytes()).unwrap();
        assert_eq!(b, MtlLine::SpecularExponent(2.0));
    }

    #[test]
    fn can_parse_ka_ambient_line() {
        let vline = "Ka 1.000 1.000 1.000  \r\n".as_bytes();
        let v = ka_ambient_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::AmbientColor(1.0, 1.0, 1.0));
    }
    #[test]
    fn can_parse_ka_diffuse_line() {
        let vline = "Kd 1.000 1.000 1.000  \r\n".as_bytes();
        let v = kd_diffuse_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::DiffuseColor(1.0, 1.0, 1.0));
    }
    #[test]
    fn can_parse_ka_specular_line() {
        let vline = "Ks 1.000 1.000 1.000  \r\n".as_bytes();
        let v = ks_specular_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::SpecularColor(1.0, 1.0, 1.0));
    }
}
