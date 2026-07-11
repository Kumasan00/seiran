//! カウンタ・定理・`ref_format` テンプレートのプレースホルダ展開（純粋関数）

/// `ref_format` テンプレートを適用して `\ref` の表示文字列を作る
///
/// 認識するプレースホルダは `{number}`（裸の番号）と `{display_name}`（種別名）のみ。
/// 未知のプレースホルダや閉じ括弧の欠落はリテラル扱いで残す。
pub(super) fn expand_ref_format(template: &str, number: &str, display_name: &str) -> String {
  return expand_placeholders(template, |name| match name {
    "number" => number.to_string(),
    "display_name" => display_name.to_string(),
    // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
    _ => format!("{{{name}}}"),
  });
}

/// `{name}` 形式のプレースホルダを `resolve` の戻り値で置換する共通ループ
///
/// テンプレート文字列を 1 文字ずつ走査し、`{...}` の中身を `resolve(name)` に渡してその
/// 戻り値を出力する。`resolve` が空文字列を返せば「ドロップ」、`{name}` を返せば「リテラル
/// 保持」を表現でき、カウンタ・定理・`ref_format` の各展開がこの 1 つの実装を共有する。
/// 閉じ括弧のない `{...` はリテラル扱いとしてそのまま残す。
pub(super) fn expand_placeholders(template: &str, resolve: impl Fn(&str) -> String) -> String {
  let mut out = String::new();
  let mut chars = template.chars().peekable();
  while let Some(c) = chars.next() {
    if c != '{' {
      out.push(c);
      continue;
    }
    let mut name = String::new();
    let mut closed = false;
    while let Some(&nc) = chars.peek() {
      chars.next();
      if nc == '}' {
        closed = true;
        break;
      }
      name.push(nc);
    }
    if !closed {
      // 閉じ括弧なしの `{...` はリテラル扱いとして残す
      out.push('{');
      out.push_str(&name);
      continue;
    }
    out.push_str(&resolve(&name));
  }
  return out;
}
