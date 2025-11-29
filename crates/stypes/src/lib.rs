//! 共通型定義モジュール
//!
//! このモジュールは、プロジェクト全体で使用される
//! グリフマッピングなどの共通型を定義します。

use std::collections::HashMap;

use indexmap::IndexSet;

/// .notdefグリフのGID
const NOTDEF_GID: u16 = 0;

/// グリフとCIDのマッピング情報を管理する構造体
///
/// PDF生成に必要なグリフID(GID)と文字ID(CID)の対応、
/// 使用されるグリフの集合、横幅情報、Unicodeマッピングを管理します。
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
  fn default() -> Self {
    Self::new()
  }
}

impl GlyphMapping {
  /// 新しいグリフマッピングを作成
  ///
  /// .notdefグリフ(GID=0)を初期状態で登録します。
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

  /// 横幅リストを構築
  ///
  /// CID順に並んだ横幅のリストを生成します。
  /// 未登録のCIDにはデフォルト幅を使用します。
  ///
  /// # 引数
  ///
  /// * `default_width` - デフォルトの横幅値
  pub fn build_advance_list(&self, default_width: f32) -> Vec<f32> {
    (0..self.gid_to_cid.len())
      .map(|cid| {
        *self
          .advance_widths
          .get(&(cid as u16))
          .unwrap_or(&default_width)
      })
      .collect()
  }

  /// CIDからGIDへのマッピングテーブルを構築
  ///
  /// PDFのCIDFontで使用するバイト列形式のマッピングテーブルを生成します。
  /// 各CIDは2バイトのビッグエンディアン値として表現されます。
  pub fn build_cid_to_gid_map(&self) -> Vec<u8> {
    let mut map = Vec::with_capacity(self.gid_to_cid.len() * 2);
    for cid in 0..self.gid_to_cid.len() {
      map.push((cid >> 8) as u8);
      map.push((cid & 0xFF) as u8);
    }
    map
  }
}
