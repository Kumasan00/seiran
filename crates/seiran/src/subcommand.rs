//! フォント情報を表示する CLI サブコマンド
//!
//! 3 つとも「フォントを調べ終えて一覧の行を作る」段と「一覧を書き出す」段に分かれ、書き出しは
//! [`listing::emit`] 1 箇所を通る。調査に失敗したら一覧を 1 行も出さずに診断エラーを返す。

mod listing;

mod variation_axes;
pub(super) use variation_axes::variation_axes;

mod ttc_names;
pub(super) use ttc_names::ttc_names;

mod script_langs;
pub(super) use script_langs::script_langs;
