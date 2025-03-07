use std::io::Write;
use std::marker::PhantomData;

mod split_words;
mod vec_window;

pub use log;
pub use parser_lib_macros::Parser;
pub use split_words::{split_words, BracketPair, Word};
pub use vec_window::VecWindow;

pub fn setup_logging(debug: bool) {
    env_logger::Builder::new()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter_level(if debug {
            log::LevelFilter::max()
        } else {
            log::LevelFilter::Info
        })
        .init();
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub expected: String,
    pub got: Option<Word>,
}

#[derive(Debug)]
pub struct ParseResult<'l, Out>(
    pub Option<Out>,
    pub VecWindow<'l, Word>,
    pub Vec<ParseError>,
);

pub trait Parser<Out> {
    fn parse(words: VecWindow<Word>) -> ParseResult<Out>;
}

#[inline(always)]
pub fn parse_to_type<T>(words: VecWindow<Word>) -> ParseResult<T>
where
    T: Parser<T>,
{
    T::parse(words)
}

/*
* Implementations for basic types
*/

#[inline(always)]
fn parse_helper<'l, T>(
    words: VecWindow<'l, Word>,
    message: &'static str,
    parse_one: fn(&Word) -> Option<T>,
) -> ParseResult<'l, T> {
    let Some(word) = words.first() else {
        log::debug!("! {} !! EOF", message);
        return ParseResult(
            None,
            words,
            vec![ParseError {
                expected: message.to_string(),
                got: None,
            }],
        );
    };
    if let Some(res) = parse_one(word) {
        log::info!("> {} -> {}", message, word);
        ParseResult(Some(res), words.skip(1), Vec::new())
    } else {
        log::debug!("! {} !! {}", message, word);
        ParseResult(
            None,
            words.clone(),
            vec![ParseError {
                expected: message.to_string(),
                got: Some(word.clone()),
            }],
        )
    }
}

impl Parser<String> for String {
    fn parse(words: VecWindow<Word>) -> ParseResult<String> {
        parse_helper(words, "<<string>>", |word| {
            let word = word.get_word()?;
            (word.starts_with('"') && word.ends_with('"'))
                .then(|| word[1..word.len() - 1].to_string())
        })
    }
}

impl Parser<i64> for i64 {
    fn parse(words: VecWindow<Word>) -> ParseResult<i64> {
        parse_helper(words, "<<integer>>", |word| {
            word.get_word()?.parse::<i64>().ok()
        })
    }
}

impl Parser<f64> for f64 {
    fn parse(words: VecWindow<Word>) -> ParseResult<f64> {
        parse_helper(words, "<<decimal>>", |word| {
            word.get_word()?.parse::<f64>().ok()
        })
    }
}

impl Parser<bool> for bool {
    fn parse(words: VecWindow<Word>) -> ParseResult<bool> {
        parse_helper(words, "<<boolean>>", |word| match word.get_word()? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeName {
    pub text: String,
    pub line_number: usize,
    pub column_from: usize,
    pub column_to: usize,
}

impl Parser<TypeName> for TypeName {
    fn parse(words: VecWindow<Word>) -> ParseResult<TypeName> {
        parse_helper(words, "<<TypeName - PascalCase>>", |word| {
            let text = word.get_word()?;
            let starts_uppercase = text.chars().next().is_some_and(|c| c.is_uppercase());
            let all_alphabetic = text.chars().all(|c| c.is_alphabetic());
            if starts_uppercase && all_alphabetic {
                Some(TypeName {
                    text: text.to_string(),
                    line_number: word.line,
                    column_from: word.column_from,
                    column_to: word.column_to,
                })
            } else {
                None
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueName {
    pub text: String,
    pub line_number: usize,
    pub column_from: usize,
    pub column_to: usize,
}

impl Parser<ValueName> for ValueName {
    fn parse(words: VecWindow<Word>) -> ParseResult<ValueName> {
        parse_helper(words, "<<ValueName - snake_case>>", |word| {
            let text = word.get_word()?;
            let all_lowercase_or_underscore = text.chars().all(|c| c.is_lowercase() || c == '_');
            if !text.is_empty() && all_lowercase_or_underscore {
                Some(ValueName {
                    text: text.to_string(),
                    line_number: word.line,
                    column_from: word.column_from,
                    column_to: word.column_to,
                })
            } else {
                None
            }
        })
    }
}

/*
* Implementations for compound types
*/

impl<T: Parser<T>> Parser<Vec<T>> for Vec<T> {
    fn parse(mut words: VecWindow<Word>) -> ParseResult<Vec<T>> {
        let mut res = Vec::new();
        let mut errors = Vec::new();
        log::info!("- Vec");
        while !words.is_empty() {
            let ParseResult(item, new_words, new_errors) = T::parse(words);
            words = new_words;
            if let Some(item) = item {
                errors.extend(new_errors);
                res.push(item);
                log::info!("--");
            } else {
                break;
            }
        }
        log::info!("> Vec");
        ParseResult(Some(res), words, errors)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonEmptyVec<T>(Vec<T>);

impl<T: Parser<T>> Parser<NonEmptyVec<T>> for NonEmptyVec<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<NonEmptyVec<T>> {
        let ParseResult(res, words, mut errors) = Vec::<T>::parse(words);
        if let Some(res) = res {
            if res.is_empty() {
                errors.push(ParseError {
                    expected: "[one or more items]".to_string(),
                    got: None,
                });
                ParseResult(None, words, errors)
            } else {
                ParseResult(Some(NonEmptyVec(res)), words, errors)
            }
        } else {
            ParseResult(None, words, errors)
        }
    }
}

impl<T: Parser<Out>, Out> Parser<Option<Out>> for Option<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<Option<Out>> {
        log::info!("- Option");
        let ParseResult(res, words, errors) = T::parse(words);
        if let Some(res) = res {
            ParseResult(Some(Some(res)), words, errors)
        } else {
            ParseResult(Some(None), words, Vec::new())
        }
    }
}

impl<T: Parser<Out>, Out> Parser<Box<Out>> for Box<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<Box<Out>> {
        log::info!("- Box");
        let ParseResult(res, words, errors) = T::parse(words);
        ParseResult(res.map(Box::new), words, errors)
    }
}

impl<T1: Parser<Out1>, Out1, T2: Parser<Out2>, Out2> Parser<(Out1, Out2)> for (T1, T2) {
    fn parse(words: VecWindow<Word>) -> ParseResult<(Out1, Out2)> {
        log::info!("- 2-tuple");
        let ParseResult(res1, words, errors1) = T1::parse(words);
        let ParseResult(res2, words, errors2) = T2::parse(words);
        log::debug!("> 2-tuple");
        if let Some(res1) = res1 {
            if let Some(res2) = res2 {
                ParseResult(Some((res1, res2)), words, [errors1, errors2].concat())
            } else {
                ParseResult(None, words, errors2)
            }
        } else {
            ParseResult(None, words, errors1)
        }
    }
}

fn brackets_helper<B, T: Parser<T>>(
    mut words: VecWindow<Word>,
    start: char,
    end: char,
    create: fn(T) -> B,
) -> ParseResult<B> {
    let Some(first) = words.first() else {
        log::debug!("! \"{}\" !! EOF", start);
        return ParseResult(
            None,
            words.clone(),
            vec![ParseError {
                expected: start.to_string(),
                got: None,
            }],
        );
    };
    let Some(inner) = first.get_brackets(start, end) else {
        log::debug!("! \"{}\" !! {}", start, first);
        return ParseResult(
            None,
            words.clone(),
            vec![ParseError {
                expected: start.to_string(),
                got: Some(first.clone()),
            }],
        );
    };
    log::info!("- \"{}\"", start);
    let ParseResult(inner_res, inner_words, errors) = T::parse(VecWindow::from(inner));
    if let Some(word) = inner_words.first() {
        log::debug!("! \"{}\" - end_bracket !! {}", end, word);
        return ParseResult(
            None,
            words.clone(),
            vec![ParseError {
                expected: end.to_string(),
                got: Some(word.clone()),
            }],
        );
    }
    log::info!("> \"{}\"", end);
    words.pop_first();
    ParseResult(inner_res.map(create), words, errors)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SquareBrackets<T>(T);

impl<T: Parser<T>> Parser<SquareBrackets<T>> for SquareBrackets<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<SquareBrackets<T>> {
        brackets_helper(words, '[', ']', SquareBrackets)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurlyBrackets<T>(T);

impl<T: Parser<T>> Parser<CurlyBrackets<T>> for CurlyBrackets<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<CurlyBrackets<T>> {
        brackets_helper(words, '{', '}', CurlyBrackets)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parentheses<T>(T);

impl<T: Parser<T>> Parser<Parentheses<T>> for Parentheses<T> {
    fn parse(words: VecWindow<Word>) -> ParseResult<Parentheses<T>> {
        brackets_helper(words, '(', ')', Parentheses)
    }
}

pub trait SeparatedBySeparator {
    const SEPARATOR: &'static str;
}

#[macro_export]
macro_rules! separator {
    ($name:ident = $sep:literal) => {
        #[derive(Clone, Debug, PartialEq, Parser)]
        pub struct $name {}
        impl parser_lib::SeparatedBySeparator for $name {
            const SEPARATOR: &'static str = $sep;
        }
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeparatedBy<BY: SeparatedBySeparator, T>(Vec<T>, PhantomData<BY>);

impl<BY: SeparatedBySeparator, T: Parser<T>> Parser<SeparatedBy<BY, T>> for SeparatedBy<BY, T> {
    fn parse(mut words: VecWindow<Word>) -> ParseResult<SeparatedBy<BY, T>> {
        log::info!("- SeparatedBy {}", BY::SEPARATOR);
        let split_words = words.split(|word| word.get_word().is_some_and(|t| t == BY::SEPARATOR));
        let mut res = Vec::new();
        let mut errors = Vec::new();
        let len = split_words.len();
        for (i, split_word) in split_words.into_iter().enumerate() {
            let ParseResult(item, new_words, new_errors) = T::parse(split_word);
            errors.extend(new_errors);
            if let Some(item) = item {
                res.push(item);
            }
            if i == len - 1 {
                words = new_words;
            } else if !new_words.is_empty() {
                if let Some(word) = new_words.first() {
                    log::debug!("! SeparatedBy {} - end_part !! {}", BY::SEPARATOR, word);
                    errors.push(ParseError {
                        expected: BY::SEPARATOR.to_string(),
                        got: Some(word.clone()),
                    });
                }
            }
        }
        log::info!("> SeparatedBy {}", BY::SEPARATOR);
        ParseResult(Some(SeparatedBy(res, PhantomData)), words, errors)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeparatedOnce<BY: SeparatedBySeparator, A, B>(A, B, PhantomData<BY>);

impl<BY: SeparatedBySeparator, A: Parser<A>, B: Parser<B>> Parser<SeparatedOnce<BY, A, B>>
    for SeparatedOnce<BY, A, B>
{
    fn parse(words: VecWindow<Word>) -> ParseResult<SeparatedOnce<BY, A, B>> {
        let Some((first, second)) =
            words.split_once(|word| word.get_word().is_some_and(|t| t == BY::SEPARATOR))
        else {
            log::debug!("! SeparatedOnce {} !! EOF", BY::SEPARATOR);
            return ParseResult(
                None,
                words.clone(),
                vec![ParseError {
                    expected: BY::SEPARATOR.to_string(),
                    got: None,
                }],
            );
        };
        let ParseResult(res1, words, mut errors) = A::parse(first);
        let Some(res1) = res1 else {
            log::debug!("! SeparatedOnce {} !! first_part", BY::SEPARATOR);
            return ParseResult(None, words, errors);
        };
        if let Some(word) = words.first() {
            log::debug!("! SeparatedOnce {} - separator !! {}", BY::SEPARATOR, word);
            errors.push(ParseError {
                expected: BY::SEPARATOR.to_string(),
                got: Some(word.clone()),
            });
            return ParseResult(None, words, errors);
        }
        let ParseResult(res2, words, new_errors) = B::parse(second);
        let Some(res2) = res2 else {
            log::debug!("! SeparatedOnce {} !! second_part", BY::SEPARATOR);
            return ParseResult(None, words, errors);
        };
        errors.extend(new_errors);
        log::info!("> SeparatedOnce {}", BY::SEPARATOR);
        ParseResult(Some(SeparatedOnce(res1, res2, PhantomData)), words, errors)
    }
}
