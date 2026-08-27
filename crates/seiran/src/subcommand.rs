//! フォント情報を表示する CLI サブコマンド

mod variation_axes;
/// バリアブルフォントの軸情報を表示する。
pub(super) use variation_axes::variation_axes;

mod ttc_names;
/// TTC ファイル内のフォント名を表示する。
pub(super) use ttc_names::ttc_names;

mod script_langs;
/// フォント対応の OpenType Script/Language タグを表示する。
pub(super) use script_langs::script_langs;
