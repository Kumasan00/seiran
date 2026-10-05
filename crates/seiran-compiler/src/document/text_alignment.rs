//! 段落の揃え [`TextAlignment`]。

use serde::Deserialize;

/// 段落の揃え（`[text].alignment` / `[heading.<level>].alignment`、寄せ環境 `flushleft` / `center` / `flushright`）。
///
/// 両端揃えの有無と寄せる向きを 1 つの値で持つので、「両端揃え × 中央・右」は表現できない。
/// `Justify` 以外の 3 値は分割点を同じ規則（左揃え）で選び、確定した行を伸縮させずに水平にずらすだけ。
/// 行が利用可能幅を超えるときは寄せない。
///
/// 従わないもの（種類ごとに揃えが決まっている）: 脚注本体・コードブロック（左）、図表・タイトルページ（中央）、
/// 数式ブロック（`[math.block].alignment`）、表のセル（`columns`）、柱（`[header]` / `[footer]`）、
/// 目次のエントリ行と索引（`[toc]` / `[index]` の 3 値の揃え）。目次の題目行は `[heading.section]` の揃えを通じて従う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextAlignment {
  /// 両端揃え（既定）。行の余り幅を伸縮点へ比例配分して行末を利用可能幅の右端に揃える。
  /// 段落最終行・強制改行直前の行は伸縮せず左に寄せる
  #[default]
  Justify,
  /// 左揃え（ragged-right）。自然幅のまま左端から並べる
  Left,
  /// 中央揃え。自然幅のまま利用可能幅の中央に置く
  Center,
  /// 右揃え（ragged-left）。自然幅のまま利用可能幅の右端に揃える
  Right,
}
