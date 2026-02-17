//! グリフマッピング管理モジュール
//!
//! フォント内のグリフ ID（GID）と PDF で使用される
//! キャラクタ ID（CID）の対応関係を管理します。
//!
//! ## 主要概念
//!
//! ### グリフ ID（GID）
//!
//! フォント内でグリフを識別するための ID。OpenType フォントでは
//! GLYF（TrueType）または CFF（PostScript）テーブルで定義されます。
//! GID 0 は常に`.notdef`グリフ（未定義文字用）です。
//!
//! ### キャラクタ ID（CID）
//!
//! PDF の `CIDFont` で使用される ID。サブセット化されたフォントでは
//! GID と異なる値となります。CID 0 は常に`.notdef`グリフをマッピングします。
//!
//! ## データ構造
//!
//! - [`GlyphMapping`] - 単一フォント種別のグリフ/CID マッピング
//! - [`GlyphMappings`] - 19 種類すべてのグリフマッピング一括管理
//!
//! ## マッピング情報の内容
//!
//! 各 `GlyphMapping` は以下の情報を保持：
//!
//! | フィールド | 説明 |
//! |----------|------|
//! | `old_to_cid` | 元の GID から CID への対応（疎配列） |
//! | `cid_to_gid` | CID から新しい GID への対応（密配列） |
//! | `widths` | 各 CID のグリフ幅（em の 1/upem 単位） |
//! | `chars` | 各 CID が表現する文字のリスト |
//!
//! ## ライフサイクル
//!
//! 1. **初期化** - `.notdef` グリフ（GID0 → CID0）のみを登録した状態で作成
//! 2. **グリフ登録** - テキストシェーピングで使用されるグリフを `.register()` で逐次登録
//! 3. **サブセット化** - `subset` モジュールで登録されたグリフのみを抽出
//! 4. **PDF 生成** - `pdf_gen` モジュールで CID → 文字のマッピングを使用
//!
//! ## 19 フォント対応
//!
//! 各フォント種別（Serif、SansSerif、Monospace、Math、日本語フォント）は
//! 独立したグリフマッピングを持ち、並列処理で効率的に管理されます。

use types::FontType;

use crate::font_info::{FontInfo, FontInfos};

/// すべてのフォント種別に対応するグリフマッピング情報
///
/// Serif、Sans Serif、Monospace、Math、日本語フォントなど 19 種類のフォント種別ごとに
/// グリフマッピング情報を保持します。各フォント種別のグリフ ID と
/// CID（Character ID）のマッピング、グリフ幅、文字情報などを管理します。
#[derive(Debug)]
pub struct GlyphMappings {
  pub serif_font: GlyphMapping,
  pub serif_bold_font: GlyphMapping,
  pub serif_italic_font: GlyphMapping,
  pub serif_bold_italic_font: GlyphMapping,
  pub sans_serif_font: GlyphMapping,
  pub sans_serif_bold_font: GlyphMapping,
  pub sans_serif_italic_font: GlyphMapping,
  pub sans_serif_bold_italic_font: GlyphMapping,
  pub monospace_font: GlyphMapping,
  pub monospace_bold_font: GlyphMapping,
  pub monospace_italic_font: GlyphMapping,
  pub monospace_bold_italic_font: GlyphMapping,
  pub math_font: GlyphMapping,
  pub japanese_serif_font: GlyphMapping,
  pub japanese_serif_bold_font: GlyphMapping,
  pub japanese_sans_serif_font: GlyphMapping,
  pub japanese_sans_serif_bold_font: GlyphMapping,
  pub japanese_monospace_font: GlyphMapping,
  pub japanese_monospace_bold_font: GlyphMapping,
}

impl GlyphMappings {
  /// フォント情報からグリフマッピングを初期化します
  ///
  /// `FontType::ALL` に列挙されたすべてのフォント種別に対して
  /// `GlyphMapping` を生成し、ひとまとめにします。
  /// 各グリフマッピングは、まず `.notdef` グリフ（GID 0）が CID 0 として登録された状態から始まります。
  ///
  /// # Arguments
  ///
  /// * `font_infos` - 各フォント種別のメタデータ情報
  ///
  /// # Panics
  ///
  /// `FontType::ALL` に含まれるフォント種別数が予期した 19 個と異なる場合にパニック。
  /// これは実装の不整合を示すプログラムエラーです。
  #[must_use]
  pub fn new(font_infos: &FontInfos) -> Self {
    let mut glyph_mappings = FontType::ALL.iter().map(|font_type| {
      let font_info = font_infos.get(*font_type);
      GlyphMapping::new(font_info)
    });

    #[allow(clippy::expect_used)]
    Self {
      serif_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      serif_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      serif_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      serif_bold_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      sans_serif_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      sans_serif_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      sans_serif_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      sans_serif_bold_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      monospace_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      monospace_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      monospace_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      monospace_bold_italic_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      math_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_serif_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_serif_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_sans_serif_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_sans_serif_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_monospace_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
      japanese_monospace_bold_font: glyph_mappings.next().expect("GlyphMappings count mismatch"),
    }
  }

  /// 指定されたフォント種別に対応するグリフマッピングの可変参照を取得します
  ///
  /// グリフマッピングを修正する際に使用します（例：新しいグリフを登録する）。
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別の `GlyphMapping` への可変参照
  #[must_use]
  pub fn get_mut(&mut self, font_type: FontType) -> &mut GlyphMapping {
    match font_type {
      FontType::Serif => &mut self.serif_font,
      FontType::SerifBold => &mut self.serif_bold_font,
      FontType::SerifItalic => &mut self.serif_italic_font,
      FontType::SerifBoldItalic => &mut self.serif_bold_italic_font,
      FontType::SansSerif => &mut self.sans_serif_font,
      FontType::SansSerifBold => &mut self.sans_serif_bold_font,
      FontType::SansSerifItalic => &mut self.sans_serif_italic_font,
      FontType::SansSerifBoldItalic => &mut self.sans_serif_bold_italic_font,
      FontType::Monospace => &mut self.monospace_font,
      FontType::MonospaceBold => &mut self.monospace_bold_font,
      FontType::MonospaceItalic => &mut self.monospace_italic_font,
      FontType::MonospaceBoldItalic => &mut self.monospace_bold_italic_font,
      FontType::Math => &mut self.math_font,
      FontType::JapaneseSerif => &mut self.japanese_serif_font,
      FontType::JapaneseSerifBold => &mut self.japanese_serif_bold_font,
      FontType::JapaneseSansSerif => &mut self.japanese_sans_serif_font,
      FontType::JapaneseSansSerifBold => &mut self.japanese_sans_serif_bold_font,
      FontType::JapaneseMonospace => &mut self.japanese_monospace_font,
      FontType::JapaneseMonospaceBold => &mut self.japanese_monospace_bold_font,
    }
  }

  /// 指定されたフォント種別に対応するグリフマッピングを取得します
  ///
  /// グリフマッピング情報を参照する際に使用します。
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別の `GlyphMapping` への不変参照
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &GlyphMapping {
    match font_type {
      FontType::Serif => &self.serif_font,
      FontType::SerifBold => &self.serif_bold_font,
      FontType::SerifItalic => &self.serif_italic_font,
      FontType::SerifBoldItalic => &self.serif_bold_italic_font,
      FontType::SansSerif => &self.sans_serif_font,
      FontType::SansSerifBold => &self.sans_serif_bold_font,
      FontType::SansSerifItalic => &self.sans_serif_italic_font,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic_font,
      FontType::Monospace => &self.monospace_font,
      FontType::MonospaceBold => &self.monospace_bold_font,
      FontType::MonospaceItalic => &self.monospace_italic_font,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic_font,
      FontType::Math => &self.math_font,
      FontType::JapaneseSerif => &self.japanese_serif_font,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold_font,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif_font,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold_font,
      FontType::JapaneseMonospace => &self.japanese_monospace_font,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold_font,
    }
  }
}

/// 単一フォントのグリフマッピング情報
///
/// グリフ ID、CID、グリフ幅、および文字情報の対応関係を管理します。
/// フォント内のグリフをサブセット化して PDF に埋め込む際に、
/// 元のグリフ ID から CID への変換情報として使用されます。
#[derive(Debug)]
pub struct GlyphMapping {
  /// グリフ ID（旧 GID）から CID への対応表
  /// インデックスは元のグリフ ID、値は割り当てられた CID（または None）
  pub old_to_cid: Vec<Option<u16>>,
  /// CID から新しいグリフ ID への対応表
  /// インデックスは CID、値は新しいグリフ ID（サブセット内の GID）
  pub cid_to_gid: Vec<u16>,
  /// 各 CID に対応するグリフの幅
  /// インデックスは CID、値はグリフの水平引き出し幅（単位は em の 1/upem）
  pub widths: Vec<u16>,
  /// 各 CID にマッピングされた文字のリスト
  /// インデックスは CID、値はその CID が表現する文字の集合
  pub chars: Vec<Vec<char>>,
}

impl GlyphMapping {
  /// フォント情報からグリフマッピングを初期化します
  ///
  /// `.notdef` グリフ（GID 0）を CID 0 として事前登録した状態でグリフマッピングを初期化します。
  /// これは PDF 仕様で必須です。
  ///
  /// # Arguments
  ///
  /// * `font_info` - フォントのメタデータ情報
  ///
  /// # Returns
  ///
  /// 初期化されたグリフマッピング（`.notdef` グリフのみを含む）
  #[must_use]
  pub fn new(font_info: &FontInfo) -> Self {
    let mut old_to_cid = vec![None; 65_536];
    old_to_cid[0] = Some(0); // GID 0（.notdef グリフ）を CID 0 にマッピング
    let cid_to_gid = vec![0]; // CID 0 に GID 0 をマッピング
    let widths = vec![font_info.upem]; // .notdef グリフの幅をフォントの UPEM に設定
    let chars = vec![vec!['\u{FFFD}']]; // .notdef グリフに Unicode 置換文字を割り当て
    Self {
      old_to_cid,
      cid_to_gid,
      widths,
      chars,
    }
  }

  /// グリフを登録し、割り当てられた CID を返します
  ///
  /// 指定されたグリフ ID が既に登録されている場合は、割り当てられた CID をそのまま返します。
  /// 未登録の場合は新しい CID を割り当て、グリフ情報を記録します。
  ///
  /// # Arguments
  ///
  /// * `glyph_id` - 登録するグリフの ID
  /// * `width` - グリフの水平引き出し幅
  /// * `chars` - グリフが表現する文字のリスト
  ///
  /// # Returns
  ///
  /// 割り当てられた CID
  pub fn register(&mut self, glyph_id: u16, width: u16, chars: Vec<char>) -> u16 {
    if let Some(cid) = self.old_to_cid[glyph_id as usize] {
      return cid;
    }

    let cid = self.old_to_cid.len() as u16;

    self.old_to_cid[glyph_id as usize] = Some(cid);
    self.cid_to_gid.push(glyph_id);
    self.chars.push(chars);
    self.widths.push(width);

    return cid;
  }
}
