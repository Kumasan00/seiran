//! 定理環境（theorem / lemma / proof …）のスタイル設定型。

use std::ops::Index;

use garde::Validate;
use serde::Deserialize;
use strum::VariantArray;

use crate::{
  document::{TheoremClass, Typeface},
  length::{Length, non_negative},
  style::{CounterName, CounterTemplate, TheoremHeadingTemplate},
};

/// 固定 10 種の定理クラス定義テーブル（`[theorems.<class>]`）。
///
/// TOML からは [`TheoremStylesTable`]（各エントリが差分指定 [`TheoremStyleOverride`]）として読み、
/// [`TheoremStyles::default`] のクラス別既定へ重ねて解決済みの値を作る。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(from = "TheoremStylesTable")]
pub(crate) struct TheoremStyles {
  /// `[theorems.theorem]`
  #[garde(dive)]
  pub theorem: TheoremStyle,
  /// `[theorems.lemma]`
  #[garde(dive)]
  pub lemma: TheoremStyle,
  /// `[theorems.proposition]`
  #[garde(dive)]
  pub proposition: TheoremStyle,
  /// `[theorems.corollary]`
  #[garde(dive)]
  pub corollary: TheoremStyle,
  /// `[theorems.definition]`
  #[garde(dive)]
  pub definition: TheoremStyle,
  /// `[theorems.axiom]`
  #[garde(dive)]
  pub axiom: TheoremStyle,
  /// `[theorems.example]`
  #[garde(dive)]
  pub example: TheoremStyle,
  /// `[theorems.remark]`
  #[garde(dive)]
  pub remark: TheoremStyle,
  /// `[theorems.claim]`
  #[garde(dive)]
  pub claim: TheoremStyle,
  /// `[theorems.proof]`
  #[garde(dive)]
  pub proof: TheoremStyle,
}

impl Default for TheoremStyles {
  fn default() -> Self {
    return Self {
      theorem: TheoremStyle {
        display_name: "Theorem".to_string(),
        ..TheoremStyle::default()
      },
      lemma: TheoremStyle {
        display_name: "Lemma".to_string(),
        ..TheoremStyle::default()
      },
      proposition: TheoremStyle {
        display_name: "Proposition".to_string(),
        ..TheoremStyle::default()
      },
      corollary: TheoremStyle {
        display_name: "Corollary".to_string(),
        ..TheoremStyle::default()
      },
      definition: TheoremStyle {
        display_name: "Definition".to_string(),
        counter: "definition".to_string(),
        presentation: TheoremPresentation {
          typeface: Typeface::Serif,
          ..TheoremPresentation::default()
        },
        ..TheoremStyle::default()
      },
      axiom: TheoremStyle {
        display_name: "Axiom".to_string(),
        counter: "axiom".to_string(),
        ..TheoremStyle::default()
      },
      example: TheoremStyle {
        display_name: "Example".to_string(),
        counter: "example".to_string(),
        presentation: TheoremPresentation {
          typeface: Typeface::Serif,
          ..TheoremPresentation::default()
        },
        ..TheoremStyle::default()
      },
      remark: TheoremStyle {
        display_name: "Remark".to_string(),
        counter: "remark".to_string(),
        presentation: TheoremPresentation {
          typeface: Typeface::Serif,
          ..TheoremPresentation::default()
        },
        ..TheoremStyle::default()
      },
      claim: TheoremStyle {
        display_name: "Claim".to_string(),
        ..TheoremStyle::default()
      },
      proof: TheoremStyle {
        display_name: "Proof".to_string(),
        counter: "proof".to_string(),
        unnumbered: true,
        qed_mark: Some("□".to_string()),
        presentation: TheoremPresentation {
          typeface: Typeface::Serif,
          heading_format: TheoremHeadingTemplate::parse("{display_name}"),
          heading_with_title: TheoremHeadingTemplate::parse("{display_name} ({title})"),
          ..TheoremPresentation::default()
        },
        ..TheoremStyle::default()
      },
    };
  }
}

impl Index<TheoremClass> for TheoremStyles {
  type Output = TheoremStyle;

  fn index(&self, class: TheoremClass) -> &TheoremStyle {
    return match class {
      TheoremClass::Theorem => &self.theorem,
      TheoremClass::Lemma => &self.lemma,
      TheoremClass::Proposition => &self.proposition,
      TheoremClass::Corollary => &self.corollary,
      TheoremClass::Definition => &self.definition,
      TheoremClass::Axiom => &self.axiom,
      TheoremClass::Example => &self.example,
      TheoremClass::Remark => &self.remark,
      TheoremClass::Claim => &self.claim,
      TheoremClass::Proof => &self.proof,
    };
  }
}

/// 1 つの定理クラスのスタイル定義（クラス別既定 + 差分上書きで解決済み）。
///
/// TOML のスキーマは [`TheoremStyleOverride`]。
#[derive(Debug, Clone, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct TheoremStyle {
  /// 表示名（例: `"Theorem"`、`"定理"`）。見出し書式の `{display_name}` から参照される
  #[garde(length(chars, min = 1))]
  pub display_name: String,
  /// 共有カウンタ名。同じ名前を指定したクラスは 1 つのカウンタを共有する
  /// （LaTeX の `\newtheorem{lemma}[theorem]{...}` 第 2 引数の明示化）
  #[garde(length(chars, min = 1))]
  pub counter: String,
  /// このクラスのカウンタのリセット先（見出しレベル or なし）
  pub reset_by: TheoremReset,
  /// 番号構築テンプレート。`{n}` で自身、`{<counter_name>}` で他カウンタの値を埋め込む
  /// （counter の `number_format` と同形。例: `"{n}"`、`"{chapter}.{n}"`）
  #[garde(dive)]
  pub number_format: CounterTemplate,
  /// 採番しない（`proof` 等）
  pub unnumbered: bool,
  /// QED マーク（`proof` 末尾に配置する記号）。`None` のときマークなし
  #[garde(inner(length(chars, min = 1)))]
  pub qed_mark: Option<String>,
  /// 見出し書式・本文/見出しフォント・上下マージン
  #[garde(dive)]
  pub presentation: TheoremPresentation,
}

impl Default for TheoremStyle {
  fn default() -> Self {
    return Self {
      display_name: "Theorem".to_string(),
      counter: "theorem".to_string(),
      reset_by: TheoremReset::None,
      number_format: CounterTemplate::parse("{n}"),
      unnumbered: false,
      qed_mark: None,
      presentation: TheoremPresentation::default(),
    };
  }
}

/// 定理カウンタのリセット先。`reset_by` フィールドで指定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, VariantArray)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TheoremReset {
  /// 部が進むたびにリセット
  Part,
  /// 章が進むたびにリセット
  Chapter,
  /// 節が進むたびにリセット
  Section,
  /// 小節が進むたびにリセット
  Subsection,
  /// リセットしない（文書全体で連番）
  None,
}

impl TheoremReset {
  /// リセット元の見出しカウンタを返す（`None` はリセットしない＝対応する見出しカウンタなし）
  ///
  /// `TheoremReset` と [`CounterName`] の対応はこの網羅 match が唯一の正典。
  #[must_use]
  pub(crate) fn counter_name(self) -> Option<CounterName> {
    return match self {
      Self::Part => Some(CounterName::Part),
      Self::Chapter => Some(CounterName::Chapter),
      Self::Section => Some(CounterName::Section),
      Self::Subsection => Some(CounterName::Subsection),
      Self::None => None,
    };
  }
}

/// 定理ブロックの見た目（見出し書式・フォント・マージン）。
///
/// TOML のスキーマは [`TheoremPresentationOverride`]。
#[derive(Debug, Clone, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct TheoremPresentation {
  /// サブタイトルなしの見出し書式。`{display_name}` と `{number}` を含められる
  #[garde(dive)]
  pub heading_format: TheoremHeadingTemplate,
  /// サブタイトルありの見出し書式。`{display_name}` / `{number}` / `{title}` を含められる
  #[garde(dive)]
  pub heading_with_title: TheoremHeadingTemplate,
  /// 証明対象（`of`）ありサブタイトルなしの見出し書式。`{display_name}` / `{of}` を含められる。
  /// `proof` の `[of=...]` 指定時に使う。
  #[garde(dive)]
  pub heading_with_of: TheoremHeadingTemplate,
  /// 証明対象（`of`）ありサブタイトルありの見出し書式。`{display_name}` / `{of}` / `{title}` を含められる。
  #[garde(dive)]
  pub heading_with_of_and_title: TheoremHeadingTemplate,
  /// 本文の書体（定理は斜体、証明・定義系はローマン）
  pub typeface: Typeface,
  /// 見出しの書体（既定は太字セリフ）
  pub heading_typeface: Typeface,
  /// 定理ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 定理ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for TheoremPresentation {
  fn default() -> Self {
    return Self {
      heading_format: TheoremHeadingTemplate::parse("{display_name} {number}"),
      heading_with_title: TheoremHeadingTemplate::parse("{display_name} {number} ({title})"),
      heading_with_of: TheoremHeadingTemplate::parse("{display_name} of {of}"),
      heading_with_of_and_title: TheoremHeadingTemplate::parse("{display_name} of {of} ({title})"),
      typeface: Typeface::SerifItalic,
      heading_typeface: Typeface::SerifBold,
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
    };
  }
}

/// `[theorems]` テーブル全体の TOML スキーマ。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct TheoremStylesTable {
  /// `theorem` クラスの上書き
  theorem: TheoremStyleOverride,
  /// `lemma` クラスの上書き
  lemma: TheoremStyleOverride,
  /// `proposition` クラスの上書き
  proposition: TheoremStyleOverride,
  /// `corollary` クラスの上書き
  corollary: TheoremStyleOverride,
  /// `definition` クラスの上書き
  definition: TheoremStyleOverride,
  /// `axiom` クラスの上書き
  axiom: TheoremStyleOverride,
  /// `example` クラスの上書き
  example: TheoremStyleOverride,
  /// `remark` クラスの上書き
  remark: TheoremStyleOverride,
  /// `claim` クラスの上書き
  claim: TheoremStyleOverride,
  /// `proof` クラスの上書き
  proof: TheoremStyleOverride,
}

impl From<TheoremStylesTable> for TheoremStyles {
  fn from(table: TheoremStylesTable) -> Self {
    let defaults = Self::default();
    return Self {
      theorem: table.theorem.apply(defaults.theorem),
      lemma: table.lemma.apply(defaults.lemma),
      proposition: table.proposition.apply(defaults.proposition),
      corollary: table.corollary.apply(defaults.corollary),
      definition: table.definition.apply(defaults.definition),
      axiom: table.axiom.apply(defaults.axiom),
      example: table.example.apply(defaults.example),
      remark: table.remark.apply(defaults.remark),
      claim: table.claim.apply(defaults.claim),
      proof: table.proof.apply(defaults.proof),
    };
  }
}

/// [`TheoremStyle`] の各フィールドを `Option<_>` で覆った差分指定型（`[theorems.<class>]` の TOML スキーマ）。
///
/// `None` のフィールドはクラス別既定のまま残す。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct TheoremStyleOverride {
  /// 表示名
  display_name: Option<String>,
  /// 共有カウンタ名
  counter: Option<String>,
  /// カウンタのリセット先
  reset_by: Option<TheoremReset>,
  /// 番号構築テンプレート
  number_format: Option<CounterTemplate>,
  /// 採番しないか
  unnumbered: Option<bool>,
  /// QED マーク（TOML からは設定のみ可。`None` への解除は非対応）
  qed_mark: Option<String>,
  /// 見た目（ネストした差分）
  presentation: TheoremPresentationOverride,
}

impl TheoremStyleOverride {
  /// 自身の `Some` 値で `base` のフィールドを置き換えた値を返す。
  ///
  /// `self` の分割と戻り値のリテラルがどちらも `..` 無しなので、差分指定型・解決済み型のどちらに
  /// フィールドを足しても、ここで扱いを決めるまでコンパイルが通らない。
  fn apply(self, base: TheoremStyle) -> TheoremStyle {
    let Self {
      display_name,
      counter,
      reset_by,
      number_format,
      unnumbered,
      qed_mark,
      presentation,
    } = self;
    return TheoremStyle {
      display_name: display_name.unwrap_or(base.display_name),
      counter: counter.unwrap_or(base.counter),
      reset_by: reset_by.unwrap_or(base.reset_by),
      number_format: number_format.unwrap_or(base.number_format),
      unnumbered: unnumbered.unwrap_or(base.unnumbered),
      qed_mark: qed_mark.or(base.qed_mark),
      presentation: presentation.apply(base.presentation),
    };
  }
}

/// [`TheoremPresentation`] の各フィールドを `Option<_>` で覆った差分指定型（`[theorems.<class>.presentation]` の TOML スキーマ）。
///
/// `None` のフィールドはクラス別既定のまま残す。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct TheoremPresentationOverride {
  /// サブタイトルなしの見出し書式
  heading_format: Option<TheoremHeadingTemplate>,
  /// サブタイトルありの見出し書式
  heading_with_title: Option<TheoremHeadingTemplate>,
  /// 証明対象（`of`）ありサブタイトルなしの見出し書式
  heading_with_of: Option<TheoremHeadingTemplate>,
  /// 証明対象（`of`）ありサブタイトルありの見出し書式
  heading_with_of_and_title: Option<TheoremHeadingTemplate>,
  /// 本文の書体
  typeface: Option<Typeface>,
  /// 見出しの書体
  heading_typeface: Option<Typeface>,
  /// 上余白
  top_margin: Option<Length>,
  /// 下余白
  bottom_margin: Option<Length>,
}

impl TheoremPresentationOverride {
  /// 自身の `Some` 値で `base` のフィールドを置き換えた値を返す。
  ///
  /// `self` の分割と戻り値のリテラルがどちらも `..` 無しなので、差分指定型・解決済み型のどちらに
  /// フィールドを足しても、ここで扱いを決めるまでコンパイルが通らない。
  fn apply(self, base: TheoremPresentation) -> TheoremPresentation {
    let Self {
      heading_format,
      heading_with_title,
      heading_with_of,
      heading_with_of_and_title,
      typeface,
      heading_typeface,
      top_margin,
      bottom_margin,
    } = self;
    return TheoremPresentation {
      heading_format: heading_format.unwrap_or(base.heading_format),
      heading_with_title: heading_with_title.unwrap_or(base.heading_with_title),
      heading_with_of: heading_with_of.unwrap_or(base.heading_with_of),
      heading_with_of_and_title: heading_with_of_and_title.unwrap_or(base.heading_with_of_and_title),
      typeface: typeface.unwrap_or(base.typeface),
      heading_typeface: heading_typeface.unwrap_or(base.heading_typeface),
      top_margin: top_margin.unwrap_or(base.top_margin),
      bottom_margin: bottom_margin.unwrap_or(base.bottom_margin),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use strum::VariantArray;

  use super::{TheoremClass, TheoremReset, TheoremStyle, TheoremStyles};
  use crate::{
    document::Typeface,
    length::Length,
    style::{CounterName, CounterTemplate, TheoremHeadingTemplate},
  };

  /// `TheoremStyles` を TOML から `[theorems.<class>]` 配下に書く形でテストするための薄いラッパ。
  #[derive(Debug, serde::Deserialize)]
  struct TheoremStylesWrapper {
    theorems: TheoremStyles,
  }

  #[test]
  fn validate_rejects_empty_qed_mark() {
    let style = TheoremStyle {
      qed_mark: Some(String::new()),
      ..TheoremStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_unknown_heading_placeholder() {
    let mut style = TheoremStyle::default();
    style.presentation.heading_format = TheoremHeadingTemplate::parse("{page}");

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_unknown_counter_reference_in_number_format() {
    let style = TheoremStyle {
      number_format: CounterTemplate::parse("{chaptr}.{n}"),
      ..TheoremStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn default_proof_is_unnumbered_with_qed_mark() {
    let theorems = TheoremStyles::default();
    let proof = &theorems[TheoremClass::Proof];

    assert!(proof.unnumbered);
    assert_eq!(proof.qed_mark.as_deref(), Some("□"));
    assert_eq!(proof.presentation.typeface, Typeface::Serif);
    assert_eq!(proof.presentation.heading_format.as_str(), "{display_name}");
  }

  #[test]
  fn default_theorem_like_classes_share_counter_and_italic_body() {
    let theorems = TheoremStyles::default();

    for class in [
      TheoremClass::Theorem,
      TheoremClass::Lemma,
      TheoremClass::Proposition,
      TheoremClass::Corollary,
      TheoremClass::Claim,
    ] {
      let style = &theorems[class];
      assert_eq!(style.counter, "theorem", "{class} should share theorem counter");
      assert_eq!(style.presentation.typeface, Typeface::SerifItalic);
      assert!(!style.unnumbered);
    }
  }

  #[test]
  fn default_remark_style_uses_roman_body_and_own_counter() {
    let theorems = TheoremStyles::default();
    let remark = &theorems[TheoremClass::Remark];

    assert_eq!(remark.counter, "remark");
    assert_eq!(remark.presentation.typeface, Typeface::Serif);
  }

  #[test]
  fn default_classes_have_expected_display_name_and_counter() {
    let expected: [(TheoremClass, &str, &str); 10] = [
      (TheoremClass::Theorem, "Theorem", "theorem"),
      (TheoremClass::Lemma, "Lemma", "theorem"),
      (TheoremClass::Proposition, "Proposition", "theorem"),
      (TheoremClass::Corollary, "Corollary", "theorem"),
      (TheoremClass::Definition, "Definition", "definition"),
      (TheoremClass::Axiom, "Axiom", "axiom"),
      (TheoremClass::Example, "Example", "example"),
      (TheoremClass::Remark, "Remark", "remark"),
      (TheoremClass::Claim, "Claim", "theorem"),
      (TheoremClass::Proof, "Proof", "proof"),
    ];
    let theorems = TheoremStyles::default();

    for (class, display_name, counter) in expected {
      assert_eq!(theorems[class].display_name, display_name, "{class} の表示名");
      assert_eq!(theorems[class].counter, counter, "{class} の共有カウンタ");
    }
  }

  #[test]
  fn partial_override_keeps_other_class_defaults() {
    let toml = "
[theorems.lemma]
display_name = \"補題\"
";

    let wrapper: TheoremStylesWrapper = toml::from_str(toml).unwrap();
    let theorems = wrapper.theorems;

    assert_eq!(theorems.lemma.display_name, "補題");
    assert_eq!(theorems.lemma.counter, "theorem");
    assert_eq!(theorems.lemma.presentation.typeface, Typeface::SerifItalic);
    assert_eq!(theorems.theorem.display_name, "Theorem");
    assert!(theorems.proof.unnumbered);
  }

  #[test]
  fn partial_override_nested_presentation_keeps_other_presentation_fields() {
    let toml = "
[theorems.theorem.presentation]
typeface = \"sans_serif_bold\"
";

    let wrapper: TheoremStylesWrapper = toml::from_str(toml).unwrap();
    let theorem = wrapper.theorems.theorem;

    assert_eq!(theorem.presentation.typeface, Typeface::SansSerifBold);
    assert_eq!(theorem.presentation.heading_format.as_str(), "{display_name} {number}");
    assert!((theorem.presentation.top_margin.to_pt() - 12.0).abs() < f32::EPSILON);
  }

  #[test]
  fn full_override_replaces_every_field() {
    // 6 キーとネスト `presentation` の 8 キーを全部書いた形（同型のフィールド同士の取り違えを検出する）
    let toml = "
[theorems.theorem]
display_name = \"定理\"
counter = \"main\"
reset_by = \"chapter\"
number_format = \"{chapter}.{n}\"
unnumbered = true
qed_mark = \"■\"

[theorems.theorem.presentation]
heading_format = \"{display_name}{number}\"
heading_with_title = \"{display_name}{number}（{title}）\"
heading_with_of = \"{of}の{display_name}\"
heading_with_of_and_title = \"{of}の{display_name}（{title}）\"
typeface = \"serif\"
heading_typeface = \"sans_serif_bold\"
top_margin = \"6pt\"
bottom_margin = \"8pt\"
";

    let wrapper: TheoremStylesWrapper = toml::from_str(toml).unwrap();
    let theorem = wrapper.theorems.theorem;

    assert_eq!(theorem.display_name, "定理");
    assert_eq!(theorem.counter, "main");
    assert_eq!(theorem.reset_by, TheoremReset::Chapter);
    assert_eq!(theorem.number_format.as_str(), "{chapter}.{n}");
    assert!(theorem.unnumbered);
    assert_eq!(theorem.qed_mark.as_deref(), Some("■"));
    assert_eq!(theorem.presentation.heading_format.as_str(), "{display_name}{number}");
    assert_eq!(theorem.presentation.heading_with_title.as_str(), "{display_name}{number}（{title}）");
    assert_eq!(theorem.presentation.heading_with_of.as_str(), "{of}の{display_name}");
    assert_eq!(theorem.presentation.heading_with_of_and_title.as_str(), "{of}の{display_name}（{title}）");
    assert_eq!(theorem.presentation.typeface, Typeface::Serif);
    assert_eq!(theorem.presentation.heading_typeface, Typeface::SansSerifBold);
    assert_eq!(theorem.presentation.top_margin, Length::pt(6.0));
    assert_eq!(theorem.presentation.bottom_margin, Length::pt(8.0));
  }

  #[test]
  fn partial_proof_override_keeps_default_qed_mark() {
    let toml = "
[theorems.proof]
display_name = \"証明\"
";

    let wrapper: TheoremStylesWrapper = toml::from_str(toml).unwrap();
    let proof = wrapper.theorems.proof;

    assert_eq!(proof.display_name, "証明");
    assert_eq!(proof.qed_mark.as_deref(), Some("□"));
    assert!(proof.unnumbered);
  }

  #[test]
  fn default_proof_of_templates_render_proof_of_target() {
    let theorems = TheoremStyles::default();
    let proof = &theorems[TheoremClass::Proof];

    assert_eq!(proof.presentation.heading_with_of.as_str(), "{display_name} of {of}");
    assert_eq!(proof.presentation.heading_with_of_and_title.as_str(), "{display_name} of {of} ({title})");
  }

  #[test]
  fn override_proof_of_template_localizes_prefix() {
    let toml = "
[theorems.proof.presentation]
heading_with_of = \"{display_name}（{of} の証明）\"
";

    let wrapper: TheoremStylesWrapper = toml::from_str(toml).unwrap();

    assert_eq!(wrapper.theorems.proof.presentation.heading_with_of.as_str(), "{display_name}（{of} の証明）");
    assert_eq!(
      wrapper.theorems.proof.presentation.heading_with_of_and_title.as_str(),
      "{display_name} of {of} ({title})"
    );
  }

  #[test]
  fn rejects_unknown_class_key() {
    let toml = "
[theorems.conjecture]
display_name = \"Conjecture\"
";

    let result: Result<TheoremStylesWrapper, _> = toml::from_str(toml);

    assert!(result.is_err(), "未知のクラス名は TOML パース時に拒否されるべき: {result:?}");
  }

  #[test]
  fn rejects_renamed_format_key() {
    let toml = "
[theorems.theorem]
format = \"{section}.{n}\"
";

    let result: Result<TheoremStylesWrapper, _> = toml::from_str(toml);

    assert!(result.is_err(), "旧キー `format` は未知フィールドとして拒否される: {result:?}");
  }

  #[test]
  fn rejects_renamed_style_subtable() {
    let toml = "
[theorems.theorem.style]
typeface = \"serif\"
";

    let message = toml::from_str::<TheoremStylesWrapper>(toml).unwrap_err().to_string();

    assert!(
      message.contains("unknown field `style`"),
      "旧サブテーブル `style` は未知フィールドとして拒否される: {message}"
    );
  }

  #[test]
  fn rejects_unknown_nested_presentation_key() {
    let toml = "
[theorems.theorem.presentation]
font_knd = \"serif\"
";

    let result: Result<TheoremStylesWrapper, _> = toml::from_str(toml);

    assert!(result.is_err(), "ネストした未知のフィールド名は拒否されるべき: {result:?}");
  }

  #[test]
  fn counter_name_covers_every_reset_level() {
    let expected: [(TheoremReset, Option<CounterName>); 5] = [
      (TheoremReset::Part, Some(CounterName::Part)),
      (TheoremReset::Chapter, Some(CounterName::Chapter)),
      (TheoremReset::Section, Some(CounterName::Section)),
      (TheoremReset::Subsection, Some(CounterName::Subsection)),
      (TheoremReset::None, None),
    ];

    for (level, want) in expected {
      assert_eq!(level.counter_name(), want, "{level:?} が指す見出しカウンタ");
    }
    assert_eq!(expected.map(|(level, _)| return level), TheoremReset::VARIANTS, "全 5 バリアントを宣言順で覆う");
  }
}
