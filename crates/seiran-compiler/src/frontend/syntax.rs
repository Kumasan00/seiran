//! テキストソースから CST（具象構文木）を生成する

mod cst;
mod lexer;
mod parser;
pub(super) mod token;

pub(super) use cst::{ast, green, kind, kind::SyntaxKind};
pub(super) use parser::{ArgMode, BodyMode, ModeResolver, ParserError, parse};
