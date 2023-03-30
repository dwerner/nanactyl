use std::io::BufRead;
use std::str;

use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::complete::{multispace0, multispace1, space1};
use nom::combinator::{map, opt};
use nom::sequence::{delimited, preceded, tuple};
use nom::IResult;

use crate::def_string_line;
/// http://paulbourke.net/dataformats/obj/
use crate::parser::common::*;

#[derive(PartialEq, Eq, Debug)]
pub struct FaceIndex(pub u32, pub Option<u32>, pub Option<u32>);

#[derive(PartialEq, Debug)]
pub enum ObjLine {
    Comment(String),
    ObjectName(String),
    GroupName(String),
    MtlLib(String),
    UseMtl(String),
    SmoothShading(String),
    Vertex(f32, f32, f32, Option<f32>), // x, y, z, then w defaults to 1.0
    VertexParam(f32, f32, f32),
    Normal(f32, f32, f32),
    Face(FaceIndex, FaceIndex, FaceIndex),
    TextureUVW(f32, f32, Option<f32>), // u,v, then w defaults to 0.0
}

def_string_line!(object_line, "o", ObjLine, ObjectName);
def_string_line!(group_line, "g", ObjLine, GroupName);
def_string_line!(mtllib_line, "mtllib", ObjLine, MtlLib);
def_string_line!(usemtl_line, "usemtl", ObjLine, UseMtl);
def_string_line!(s_line, "s", ObjLine, SmoothShading);

fn vertex_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("v"), float_triple_opt_4th, take_while(|c| c == '\n')),
        |(x, y, z, w)| ObjLine::Vertex(x, y, z, w),
    )(input)
}

fn normal_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vn"), float_triple, take_while(|c| c == '\n')),
        |(x, y, z)| ObjLine::Normal(x, y, z),
    )(input)
}

fn texcoord_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vt"), float_pair_opt_3rd, take_while(|c| c == '\n')),
        |(u, v, w)| ObjLine::TextureUVW(u, v, w),
    )(input)
}

fn vertex_param_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vp"), float_triple, take_while(|c| c == '\n')),
        |(x, y, z)| ObjLine::VertexParam(x, y, z),
    )(input)
}

fn comment_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(
            tag("#"),
            delimited(multispace0, take_while1(|c| c != '\n'), multispace0),
            take_while(|c| c == '\n'),
        ),
        |s: &str| ObjLine::Comment(s.trim().to_string()),
    )(input)
}

fn face_index(input: &str) -> IResult<&str, FaceIndex> {
    let (input, _) = sp(input)?;
    let (input, v) = unsigned_integer(input)?;
    let (input, uv) = opt(preceded(tag("/"), unsigned_integer))(input)?;
    let (input, n) = opt(preceded(tag("/"), unsigned_integer))(input)?;
    Ok((input, FaceIndex(v, uv, n)))
}

fn face_triple(input: &str) -> IResult<&str, FaceIndex> {
    let (input, v) = unsigned_integer(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vt) = opt(unsigned_integer)(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vn) = opt(unsigned_integer)(input)?;

    Ok((input, FaceIndex(v, vt, vn)))
}

fn face_pair(input: &str) -> IResult<&str, FaceIndex> {
    let (input, v) = unsigned_integer(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vt) = opt(unsigned_integer)(input)?;

    Ok((input, FaceIndex(v, vt, None)))
}

fn face_line(input: &str) -> IResult<&str, ObjLine> {
    let (input, _) = delimited(opt(multispace1), tag("f"), space1)(input)?;
    let (input, face) = alt((
        tuple((face_index, face_index, face_index)),
        tuple((face_pair, face_pair, face_pair)),
        tuple((face_triple, face_triple, face_triple)),
    ))(input)?;

    Ok((input, ObjLine::Face(face.0, face.1, face.2)))
}

pub fn parse_obj_line(input: &str) -> IResult<&str, ObjLine> {
    alt((
        comment_line,
        object_line,
        group_line,
        mtllib_line,
        usemtl_line,
        s_line,
        vertex_line,
        normal_line,
        texcoord_line,
        vertex_param_line,
        face_line,
    ))(input)
}

pub fn parse_obj<T: BufRead>(input: T) -> Result<Vec<ObjLine>, std::io::Error> {
    let mut result = Vec::new();

    for line in input.lines() {
        let line = line?;
        let (_, line) = parse_obj_line(&line).unwrap();
        result.push(line);
    }

    Ok(result)
}

pub struct ObjParser<R> {
    reader: R,
}

impl<R> ObjParser<R>
where
    R: BufRead,
{
    pub fn new(reader: R) -> Self {
        ObjParser { reader }
    }
}

impl<R> Iterator for ObjParser<R>
where
    R: BufRead,
{
    type Item = ObjLine;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();
        let read_result = self.reader.read_line(&mut line);
        match read_result {
            Ok(len) => {
                if len > 0 {
                    let result = parse_obj_line(&line);
                    match result {
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
    fn test_face_index() {
        assert_eq!(
            face_index("1/2/3"),
            Ok(("", FaceIndex(1, Some(2), Some(3))))
        );
        assert_eq!(face_index("4/5"), Ok(("", FaceIndex(4, Some(5), None))));
        assert_eq!(face_index("6"), Ok(("", FaceIndex(6, None, None))));
    }

    #[test]
    fn test_face_triple() {
        assert_eq!(
            face_triple("1/2/3"),
            Ok(("", FaceIndex(1, Some(2), Some(3))))
        );
        assert_eq!(
            face_triple("4/5"),
            Err(nom::Err::Error(nom::error::Error::new(
                "4/5",
                nom::error::ErrorKind::Tag
            )))
        );
        assert_eq!(
            face_triple("6"),
            Err(nom::Err::Error(nom::error::Error::new(
                "6",
                nom::error::ErrorKind::Tag
            )))
        );
    }

    #[test]
    fn test_face_pair() {
        assert_eq!(face_pair("1/2"), Ok(("", FaceIndex(1, Some(2), None))));
        assert_eq!(face_pair("3"), Ok(("", FaceIndex(3, None, None))));
    }

    #[test]
    fn can_parse_any_line() {
        let result = parse_obj_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n");
        let (_, line) = result.unwrap();
        assert_eq!(
            line,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_ignore_comment_at_eol() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_1() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_2() {
        //
        let ff = face_line("f 1/3 2/62 4/3\n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(3), None),
                FaceIndex(2, Some(62), None),
                FaceIndex(4, Some(3), None),
            )
        );
    }

    #[test]
    fn can_parse_face_line_3() {
        let ff = face_line("f 1//4 1//4 1//11  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, None, Some(4)),
                FaceIndex(1, None, Some(4)),
                FaceIndex(1, None, Some(11))
            )
        );
    }

    #[test]
    fn can_parse_face_line_4() {
        let ff = face_line("f 42 1 11  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_5() {
        let ff = face_line("f 42/ 1/ 11/  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_6() {
        let ff = face_line("f 42// 1// 11// \t \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_texcoord_line() {
        let vline = "vt -1.000000 -1.000000 \r\n";
        let v = texcoord_line(vline);
        let (_a, b) = v.unwrap();
        assert_eq!(b, ObjLine::TextureUVW(-1.0, -1.0, None));
    }

    #[test]
    fn can_parse_normal_line() {
        let vline = "vn -1.000000 -1.000000 1.000000  \r\n";
        let v = normal_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Normal(-1.0, -1.0, 1.0));
    }

    #[test]
    #[should_panic]
    fn invalid_vertex_line_fails() {
        let vline = "vZZ -1.000000 -1.000000 1.000000 \r\n";
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, None));
    }

    #[test]
    fn can_parse_vertex_parameter_line() {
        let vline = "vp -1.000000 -1.000000 1.000000 \r\n";
        let v = vertex_param_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::VertexParam(-1.0, -1.0, 1.0));
    }

    #[test]
    fn can_parse_vertex_line_with_optional_w_value() {
        let vline = "v -1.000000 -1.000000 1.000000 42.000\r\n";
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, Some(42.0)));
    }

    #[test]
    fn can_parse_vertex_line() {
        let vline = "v -1.000000 -1.000000 1.000000 \r\n";
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, None));
    }

    #[test]
    fn can_parse_object_line() {
        let cmt = object_line("o someobject.999asdf.7 \n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::ObjectName("someobject.999asdf.7".to_string()));
    }

    #[test]
    fn can_parse_mtllib_line() {
        let cmt = mtllib_line("mtllib somelib \n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::MtlLib("somelib".to_string()));
    }

    #[test]
    fn can_parse_usemtl_line() {
        let cmt = usemtl_line("usemtl SomeMaterial\n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::UseMtl("SomeMaterial".to_string()));
    }

    #[test]
    fn can_parse_s_line() {
        let cmt = s_line("s off\n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::SmoothShading("off".to_string()));
    }
}
