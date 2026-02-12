//! 共通型定義モジュール
//!
//! このモジュールは、プロジェクト全体で使用される
//! グリフマッピング、アドバンス幅リスト、CID-GIDマッピング、
//! `ToUnicode` `CMap`などの共通型を定義します。

use std::collections::HashMap;

use indexmap::IndexSet;
use pdf_writer::types::UnicodeCmap;

/// .notdefグリフのGID
const NOTDEF_GID: u16 = 0;

/// フォントの種類を表す列挙型
///
/// 設定から適切なフォント情報を取得するために使用されます。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontType {
  Serif,
  SerifBold,
  SerifItalic,
  SerifBoldItalic,
  SansSerif,
  SansSerifBold,
  SansSerifItalic,
  SansSerifBoldItalic,
  Monospace,
  MonospaceBold,
  MonospaceItalic,
  MonospaceBoldItalic,
  Math,
  JapaneseSerif,
  JapaneseSerifBold,
  JapaneseSansSerif,
  JapaneseSansSerifBold,
  JapaneseMonospace,
  JapaneseMonospaceBold,
}

impl FontType {
  pub const ALL: [FontType; 19] = [
    FontType::Serif,
    FontType::SerifBold,
    FontType::SerifItalic,
    FontType::SerifBoldItalic,
    FontType::SansSerif,
    FontType::SansSerifBold,
    FontType::SansSerifItalic,
    FontType::SansSerifBoldItalic,
    FontType::Monospace,
    FontType::MonospaceBold,
    FontType::MonospaceItalic,
    FontType::MonospaceBoldItalic,
    FontType::Math,
    FontType::JapaneseSerif,
    FontType::JapaneseSerifBold,
    FontType::JapaneseSansSerif,
    FontType::JapaneseSansSerifBold,
    FontType::JapaneseMonospace,
    FontType::JapaneseMonospaceBold,
  ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
  Serif,
  SerifBold,
  SerifItalic,
  SerifBoldItalic,
  SansSerif,
  SansSerifBold,
  SansSerifItalic,
  SansSerifBoldItalic,
  Monospace,
  MonospaceBold,
  MonospaceItalic,
  MonospaceBoldItalic,
  Math,
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

/// グリフとCIDのマッピング情報を管理する構造体
///
/// PDF生成に必要なグリフID(GID)と文字ID(CID)の対応、
/// 使用されるグリフの集合、アドバンス幅情報、Unicodeマッピングを管理します。
/// 各フォントごとに1つのインスタンスが作成されます。
#[derive(Debug)]
pub struct GlyphMapping {
  /// GIDからCIDへのマッピング
  pub gid_to_cid: HashMap<u16, u16>,
  /// 使用されるグリフIDの集合
  pub used_gids: IndexSet<u16>,
  /// 各グリフの横幅情報
  pub advance_widths: HashMap<u16, u16>,
  /// `CID`から`Unicode`文字へのマッピング
  pub cid_to_chars: HashMap<u16, Vec<char>>,
}

impl Default for GlyphMapping {
  /// 新しいグリフマッピングを作成
  ///
  /// .notdefグリフ(GID=0, CID=0)を初期状態で登録します。
  /// .notdefはフォントに存在しない文字を表示する際に使用される特殊なグリフです。
  fn default() -> Self {
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
}

impl GlyphMapping {
  /// アドバンス幅リストを構築
  ///
  /// CID順に並んだアドバンス幅（水平進行幅）のリストを生成します。
  /// 未登録のCIDにはデフォルト幅（通常はUPEM値）を使用します。
  /// PDFの`CIDFont`の`Widths`配列として使用されます。
  ///
  /// # 引数
  ///
  /// * `default_width` - デフォルトのアドバンス幅値
  #[must_use]
  pub fn build_advance_list(&self, default_width: u16) -> Vec<u16> {
    (0..self.gid_to_cid.len())
      .map(|cid_index| *self.advance_widths.get(&(cid_index as u16)).unwrap_or(&default_width))
      .collect()
  }

  /// CIDからGIDへのマッピングテーブルを構築
  ///
  /// PDFの`CIDFont`で使用するバイト列形式のマッピングテーブルを生成します。
  /// 各CIDは2バイトのビッグエンディアン値として表現されます。
  /// このテーブルはPDF内でCIDを実際のフォントグリフに変換するために使用されます。
  #[must_use]
  pub fn build_cid_to_gid_map(&self) -> Vec<u8> {
    let mut map = Vec::with_capacity(self.gid_to_cid.len() * 2);
    for cid_index in 0..self.gid_to_cid.len() {
      map.push((cid_index >> 8) as u8);
      map.push((cid_index & 0xff) as u8);
    }
    map
  }
}

/// 全フォントのアドバンス幅リストを保持する構造体
///
/// 19種類のフォント各々のアドバンス幅リストをまとめて管理します。
/// PDF生成時の`CIDFont`の`Widths`配列として使用されます。
pub struct AdvanceLists {
  pub serif: Vec<u16>,
  pub serif_bold: Vec<u16>,
  pub serif_italic: Vec<u16>,
  pub serif_bold_italic: Vec<u16>,
  pub sans_serif: Vec<u16>,
  pub sans_serif_bold: Vec<u16>,
  pub sans_serif_italic: Vec<u16>,
  pub sans_serif_bold_italic: Vec<u16>,
  pub monospace: Vec<u16>,
  pub monospace_bold: Vec<u16>,
  pub monospace_italic: Vec<u16>,
  pub monospace_bold_italic: Vec<u16>,
  pub math: Vec<u16>,
  pub japanese_serif: Vec<u16>,
  pub japanese_serif_bold: Vec<u16>,
  pub japanese_sans_serif: Vec<u16>,
  pub japanese_sans_serif_bold: Vec<u16>,
  pub japanese_monospace: Vec<u16>,
  pub japanese_monospace_bold: Vec<u16>,
}

/// 全フォントのCID-GIDマッピングテーブルを保持する構造体
///
/// 19種類のフォント各々のCIDからGIDへのマッピングテーブル（バイト列）を
/// まとめて管理します。PDFのCIDFontのストリームとして使用されます。
pub struct CidToGidMaps {
  pub serif: Vec<u8>,
  pub serif_bold: Vec<u8>,
  pub serif_italic: Vec<u8>,
  pub serif_bold_italic: Vec<u8>,
  pub sans_serif: Vec<u8>,
  pub sans_serif_bold: Vec<u8>,
  pub sans_serif_italic: Vec<u8>,
  pub sans_serif_bold_italic: Vec<u8>,
  pub monospace: Vec<u8>,
  pub monospace_bold: Vec<u8>,
  pub monospace_italic: Vec<u8>,
  pub monospace_bold_italic: Vec<u8>,
  pub math: Vec<u8>,
  pub japanese_serif: Vec<u8>,
  pub japanese_serif_bold: Vec<u8>,
  pub japanese_sans_serif: Vec<u8>,
  pub japanese_sans_serif_bold: Vec<u8>,
  pub japanese_monospace: Vec<u8>,
  pub japanese_monospace_bold: Vec<u8>,
}

/// 全フォントの`ToUnicode` `CMap`を保持する構造体
///
/// 19種類のフォント各々の`ToUnicode` `CMap`をまとめて管理します。
/// `ToUnicode` `CMap`はPDFビューアーでのテキスト検索やコピー機能を可能にします。
pub struct ToUnicodeCmaps {
  pub serif: UnicodeCmap,
  pub serif_bold: UnicodeCmap,
  pub serif_italic: UnicodeCmap,
  pub serif_bold_italic: UnicodeCmap,
  pub sans_serif: UnicodeCmap,
  pub sans_serif_bold: UnicodeCmap,
  pub sans_serif_italic: UnicodeCmap,
  pub sans_serif_bold_italic: UnicodeCmap,
  pub monospace: UnicodeCmap,
  pub monospace_bold: UnicodeCmap,
  pub monospace_italic: UnicodeCmap,
  pub monospace_bold_italic: UnicodeCmap,
  pub math: UnicodeCmap,
  pub japanese_serif: UnicodeCmap,
  pub japanese_serif_bold: UnicodeCmap,
  pub japanese_sans_serif: UnicodeCmap,
  pub japanese_sans_serif_bold: UnicodeCmap,
  pub japanese_monospace: UnicodeCmap,
  pub japanese_monospace_bold: UnicodeCmap,
}
