//! コマンドディスパッチ
//!
//! コマンド名を phf パーフェクトハッシュマップで `CommandKind` に解決し、
//! 対応するハンドラに委譲します。
//!
//! ## コマンドの追加方法
//!
//! 新しいコマンドを追加するには、以下の手順に従います:
//!
//! 1. **単一文字コマンド**（`\alpha` 等）:
//!    `COMMAND_MAP` に `"name" => CommandKind::SingleChar('文字')` を追加するだけ。
//!
//! 2. **インラインラッパーコマンド**（`\textbf` 等）:
//!    `COMMAND_MAP` に `"name" => CommandKind::InlineWrapper(InlineNode::Variant)` を追加するだけ。
//!
//! 3. **見出しコマンド**（`\section` 等）:
//!    `COMMAND_MAP` に `"name" => CommandKind::Headline(HeadingLevel::Level)` を追加し、
//!    `HeadingLevel` に新しいバリアントを追加します。
//!
//! 4. **独自ロジックを持つコマンド**:
//!    `CommandKind` にバリアントを追加し、`execute()` にハンドラを実装します。

use phf::phf_map;
use syntax::ast::CommandView;

use crate::{
  document::{DocNode, HeadingLevel, InlineNode},
  evaluator::{EvalError, Evaluator, opt_args::collect_command_opt_args},
};

mod control;
mod headline;
mod inline;

/// コマンドの実行結果
///
/// コマンドはブロックレベル（見出し等）またはインラインレベル（ギリシャ文字等）の
/// いずれかのノードを生成します。
pub(crate) enum CommandResult {
  /// ブロックレベルのドキュメントノード（見出し、スペース等）
  Block(Vec<DocNode>),
  /// インラインレベルのドキュメントノード（記号文字等）
  Inline(Vec<InlineNode>),
}

/// コマンドの種類
///
/// パラメータ付きバリアントにより、同一パターンのコマンドを
/// 個別のバリアントなしで表現できます。
#[derive(Clone, Copy, Debug)]
pub(crate) enum CommandKind {
  /// `\space{N}` — 垂直スペース挿入
  Space,
  /// 見出しコマンド（`\part`, `\chapter`, `\section` 等）
  Headline(HeadingLevel),
  /// 引数1つを取りインラインノードでラップするコマンド（`\textbf`, `\emph` 等）
  InlineWrapper(fn(Vec<InlineNode>) -> InlineNode),
  /// 引数なしで単一文字を出力するコマンド（ギリシャ文字・数学記号）
  SingleChar(char),
  /// 未定義のコマンド
  Undefined,
}

impl CommandKind {
  /// コマンドを実行し、対応する `CommandResult` を生成する
  fn execute(self, view: &CommandView, evaluator: &mut Evaluator) -> Result<CommandResult, EvalError> {
    match self {
      Self::Space => control::space(view).map(CommandResult::Block),

      Self::Headline(level) => headline::heading(view, level, &mut evaluator.context).map(CommandResult::Block),

      Self::InlineWrapper(wrapper) => inline::inline_wrapper(view, wrapper).map(CommandResult::Inline),

      Self::SingleChar(ch) => {
        let _opt_args = collect_command_opt_args(view, &[])?;
        if !view.args_is_empty() {
          return Err(EvalError::ExtraCommandArgument {
            name: view.name().to_string(),
            span: view.span().into(),
          });
        }
        return Ok(CommandResult::Inline(vec![InlineNode::Symbol(ch)]));
      },

      Self::Undefined => Err(EvalError::UnknownCommand {
        name: view.name().to_string(),
        span: view.span().into(),
      }),
    }
  }
}

pub(crate) static COMMAND_MAP: phf::Map<&'static str, CommandKind> = phf_map! {
  // 制御コマンド
  "space" => CommandKind::Space,

  // インラインラッパーコマンド（テキスト装飾）
  "textbf" => CommandKind::InlineWrapper(InlineNode::Strong),
  "emph" => CommandKind::InlineWrapper(InlineNode::Emphasis),
  "textit" => CommandKind::InlineWrapper(InlineNode::Emphasis),
  "texttt" => CommandKind::InlineWrapper(InlineNode::Code),
  "textsf" => CommandKind::InlineWrapper(InlineNode::SansSerif),

  // 見出しコマンド
  "part" => CommandKind::Headline(HeadingLevel::Part),
  "chapter" => CommandKind::Headline(HeadingLevel::Chapter),
  "section" => CommandKind::Headline(HeadingLevel::Section),
  "subsection" => CommandKind::Headline(HeadingLevel::Subsection),
  "paragraph" => CommandKind::Headline(HeadingLevel::Paragraph),
  "subparagraph" => CommandKind::Headline(HeadingLevel::Subparagraph),

  // ギリシャ文字（大文字）
  "Alpha" => CommandKind::SingleChar('\u{0391}'),
  "Beta" => CommandKind::SingleChar('\u{0392}'),
  "Gamma" => CommandKind::SingleChar('\u{0393}'),
  "Delta" => CommandKind::SingleChar('\u{0394}'),
  "Epsilon" => CommandKind::SingleChar('\u{0395}'),
  "Zeta" => CommandKind::SingleChar('\u{0396}'),
  "Eta" => CommandKind::SingleChar('\u{0397}'),
  "Theta" => CommandKind::SingleChar('\u{0398}'),
  "Iota" => CommandKind::SingleChar('\u{0399}'),
  "Kappa" => CommandKind::SingleChar('\u{039A}'),
  "Lambda" => CommandKind::SingleChar('\u{039B}'),
  "Mu" => CommandKind::SingleChar('\u{039C}'),
  "Nu" => CommandKind::SingleChar('\u{039D}'),
  "Xi" => CommandKind::SingleChar('\u{039E}'),
  "Omicron" => CommandKind::SingleChar('\u{039F}'),
  "Pi" => CommandKind::SingleChar('\u{03A0}'),
  "Rho" => CommandKind::SingleChar('\u{03A1}'),
  "Sigma" => CommandKind::SingleChar('\u{03A3}'),
  "Tau" => CommandKind::SingleChar('\u{03A4}'),
  "Upsilon" => CommandKind::SingleChar('\u{03A5}'),
  "Phi" => CommandKind::SingleChar('\u{03A6}'),
  "Chi" => CommandKind::SingleChar('\u{03A7}'),
  "Psi" => CommandKind::SingleChar('\u{03A8}'),
  "Omega" => CommandKind::SingleChar('\u{03A9}'),

  // ギリシャ文字（小文字）
  "alpha" => CommandKind::SingleChar('\u{03B1}'),
  "beta" => CommandKind::SingleChar('\u{03B2}'),
  "gamma" => CommandKind::SingleChar('\u{03B3}'),
  "delta" => CommandKind::SingleChar('\u{03B4}'),
  "epsilon" => CommandKind::SingleChar('\u{03B5}'),
  "varepsilon" => CommandKind::SingleChar('\u{03F5}'),
  "zeta" => CommandKind::SingleChar('\u{03B6}'),
  "eta" => CommandKind::SingleChar('\u{03B7}'),
  "theta" => CommandKind::SingleChar('\u{03B8}'),
  "vartheta" => CommandKind::SingleChar('\u{03D1}'),
  "iota" => CommandKind::SingleChar('\u{03B9}'),
  "kappa" => CommandKind::SingleChar('\u{03BA}'),
  "varkappa" => CommandKind::SingleChar('\u{03F0}'),
  "lambda" => CommandKind::SingleChar('\u{03BB}'),
  "mu" => CommandKind::SingleChar('\u{03BC}'),
  "nu" => CommandKind::SingleChar('\u{03BD}'),
  "xi" => CommandKind::SingleChar('\u{03BE}'),
  "omicron" => CommandKind::SingleChar('\u{03BF}'),
  "pi" => CommandKind::SingleChar('\u{03C0}'),
  "varpi" => CommandKind::SingleChar('\u{03D6}'),
  "rho" => CommandKind::SingleChar('\u{03C1}'),
  "varrho" => CommandKind::SingleChar('\u{03F1}'),
  "sigma" => CommandKind::SingleChar('\u{03C3}'),
  "varsigma" => CommandKind::SingleChar('\u{03C2}'),
  "tau" => CommandKind::SingleChar('\u{03C4}'),
  "upsilon" => CommandKind::SingleChar('\u{03C5}'),
  "phi" => CommandKind::SingleChar('\u{03C6}'),
  "chi" => CommandKind::SingleChar('\u{03C7}'),
  "psi" => CommandKind::SingleChar('\u{03C8}'),
  "omega" => CommandKind::SingleChar('\u{03C9}'),

  // 数学記号
  "forall" => CommandKind::SingleChar('\u{2200}'),
  "complement" => CommandKind::SingleChar('\u{2201}'),
  "partial" => CommandKind::SingleChar('\u{2202}'),
  "exists" => CommandKind::SingleChar('\u{2203}'),
  "notexists" => CommandKind::SingleChar('\u{2204}'),
  "emptyset" => CommandKind::SingleChar('\u{2205}'),
  "increment" => CommandKind::SingleChar('\u{2206}'),
  "nabla" => CommandKind::SingleChar('\u{2207}'),
  "in" => CommandKind::SingleChar('\u{2208}'),
  "notin" => CommandKind::SingleChar('\u{2209}'),
  "ni" => CommandKind::SingleChar('\u{220B}'),
  "notni" => CommandKind::SingleChar('\u{220C}'),
  "qed" => CommandKind::SingleChar('\u{220E}'),
  "prod" => CommandKind::SingleChar('\u{220F}'),
  "coprod" => CommandKind::SingleChar('\u{2210}'),
  "sum" => CommandKind::SingleChar('\u{2211}'),
  "minus" => CommandKind::SingleChar('\u{2212}'),
  "mp" => CommandKind::SingleChar('\u{2213}'),
  "dotplus" => CommandKind::SingleChar('\u{2214}'),
  "slash" => CommandKind::SingleChar('\u{2215}'),
  "surd" => CommandKind::SingleChar('\u{221A}'),
  "propto" => CommandKind::SingleChar('\u{221D}'),
  "infty" => CommandKind::SingleChar('\u{221E}'),
  "rightangle" => CommandKind::SingleChar('\u{221F}'),
  "angle" => CommandKind::SingleChar('\u{2220}'),
  "parallel" => CommandKind::SingleChar('\u{2225}'),
  "notparallel" => CommandKind::SingleChar('\u{2226}'),
  "land" => CommandKind::SingleChar('\u{2227}'),
  "lor" => CommandKind::SingleChar('\u{2228}'),
  "cap" => CommandKind::SingleChar('\u{2229}'),
  "cup" => CommandKind::SingleChar('\u{222A}'),
  "int" => CommandKind::SingleChar('\u{222B}'),
  "iint" => CommandKind::SingleChar('\u{222C}'),
  "iiint" => CommandKind::SingleChar('\u{222D}'),
  "oint" => CommandKind::SingleChar('\u{222E}'),
  "oiint" => CommandKind::SingleChar('\u{222F}'),
  "oiiint" => CommandKind::SingleChar('\u{2230}'),
  "therefore" => CommandKind::SingleChar('\u{2234}'),
  "because" => CommandKind::SingleChar('\u{2235}'),
  "backsim" => CommandKind::SingleChar('\u{223D}'),
};

impl Evaluator {
  /// コマンドを評価し、対応する `CommandResult` を生成する
  ///
  /// # Arguments
  ///
  /// * `view` - コマンドの型付きビュー
  ///
  /// # Returns
  ///
  /// 生成された `CommandResult`、またはエラー
  ///
  /// # Errors
  ///
  /// 未知のコマンドやコマンド実行中のエラーが発生した場合
  pub(crate) fn evaluate_command(&mut self, view: &CommandView) -> Result<CommandResult, EvalError> {
    let command_kind = COMMAND_MAP.get(view.name()).copied().unwrap_or(CommandKind::Undefined);
    return command_kind.execute(view, self);
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::parse;

  use super::*;

  #[test]
  fn single_char_rejects_unknown_opt_arg_key() {
    // Arrange — `\alpha` は SingleChar コマンドで任意引数を受け付けない
    let arena = Bump::new();
    let source = r"\alpha[k=v]";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
