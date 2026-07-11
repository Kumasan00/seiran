//! カウンタ・定理・`ref_format` テンプレートのプレースホルダ展開（純粋関数）

/// `ref_format` テンプレートを適用して `\ref` の表示文字列を作る
///
/// 認識するプレースホルダは `{number}`（裸の番号）と `{display_name}`（種別名）のみ。
/// 未知のプレースホルダや閉じ括弧の欠落はリテラル扱いで残す。
pub(super) fn expand_ref_format(template: &str, number: &str, display_name: &str) -> String {
  return crate::placeholder::expand(template, |name| match name {
    "number" => number.to_string(),
    "display_name" => display_name.to_string(),
    // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
    _ => format!("{{{name}}}"),
  });
}
