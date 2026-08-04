//! 数式要素。

use crate::model::Span;

/// 数式ノード
///
/// インライン数式（`$...$`）およびディスプレイ数式内の構造を表現します。
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
  /// テキスト / 記号（変数名、数字、演算子等）
  Text(String),
  /// 数式記号（`\alpha`, `+`, `=` 等）
  Symbol(char),
  /// 中括弧グループ（`{...}`）
  Group(Vec<MathNode>),
  /// 上付き（`x^2`）
  Superscript(Box<MathNode>),
  /// 下付き（`x_i`）
  Subscript(Box<MathNode>),
  /// 分数（`\frac{numer}{denom}`）
  Frac {
    /// 分子
    numer: Box<MathNode>,
    /// 分母
    denom: Box<MathNode>,
  },
  /// 平方根（`\sqrt[n]{x}`）
  Sqrt {
    /// 根のインデックス（`\sqrt[3]{x}` の `3`、省略時 `None`）
    index: Option<Box<MathNode>>,
    /// 被根号
    radicand: Box<MathNode>,
  },
  /// 数式スタイル指定（`\mathbold` `\mathitalic` 等）
  ///
  /// body 内の ASCII 英字・数字・Greek を、ローワリング層で
  /// Unicode Mathematical Alphanumeric Symbols のコードポイントに変換する。
  /// ネスト時は内側の `style` が完全に上書きする。
  Styled {
    /// 適用するスタイル
    style: MathStyle,
    /// 本体
    body: Vec<MathNode>,
  },
}

/// 数式中のフォントスタイル指定
///
/// `\mathbold{...}` 等のコマンドで指定され、ローワリング層で
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）の
/// 該当コードポイントへ ASCII 英字・数字・Greek 文字を変換する。
///
/// `FontKind::Math` のままで字形を切り替える設計のため、
/// 数式フォントが対応するグリフを持っている前提で動作する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
  /// `\mathserif` — セリフ立体（素通し）
  Serif,
  /// `\mathitalic` — セリフイタリック
  Italic,
  /// `\mathbold` — セリフ太字
  Bold,
  /// `\mathbolditalic` — セリフ太字イタリック
  BoldItalic,
  /// `\mathsans` — サンセリフ立体
  Sans,
  /// `\mathsansitalic` — サンセリフイタリック
  SansItalic,
  /// `\mathsansbold` — サンセリフ太字
  SansBold,
  /// `\mathsansbolditalic` — サンセリフ太字イタリック
  SansBoldItalic,
  /// `\mathmono` — 等幅
  Mono,
  /// `\mathdoublestruck` — 黒板太字（double-struck, ℝ ℂ ℕ 等）
  DoubleStruck,
  /// `\mathscript` — スクリプト（roundhand, 花文字）
  Script,
  /// `\mathcalligraphic` — カリグラフィー（chancery, 花文字の筆記体）
  ///
  /// スクリプトと同一の基底コードポイントに異体字セレクタ VS1（U+FE00）を付与して
  /// chancery 字形を要求する。Unicode の数式異体字シーケンスに対応した数式フォントでのみ
  /// chancery 字形が選ばれ、非対応フォントでは VS1 が無視されてスクリプト字形に
  /// フォールバックする（フォント非依存対応は OpenType `ss01` を使う別 issue で行う）。
  Calligraphic,
  /// `\mathfraktur` — フラクトゥール（ドイツ文字, ℌ ℑ ℜ 等）
  Fraktur,
  /// `\mathscriptbold` — 太字スクリプト（bold roundhand, 花文字の太字）
  ScriptBold,
  /// `\mathfrakturbold` — 太字フラクトゥール（bold ドイツ文字）
  FrakturBold,
}

/// ディスプレイ数式環境の 1 行
///
/// `cells` は `&` で分割された列（`equation` / `gather` は 1 列、`cases` は 2 列、
/// `align` / `matrix` は複数列）。各列は数式ノード列。`numbered` はこの行が採番対象かどうか
/// （実際の発番・書式化は `lowering` 層が担う）。`label` は `\ref` 解決用の
/// 行ラベル（`equation` の `[label=...]`、`align` / `gather` の行末マーカー `\label{...}`）。
#[derive(Debug, Clone, PartialEq)]
pub struct MathRow {
  /// 列（`&` 区切り）。各列は数式ノード列
  pub cells: Vec<Vec<MathNode>>,
  /// 採番対象かどうか（`false` は非採番）
  pub numbered: bool,
  /// `\ref` 解決用ラベル（`None` は参照対象外）
  pub label: Option<String>,
  /// 行末マーカー `\label{...}` のソース位置。`None` の場合は環境（`DocNode::MathBlock`）の
  /// `span` をフォールバックとして使う（`equation` の `[label=...]` など、行に固有の位置がない場合）
  pub label_span: Option<Span>,
}

impl MathStyle {
  /// コマンド名から対応する `MathStyle` を解決する
  ///
  /// 数式モード内で `evaluate_math_command` から呼び出される。
  /// 未対応の名前は `None` を返す。
  #[must_use]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "mathserif" => Some(MathStyle::Serif),
      "mathitalic" => Some(MathStyle::Italic),
      "mathbold" => Some(MathStyle::Bold),
      "mathbolditalic" => Some(MathStyle::BoldItalic),
      "mathsans" => Some(MathStyle::Sans),
      "mathsansitalic" => Some(MathStyle::SansItalic),
      "mathsansbold" => Some(MathStyle::SansBold),
      "mathsansbolditalic" => Some(MathStyle::SansBoldItalic),
      "mathmono" => Some(MathStyle::Mono),
      "mathdoublestruck" => Some(MathStyle::DoubleStruck),
      "mathscript" => Some(MathStyle::Script),
      "mathcalligraphic" => Some(MathStyle::Calligraphic),
      "mathfraktur" => Some(MathStyle::Fraktur),
      "mathscriptbold" => Some(MathStyle::ScriptBold),
      "mathfrakturbold" => Some(MathStyle::FrakturBold),
      _ => None,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn math_node_equality() {
    assert_eq!(MathNode::Text("x".to_string()), MathNode::Text("x".to_string()));
    assert_eq!(MathNode::Symbol('+'), MathNode::Symbol('+'));
    assert_ne!(MathNode::Text("x".to_string()), MathNode::Symbol('x'));
  }

  #[test]
  fn math_node_frac() {
    let node = MathNode::Frac {
      numer: Box::new(MathNode::Text("a".to_string())),
      denom: Box::new(MathNode::Text("b".to_string())),
    };
    match &node {
      MathNode::Frac { numer, denom } => {
        assert_eq!(**numer, MathNode::Text("a".to_string()));
        assert_eq!(**denom, MathNode::Text("b".to_string()));
      },
      _ => panic!("Frac が期待されます"),
    }
  }

  #[test]
  fn math_node_sqrt() {
    let node = MathNode::Sqrt {
      index: Some(Box::new(MathNode::Text("3".to_string()))),
      radicand: Box::new(MathNode::Text("x".to_string())),
    };
    match &node {
      MathNode::Sqrt { index, radicand } => {
        assert!(index.is_some());
        assert_eq!(**radicand, MathNode::Text("x".to_string()));
      },
      _ => panic!("Sqrt が期待されます"),
    }
  }

  #[test]
  fn math_node_superscript_subscript() {
    let sup = MathNode::Superscript(Box::new(MathNode::Text("2".to_string())));
    let sub = MathNode::Subscript(Box::new(MathNode::Text("i".to_string())));
    assert_eq!(sup, MathNode::Superscript(Box::new(MathNode::Text("2".to_string()))));
    assert_eq!(sub, MathNode::Subscript(Box::new(MathNode::Text("i".to_string()))));
  }

  #[test]
  fn math_node_group() {
    let node = MathNode::Group(vec![
      MathNode::Text("x".to_string()),
      MathNode::Symbol('+'),
      MathNode::Text("1".to_string()),
    ]);
    match &node {
      MathNode::Group(children) => {
        assert_eq!(children.len(), 3);
      },
      _ => panic!("Group が期待されます"),
    }
  }

  #[test]
  fn math_style_from_command_name_resolves_all_styles() {
    // Arrange & Act & Assert — 15 個のスタイルコマンドが正しく解決される
    assert_eq!(MathStyle::from_command_name("mathserif"), Some(MathStyle::Serif));
    assert_eq!(MathStyle::from_command_name("mathitalic"), Some(MathStyle::Italic));
    assert_eq!(MathStyle::from_command_name("mathbold"), Some(MathStyle::Bold));
    assert_eq!(MathStyle::from_command_name("mathbolditalic"), Some(MathStyle::BoldItalic));
    assert_eq!(MathStyle::from_command_name("mathsans"), Some(MathStyle::Sans));
    assert_eq!(MathStyle::from_command_name("mathsansitalic"), Some(MathStyle::SansItalic));
    assert_eq!(MathStyle::from_command_name("mathsansbold"), Some(MathStyle::SansBold));
    assert_eq!(MathStyle::from_command_name("mathsansbolditalic"), Some(MathStyle::SansBoldItalic));
    assert_eq!(MathStyle::from_command_name("mathmono"), Some(MathStyle::Mono));
    assert_eq!(MathStyle::from_command_name("mathdoublestruck"), Some(MathStyle::DoubleStruck));
    assert_eq!(MathStyle::from_command_name("mathscript"), Some(MathStyle::Script));
    assert_eq!(MathStyle::from_command_name("mathcalligraphic"), Some(MathStyle::Calligraphic));
    assert_eq!(MathStyle::from_command_name("mathfraktur"), Some(MathStyle::Fraktur));
    assert_eq!(MathStyle::from_command_name("mathscriptbold"), Some(MathStyle::ScriptBold));
    assert_eq!(MathStyle::from_command_name("mathfrakturbold"), Some(MathStyle::FrakturBold));
  }

  #[test]
  fn math_style_from_command_name_rejects_unknown() {
    // Arrange & Act & Assert — 未知名は None
    assert_eq!(MathStyle::from_command_name("mathrm"), None);
    assert_eq!(MathStyle::from_command_name("mathbf"), None);
    assert_eq!(MathStyle::from_command_name("foo"), None);
    assert_eq!(MathStyle::from_command_name(""), None);
  }

  #[test]
  fn math_node_styled() {
    // Arrange — Styled バリアントの構築と分解
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Text("x".to_string())],
    };

    // Act & Assert
    match &node {
      MathNode::Styled { style, body } => {
        assert_eq!(*style, MathStyle::Bold);
        assert_eq!(body.len(), 1);
      },
      _ => panic!("Styled が期待されます"),
    }
  }
}
