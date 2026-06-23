//! 定理環境（theorem / lemma / proof …）のスタイル設定型。
//!
//! マクロ禁止（軸 1-C）のため `\newtheorem` 相当は持たず、10 種のビルトイン定理クラスを
//! `style.toml` の `[theorems.<class>]` で宣言する。`<class>` は固定 10 種（[`TheoremClass`]）の
//! みが許可され、それ以外のキーは TOML パース時に拒否される。
//!
//! TOML 上の `[theorems]` テーブルは [`crate::heading`] と同じ 2 レイヤーで解釈する:
//!
//! 1. [`default_for_class`] で得る Rust 既定（定理は本文斜体・見出し太字、証明は本文ローマン＋QED）
//! 2. `[theorems.<class>]` テーブル（クラス別の差分上書き）
//!
//! このマージは [`TheoremsTable`] が `#[serde(from = ...)]` 経由で実行する。ネストした
//! `[theorems.<class>.style]` の部分上書きも [`TheoremPresentationOverride`] で同様に処理する。
//!
//! 本モジュールは `read_style` 層の責務（読込・検証・既定マージ）のみを担う。環境登録・カウンタ
//! 共有・採番は parser、見出し書式・斜体本文・QED 描画は lowering / `pdf_gen` が後段で担当する。

use garde::Validate;
use serde::{Deserialize, Serialize};
// 定理クラス enum は `types` を単一ソースとし、`read_style::theorem::TheoremClass` として再エクスポートする。
// `document::DocNode::Theorem` も同じ enum を共有するため（`HeadingLevel` / `MathEnvKind` と同じ配置方針）。
pub use types::TheoremClass;
use types::{
  FontKind,
  length::{Length, non_negative},
};

/// 固定 10 種の定理クラス定義テーブル（`[theorems.<class>]`）。
///
/// TOML 上は `[theorems.<class>]` テーブル群（[`TheoremsTable`] 経由）から 2 レイヤーマージ
/// （クラス別既定 → 差分）でデシリアライズする。消費側は [`Theorems::get`] でアクセスする。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "TheoremsTable")]
pub struct Theorems {
  /// `[theorems.theorem]`
  pub theorem: TheoremStyle,
  /// `[theorems.lemma]`
  pub lemma: TheoremStyle,
  /// `[theorems.proposition]`
  pub proposition: TheoremStyle,
  /// `[theorems.corollary]`
  pub corollary: TheoremStyle,
  /// `[theorems.definition]`
  pub definition: TheoremStyle,
  /// `[theorems.axiom]`
  pub axiom: TheoremStyle,
  /// `[theorems.example]`
  pub example: TheoremStyle,
  /// `[theorems.remark]`
  pub remark: TheoremStyle,
  /// `[theorems.claim]`
  pub claim: TheoremStyle,
  /// `[theorems.proof]`
  pub proof: TheoremStyle,
}

impl Theorems {
  /// 指定したクラスの定義への不変参照を返す（10 種固定のため必ず存在する）。
  #[must_use]
  pub fn get(&self, class: TheoremClass) -> &TheoremStyle {
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

  /// 各クラスにクラス名を添えて走査するイテレータ。
  ///
  /// バリデーションのパスプレフィックス（例 `theorems.theorem`）を構築する用途で使う。
  pub fn iter_with_class(&self) -> impl Iterator<Item = (TheoremClass, &TheoremStyle)> {
    return [
      (TheoremClass::Theorem, &self.theorem),
      (TheoremClass::Lemma, &self.lemma),
      (TheoremClass::Proposition, &self.proposition),
      (TheoremClass::Corollary, &self.corollary),
      (TheoremClass::Definition, &self.definition),
      (TheoremClass::Axiom, &self.axiom),
      (TheoremClass::Example, &self.example),
      (TheoremClass::Remark, &self.remark),
      (TheoremClass::Claim, &self.claim),
      (TheoremClass::Proof, &self.proof),
    ]
    .into_iter();
  }
}

impl Default for Theorems {
  /// 各クラスの [`default_for_class`] を集めた既定値。
  fn default() -> Self { return Self::from(TheoremsTable::default()); }
}

/// 1 つの定理クラスのスタイル定義（クラス別既定 + 差分上書きで解決済み）。
///
/// TOML 上では `[theorems.<class>]` テーブルにマップされる。`format` はカウンタと同形の
/// テンプレート文字列（`"{n}"` / `"{chapter}.{n}"`）で、プレースホルダの実レンダリングは
/// parser 段で行う（本層では非空のみ検証）。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TheoremStyle {
  /// 表示名（例: `"Theorem"`、`"定理"`）。見出し書式の `{display_name}` から参照される
  #[garde(length(chars, min = 1))]
  pub display_name: String,
  /// 共有カウンタ名。同じ名前を指定したクラスは 1 つのカウンタを共有する
  /// （LaTeX の `\newtheorem{lemma}[theorem]{...}` 第 2 引数の明示化）
  #[garde(length(chars, min = 1))]
  pub counter: String,
  /// このクラスのカウンタのリセット先（見出しレベル or なし）
  pub reset_by: TheoremReset,
  /// 番号テンプレート。`{n}` で自身、`{<counter_name>}` で他カウンタの値を埋め込む
  /// （counter の `format` と同形。例: `"{n}"`、`"{chapter}.{n}"`）
  #[garde(length(chars, min = 1))]
  pub format: String,
  /// 採番しない（`proof` 等）。`true` のとき番号は付かない
  pub unnumbered: bool,
  /// QED マーク（`proof` 末尾に配置する記号）。`None` のときマークなし
  #[garde(inner(length(chars, min = 1)))]
  pub qed_mark: Option<String>,
  /// 見出し書式・本文/見出しフォント・上下マージン
  #[garde(dive)]
  pub style: TheoremPresentation,
}

impl Default for TheoremStyle {
  /// 汎用デフォルト（定理クラス相当: `"Theorem"` / counter `"theorem"` / 本文斜体）。
  ///
  /// クラスごとの差分は [`default_for_class`] が与える。デシリアライズ時の `#[serde(default)]`
  /// で部分指定をサポートするために必要。
  fn default() -> Self {
    return Self {
      display_name: "Theorem".to_string(),
      counter: "theorem".to_string(),
      reset_by: TheoremReset::None,
      format: "{n}".to_string(),
      unnumbered: false,
      qed_mark: None,
      style: TheoremPresentation::default(),
    };
  }
}

/// 定理カウンタのリセット先。`reset_by` フィールドで指定する。
///
/// 見出しレベル（part / chapter / section / subsection）でリセットするか、リセットしない
/// （[`TheoremReset::None`]＝文書全体で連番）かを選ぶ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TheoremReset {
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

/// 定理ブロックの見た目（見出し書式・フォント・マージン）。
///
/// TOML 上では `[theorems.<class>.style]` テーブルにマップされる。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TheoremPresentation {
  /// サブタイトルなしの見出し書式。`{display_name}` と `{number}` を含められる
  #[garde(length(chars, min = 1))]
  pub heading_format: String,
  /// サブタイトルありの見出し書式。`{display_name}` / `{number}` / `{title}` を含められる
  #[garde(length(chars, min = 1))]
  pub heading_with_title: String,
  /// 本文のフォント種別（定理は斜体、証明・定義系はローマン）
  pub font_kind: FontKind,
  /// 見出しのフォント種別（既定は太字セリフ）
  pub heading_font_kind: FontKind,
  /// 定理ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 定理ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for TheoremPresentation {
  /// 既定値: `"{display_name} {number}"` / 本文斜体・見出し太字 / 上下 12pt。
  fn default() -> Self {
    return Self {
      heading_format: "{display_name} {number}".to_string(),
      heading_with_title: "{display_name} {number} ({title})".to_string(),
      font_kind: FontKind::SerifItalic,
      heading_font_kind: FontKind::SerifBold,
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
    };
  }
}

/// 指定クラスの [`TheoremStyle`] デフォルトを返す。
///
/// 汎用既定（[`TheoremStyle::default`]＝定理クラス相当）に対し、クラスごとの差分のみ上書きする。
/// theorem / lemma / proposition / corollary / claim は counter `"theorem"` を共有して本文斜体、
/// definition / example / remark は各自のカウンタで本文ローマン、proof は採番なし＋QED マーク。
#[must_use]
pub fn default_for_class(class: TheoremClass) -> TheoremStyle {
  let mut style = TheoremStyle::default();
  match class {
    TheoremClass::Theorem => {
      style.display_name = "Theorem".to_string();
    },
    TheoremClass::Lemma => {
      style.display_name = "Lemma".to_string();
    },
    TheoremClass::Proposition => {
      style.display_name = "Proposition".to_string();
    },
    TheoremClass::Corollary => {
      style.display_name = "Corollary".to_string();
    },
    TheoremClass::Claim => {
      style.display_name = "Claim".to_string();
    },
    TheoremClass::Axiom => {
      style.display_name = "Axiom".to_string();
      style.counter = "axiom".to_string();
    },
    TheoremClass::Definition => {
      style.display_name = "Definition".to_string();
      style.counter = "definition".to_string();
      style.style.font_kind = FontKind::Serif;
    },
    TheoremClass::Example => {
      style.display_name = "Example".to_string();
      style.counter = "example".to_string();
      style.style.font_kind = FontKind::Serif;
    },
    TheoremClass::Remark => {
      style.display_name = "Remark".to_string();
      style.counter = "remark".to_string();
      style.style.font_kind = FontKind::Serif;
    },
    TheoremClass::Proof => {
      style.display_name = "Proof".to_string();
      style.counter = "proof".to_string();
      style.unnumbered = true;
      style.qed_mark = Some("□".to_string());
      style.style.font_kind = FontKind::Serif;
      style.style.heading_format = "{display_name}".to_string();
      style.style.heading_with_title = "{display_name} ({title})".to_string();
    },
  }
  return style;
}

/// `[theorems]` テーブル全体の TOML スキーマ。
///
/// 各クラスキー（`theorem` / `lemma` / …）には [`TheoremStyleOverride`] が入る。
/// `#[serde(deny_unknown_fields)]` により、未知のクラス名は TOML パース時に拒否される。
///
/// 実行時は [`From`] 実装が各 override を [`default_for_class`] に適用し、[`Theorems`] を構築する。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct TheoremsTable {
  theorem: TheoremStyleOverride,
  lemma: TheoremStyleOverride,
  proposition: TheoremStyleOverride,
  corollary: TheoremStyleOverride,
  definition: TheoremStyleOverride,
  axiom: TheoremStyleOverride,
  example: TheoremStyleOverride,
  remark: TheoremStyleOverride,
  claim: TheoremStyleOverride,
  proof: TheoremStyleOverride,
}

impl From<TheoremsTable> for Theorems {
  fn from(table: TheoremsTable) -> Self {
    let build = |class: TheoremClass, over: TheoremStyleOverride| -> TheoremStyle {
      let mut style = default_for_class(class);
      over.apply(&mut style);
      return style;
    };
    return Self {
      theorem: build(TheoremClass::Theorem, table.theorem),
      lemma: build(TheoremClass::Lemma, table.lemma),
      proposition: build(TheoremClass::Proposition, table.proposition),
      corollary: build(TheoremClass::Corollary, table.corollary),
      definition: build(TheoremClass::Definition, table.definition),
      axiom: build(TheoremClass::Axiom, table.axiom),
      example: build(TheoremClass::Example, table.example),
      remark: build(TheoremClass::Remark, table.remark),
      claim: build(TheoremClass::Claim, table.claim),
      proof: build(TheoremClass::Proof, table.proof),
    };
  }
}

/// [`TheoremStyle`] の各フィールドを `Option<_>` で覆った差分指定型。
///
/// `[theorems.<class>]` のクラス別差分を受ける TOML スキーマ。未指定（`None`）フィールドは
/// [`default_for_class`] の既定値を残す。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct TheoremStyleOverride {
  /// 表示名
  pub display_name: Option<String>,
  /// 共有カウンタ名
  pub counter: Option<String>,
  /// カウンタのリセット先
  pub reset_by: Option<TheoremReset>,
  /// 番号テンプレート
  pub format: Option<String>,
  /// 採番しないか
  pub unnumbered: Option<bool>,
  /// QED マーク（TOML からは設定のみ可。`None` への解除は非対応）
  pub qed_mark: Option<String>,
  /// 見た目（ネストした差分）
  pub style: TheoremPresentationOverride,
}

impl TheoremStyleOverride {
  /// 自身の `Some` 値で `target` のフィールドを上書きする。
  fn apply(&self, target: &mut TheoremStyle) {
    if let Some(display_name) = &self.display_name {
      target.display_name.clone_from(display_name);
    }
    if let Some(counter) = &self.counter {
      target.counter.clone_from(counter);
    }
    if let Some(reset_by) = self.reset_by {
      target.reset_by = reset_by;
    }
    if let Some(format) = &self.format {
      target.format.clone_from(format);
    }
    if let Some(unnumbered) = self.unnumbered {
      target.unnumbered = unnumbered;
    }
    if let Some(qed_mark) = &self.qed_mark {
      target.qed_mark = Some(qed_mark.clone());
    }
    self.style.apply(&mut target.style);
  }
}

/// [`TheoremPresentation`] の各フィールドを `Option<_>` で覆った差分指定型。
///
/// `[theorems.<class>.style]` のネストした差分を受ける TOML スキーマ。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct TheoremPresentationOverride {
  /// サブタイトルなしの見出し書式
  pub heading_format: Option<String>,
  /// サブタイトルありの見出し書式
  pub heading_with_title: Option<String>,
  /// 本文のフォント種別
  pub font_kind: Option<FontKind>,
  /// 見出しのフォント種別
  pub heading_font_kind: Option<FontKind>,
  /// 上余白
  pub top_margin: Option<Length>,
  /// 下余白
  pub bottom_margin: Option<Length>,
}

impl TheoremPresentationOverride {
  /// 自身の `Some` 値で `target` のフィールドを上書きする。
  fn apply(&self, target: &mut TheoremPresentation) {
    if let Some(heading_format) = &self.heading_format {
      target.heading_format.clone_from(heading_format);
    }
    if let Some(heading_with_title) = &self.heading_with_title {
      target.heading_with_title.clone_from(heading_with_title);
    }
    if let Some(font_kind) = self.font_kind {
      target.font_kind = font_kind;
    }
    if let Some(heading_font_kind) = self.heading_font_kind {
      target.heading_font_kind = heading_font_kind;
    }
    if let Some(top_margin) = self.top_margin {
      target.top_margin = top_margin;
    }
    if let Some(bottom_margin) = self.bottom_margin {
      target.bottom_margin = bottom_margin;
    }
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::FontKind;

  use super::{TheoremClass, TheoremReset, TheoremStyle, Theorems, default_for_class};

  /// `Theorems` を TOML から `[theorems.<class>]` 配下に書く形でテストするための薄いラッパ。
  /// 本番では `Style.theorems` が同形でこの型を保持する。
  #[derive(Debug, serde::Deserialize)]
  struct TheoremsWrapper {
    theorems: Theorems,
  }

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(TheoremStyle::default().validate().is_ok());
  }

  #[test]
  fn all_default_classes_pass_validation() {
    // Arrange / Act / Assert
    for class in TheoremClass::ALL {
      assert!(default_for_class(class).validate().is_ok(), "{} should validate", class.as_str());
    }
  }

  #[test]
  fn validate_rejects_empty_display_name() {
    // Arrange
    let style = TheoremStyle {
      display_name: String::new(),
      ..TheoremStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_format() {
    // Arrange
    let style = TheoremStyle {
      format: String::new(),
      ..TheoremStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_qed_mark() {
    // Arrange: Some("") は inner 検証で弾かれる
    let style = TheoremStyle {
      qed_mark: Some(String::new()),
      ..TheoremStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    // Arrange
    let mut style = TheoremStyle::default();
    style.style.top_margin = types::length::Length::pt(-0.1);

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn default_proof_is_unnumbered_with_qed_mark() {
    // Arrange / Act
    let proof = default_for_class(TheoremClass::Proof);

    // Assert
    assert!(proof.unnumbered);
    assert_eq!(proof.qed_mark.as_deref(), Some("□"));
    assert_eq!(proof.style.font_kind, FontKind::Serif);
    assert_eq!(proof.style.heading_format, "{display_name}");
  }

  #[test]
  fn default_theorem_like_classes_share_counter_and_italic_body() {
    // Arrange / Act / Assert: theorem / lemma / proposition / corollary / claim は counter 共有・斜体
    for class in [
      TheoremClass::Theorem,
      TheoremClass::Lemma,
      TheoremClass::Proposition,
      TheoremClass::Corollary,
      TheoremClass::Claim,
    ] {
      let style = default_for_class(class);
      assert_eq!(style.counter, "theorem", "{} should share theorem counter", class.as_str());
      assert_eq!(style.style.font_kind, FontKind::SerifItalic);
      assert!(!style.unnumbered);
    }
  }

  #[test]
  fn default_remark_style_uses_roman_body_and_own_counter() {
    // Arrange / Act
    let remark = default_for_class(TheoremClass::Remark);

    // Assert
    assert_eq!(remark.counter, "remark");
    assert_eq!(remark.style.font_kind, FontKind::Serif);
  }

  #[test]
  fn iter_with_class_yields_all_ten_classes_in_order() {
    // Arrange
    let theorems = Theorems::default();

    // Act
    let classes: Vec<TheoremClass> = theorems.iter_with_class().map(|(class, _)| class).collect();

    // Assert
    assert_eq!(classes, TheoremClass::ALL.to_vec());
  }

  #[test]
  fn get_returns_matching_field() {
    // Arrange
    let theorems = Theorems::default();

    // Act / Assert
    assert!(std::ptr::eq(theorems.get(TheoremClass::Lemma), &raw const theorems.lemma));
    assert!(std::ptr::eq(theorems.get(TheoremClass::Proof), &raw const theorems.proof));
  }

  #[test]
  fn partial_override_keeps_other_class_defaults() {
    // Arrange: lemma の display_name だけ上書き
    let toml = "
[theorems.lemma]
display_name = \"補題\"
";

    // Act
    let wrapper: TheoremsWrapper = toml::from_str(toml).unwrap();
    let theorems = wrapper.theorems;

    // Assert: lemma は display_name のみ変わり、counter 等はクラス既定を維持
    assert_eq!(theorems.lemma.display_name, "補題");
    assert_eq!(theorems.lemma.counter, "theorem");
    assert_eq!(theorems.lemma.style.font_kind, FontKind::SerifItalic);
    // 他クラスは default_for_class のまま
    assert_eq!(theorems.theorem.display_name, "Theorem");
    assert!(theorems.proof.unnumbered);
  }

  #[test]
  fn partial_override_nested_style_keeps_other_style_fields() {
    // Arrange: theorem.style の font_kind だけ上書き
    let toml = "
[theorems.theorem.style]
font_kind = \"sans_serif_bold\"
";

    // Act
    let wrapper: TheoremsWrapper = toml::from_str(toml).unwrap();
    let theorem = wrapper.theorems.theorem;

    // Assert: font_kind は上書き、heading_format 等はクラス既定を維持
    assert_eq!(theorem.style.font_kind, FontKind::SansSerifBold);
    assert_eq!(theorem.style.heading_format, "{display_name} {number}");
    assert!((theorem.style.top_margin.to_pt() - 12.0).abs() < f32::EPSILON);
  }

  #[test]
  fn override_reset_by_is_applied() {
    // Arrange
    let toml = "
[theorems.theorem]
reset_by = \"section\"
";

    // Act
    let wrapper: TheoremsWrapper = toml::from_str(toml).unwrap();

    // Assert
    assert_eq!(wrapper.theorems.theorem.reset_by, TheoremReset::Section);
  }

  #[test]
  fn rejects_unknown_class_key() {
    // Arrange: `conjecture` は固定 10 種に含まれない
    let toml = "
[theorems.conjecture]
display_name = \"Conjecture\"
";

    // Act
    let result: Result<TheoremsWrapper, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知のクラス名は TOML パース時に拒否されるべき: {result:?}");
  }

  #[test]
  fn rejects_unknown_field_key() {
    // Arrange: `[theorems.theorem]` 内の typo
    let toml = "
[theorems.theorem]
displ_name = \"Theorem\"
";

    // Act
    let result: Result<TheoremsWrapper, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知のフィールド名は拒否されるべき: {result:?}");
  }

  #[test]
  fn rejects_unknown_nested_style_key() {
    // Arrange: `[theorems.theorem.style]` 内の typo
    let toml = "
[theorems.theorem.style]
font_knd = \"serif\"
";

    // Act
    let result: Result<TheoremsWrapper, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "ネストした未知のフィールド名は拒否されるべき: {result:?}");
  }
}
