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

use types::{FontMap, FontType};

use crate::font_info::{FontInfo, FontInfos};

/// すべてのフォント種別に対応するグリフマッピング情報
///
/// Serif、Sans Serif、Monospace、Math、日本語フォントなど 19 種類のフォント種別ごとに
/// グリフマッピング情報を保持します。各フォント種別のグリフ ID と
/// CID（Character ID）のマッピング、グリフ幅、文字情報などを管理します。
pub type GlyphMappings = FontMap<GlyphMapping>;

/// `GlyphMappings` のコンストラクタを提供するトレイト
pub trait GlyphMappingsExt {
  /// フォント情報からグリフマッピングを初期化します
  ///
  /// `FontType::ALL` に列挙されたすべてのフォント種別に対して
  /// `GlyphMapping` を生成し、ひとまとめにします。
  /// 各グリフマッピングは、まず `.notdef` グリフ（GID 0）が CID 0 として登録された状態から始まります。
  ///
  /// # Arguments
  ///
  /// * `font_infos` - 各フォント種別のメタデータ情報
  fn new(font_infos: &FontInfos) -> Self;
}

impl GlyphMappingsExt for GlyphMappings {
  fn new(font_infos: &FontInfos) -> Self {
    let glyph_mappings: Vec<GlyphMapping> = FontType::ALL
      .iter()
      .map(|font_type| {
        let font_info = font_infos.get(*font_type);
        GlyphMapping::new(font_info)
      })
      .collect();
    return GlyphMappings::from_all(glyph_mappings);
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

    let cid = self.cid_to_gid.len() as u16;

    self.old_to_cid[glyph_id as usize] = Some(cid);
    self.cid_to_gid.push(glyph_id);
    self.chars.push(chars);
    self.widths.push(width);

    return cid;
  }
}
