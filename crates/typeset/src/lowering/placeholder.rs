//! `{name}` 形式プレースホルダの共通トークナイザ

/// テンプレート文字列を分割した 1 区間
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Segment<'a> {
  /// プレースホルダに挟まれない、そのまま出力すべき文字列区間
  Literal(&'a str),
  /// `{name}` の中身（波括弧を除いた `name` 部分）
  Placeholder(&'a str),
}

/// `template` を「リテラル区間」と `{name}` プレースホルダに分割するイテレータを返す
pub(crate) fn segments(template: &str) -> impl Iterator<Item = Segment<'_>> { return Segments { rest: template }; }

/// [`segments`] の内部イテレータ状態（未処理の残り文字列を保持する）
struct Segments<'a> {
  /// 未処理の残り文字列
  rest: &'a str,
}

impl<'a> Iterator for Segments<'a> {
  type Item = Segment<'a>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.rest.is_empty() {
      return None;
    }
    let Some(brace_pos) = self.rest.find('{') else {
      let literal = self.rest;
      self.rest = "";
      return Some(Segment::Literal(literal));
    };
    if brace_pos > 0 {
      let (literal, remainder) = self.rest.split_at(brace_pos);
      self.rest = remainder;
      return Some(Segment::Literal(literal));
    }
    let after_brace = &self.rest[1..];
    let Some(close_pos) = after_brace.find('}') else {
      let literal = self.rest;
      self.rest = "";
      return Some(Segment::Literal(literal));
    };
    let name = &after_brace[..close_pos];
    self.rest = &after_brace[close_pos + 1..];
    return Some(Segment::Placeholder(name));
  }
}

/// `segments` を順に畳み込み、`Literal` はそのまま出力へ、`Placeholder(name)` は
/// `resolve(name)` の戻り値を出力へ連結して 1 つの文字列を作る
pub(crate) fn expand(template: &str, resolve: impl Fn(&str) -> String) -> String {
  let mut out = String::new();
  for segment in segments(template) {
    match segment {
      Segment::Literal(s) => out.push_str(s),
      Segment::Placeholder(name) => out.push_str(&resolve(name)),
    }
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn segments_split_literal_and_placeholder() {
    // Arrange / Act
    let result: Vec<Segment<'_>> = segments("第{n}章{chapter}.").collect();

    // Assert
    assert_eq!(
      result,
      vec![
        Segment::Literal("第"),
        Segment::Placeholder("n"),
        Segment::Literal("章"),
        Segment::Placeholder("chapter"),
        Segment::Literal("."),
      ]
    );
  }

  #[test]
  fn unclosed_brace_is_literal() {
    // Arrange / Act
    let result: Vec<Segment<'_>> = segments("abc{def").collect();

    // Assert
    assert_eq!(result, vec![Segment::Literal("abc"), Segment::Literal("{def")]);
  }

  #[test]
  fn empty_placeholder_resolves_to_empty_name() {
    // Arrange / Act
    let result: Vec<Segment<'_>> = segments("{}").collect();

    // Assert
    assert_eq!(result, vec![Segment::Placeholder("")]);
  }

  #[test]
  fn expand_replaces_placeholders_and_keeps_literal() {
    // Arrange / Act
    let out = expand("第{n}章", |name| match name {
      "n" => return "3".to_string(),
      _ => return format!("{{{name}}}"),
    });

    // Assert
    assert_eq!(out, "第3章");
  }
}
