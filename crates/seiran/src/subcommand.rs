//! フォント情報を表示する CLI サブコマンド
//!
//! 3 つとも「フォントファイルを読む」前段・「フォントを調べて一覧の行を作る」段・「一覧を書き出す」段に
//! 分かれる。前段は [`font_file`]（`--font-index` を取る 2 つはそこで face も選ぶ。`ttc-names` はコレクションを
//! 自分で列挙する）、書き出しは [`listing::emit`] の 1 箇所を通る。調査に失敗したら一覧を 1 行も出さずに
//! 診断エラーを返す。

mod font_file;
mod listing;

mod variation_axes;
pub(super) use variation_axes::variation_axes;

mod ttc_names;
pub(super) use ttc_names::ttc_names;

mod script_langs;
pub(super) use script_langs::script_langs;
