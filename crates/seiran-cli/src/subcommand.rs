//! フォント情報を表示する CLI サブコマンド

mod get_variation_axes;
/// バリアブルフォントの軸情報を表示する。
pub(super) use get_variation_axes::get_variation_axes;

mod get_ttc_names;
/// TTC ファイル内のフォント名を表示する。
pub(super) use get_ttc_names::get_ttc_names;

mod script_langs;
/// フォント対応の OpenType Script/Language タグを表示する。
pub(super) use script_langs::script_langs;
