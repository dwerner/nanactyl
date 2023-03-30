use std::str::{self, FromStr};

use nom::branch::alt;
use nom::bytes::complete::{tag, take_until, take_while};
use nom::character::complete::{digit1, line_ending, multispace0};
use nom::combinator::{eof, map_res, opt, recognize};
use nom::sequence::{delimited, tuple};
use nom::{error, IResult};

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

pub fn uint(input: &[u8]) -> IResult<&[u8], u32> {
    map_res(
        map_res(recognize(digit1), str::from_utf8),
        FromStr::from_str,
    )(input)
}

pub fn unsigned_float(input: &[u8]) -> IResult<&[u8], f32> {
    map_res(
        map_res(
            recognize(alt((
                delimited(digit1, tag("."), opt(digit1)),
                delimited(opt(digit1), tag("."), digit1),
                digit1,
            ))),
            str::from_utf8,
        ),
        FromStr::from_str,
    )(input)
}

pub fn float(input: &[u8]) -> IResult<&[u8], f32> {
    let (i, sign) = opt(alt((tag("+"), tag("-"))))(input)?;
    let (i, value) = unsigned_float(i)?;
    Ok((
        i,
        sign.and_then(|s| if s[0] == b'-' { Some(-1f32) } else { None })
            .unwrap_or(1f32)
            * value,
    ))
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
        assert_eq!(cmt, Ok((&b""[..], &b" a comment exists here "[..])));
    }

    #[test]
    fn can_parse_comments_2() {
        let cmt = comment(b"# Blender v2.78 (sub 0) OBJ File: 'untitled.blend'\n");
        assert_eq!(
            cmt,
            Ok((
                &b""[..],
                &b" Blender v2.78 (sub 0) OBJ File: 'untitled.blend'"[..]
            ))
        );
    }
}
