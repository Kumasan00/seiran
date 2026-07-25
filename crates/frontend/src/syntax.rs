//! テキストソースから CST（具象構文木）を生成する

mod cst;
mod lexer;
mod parser;
pub mod token;

pub use cst::{ast, green, kind, kind::SyntaxKind};
pub use parser::{ParseMode, ParserError, parse};
