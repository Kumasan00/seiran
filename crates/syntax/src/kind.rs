//! CST（具象構文木）の種別定義
//!
//! CST ノードの種別（`SyntaxKind`）を定義します。
//! ノード型自体はアリーナベースの [`crate::green`] モジュールで定義されています。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer]
//! Token 列
//!   ↓ [Parser]
//! Green Tree (green::GreenNode)  ← SyntaxKind はここで使用
//!   ↓ [Evaluator]               ← 型付きビュー (ast) 経由で直接評価
//! Document IR (DocNode)
//! ```
//!
//! ## 設計方針
//!
//! - **ロスレス**: コメント・空白を含む完全なソース情報を保持
//! - **Error ノード**: パースエラーがあっても木全体が構築可能
//! - **Span**: 各ノードがソース上のバイト範囲を保持
//! - **アリーナベース**: `bumpalo::Bump` による単一アリーナ確保で `Vec` ヒープ確保を排除

// =============================================================================
// 構文種別
// =============================================================================

/// CST ノードの種別
///
/// トークンレベル（リーフ）と合成ノード（内部ノード）の両方を表現します。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
  // // ---- トークン（リーフノード）----
  // /// コマンドトークン（`\name`）
  // CommandToken,
  // /// テキストトークン
  // TextToken,
  // /// 左中括弧 `{`
  // LBrace,
  // /// 右中括弧 `}`
  // RBrace,
  // /// 左角括弧 `[`
  // LBracket,
  // /// 右角括弧 `]`
  // RBracket,
  // /// ドル記号 `$`
  // Dollar,
  // /// アンパサンド `&`
  // Ampersand,
  // /// エスケープ文字（`\$` など）
  // Escaped,
  // /// 強制改行 `\\`
  // LineBreak,
  // /// パラグラフ区切り（空行）
  // ParagraphBreak,
  // /// コメント（`// ...`）
  // Comment,
  // /// 認識できない文字
  // Unknown,

  // ---- 合成ノード（内部ノード）----
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
