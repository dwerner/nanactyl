use std::str::{self, FromStr};

use nom::branch::alt;
use nom::bytes::complete::{is_not, tag, take_while};
use nom::character::complete::{digit1, line_ending, multispace0};
use nom::combinator::{eof, map_res, opt, recognize};
use nom::number::complete::recognize_float;
use nom::sequence::tuple;
use nom::{error, IResult};

fn eol(input: &str) -> IResult<&str, &str> {
    line_ending(input)
}

pub(crate) fn end_of_line_maybe_comment(input: &str) -> IResult<&str, ()> {
    let (i, _) = alt((eof, eol, comment))(input)?;
    Ok((i, ()))
}

pub(crate) fn comment(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag("#")(input)?;
    let (input, comment) = is_not("\r\n")(input)?;
    let (input, _) = line_ending(input)?;
    Ok((input, comment))
}

pub(crate) fn unsigned_integer(input: &str) -> IResult<&str, u32> {
    map_res(recognize(digit1), FromStr::from_str)(input)
}

fn unsigned_float(input: &str) -> IResult<&str, f32> {
    map_res(recognize_float, |s: &str| s.parse::<f32>())(input)
}

fn float(input: &str) -> IResult<&str, f32> {
    let (i, sign) = opt(alt((tag("+"), tag("-"))))(input)?;
    let (i, value) = unsigned_float(i)?;
    Ok((
        i,
        sign.and_then(|s| {
            if let Some('-') = s.chars().next() {
                Some(-1f32)
            } else {
                None
            }
        })
        .unwrap_or(1f32)
            * value,
    ))
}

pub(crate) fn float_triple_opt_4th(input: &str) -> IResult<&str, (f32, f32, f32, Option<f32>)> {
    let (i, tuple_result) =
        tuple((spaced_float, spaced_float, spaced_float, opt(spaced_float)))(input)?;
    Ok((i, tuple_result))
}

pub(crate) fn float_pair_opt_3rd(input: &str) -> IResult<&str, (f32, f32, Option<f32>)> {
    let (i, tuple_result) = tuple((spaced_float, spaced_float, opt(spaced_float)))(input)?;
    Ok((i, tuple_result))
}

pub(crate) fn float_triple(input: &str) -> IResult<&str, (f32, f32, f32)> {
    let (i, tuple_result) = tuple((spaced_float, spaced_float, spaced_float))(input)?;
    Ok((i, tuple_result))
}

pub(crate) fn float_pair(input: &str) -> IResult<&str, (f32, f32)> {
    let (i, tuple_result) = tuple((spaced_float, spaced_float))(input.trim())?;
    Ok((i, tuple_result))
}

pub(crate) fn spaced_float(input: &str) -> IResult<&str, f32> {
    let (i, _) = multispace0(input)?;
    let (i, value) = float(i)?;
    let (i, _) = multispace0(i)?;
    Ok((i, value))
}

pub(crate) fn spaced_uint(input: &str) -> IResult<&str, u32> {
    let (i, _) = multispace0(input)?;
    let (i, value) = unsigned_integer(i)?;
    let (i, _) = multispace0(i)?;
    Ok((i, value))
}

#[cfg(test)]
mod tests {

    use nom::error::{Error, ErrorKind};
    use nom::Err;

    use super::*;

    #[test]
    fn test_unsigned_float() {
        assert_eq!(unsigned_float("3.14"), Ok(("", 3.14)));
        assert_eq!(unsigned_float(".5"), Ok(("", 0.5)));
        assert_eq!(unsigned_float("123"), Ok(("", 123.0)));
        assert_eq!(unsigned_float("0"), Ok(("", 0.0)));
        assert_eq!(unsigned_float("10."), Ok(("", 10.0)));
        assert_eq!(unsigned_float("2.5e-2"), Ok(("", 0.025)));
        assert_eq!(unsigned_float("1.e3"), Ok(("", 1000.0)));
        assert!(unsigned_float("abc").is_err());
    }

    #[test]
    fn test_float() {
        assert_eq!(float("3.14"), Ok(("", 3.14)));
        assert_eq!(float("+3.14"), Ok(("", 3.14)));
        assert_eq!(float("-3.14"), Ok(("", -3.14)));
        assert_eq!(float("+123"), Ok(("", 123.0)));
        assert_eq!(float("-.5"), Ok(("", -0.5)));
        assert_eq!(float("-10."), Ok(("", -10.0)));
        assert_eq!(float("-2.5e-2"), Ok(("", -0.025)));
        assert_eq!(float("+1.e3"), Ok(("", 1000.0)));
        assert!(float("abc").is_err());
    }

    #[test]
    fn can_parse_float_pair() {
        let ff = float_pair("     -1.000001 7742.9 ");
        assert_eq!(ff, Ok(("", (-1.000001, 7742.9))));
    }

    #[test]
    fn can_parse_float_triple() {
        let fff = float_triple("    0.95  -1.000001 42.9 ");
        assert_eq!(fff, Ok(("", (0.95, -1.000001, 42.9))));
    }

    #[test]
    fn can_parse_comments() {
        let cmt = comment("# a comment exists here \n");
        assert_eq!(cmt, Ok(("", " a comment exists here ")));
    }

    #[test]
    fn can_parse_comments_2() {
        let cmt = comment("# Blender v2.78 (sub 0) OBJ File: 'untitled.blend'\n");
        assert_eq!(
            cmt,
            Ok(("", " Blender v2.78 (sub 0) OBJ File: 'untitled.blend'"))
        );
    }

    #[test]
    fn test_end_of_line_maybe_comment_eof() {
        assert_eq!(end_of_line_maybe_comment(""), Ok(("", ())));
    }

    #[test]
    fn test_end_of_line_maybe_comment_eol() {
        assert_eq!(end_of_line_maybe_comment("\n"), Ok(("", ())));
    }

    #[test]
    fn test_end_of_line_maybe_comment_comment() {
        assert_eq!(
            end_of_line_maybe_comment("# This is a comment\n"),
            Ok(("", ()))
        );
    }

    #[test]
    fn test_end_of_line_maybe_comment_comment_no_newline() {
        // still an error, becase there's no newline
        assert_eq!(
            end_of_line_maybe_comment("# This is a comment"),
            Err(Err::Error(Error::new("", ErrorKind::CrLf)))
        );
    }

    #[test]
    fn test_end_of_line_maybe_comment_comment_newline() {
        assert_eq!(
            end_of_line_maybe_comment("# This is a comment\n"),
            Ok(("", ()))
        );
    }

    #[test]
    fn test_end_of_line_maybe_comment_text() {
        assert_eq!(
            end_of_line_maybe_comment("This is not a comment"),
            Err(Err::Error(Error::new(
                "This is not a comment",
                ErrorKind::Tag // missing # tag
            )))
        );
    }
}
