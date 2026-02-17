//! Seiran の CLI サブコマンド群
//!
//! メイン機能（PDF 生成）以外の補助コマンドを提供します。
//! フォントの詳細情報確認や検査目的で使用されます。
//!
//! ## サブコマンド一覧
//!
//! - **`get_variation_axes`** - バリアブルフォント軸の情報を表示
//! - **`get_ttc_names`** - TrueType Collection ファイル内のフォント名を表示
//! - **`script_langs`** - フォント対応の OpenType Script/Language タグを表示

mod get_variation_axes;
/// バリアブルフォント軸情報を取得・表示するサブコマンド
///
/// 可変フォント（Variable Font）が定義する軸（weight、width など）の
/// 名前と値の範囲を取得して、JSON 形式で表示します。
///
/// # 用途
///
/// - フォント調査 - どの軸が利用可能か確認
/// - 設定ファイル作成 - `variation_axes` フィールドの記入時に参考
/// - バリアブルフォント仕様確認 - 軸の最小値・デフォルト・最大値を把握
pub(crate) use get_variation_axes::get_variation_axes;

mod get_ttc_names;
/// TrueType Collection ファイル内のフォント名を取得・表示するサブコマンド
///
/// 複数のフォントを1ファイルにまとめた TTC ファイルの構成を確認します。
/// 各フォントの名前と、`variation_axes` コマンドの `--font-index` に指定する
/// インデックスを表示します。
///
/// # 用途
///
/// - TTC 構成確認 - ファイルに含まれるフォント一覧を表示
/// - コレクション検査 - 期待するフォントが含まれているか確認
/// - 設定ファイル作成 - 複数フォント構成時に正しいインデックスを利用
pub(crate) use get_ttc_names::get_ttc_names;

mod script_langs;
/// フォント対応の OpenType Script/Language タグを取得・表示するサブコマンド
///
/// フォントが対応している言語・文字体系（Script）と言語体系（Language）の
/// 組み合わせを確認します。これにより、フォントがどの言語で正しく機能するか
/// を判定できます。
///
/// # 対応タグの例
///
/// - **Script**: `latn`（Latin）、`arab`（Arabic）、`dev2`（Devanagari）
/// - **Language**: `ENG`（English）、`JAN`（Japanese）、`ARA`（Arabic）
///
/// # 用途
///
/// - 言語対応確認 - フォントが特定言語をサポートしているか確認
/// - シェイピング設定 - 正しい Script/Language タグを設定ファイルに記入
/// - 多言語フォント検査 - 複数言語サポート状況を把握
pub(crate) use script_langs::script_langs;
