//! 共通型定義モジュール
//!
//! このモジュールは、Seiran の核となる **19 フォント種別システム** と、
//! PDF 生成全体で使用される共通型を定義します。
//!
//! ## モジュール構成
//!
//! - [`font`] - フォント種別 [`FontType`]（19 種別）とスタイル分類 [`FontKind`]（13 種別）
//! - [`font_map`] - 全フォント種別に値を対応付ける汎用コンテナ [`FontMap`]
//! - [`heading_level`] - 見出しのレベル [`HeadingLevel`]
//! - [`length`] - 単位付き長さ値 [`Length`]
//! - [`table`] - 表の列指定 [`TableColumn`] / [`ColumnAlign`] / [`ColumnWidth`]
//! - [`anchor_mark`] - リンク行き先のアンカー [`AnchorMark`]（PDF しおり / 内部リンク）
//! - [`link_target`] - ハイパーリンクの行き先 [`LinkTarget`]（内部 / 外部）
//! - [`color`] - 8bit RGB 色 [`Color`]（背景色 / 罫線色 / テキスト色の共通表現）
//!
//! ## `FontMap<T>`
//!
//! [`FontMap`] は 19 フォント種別それぞれに値を対応付ける汎用コンテナです。
//! 内部実装は `HashMap<FontType, T>` で、イテレーションは [`FontType::ALL`] の
//! 順序で返されます。
//!
//! ## 19 フォント種別システム
//!
//! Seiran では、すべてのテキストを以下の 19 フォント種別に分類して処理します：
//!
//! | カテゴリ         | 種別数 | フォント                                              |
//! | -------------- | ------ | -------------------------------------------------- |
//! | Serif (セリフ)  | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Sans Serif     | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Monospace      | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Math (数式)    | 1      | 数式用フォント（OpenType Math テーブル対応）            |
//! | Japanese       | 6      | Serif (標準/太字) + Sans Serif (標準/太字) + Monospace (標準/太字) |
//!
//! 各文字が解析される際、その属性（言語、スタイル、用途）に基づいて
//! 適切なフォント種別が決定されます。
//!
//! - **`FontType`**: すべてのフォント種別（19 個）
//! - **`FontKind`**: 言語判定前のスタイル分類（13 個、日本語フォント除外）
//!
//! 関連クレート：
//! - 設定: `read_config`
//! - フォント処理: `font`
//! - スクリプト判定と `FontKind` → `FontType` の解決: `layout`

pub mod anchor_mark;
pub mod color;
pub mod font;
pub mod font_map;
pub mod heading_level;
pub mod length;
pub mod link_target;
pub mod table;

pub use anchor_mark::AnchorMark;
pub use color::Color;
pub use font::{FontKind, FontType};
pub use font_map::{FontMap, FontMapIter, FontMapIterMut};
pub use heading_level::HeadingLevel;
pub use length::Length;
pub use link_target::LinkTarget;
pub use table::{ColumnAlign, ColumnWidth, TableColumn};
