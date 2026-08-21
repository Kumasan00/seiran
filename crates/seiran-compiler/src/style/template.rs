//! 書式テンプレート（`{name}` プレースホルダを含む style.toml 文字列）の解析・検証・展開。
//!
//! テンプレートは style.toml の読込時に 1 回だけ解析され、以降は解析済みの値型として持ち回る。
//! 波括弧はプレースホルダ専用で、リテラルやエスケープ（`{{`）は扱わない。
//!
//! 用途ごとに「どのプレースホルダを書けるか」が違うため、private な共通実装 [`Analyzed`] の上に
//! 用途別の具体型（[`NumberTitleTemplate`] 等）を載せる。caller はこの具体型の
//! `expand` だけを使い、テンプレートの文法や許可リストを知らない。
//!
//! 解析は失敗しない。不正な波括弧・未知のプレースホルダは値の中に「問題」として溜め、
//! `garde` の検証違反として `style::validate_values` が他フィールドの違反と一括で報告する
//! （`Deserialize` を失敗させると `ReadStyleError::ParseToml` へ早期変換され、
//! 複数フィールドの一括報告が打ち切られてしまうため）。
//!
//! 展開は検証を通った値にだけ許可する。問題を持つテンプレートが `expand` へ届いたら、それは
//! `style::parse` / `validate_values` の上流保証が破れているので `unreachable!` で落とす。

use std::ops::Range;

use garde::{Path, Report, Validate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::style::counter::CounterName;

/// 用途別プレースホルダ集合が満たす性質
///
/// 「どの名前を書けるか」と「空テンプレートを許すか」の 2 点だけが用途ごとの差分で、
/// 解析器・検証器・展開器はこの trait 越しに用途差を吸収する。
trait Placeholder: Sized {
  /// 空文字列のテンプレートを許可するか（走り文スロットだけが `true`）
  const ALLOW_EMPTY: bool;

  /// プレースホルダ名を値へ写す。この用途で使えない名前は `None`
  fn from_name(name: &str) -> Option<Self>;
}

/// 解析済みテンプレート 1 本（元文字列・区間列・解析時に見つけた問題）
#[derive(Debug, Clone, PartialEq, Eq)]
struct Analyzed<P> {
  /// TOML に書かれた元の文字列。直列化と `as_str` が返す唯一の真実
  source: String,
  /// 解析結果の区間列。`Literal` は [`Analyzed::source`] 上の byte range
  parts: Vec<Part<P>>,
  /// 解析時に見つけた問題（空なら健全）
  problems: Vec<String>,
}

/// 解析済みテンプレートを構成する 1 区間
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part<P> {
  /// そのまま出力する、元文字列上の範囲
  Literal(Range<usize>),
  /// 用途別プレースホルダ
  Placeholder(P),
}

impl<P: Placeholder> Analyzed<P> {
  /// テンプレート文字列を 1 走査で区間列へ分解する（失敗せず、問題は値に溜める）
  ///
  /// 不正な波括弧（未閉じ `{`・対応しない `}`・空 `{}`・ネスト）と未知の名前を、
  /// 現れた順に [`Analyzed::problems`] へ積む。問題のある箇所の原文はリテラルとして
  /// 区間列に残すが、そのテンプレートは検証で弾かれるため展開へは届かない。
  fn parse(source: &str) -> Self {
    let mut parts: Vec<Part<P>> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut literal_start: usize = 0;
    let mut chars = source.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
      if ch == '}' {
        problems.push("対応する '{' のない '}' があります".to_string());
        continue;
      }
      if ch != '{' {
        continue;
      }

      push_literal(&mut parts, literal_start..index);
      // 対応する '}' まで名前を読み取る。途中で '{' が再出現したらネストとして扱う。
      let name_start = index + '{'.len_utf8();
      let mut name_end = name_start;
      let mut closed = false;
      let mut nested = false;
      while let Some(&(next_index, next_ch)) = chars.peek() {
        if next_ch == '}' {
          chars.next();
          name_end = next_index;
          closed = true;
          break;
        }
        if next_ch == '{' {
          name_end = next_index;
          nested = true;
          break;
        }
        chars.next();
        name_end = next_index + next_ch.len_utf8();
      }

      // 問題があった箇所は原文（'{' を含む）をリテラル扱いへ戻す
      literal_start = index;
      if nested {
        // 内側の '{' は外側ループが改めて処理する（そこでも未閉じ等として検出される）。
        problems.push("プレースホルダがネストしています（'{' の内側に '{' があります）".to_string());
        continue;
      }
      if !closed {
        problems.push("閉じられていない '{' があります".to_string());
        // 以降は構文として解釈できないため走査を打ち切る。
        break;
      }

      let name = &source[name_start..name_end];
      if name.is_empty() {
        problems.push("空のプレースホルダ '{}' があります".to_string());
        continue;
      }
      let Some(placeholder) = P::from_name(name) else {
        problems.push(format!("未知のプレースホルダ '{{{name}}}' があります"));
        continue;
      };
      parts.push(Part::Placeholder(placeholder));
      literal_start = name_end + '}'.len_utf8();
    }

    push_literal(&mut parts, literal_start..source.len());
    return Analyzed {
      source: source.to_string(),
      parts,
      problems,
    };
  }

  /// 溜めた問題（と空テンプレート違反）を `garde` のフィールド違反として報告する
  ///
  /// 1 テンプレート ＝ 違反 1 件にまとめる（複数の問題は `"; "` 区切りで 1 メッセージに繋ぐ）。
  /// フィールド単位の集約は `style::validate_values` の担当なので、ここでは分割しない。
  fn validate_into(&self, parent: &mut dyn FnMut() -> Path, report: &mut Report) {
    if !P::ALLOW_EMPTY && self.source.is_empty() {
      report.append(parent(), garde::Error::new("テンプレートが空です"));
      return;
    }
    if self.problems.is_empty() {
      return;
    }
    report.append(parent(), garde::Error::new(self.problems.join("; ")));
  }
}

impl<P> Analyzed<P> {
  /// TOML に書かれた元の文字列
  fn as_str(&self) -> &str { return &self.source; }

  /// 解析時の問題が残っていないこと（＝検証を通った値であること）を確かめる
  fn ensure_validated(&self) {
    if !self.problems.is_empty() {
      unreachable!(
        "検証を通ったテンプレートだけが展開へ届く（style::parse / validate_values が保証する）: {}",
        self.source
      );
    }
  }

  /// すべてのプレースホルダを文字列へ解決し、1 本の文字列にする（単一パス）
  ///
  /// 解決結果は再解析しないので、値に `{page}` 等が含まれていても展開されない。
  fn expand_to_string(&self, mut resolve: impl FnMut(&P) -> String) -> String {
    self.ensure_validated();
    let mut out = String::new();
    for part in &self.parts {
      match part {
        Part::Literal(range) => out.push_str(&self.source[range.clone()]),
        Part::Placeholder(placeholder) => out.push_str(&resolve(placeholder)),
      }
    }
    return out;
  }
}

/// 空でない範囲だけをリテラル区間として積む
fn push_literal<P>(parts: &mut Vec<Part<P>>, range: Range<usize>) {
  if range.is_empty() {
    return;
  }
  parts.push(Part::Literal(range));
}

/// 溜めたリテラル文字列を出力型へ変換して書き出し、バッファを空にする
fn flush_literal<T>(nodes: &mut Vec<T>, buffer: &mut String, literal: &mut impl FnMut(&str) -> T) {
  if buffer.is_empty() {
    return;
  }
  nodes.push(literal(buffer));
  buffer.clear();
}

/// 用途別テンプレート型の共通実装（newtype・解析・`serde`・`garde`）を生成する
///
/// 用途ごとに違うのは許可プレースホルダと `expand` の引数だけなので、配管はここで 1 度だけ書く。
macro_rules! define_template {
  ($(#[$meta:meta])* $name:ident($placeholder:ty)) => {
    $(#[$meta])*
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct $name(Analyzed<$placeholder>);

    impl $name {
      /// テンプレート文字列を解析する（解析は失敗せず、問題は検証で報告される）
      #[must_use]
      pub(crate) fn parse(source: &str) -> Self { return Self(Analyzed::parse(source)); }

      /// TOML に書かれた元の文字列
      #[must_use]
      pub(super) fn as_str(&self) -> &str { return self.0.as_str(); }
    }

    impl<'de> Deserialize<'de> for $name {
      fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        return Ok(Self::parse(&String::deserialize(deserializer)?));
      }
    }

    impl Serialize for $name {
      fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.serialize_str(self.as_str());
      }
    }

    impl Validate for $name {
      type Context = ();

      fn validate_into(&self, _: &Self::Context, parent: &mut dyn FnMut() -> Path, report: &mut Report) {
        self.0.validate_into(parent, report);
      }
    }
  };
}

/// 見出し・図表キャプション書式で使えるプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberTitlePlaceholder {
  /// `{number}` — 採番された表示番号
  Number,
  /// `{title}` — 見出し・キャプション本文
  Title,
}

impl Placeholder for NumberTitlePlaceholder {
  const ALLOW_EMPTY: bool = false;

  fn from_name(name: &str) -> Option<Self> {
    return match name {
      "number" => Some(Self::Number),
      "title" => Some(Self::Title),
      _ => None,
    };
  }
}

/// 番号だけを埋める書式（数式タグ・脚注マーカー・順序付きリストのマーカー）のプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberPlaceholder {
  /// `{number}` — 採番された表示番号
  Number,
}

impl Placeholder for NumberPlaceholder {
  const ALLOW_EMPTY: bool = false;

  fn from_name(name: &str) -> Option<Self> {
    return match name {
      "number" => Some(Self::Number),
      _ => None,
    };
  }
}

/// カウンタ番号書式（`number_format`）で使えるプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterPlaceholder {
  /// `{n}` — このテンプレートを持つカウンタ自身の値
  Own,
  /// `{<counter_name>}` — 他カウンタ（祖先チェーン上）の値
  Counter(CounterName),
}

impl Placeholder for CounterPlaceholder {
  const ALLOW_EMPTY: bool = false;

  fn from_name(name: &str) -> Option<Self> {
    if name == "n" {
      return Some(Self::Own);
    }
    return CounterName::from_name(name).map(Self::Counter);
  }
}

/// 参照書式（`ref_format`）で使えるプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferencePlaceholder {
  /// `{number}` — 参照先の表示番号
  Number,
  /// `{display_name}` — 参照先カウンタの表示名（「図」「Theorem」等）
  DisplayName,
}

impl Placeholder for ReferencePlaceholder {
  const ALLOW_EMPTY: bool = false;

  fn from_name(name: &str) -> Option<Self> {
    return match name {
      "number" => Some(Self::Number),
      "display_name" => Some(Self::DisplayName),
      _ => None,
    };
  }
}

/// 定理見出し書式で使えるプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheoremHeadingPlaceholder {
  /// `{display_name}` — 定理クラスの表示名
  DisplayName,
  /// `{number}` — 採番された表示番号
  Number,
  /// `{title}` — 定理に付けられたタイトル
  Title,
  /// `{of}` — proof の証明対象への参照表示
  Of,
}

impl Placeholder for TheoremHeadingPlaceholder {
  const ALLOW_EMPTY: bool = false;

  fn from_name(name: &str) -> Option<Self> {
    return match name {
      "display_name" => Some(Self::DisplayName),
      "number" => Some(Self::Number),
      "title" => Some(Self::Title),
      "of" => Some(Self::Of),
      _ => None,
    };
  }
}

/// 走り文（ヘッダー・フッター）スロットで使えるプレースホルダ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunningPlaceholder {
  /// `{page}` — 印字ページ番号
  Page,
  /// `{pages}` — 総ページ数
  Pages,
  /// `{title}` — 文書タイトル
  Title,
  /// `{author}` — 著者
  Author,
  /// `{date}` — 日付
  Date,
}

impl Placeholder for RunningPlaceholder {
  const ALLOW_EMPTY: bool = true;

  fn from_name(name: &str) -> Option<Self> {
    return match name {
      "page" => Some(Self::Page),
      "pages" => Some(Self::Pages),
      "title" => Some(Self::Title),
      "author" => Some(Self::Author),
      "date" => Some(Self::Date),
      _ => None,
    };
  }
}

define_template! {
  /// `{number}` / `{title}` を持つ書式（見出し・図表キャプション）
  NumberTitleTemplate(NumberTitlePlaceholder)
}

define_template! {
  /// `{number}` だけを持つ書式（数式タグ・脚注マーカー・順序付きリストのマーカー）
  NumberTemplate(NumberPlaceholder)
}

define_template! {
  /// `{n}` / `{<counter_name>}` を持つ書式（カウンタ・定理の `number_format`）
  CounterTemplate(CounterPlaceholder)
}

define_template! {
  /// `{number}` / `{display_name}` を持つ書式（カウンタの `ref_format`）
  ReferenceTemplate(ReferencePlaceholder)
}

define_template! {
  /// `{display_name}` / `{number}` / `{title}` / `{of}` を持つ書式（定理見出しの 4 形式）
  TheoremHeadingTemplate(TheoremHeadingPlaceholder)
}

define_template! {
  /// `{page}` / `{pages}` / `{title}` / `{author}` / `{date}` を持つ書式（走り文スロット）
  RunningTemplate(RunningPlaceholder)
}

impl NumberTitleTemplate {
  /// 番号とタイトルを埋めて出力型 `T` の列に展開する
  ///
  /// `literal` はリテラル区間を出力型へ変換するクロージャ、`title` は `{title}` の位置で
  /// タイトルを生成するクロージャ。`title` はテンプレート中の `{title}` の出現回数ぶんだけ、
  /// 出現したときにだけ呼ぶ（タイトルの生成には `\footnote` の通し index を払い出す副作用が
  /// あるため、出現しないテンプレートで呼ぶと以降の脚注番号がずれる）。
  pub(crate) fn expand<T>(
    &self,
    number: &str,
    mut title: impl FnMut() -> Vec<T>,
    mut literal: impl FnMut(&str) -> T,
  ) -> Vec<T> {
    self.0.ensure_validated();
    let mut nodes: Vec<T> = Vec::new();
    let mut buffer = String::new();
    for part in &self.0.parts {
      match part {
        Part::Literal(range) => buffer.push_str(&self.0.source[range.clone()]),
        Part::Placeholder(NumberTitlePlaceholder::Number) => buffer.push_str(number),
        Part::Placeholder(NumberTitlePlaceholder::Title) => {
          flush_literal(&mut nodes, &mut buffer, &mut literal);
          nodes.extend(title());
        },
      }
    }
    flush_literal(&mut nodes, &mut buffer, &mut literal);
    return nodes;
  }
}

impl NumberTemplate {
  /// `{number}` に表示番号を埋めた文字列を返す
  #[must_use]
  pub(crate) fn expand(&self, number: &str) -> String {
    return self.0.expand_to_string(|placeholder| {
      return match placeholder {
        NumberPlaceholder::Number => number.to_string(),
      };
    });
  }
}

impl CounterTemplate {
  /// 各プレースホルダを `resolve` で表示文字列にした結果を返す
  ///
  /// カウンタ値の復元（祖先チェーンの探索・`number_style` の適用）は caller の責務で、
  /// このテンプレートは「どの位置にどのカウンタを置くか」だけを知っている。
  #[must_use]
  pub(crate) fn expand(&self, mut resolve: impl FnMut(CounterPlaceholder) -> String) -> String {
    return self.0.expand_to_string(|placeholder| return resolve(*placeholder));
  }
}

impl ReferenceTemplate {
  /// `{number}` / `{display_name}` を埋めた文字列を返す
  #[must_use]
  pub(crate) fn expand(&self, number: &str, display_name: &str) -> String {
    return self.0.expand_to_string(|placeholder| {
      return match placeholder {
        ReferencePlaceholder::Number => number.to_string(),
        ReferencePlaceholder::DisplayName => display_name.to_string(),
      };
    });
  }
}

/// 定理見出しテンプレートへ渡す文字列値
#[derive(Debug, Clone, Copy)]
pub(crate) struct TheoremHeadingValues<'a> {
  /// `{display_name}` — 定理クラスの表示名
  pub display_name: &'a str,
  /// `{number}` — 表示番号
  pub number: &'a str,
  /// `{of}` — proof の証明対象への参照表示。`None` のときは空文字列に展開する
  pub of: Option<&'a str>,
}

impl TheoremHeadingTemplate {
  /// 定理見出しを出力型 `T` の列に展開する
  ///
  /// `{title}` の扱いは [`NumberTitleTemplate::expand`] と同じ（出現回数ぶんだけ遅延生成する）。
  /// `{of}` は `\ref` と違いリンクにしないので、前後のリテラルと 1 続きの値として繋ぐ。
  pub(crate) fn expand<T>(
    &self,
    values: TheoremHeadingValues<'_>,
    mut title: impl FnMut() -> Vec<T>,
    mut literal: impl FnMut(&str) -> T,
  ) -> Vec<T> {
    self.0.ensure_validated();
    let mut nodes: Vec<T> = Vec::new();
    let mut buffer = String::new();
    for part in &self.0.parts {
      match part {
        Part::Literal(range) => buffer.push_str(&self.0.source[range.clone()]),
        Part::Placeholder(TheoremHeadingPlaceholder::DisplayName) => buffer.push_str(values.display_name),
        Part::Placeholder(TheoremHeadingPlaceholder::Number) => buffer.push_str(values.number),
        Part::Placeholder(TheoremHeadingPlaceholder::Of) => buffer.push_str(values.of.unwrap_or_default()),
        Part::Placeholder(TheoremHeadingPlaceholder::Title) => {
          flush_literal(&mut nodes, &mut buffer, &mut literal);
          nodes.extend(title());
        },
      }
    }
    flush_literal(&mut nodes, &mut buffer, &mut literal);
    return nodes;
  }
}

/// 走り文テンプレートへ渡す文字列値
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunningValues<'a> {
  /// `{page}` — 印字ページ番号
  pub page: &'a str,
  /// `{pages}` — 総ページ数
  pub pages: &'a str,
  /// `{title}` — 文書タイトル
  pub title: &'a str,
  /// `{author}` — 著者
  pub author: &'a str,
  /// `{date}` — 日付
  pub date: &'a str,
}

impl RunningTemplate {
  /// メタデータとページ番号を埋めた文字列を返す
  ///
  /// 置換は単一パスなので、埋めた値（タイトル等）に `{page}` が含まれていても再解釈しない。
  #[must_use]
  pub(crate) fn expand(&self, values: RunningValues<'_>) -> String {
    return self.0.expand_to_string(|placeholder| {
      return match placeholder {
        RunningPlaceholder::Page => values.page.to_string(),
        RunningPlaceholder::Pages => values.pages.to_string(),
        RunningPlaceholder::Title => values.title.to_string(),
        RunningPlaceholder::Author => values.author.to_string(),
        RunningPlaceholder::Date => values.date.to_string(),
      };
    });
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use serde::{Deserialize, Serialize};

  use super::{
    CounterPlaceholder, CounterTemplate, NumberTemplate, NumberTitleTemplate, ReferenceTemplate, RunningTemplate,
    RunningValues, TheoremHeadingTemplate, TheoremHeadingValues,
  };
  use crate::style::CounterName;

  /// 検証違反のメッセージを取り出す（違反が無ければ `None`）
  fn violation<T: Validate<Context = ()>>(template: &T) -> Option<String> {
    return template.validate().err().map(|report| {
      return report.iter().map(|(_, error)| return error.to_string()).collect::<Vec<_>>().join(" / ");
    });
  }

  /// `{title}` をプレーンな文字列ノードとして扱う出力型（構造展開のテスト用）
  #[derive(Debug, PartialEq, Eq)]
  enum Node {
    /// リテラル区間
    Literal(String),
    /// `{title}` の位置で生成されたタイトル
    Title(String),
  }

  #[test]
  fn expands_literals_and_placeholders() {
    // Arrange
    let template = NumberTemplate::parse("({number})");

    // Act / Assert
    assert_eq!(template.expand("1.1"), "(1.1)");
    assert_eq!(template.as_str(), "({number})");
  }

  #[test]
  fn expands_japanese_literals_without_breaking_utf8() {
    // Arrange
    let template = CounterTemplate::parse("第{n}章・{chapter}節");

    // Act
    let text = template.expand(|placeholder| {
      return match placeholder {
        CounterPlaceholder::Own => "3".to_string(),
        CounterPlaceholder::Counter(CounterName::Chapter) => "２".to_string(),
        CounterPlaceholder::Counter(_) => String::new(),
      };
    });

    // Assert
    assert_eq!(text, "第3章・２節");
  }

  #[test]
  fn expansion_is_single_pass_and_does_not_reinterpret_values() {
    // Arrange — 埋める値そのものがプレースホルダの見た目をしている
    let template = RunningTemplate::parse("{title} — {page}");

    // Act
    let text = template.expand(RunningValues {
      page: "7",
      pages: "9",
      title: "{page} 章",
      author: "",
      date: "",
    });

    // Assert
    assert_eq!(text, "{page} 章 — 7", "埋めた値の中の {{page}} は再解釈されない");
  }

  #[test]
  fn rejects_unclosed_brace() {
    // Arrange / Act / Assert
    assert_eq!(violation(&NumberTemplate::parse("{number")).unwrap(), "閉じられていない '{' があります");
  }

  #[test]
  fn rejects_stray_closing_brace() {
    // Arrange / Act / Assert
    assert_eq!(violation(&NumberTemplate::parse("x}y")).unwrap(), "対応する '{' のない '}' があります");
  }

  #[test]
  fn rejects_empty_placeholder() {
    // Arrange / Act / Assert
    assert_eq!(violation(&NumberTemplate::parse("{}")).unwrap(), "空のプレースホルダ '{}' があります");
  }

  #[test]
  fn rejects_nested_braces() {
    // Arrange / Act
    let message = violation(&NumberTemplate::parse("{a{b}}")).unwrap();

    // Assert
    assert!(message.starts_with("プレースホルダがネストしています"), "{message}");
  }

  #[test]
  fn rejects_unknown_placeholder_by_name() {
    // Arrange / Act / Assert
    assert_eq!(violation(&NumberTemplate::parse("{nubmer}")).unwrap(), "未知のプレースホルダ '{nubmer}' があります");
  }

  #[test]
  fn reports_multiple_problems_in_one_violation_in_source_order() {
    // Arrange / Act
    let message = violation(&NumberTemplate::parse("{a} } {b}")).unwrap();

    // Assert — 1 テンプレート ＝ 違反 1 件（フィールド単位の集約は style::validate_values の担当）
    assert_eq!(
      message,
      "未知のプレースホルダ '{a}' があります; 対応する '{' のない '}' があります; \
       未知のプレースホルダ '{b}' があります"
    );
  }

  #[test]
  fn each_template_type_allows_only_its_own_placeholders() {
    // Arrange / Act / Assert
    assert!(violation(&NumberTitleTemplate::parse("{number} {title}")).is_none());
    assert!(violation(&NumberTitleTemplate::parse("{display_name}")).is_some());
    assert!(violation(&NumberTemplate::parse("{title}")).is_some());
    assert!(violation(&ReferenceTemplate::parse("{display_name} {number}")).is_none());
    assert!(violation(&ReferenceTemplate::parse("{title}")).is_some());
    assert!(violation(&TheoremHeadingTemplate::parse("{display_name} of {of} ({title}) {number}")).is_none());
    assert!(violation(&TheoremHeadingTemplate::parse("{page}")).is_some());
    assert!(violation(&RunningTemplate::parse("{page} / {pages} — {title}{author}{date}")).is_none());
    assert!(violation(&RunningTemplate::parse("{pagee}")).is_some());
  }

  #[test]
  fn counter_template_accepts_self_and_all_nine_counters() {
    // Arrange
    let source: String = std::iter::once("{n}".to_string())
      .chain(CounterName::ALL.iter().map(|counter| return format!("{{{}}}", counter.as_str())))
      .collect();

    // Act / Assert
    assert!(violation(&CounterTemplate::parse(&source)).is_none());
    assert!(violation(&CounterTemplate::parse("{foo}")).is_some());
  }

  #[test]
  fn only_the_running_template_accepts_an_empty_source() {
    // Arrange / Act / Assert
    assert!(violation(&RunningTemplate::parse("")).is_none());
    assert_eq!(violation(&NumberTemplate::parse("")).unwrap(), "テンプレートが空です");
    assert!(violation(&NumberTitleTemplate::parse("")).is_some());
    assert!(violation(&CounterTemplate::parse("")).is_some());
    assert!(violation(&ReferenceTemplate::parse("")).is_some());
    assert!(violation(&TheoremHeadingTemplate::parse("")).is_some());
  }

  #[test]
  fn serde_round_trip_keeps_the_source_string() {
    // Arrange
    #[derive(Debug, Deserialize, Serialize)]
    struct Wrapper {
      /// テンプレート 1 本だけを持つ TOML テーブル
      format: NumberTitleTemplate,
    }
    let toml = "format = \"第{number}章 {title}\"\n";

    // Act
    let wrapper: Wrapper = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(wrapper.format.as_str(), "第{number}章 {title}");
    assert_eq!(toml::to_string(&wrapper).unwrap(), toml);
  }

  #[test]
  fn deserialize_keeps_invalid_templates_for_garde_to_report() {
    // Arrange
    #[derive(Debug, Deserialize)]
    struct Wrapper {
      /// 未知プレースホルダを含むテンプレート
      format: NumberTemplate,
    }

    // Act — 構文エラーでも deserialize は成功する（複数フィールドの一括報告を打ち切らないため）
    let wrapper: Wrapper = toml::from_str("format = \"{nope}\"\n").unwrap();

    // Assert
    assert!(violation(&wrapper.format).is_some());
  }

  #[test]
  fn structural_expansion_keeps_literal_and_title_order() {
    // Arrange
    let template = NumberTitleTemplate::parse("第{number}章 {title} 終");

    // Act
    let nodes = template.expand(
      "3",
      || return vec![Node::Title("序論".to_string())],
      |literal| return Node::Literal(literal.to_string()),
    );

    // Assert
    assert_eq!(
      nodes,
      vec![
        Node::Literal("第3章 ".to_string()),
        Node::Title("序論".to_string()),
        Node::Literal(" 終".to_string()),
      ]
    );
  }

  #[test]
  fn title_closure_is_not_called_without_a_title_placeholder() {
    // Arrange — タイトル生成には脚注 index を払い出す副作用があるので呼んではならない
    let template = NumberTitleTemplate::parse("No.{number}");
    let mut calls = 0_u32;

    // Act
    let nodes = template.expand(
      "7",
      || {
        calls += 1;
        return vec![Node::Title("捨てられる".to_string())];
      },
      |literal| return Node::Literal(literal.to_string()),
    );

    // Assert
    assert_eq!(calls, 0);
    assert_eq!(nodes, vec![Node::Literal("No.7".to_string())]);
  }

  #[test]
  fn title_closure_is_called_once_per_occurrence() {
    // Arrange
    let template = NumberTitleTemplate::parse("{title} / {title}");
    let mut calls = 0_u32;

    // Act
    let nodes = template.expand(
      "7",
      || {
        calls += 1;
        return vec![Node::Title(format!("T{calls}"))];
      },
      |literal| return Node::Literal(literal.to_string()),
    );

    // Assert — 2 回とも別々に生成される（clone で同じノードを 2 つ置くのではない）
    assert_eq!(calls, 2);
    assert_eq!(
      nodes,
      vec![
        Node::Title("T1".to_string()),
        Node::Literal(" / ".to_string()),
        Node::Title("T2".to_string()),
      ]
    );
  }

  #[test]
  fn theorem_heading_omits_of_when_absent() {
    // Arrange
    let template = TheoremHeadingTemplate::parse("{display_name} {number}{of}");

    // Act
    let nodes = template.expand(
      TheoremHeadingValues {
        display_name: "Theorem",
        number: "1",
        of: None,
      },
      Vec::new,
      |literal| return Node::Literal(literal.to_string()),
    );

    // Assert
    assert_eq!(nodes, vec![Node::Literal("Theorem 1".to_string())]);
  }

  #[test]
  #[should_panic(expected = "検証を通ったテンプレートだけが展開へ届く")]
  fn expanding_an_unvalidated_template_panics() {
    // Arrange — style::parse を通っていれば到達しない状態
    let template = NumberTemplate::parse("{nope}");

    // Act / Assert
    let _ = template.expand("1");
  }
}
