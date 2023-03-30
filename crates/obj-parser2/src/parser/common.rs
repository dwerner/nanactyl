use std::str;
use std::str::FromStr;

use nom::{
    branch::alt,
    bytes::complete::{tag, take_until, take_while},
    character::complete::{digit1, line_ending, multispace0},
    combinator::{eof, map, map_res, opt, recognize},
    error,
    multi::many1,
    number::complete::u32 as complete_u32,
    sequence::{delimited, pair, tuple},
    IResult,
};

pub fn sp(input: &[u8]) -> Result<(&[u8], &[u8]), nom::Err<error::Error<&[u8]>>> {
    take_while(|c| c == b' ' || c == b'\t')(input)
}

pub fn whitespace(input: &[u8]) -> IResult<&[u8], ()> {
    let (i, _) = multispace0(input)?;
    Ok((i, ()))
}

pub fn slashes(input: &[u8]) -> IResult<&[u8], ()> {
    let (i, _) = tag("/")(input)?;
    Ok((i, ()))
}

fn eol(input: &[u8]) -> IResult<&[u8], &[u8]> {
    line_ending(input)
}

pub fn end_of_line_maybe_comment(input: &[u8]) -> IResult<&[u8], ()> {
    let (i, _) = alt((eof, eol, comment))(input)?;
    Ok((i, ()))
}

pub fn comment(input: &[u8]) -> IResult<&[u8], &[u8]> {
    delimited(tag("#"), take_until("\n"), alt((eof, eol)))(input)
}

pub fn unsigned_float(input: &str) -> IResult<&str, f32> {
    map_res(
        recognize(pair(
            opt(tag("-")),
            alt((
                tag("0"),
                recognize(tuple((digit1, opt(tag(".")), opt(digit1)))),
            )),
        )),
        str::parse::<f32>,
    )(input)
}

pub fn float(input: &[u8]) -> IResult<&[u8], f32> {
    let (i, (sign, value)) = tuple((opt(alt((tag("+"), tag("-")))), unsigned_float))(input)?;

    let final_value = sign
        .and_then(|s| if s[0] == b'-' { Some(-1f32) } else { None })
        .unwrap_or(1f32)
        * value;
    Ok((i, final_value))
}

pub fn float_s(input: &[u8]) -> IResult<&[u8], f32> {
    map_res(
        recognize(pair(opt(alt((tag("+"), tag("-")))), float)),
        str::from_utf8,
    )(input)
}

pub fn uint(input: &[u8]) -> IResult<&[u8], u32> {
    recognize(complete_u32)(input)
}

pub fn float_triple_opt_4th(input: &[u8]) -> IResult<&[u8], (f32, f32, f32, Option<f32>)> {
    let (i, tuple_result) = tuple((float, float, float, opt(float)))(input)?;
    Ok((i, tuple_result))
}

pub fn float_pair_opt_3rd(input: &[u8]) -> IResult<&[u8], (f32, f32, Option<f32>)> {
    let (i, tuple_result) = tuple((float, float, opt(float)))(input)?;
    Ok((i, tuple_result))
}

pub fn float_triple(input: &[u8]) -> IResult<&[u8], (f32, f32, f32)> {
    let (i, tuple_result) = tuple((float, float, float))(input)?;
    Ok((i, tuple_result))
}

pub fn float_pair(input: &[u8]) -> IResult<&[u8], (f32, f32)> {
    let (i, tuple_result) = tuple((float, float))(input)?;
    Ok((i, tuple_result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_signed_floats() {
        let something = float(b"-0.00005");
        assert_eq!(something, Ok((&b""[..], -0.00005)));
    }

    #[test]
    fn can_parse_float_pair() {
        let ff = float_pair(b"     -1.000001 7742.9 ");
        assert_eq!(ff, Ok((&b""[..], (-1.000001, 7742.9))));
    }

    #[test]
    fn can_parse_float_triple() {
        let fff = float_triple(b"    0.95  -1.000001 42.9 ");
        assert_eq!(fff, Ok((&b""[..], (0.95, -1.000001, 42.9))));
    }

    #[test]
    fn can_parse_comments() {
        let cmt = comment(b"# a comment exists here \n");
        assert_eq!(cmt, Ok((&b"\n"[..], &b" a comment exists here "[..])));
    }

    #[test]
    fn can_parse_comments_2() {
        let cmt = comment(b"# Blender v2.78 (sub 0) OBJ File: 'untitled.blend'\n");
        assert_eq!(
            cmt,
            Ok((
                &b"\n"[..],
                &b" Blender v2.78 (sub 0) OBJ File: 'untitled.blend' "[..]
            ))
        );
    }
}
