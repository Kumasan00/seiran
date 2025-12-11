//! 共通型定義モジュール
//!
//! このモジュールは、プロジェクト全体で使用される
//! グリフマッピング、アドバンス幅リスト、CID-GIDマッピング、
//! ToUnicode CMapなどの共通型を定義します。

use std::collections::HashMap;

use indexmap::IndexSet;
use pdf_writer::types::UnicodeCmap;

/// .notdefグリフのGID
const NOTDEF_GID: u16 = 0;

/// グリフとCIDのマッピング情報を管理する構造体
///
/// PDF生成に必要なグリフID(GID)と文字ID(CID)の対応、
/// 使用されるグリフの集合、アドバンス幅情報、Unicodeマッピングを管理します。
/// 各フォントごとに1つのインスタンスが作成されます。
pub struct GlyphMapping {
  /// GIDからCIDへのマッピング
  pub gid_to_cid: HashMap<u16, u16>,
  /// 使用されるグリフIDの集合
  pub used_gids: IndexSet<u16>,
  /// 各グリフの横幅情報
  pub advance_widths: HashMap<u16, f32>,
  /// CIDからUnicode文字へのマッピング
  pub cid_to_chars: HashMap<u16, Vec<char>>,
}

impl Default for GlyphMapping {
  fn default() -> Self { Self::new() }
}

impl GlyphMapping {
  /// 新しいグリフマッピングを作成
  ///
  /// .notdefグリフ(GID=0, CID=0)を初期状態で登録します。
  /// .notdefはフォントに存在しない文字を表示する際に使用される特殊なグリフです。
  pub fn new() -> Self {
    let mut gid_to_cid = HashMap::new();
    gid_to_cid.insert(NOTDEF_GID, NOTDEF_GID);

    let mut used_gids = IndexSet::new();
    used_gids.insert(NOTDEF_GID);

    Self {
      gid_to_cid,
      used_gids,
      advance_widths: HashMap::new(),
      cid_to_chars: HashMap::new(),
    }
  }

  /// アドバンス幅リストを構築
  ///
  /// CID順に並んだアドバンス幅（水平進行幅）のリストを生成します。
  /// 未登録のCIDにはデフォルト幅（通常はUPEM値）を使用します。
  /// PDFのCIDFontのWidths配列として使用されます。
  ///
  /// # 引数
  ///
  /// * `default_width` - デフォルトのアドバンス幅値
  pub fn build_advance_list(&self, default_width: f32) -> Vec<f32> {
    (0..self.gid_to_cid.len())
      .map(|cid_index| {
        *self
          .advance_widths
          .get(&(cid_index as u16))
          .unwrap_or(&default_width)
      })
      .collect()
  }

  /// CIDからGIDへのマッピングテーブルを構築
  ///
  /// PDFのCIDFontで使用するバイト列形式のマッピングテーブルを生成します。
  /// 各CIDは2バイトのビッグエンディアン値として表現されます。
  /// このテーブルはPDF内でCIDを実際のフォントグリフに変換するために使用されます。
  pub fn build_cid_to_gid_map(&self) -> Vec<u8> {
    let mut map = Vec::with_capacity(self.gid_to_cid.len() * 2);
    for cid_index in 0..self.gid_to_cid.len() {
      map.push((cid_index >> 8) as u8);
      map.push((cid_index & 0xff) as u8);
    }
    map
  }
}

/// 全フォントのグリフマッピング情報を保持する構造体
///
/// 19種類のフォント（Serif/Sans Serif/Monospace/Math/日本語の各バリエーション）の
/// グリフマッピングをまとめて管理します。
#[derive(Default)]
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
  pub fn new() -> Self { Self::default() }
}

/// 全フォントのアドバンス幅リストを保持する構造体
///
/// 19種類のフォント各々のアドバンス幅リストをまとめて管理します。
/// PDF生成時のCIDFontのWidths配列として使用されます。
pub struct AdvanceLists {
  pub serif_font: Vec<f32>,
  pub serif_bold_font: Vec<f32>,
  pub serif_italic_font: Vec<f32>,
  pub serif_bold_italic_font: Vec<f32>,
  pub sans_serif_font: Vec<f32>,
  pub sans_serif_bold_font: Vec<f32>,
  pub sans_serif_italic_font: Vec<f32>,
  pub sans_serif_bold_italic_font: Vec<f32>,
  pub monospace_font: Vec<f32>,
  pub monospace_bold_font: Vec<f32>,
  pub monospace_italic_font: Vec<f32>,
  pub monospace_bold_italic_font: Vec<f32>,
  pub math_font: Vec<f32>,
  pub japanese_serif_font: Vec<f32>,
  pub japanese_serif_bold_font: Vec<f32>,
  pub japanese_sans_serif_font: Vec<f32>,
  pub japanese_sans_serif_bold_font: Vec<f32>,
  pub japanese_monospace_font: Vec<f32>,
  pub japanese_monospace_bold_font: Vec<f32>,
}

/// 全フォントのCID-GIDマッピングテーブルを保持する構造体
///
/// 19種類のフォント各々のCIDからGIDへのマッピングテーブル（バイト列）を
/// まとめて管理します。PDFのCIDFontのストリームとして使用されます。
pub struct CidToGidMaps {
  pub serif_font: Vec<u8>,
  pub serif_bold_font: Vec<u8>,
  pub serif_italic_font: Vec<u8>,
  pub serif_bold_italic_font: Vec<u8>,
  pub sans_serif_font: Vec<u8>,
  pub sans_serif_bold_font: Vec<u8>,
  pub sans_serif_italic_font: Vec<u8>,
  pub sans_serif_bold_italic_font: Vec<u8>,
  pub monospace_font: Vec<u8>,
  pub monospace_bold_font: Vec<u8>,
  pub monospace_italic_font: Vec<u8>,
  pub monospace_bold_italic_font: Vec<u8>,
  pub math_font: Vec<u8>,
  pub japanese_serif_font: Vec<u8>,
  pub japanese_serif_bold_font: Vec<u8>,
  pub japanese_sans_serif_font: Vec<u8>,
  pub japanese_sans_serif_bold_font: Vec<u8>,
  pub japanese_monospace_font: Vec<u8>,
  pub japanese_monospace_bold_font: Vec<u8>,
}

/// 全フォントのToUnicode CMapを保持する構造体
///
/// 19種類のフォント各々のToUnicode CMapをまとめて管理します。
/// ToUnicode CMapはPDFビューアーでのテキスト検索やコピー機能を可能にします。
pub struct ToUnicodeCmaps {
  pub serif_font: UnicodeCmap,
  pub serif_bold_font: UnicodeCmap,
  pub serif_italic_font: UnicodeCmap,
  pub serif_bold_italic_font: UnicodeCmap,
  pub sans_serif_font: UnicodeCmap,
  pub sans_serif_bold_font: UnicodeCmap,
  pub sans_serif_italic_font: UnicodeCmap,
  pub sans_serif_bold_italic_font: UnicodeCmap,
  pub monospace_font: UnicodeCmap,
  pub monospace_bold_font: UnicodeCmap,
  pub monospace_italic_font: UnicodeCmap,
  pub monospace_bold_italic_font: UnicodeCmap,
  pub math_font: UnicodeCmap,
  pub japanese_serif_font: UnicodeCmap,
  pub japanese_serif_bold_font: UnicodeCmap,
  pub japanese_sans_serif_font: UnicodeCmap,
  pub japanese_sans_serif_bold_font: UnicodeCmap,
  pub japanese_monospace_font: UnicodeCmap,
  pub japanese_monospace_bold_font: UnicodeCmap,
}
