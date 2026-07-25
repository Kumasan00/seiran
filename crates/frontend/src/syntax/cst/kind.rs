//! CST（具象構文木）の種別定義

/// CST ノードの種別
///
/// トークンレベル（リーフ）と合成ノード（内部ノード）の両方を表現します。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
  /// ドキュメント全体のルートノード
  Root,
  /// コマンド呼び出し（`\name[opt]{arg}`）
  CommandCall,
  /// 環境（`\begin{name}...\end{name}`）
  Environment,
  /// 環境の開始タグ（`\begin{name}`）
  EnvironmentBegin,
  /// 環境の終了タグ（`\end{name}`）
  EnvironmentEnd,
  /// 環境の本体
  EnvironmentBody,
  /// 中括弧グループ（`{...}`）
  // 現時点でパーサは構築しない（P4: 裸の `{...}` は構文エラー）。#201 で dead_code 検出の対象に
  // なったが、evaluator 側は防御的に分岐を持つため据え置く。
  #[allow(dead_code)]
  Group,
  /// 任意引数（`[...]`）
  OptArg,
  /// 必須引数（`{...}`）
  MandatoryArg,
  /// インライン数式（`$...$`）
  InlineMath,
  /// 数式内グループ（`{...}` 数式モード内）
  MathGroup,
  /// 数式内下付き（`_` の後の要素）
  MathSubscript,
  /// 数式内上付き（`^` の後の要素）
  MathSuperscript,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn syntax_kind_equality() {
    assert_eq!(SyntaxKind::Root, SyntaxKind::Root);
    assert_ne!(SyntaxKind::Root, SyntaxKind::CommandCall);
  }

  #[test]
  fn syntax_kind_debug() {
    let kind = SyntaxKind::CommandCall;
    let debug_str = format!("{kind:?}");
    assert_eq!(debug_str, "CommandCall");
  }
}
