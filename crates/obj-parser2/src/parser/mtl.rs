use std::io::BufRead;
use std::str;

use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::combinator::map;
use nom::sequence::{delimited, tuple};
use nom::IResult;

use super::common::*;
use crate::def_string_line;

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

pub fn ka_ambient_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, tuple_result) = delimited(tag("Ka"), float_triple, end_of_line_maybe_comment)(input)?;
    let (r, g, b) = tuple_result;
    Ok((i, MtlLine::AmbientColor(r, g, b)))
}

pub fn transmission_filter_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, tuple_result) = delimited(tag("Tf"), float_triple, end_of_line_maybe_comment)(input)?;
    let (r, g, b) = tuple_result;
    Ok((i, MtlLine::TransmissionFilter(r, g, b)))
}

pub fn kd_diffuse_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, tuple_result) = delimited(tag("Kd"), float_triple, end_of_line_maybe_comment)(input)?;
    let (r, g, b) = tuple_result;
    Ok((i, MtlLine::DiffuseColor(r, g, b)))
}

pub fn ks_specular_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, tuple_result) = delimited(tag("Ks"), float_triple, end_of_line_maybe_comment)(input)?;
    let (r, g, b) = tuple_result;
    Ok((i, MtlLine::SpecularColor(r, g, b)))
}

pub fn ke_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, tuple_result) = delimited(tag("Ke"), float_triple, end_of_line_maybe_comment)(input)?;
    let (r, g, b) = tuple_result;
    Ok((i, MtlLine::KeColor(r, g, b)))
}

pub fn transparency_line_d(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((tag("d"), float, end_of_line_maybe_comment)),
        |(_, float_result, _)| MtlLine::TransparencyD(float_result),
    )(input)
}

pub fn transparency_line_tr(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((tag("Tr"), float, end_of_line_maybe_comment)),
        |(_, float_result, _)| MtlLine::TransparencyTr(float_result),
    )(input)
}

pub fn optical_density_line(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((tag("Ni"), float, end_of_line_maybe_comment)),
        |(_, float_result, _)| MtlLine::OpticalDensity(float_result),
    )(input)
}

pub fn illum_line(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((tag("illum"), unsigned_integer, end_of_line_maybe_comment)),
        |(_, uint_result, _)| MtlLine::IlluminationModel(uint_result),
    )(input)
}

pub fn sharpness_line(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((
            tag("sharpness"),
            unsigned_integer,
            end_of_line_maybe_comment,
        )),
        |(_, uint_result, _)| MtlLine::Sharpness(uint_result),
    )(input)
}

pub fn specular_exponent_line(input: &str) -> IResult<&str, MtlLine> {
    map(
        tuple((tag("Ns"), float, end_of_line_maybe_comment)),
        |(_, float_result, _)| MtlLine::SpecularExponent(float_result),
    )(input)
}

pub fn comment_line(input: &str) -> IResult<&str, MtlLine> {
    let (input, comment) = comment(input)?;
    Ok((input, MtlLine::Comment(comment.trim().to_string())))
}

pub fn blank_line(input: &str) -> IResult<&str, MtlLine> {
    let (i, _) = end_of_line_maybe_comment(input)?;
    Ok((i, MtlLine::Blank))
}

pub fn parse_mtl_line(input: &str) -> IResult<&str, MtlLine> {
    alt((
        newmtl_line,
        ambient_map_line,
        diffuse_map_line,
        specular_map_line,
        bump_map_line,
        ka_ambient_line,
        kd_diffuse_line,
        ks_specular_line,
        ke_line,
        transparency_line_d,
        transparency_line_tr,
        optical_density_line,
        illum_line,
        sharpness_line,
        specular_exponent_line,
        comment_line,
        blank_line,
    ))(input)
}

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
        let mut line = String::new();
        let read_result = self.reader.read_line(&mut line);
        match read_result {
            Ok(len) => {
                if len > 0 {
                    match parse_mtl_line(line.as_str()) {
                        Ok((_, o)) => Some(o),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn can_parse_newmtl_line() {
        let (_, b) = newmtl_line("newmtl material/name\n").unwrap();
        assert_eq!(b, MtlLine::NewMtl("material/name".to_string()));
    }

    #[test]
    fn can_parse_ambient_map_line() {
        let (_, b) = ambient_map_line("map_Ka sometexture.png\n").unwrap();
        assert_eq!(b, MtlLine::AmbientMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_diffuse_map_line() {
        let (_, b) = diffuse_map_line("map_Kd sometexture.png\n").unwrap();
        assert_eq!(b, MtlLine::DiffuseMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_specular_map_line() {
        let (_, b) = specular_map_line("map_Ks sometexture.png\n").unwrap();
        assert_eq!(b, MtlLine::SpecularMap("sometexture.png".to_string()));
    }

    #[test]
    fn can_parse_transparency_d_line() {
        let (_, b) = transparency_line_d("d 0.5\n").unwrap();
        assert_eq!(b, MtlLine::TransparencyD(0.5));
    }

    #[test]
    fn can_parse_transparency_tr_line() {
        let (_, b) = transparency_line_tr("Tr 0.5\n").unwrap();
        assert_eq!(b, MtlLine::TransparencyTr(0.5));
    }

    #[test]
    fn can_parse_illumination_model_line() {
        let (_, b) = illum_line("illum 2\n").unwrap();
        assert_eq!(b, MtlLine::IlluminationModel(2));
    }

    #[test]
    fn can_parse_specular_exponent_line() {
        let (_, b) = specular_exponent_line("Ns 2\n").unwrap();
        assert_eq!(b, MtlLine::SpecularExponent(2.0));
    }

    #[test]
    fn can_parse_ka_ambient_line() {
        let vline = "Ka 1.000 1.000 1.000  \r\n";
        let v = ka_ambient_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::AmbientColor(1.0, 1.0, 1.0));
    }
    #[test]
    fn can_parse_ka_diffuse_line() {
        let vline = "Kd 1.000 1.000 1.000  \r\n";
        let v = kd_diffuse_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::DiffuseColor(1.0, 1.0, 1.0));
    }
    #[test]
    fn can_parse_ka_specular_line() {
        let vline = "Ks 1.000 1.000 1.000  \r\n";
        let v = ks_specular_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, MtlLine::SpecularColor(1.0, 1.0, 1.0));
    }
}
